//! What `run_command` may start (design, "Allow-list"): a fixed table of
//! programs, the verbs of each and the flags that are never accepted. Pure:
//! no IO, no shell. The table bounds the VERB, not what the program does; the
//! real controls are the per-command `y`, the jail and the review warnings.
//!
//! Flags are judged only before a bare `--` (and, for `go`, `-args`): what
//! follows belongs to the program the tool runs (`cargo test -- -Z
//! unstable-options` is libtest's flag). Paths are judged in every argument.

use std::path::PathBuf;
use std::time::Duration;

use serde_json::Value;

use super::ToolError;
use super::paths::plain_relative;
use super::registry::{ArgDef, ArgKind, ArgValue, FamilySet, parse_args};

/// A program the runner may start.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Program {
    Cargo,
    Npm,
    Pnpm,
    Yarn,
    Pytest,
    Go,
}

impl Program {
    /// The program named exactly `name`; a path or any other spelling is `None`.
    pub fn parse(name: &str) -> Option<Program> {
        Some(match name {
            "cargo" => Program::Cargo,
            "npm" => Program::Npm,
            "pnpm" => Program::Pnpm,
            "yarn" => Program::Yarn,
            "pytest" => Program::Pytest,
            "go" => Program::Go,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Program::Cargo => "cargo",
            Program::Npm => "npm",
            Program::Pnpm => "pnpm",
            Program::Yarn => "yarn",
            Program::Pytest => "pytest",
            Program::Go => "go",
        }
    }
}

/// Why a call is refused before anything starts. The text is fixed and never
/// echoes the call, so it may go back to a model.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    Program,
    Verb,
    Flag,
    /// An absolute path or `..` in an argument, or a `cwd` outside the root.
    Path,
}

impl Refusal {
    pub fn text(self) -> &'static str {
        match self {
            Refusal::Program => "program not allowed",
            Refusal::Verb => "subcommand not allowed",
            Refusal::Flag => "flag not allowed",
            Refusal::Path => "path not available",
        }
    }
}

const CARGO_VERBS: &[&str] = &["build", "check", "test", "clippy", "fmt", "run", "doc"];
const CARGO_DENIED: &[&str] = &[
    "--config",
    "--manifest-path",
    "--target-dir",
    "--fix",
    "--allow-dirty",
    "--allow-staged",
    "--allow-no-vcs",
];
/// Cargo flags that may carry their value in the same word (`-Zfoo`).
const CARGO_ATTACHED: &[&str] = &["-Z"];

const NODE_DENIED: &[&str] = &[
    "-g",
    "--global",
    "--location",
    "--prefix",
    "--userconfig",
    "--globalconfig",
    "--config",
    "--registry",
    "--store-dir",
    "--global-folder",
    "--cwd",
    "--dir",
    "-C",
];
/// `-C<dir>` and pnpm's `--config.<key>` carry their value in the same word.
const NODE_ATTACHED: &[&str] = &["-C", "--config."];

const GO_VERBS: &[&str] = &["test", "vet", "build", "run"];
/// Go flags by their bare name: `-x` and `--x` are the same flag to Go.
const GO_DENIED: &[&str] = &["modfile", "toolexec", "exec", "overlay", "vettool", "C"];

/// Checks `program` and `args` against the table. Order: program, verb,
/// flags, paths. `scripts` are the keys of the project's `package.json`
/// `scripts` object: `npm run`, `yarn run` and `pnpm run` accept only those.
pub fn check(program: &str, args: &[String], scripts: &[String]) -> Result<Program, Refusal> {
    let program = Program::parse(program).ok_or(Refusal::Program)?;
    verb(program, args, scripts)?;
    // argparse expands `@file` in every word, also after `--`.
    if program == Program::Pytest && args.iter().any(|arg| arg.starts_with('@')) {
        return Err(Refusal::Flag);
    }
    flags(program, args)?;
    if args.iter().any(|arg| path_like(arg)) {
        return Err(Refusal::Path);
    }
    Ok(program)
}

fn verb(program: Program, args: &[String], scripts: &[String]) -> Result<(), Refusal> {
    let first = args.first().map(String::as_str);
    let ok = match program {
        Program::Cargo => match first {
            Some("fmt") => args.iter().any(|arg| arg == "--check"),
            Some(verb) => CARGO_VERBS.contains(&verb),
            None => false,
        },
        Program::Go => first.is_some_and(|verb| GO_VERBS.contains(&verb)),
        Program::Pytest => true,
        Program::Npm | Program::Pnpm | Program::Yarn => match first {
            Some("test") => true,
            // yarn and pnpm fall back to running a binary of that name
            // (`yarn run bash -c x`), and the body of an npm script is read
            // when it runs: only a declared script is a script, and the
            // prompt shows its body.
            Some("run") => args
                .get(1)
                .is_some_and(|script| scripts.iter().any(|known| known == script)),
            Some("install" | "ci") => args[1..].iter().all(|arg| arg.starts_with('-')),
            _ => false,
        },
    };
    if ok { Ok(()) } else { Err(Refusal::Verb) }
}

fn flags(program: Program, args: &[String]) -> Result<(), Refusal> {
    let own = match program {
        Program::Go => {
            let end = args.iter().position(|a| a == "--" || a == "-args");
            &args[..end.unwrap_or(args.len())]
        }
        _ => {
            let end = args.iter().position(|a| a == "--");
            &args[..end.unwrap_or(args.len())]
        }
    };
    let denied = own.iter().any(|arg| match program {
        Program::Cargo => named(arg, CARGO_DENIED, CARGO_ATTACHED),
        Program::Go => go_flag(arg).is_some_and(|flag| GO_DENIED.contains(&flag)),
        Program::Pytest => false,
        _ => named(arg, NODE_DENIED, NODE_ATTACHED) || global_bundle(arg),
    });
    if denied { Err(Refusal::Flag) } else { Ok(()) }
}

/// `arg` is one of `denied` (alone or as `flag=value`) or starts with one of
/// `attached`.
fn named(arg: &str, denied: &[&str], attached: &[&str]) -> bool {
    denied.iter().any(|flag| {
        arg == *flag
            || arg
                .strip_prefix(flag)
                .is_some_and(|rest| rest.starts_with('='))
    }) || attached.iter().any(|flag| arg.starts_with(flag))
}

/// A bundle of short flags that includes `g` (`-gD`): a global install.
fn global_bundle(arg: &str) -> bool {
    arg.strip_prefix('-')
        .is_some_and(|rest| rest.chars().all(|c| c.is_ascii_alphabetic()) && rest.contains('g'))
}

/// The bare name of a Go flag word: `-modfile=x` and `--modfile` give `modfile`.
fn go_flag(arg: &str) -> Option<&str> {
    let bare = arg.strip_prefix("--").or_else(|| arg.strip_prefix('-'))?;
    Some(bare.split('=').next().unwrap_or(bare))
}

/// An absolute path or a `..` part in the word, in every `=`-separated tail
/// (`--flag=value`, `-ldflags=-extld=/bin/sh`) or in the value glued to a
/// short flag (`-c/etc/x`), of the word and of each tail.
fn path_like(arg: &str) -> bool {
    let tails = arg.match_indices('=').map(|(at, _)| &arg[at + 1..]);
    std::iter::once(arg).chain(tails).any(|word| {
        let glued = if word.starts_with('-') && !word.starts_with("--") {
            word.get(2..)
        } else {
            None
        };
        [Some(word), glued]
            .into_iter()
            .flatten()
            .any(|word| absolute(word) || word.split(['/', '\\']).any(|part| part == ".."))
    })
}

fn absolute(word: &str) -> bool {
    let bytes = word.as_bytes();
    word.starts_with(['/', '\\'])
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'/' | b'\\'))
}

/// The longest `program` word: the longest name in the table is 6.
const PROGRAM_MAX: usize = 16;
/// Arguments one call may pass, and the bytes of each.
const ARGS_MAX: usize = 64;
const ARG_MAX: usize = 512;
/// Seconds a run may last when the call names none, and the most it may ask.
pub const TIMEOUT_DEFAULT: i64 = 120;
pub const TIMEOUT_MAX: i64 = 600;

/// The arguments of `run_command`. No `shell` string exists: `args` is a list
/// and reaches the program as literal elements.
pub const RUN_ARGS: &[ArgDef] = &[
    ArgDef {
        name: "program",
        kind: ArgKind::Str { max: PROGRAM_MAX },
        required: true,
        gate: None,
    },
    ArgDef {
        name: "args",
        kind: ArgKind::StrList {
            items: ARGS_MAX,
            each: ARG_MAX,
        },
        required: true,
        gate: None,
    },
    ArgDef {
        name: "cwd",
        kind: ArgKind::Str { max: ARG_MAX },
        required: false,
        gate: None,
    },
    ArgDef {
        name: "timeout_secs",
        kind: ArgKind::Int {
            min: 1,
            max: TIMEOUT_MAX,
        },
        required: false,
        gate: None,
    },
];

/// A `run_command` call that passed the table.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RunRequest {
    pub program: Program,
    pub args: Vec<String>,
    /// Relative to the root; empty is the root. Judged by text only: the
    /// runtime still resolves it against the real disk.
    pub cwd: PathBuf,
    pub timeout: Duration,
}

/// Why a call did not become a [`RunRequest`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunError {
    /// The arguments are not shaped as `RUN_ARGS` says.
    Shape(ToolError),
    Refused(Refusal),
}

/// Reads and checks the arguments of a `run_command` call. `timeout_secs`
/// defaults to 120 and is clamped into 1..=600, never refused for its size.
pub fn parse_run(args: &Value, scripts: &[String]) -> Result<RunRequest, RunError> {
    let parsed = parse_args(RUN_ARGS, args, FamilySet::LEGACY).map_err(RunError::Shape)?;
    let text = |name| match parsed.get(name) {
        Some(ArgValue::Str(text)) => Some(text.as_str()),
        _ => None,
    };
    let list = match parsed.get("args") {
        Some(ArgValue::List(list)) => list.clone(),
        _ => Vec::new(),
    };
    let program =
        check(text("program").unwrap_or_default(), &list, scripts).map_err(RunError::Refused)?;
    let cwd = match text("cwd") {
        Some(cwd) => plain_relative(cwd).ok_or(RunError::Refused(Refusal::Path))?,
        None => PathBuf::new(),
    };
    let secs = match parsed.get("timeout_secs") {
        Some(ArgValue::Int(secs)) => *secs,
        _ => TIMEOUT_DEFAULT,
    };
    Ok(RunRequest {
        program,
        args: list,
        cwd,
        timeout: Duration::from_secs(secs as u64),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|a| (*a).to_owned()).collect()
    }

    /// The `scripts` of the project's `package.json` the tests run against.
    fn scripts() -> Vec<String> {
        argv(&["build", "lint", "b", "test"])
    }

    fn pass(program: &str, args: &[&str]) {
        assert!(
            check(program, &argv(args), &scripts()).is_ok(),
            "{program} {args:?} should pass"
        );
    }

    fn refused(program: &str, args: &[&str]) -> Refusal {
        match check(program, &argv(args), &scripts()) {
            Err(why) => why,
            Ok(p) => panic!("{program} {args:?} passed as {p:?}"),
        }
    }

    // Spec (command-runner, Argv input and allow-list): the program is matched
    // by exact name, never by path.
    #[test]
    fn only_the_six_programs_pass_and_only_by_exact_name() {
        for (program, args) in [
            ("cargo", &["test"][..]),
            ("npm", &["test"]),
            ("pnpm", &["test"]),
            ("yarn", &["test"]),
            ("pytest", &[]),
            ("go", &["test"]),
        ] {
            pass(program, args);
        }
        for program in [
            "make",
            "python",
            "sh",
            "bash",
            "/usr/bin/cargo",
            "./cargo",
            "Cargo",
            "cargo ",
            "",
            "npx",
        ] {
            assert_eq!(refused(program, &["test"]), Refusal::Program, "{program:?}");
        }
        assert_eq!(
            check("pnpm", &argv(&["test"]), &scripts()),
            Ok(Program::Pnpm)
        );
        assert_eq!(Program::Go.name(), "go");
    }

    #[test]
    fn cargo_takes_its_seven_verbs_and_nothing_else() {
        for verb in ["build", "check", "test", "clippy", "run", "doc"] {
            pass("cargo", &[verb]);
        }
        pass("cargo", &["fmt", "--check"]);
        pass("cargo", &["test", "--lib", "--", "--nocapture"]);
        pass(
            "cargo",
            &["clippy", "--all-targets", "--", "-D", "warnings"],
        );
        for verb in [
            "install", "add", "publish", "login", "new", "fmt", "+nightly",
        ] {
            assert_eq!(refused("cargo", &[verb]), Refusal::Verb, "{verb}");
        }
        assert_eq!(refused("cargo", &[]), Refusal::Verb);
        // A global flag before the verb is not a verb.
        assert_eq!(
            refused("cargo", &["-Zunstable-options", "test"]),
            Refusal::Verb
        );
        assert_eq!(
            refused("cargo", &["--config", "x=1", "test"]),
            Refusal::Verb
        );
    }

    #[test]
    fn cargo_fmt_needs_check() {
        assert_eq!(refused("cargo", &["fmt"]), Refusal::Verb);
        assert_eq!(refused("cargo", &["fmt", "--all"]), Refusal::Verb);
        pass("cargo", &["fmt", "--all", "--check"]);
        pass("cargo", &["fmt", "--", "--check"]);
    }

    #[test]
    fn cargo_denied_flags_in_every_spelling() {
        for flag in [
            "--config",
            "--config=build.rustc='x'",
            "-Z",
            "-Zunstable-options",
            "--manifest-path",
            "--manifest-path=Cargo.toml",
            "--target-dir",
            "--target-dir=out",
            "--fix",
            "--allow-dirty",
            "--allow-staged",
            "--allow-no-vcs",
        ] {
            assert_eq!(refused("cargo", &["test", flag]), Refusal::Flag, "{flag}");
        }
        assert_eq!(refused("cargo", &["clippy", "--fix"]), Refusal::Flag);
        // Near misses are fine.
        pass("cargo", &["test", "--config-file-name-filter"]);
        pass("cargo", &["test", "--target", "x86_64-unknown-linux-gnu"]);
        pass("cargo", &["test", "-p", "lazarobox"]);
    }

    #[test]
    fn what_follows_a_double_dash_belongs_to_the_program_run() {
        // `-Z unstable-options` is libtest's here, not cargo's.
        pass("cargo", &["test", "--", "-Z", "unstable-options"]);
        pass("cargo", &["run", "--", "--config", "x"]);
        assert_eq!(refused("cargo", &["test", "-Z", "x", "--"]), Refusal::Flag);
    }

    #[test]
    fn the_node_managers_share_one_table() {
        for manager in ["npm", "pnpm", "yarn"] {
            pass(manager, &["test"]);
            pass(manager, &["test", "--watch=false"]);
            pass(manager, &["run", "build"]);
            pass(manager, &["run", "lint", "--", "--fix"]);
            pass(manager, &["install"]);
            pass(manager, &["install", "--frozen-lockfile"]);
            pass(manager, &["ci"]);
            for verb in ["exec", "dlx", "x", "publish", "add", "link", "i", "start"] {
                assert_eq!(refused(manager, &[verb]), Refusal::Verb, "{manager} {verb}");
            }
            assert_eq!(refused(manager, &[]), Refusal::Verb);
            assert_eq!(refused(manager, &["run"]), Refusal::Verb);
            assert_eq!(refused(manager, &["run", "--silent"]), Refusal::Verb);
        }
    }

    #[test]
    fn install_and_ci_take_no_package_args() {
        for verb in ["install", "ci"] {
            assert_eq!(refused("npm", &[verb, "left-pad"]), Refusal::Verb, "{verb}");
            assert_eq!(
                refused("pnpm", &[verb, "--save", "left-pad"]),
                Refusal::Verb
            );
        }
        pass("npm", &["ci", "--ignore-scripts"]);
    }

    #[test]
    fn node_denied_flags() {
        for flag in [
            "-g",
            "--global",
            "--prefix",
            "--prefix=elsewhere",
            "--userconfig=rc",
            "--globalconfig=rc",
            "--cwd",
            "--dir=x",
            "-C",
        ] {
            assert_eq!(refused("npm", &["test", flag]), Refusal::Flag, "{flag}");
            assert_eq!(
                refused("pnpm", &["run", "b", flag]),
                Refusal::Flag,
                "{flag}"
            );
            assert_eq!(refused("yarn", &["install", flag]), Refusal::Flag, "{flag}");
        }
        pass("npm", &["test", "--", "-g", "pattern"]);
    }

    #[test]
    fn pytest_takes_any_args_but_not_a_rootdir_or_config_outside() {
        pass("pytest", &[]);
        pass(
            "pytest",
            &["-x", "tests/test_a.py::test_one", "-k", "a or b"],
        );
        pass("pytest", &["-c", "pytest.ini"]);
        pass("pytest", &["--rootdir=."]);
        assert_eq!(refused("pytest", &["--rootdir", "/etc"]), Refusal::Path);
        assert_eq!(refused("pytest", &["--rootdir=/etc"]), Refusal::Path);
        assert_eq!(refused("pytest", &["-c", "../pytest.ini"]), Refusal::Path);
        assert_eq!(refused("pytest", &["-c/etc/pytest.ini"]), Refusal::Path);
    }

    // User decision: `yarn run <x>` / `pnpm run <x>` start only a script the
    // project's package.json declares; the bin fallback of those managers
    // (`yarn run bash -c x`) is never reachable.
    #[test]
    fn npm_yarn_and_pnpm_run_only_a_declared_script() {
        for manager in ["npm", "yarn", "pnpm"] {
            assert_eq!(
                refused(manager, &["run", "bash", "-c", "x"]),
                Refusal::Verb,
                "{manager}"
            );
            assert_eq!(refused(manager, &["run", "missing"]), Refusal::Verb);
            pass(manager, &["run", "test"]);
            // Without that script in the project, `test` is refused too.
            let none: Vec<String> = Vec::new();
            assert_eq!(
                check(manager, &argv(&["run", "test"]), &none),
                Err(Refusal::Verb)
            );
            let only_lint = argv(&["lint"]);
            assert_eq!(
                check(manager, &argv(&["run", "test"]), &only_lint),
                Err(Refusal::Verb)
            );
            assert!(check(manager, &argv(&["run", "lint"]), &only_lint).is_ok());
        }
        // npm reads the body of the script when it runs: an undeclared name
        // is refused here too, like yarn and pnpm.
        assert_eq!(refused("npm", &["run", "anything"]), Refusal::Verb);
        assert_eq!(refused("npm", &["run", "bash", "-c", "x"]), Refusal::Verb);
        pass("npm", &["run", "build"]);
    }

    #[test]
    fn node_config_location_and_store_flags_are_denied() {
        for flag in [
            "--location=global",
            "--location",
            "--registry=http://x",
            "--registry",
            "--config.registry=x",
            "--config.foo",
            "--config",
            "--store-dir=s",
            "--store-dir",
            "--global-folder=g",
            "--global-folder",
        ] {
            for manager in ["npm", "pnpm", "yarn"] {
                assert_eq!(refused(manager, &["test", flag]), Refusal::Flag, "{flag}");
            }
        }
        // Short bundles with `g` are global installs in disguise.
        for flag in ["-gD", "-Dg", "-gw", "-rg"] {
            assert_eq!(refused("npm", &["install", flag]), Refusal::Flag, "{flag}");
            assert_eq!(refused("pnpm", &["test", flag]), Refusal::Flag, "{flag}");
        }
        // Near misses and what follows `--` stay fine.
        pass("npm", &["install", "-D"]);
        pass("npm", &["test", "-j4"]);
        pass("npm", &["test", "--legacy-peer-deps"]);
        pass("npm", &["test", "--", "--location=x", "-gD"]);
    }

    #[test]
    fn every_equals_tail_is_judged_as_a_path() {
        for bad in [
            "-ldflags=-extld=/bin/sh",
            "--foo=-o=/etc/x",
            "a=b=../c",
            "a=b=c=/x",
            "--out=-o/etc/x",
        ] {
            assert_eq!(refused("cargo", &["test", bad]), Refusal::Path, "{bad}");
            assert_eq!(refused("go", &["build", bad]), Refusal::Path, "{bad}");
        }
        pass("cargo", &["test", "a=b=c"]);
        pass("cargo", &["test", "--features=a=b"]);
    }

    #[test]
    fn pytest_refuses_an_argsfile() {
        assert_eq!(refused("pytest", &["@args.txt"]), Refusal::Flag);
        assert_eq!(refused("pytest", &["-x", "@args.txt"]), Refusal::Flag);
        // argparse expands `@file` after `--` too.
        assert_eq!(refused("pytest", &["--", "@args.txt"]), Refusal::Flag);
        pass("pytest", &["tests/test_a.py::test[a@b]"]);
        pass("pytest", &["-k", "a@b"]);
    }

    #[test]
    fn go_takes_four_verbs() {
        for verb in ["test", "vet", "build", "run"] {
            pass("go", &[verb]);
        }
        pass("go", &["test", "./...", "-run", "TestA", "-count=1"]);
        for verb in ["mod", "install", "get", "generate", "work", "env", "tool"] {
            assert_eq!(refused("go", &[verb]), Refusal::Verb, "{verb}");
        }
        assert_eq!(refused("go", &[]), Refusal::Verb);
    }

    #[test]
    fn go_denied_flags_with_one_or_two_dashes() {
        for flag in [
            "-modfile=x.mod",
            "--modfile=x.mod",
            "-modfile",
            "-toolexec=wrap",
            "-toolexec",
            "-exec=wrap",
            "--exec",
            "-overlay=o.json",
            "-vettool=./v",
            "-C",
        ] {
            assert_eq!(refused("go", &["test", flag]), Refusal::Flag, "{flag}");
        }
        // What follows `-args` or `--` is the test binary's.
        pass("go", &["test", "-args", "-exec", "x"]);
        pass("go", &["test", "--", "-modfile=x"]);
        // `-exec` is not `-executable` and `-run` is not `-r`.
        pass("go", &["test", "-executable-name-x"]);
        pass("go", &["test", "-run", "TestExec"]);
    }

    // Spec (Argv input): the argument reaches the program as one literal
    // element; there is no shell, so `;` is data, not refused.
    #[test]
    fn shell_text_is_data() {
        pass("cargo", &["test", "x; rm -rf ~"]);
        pass("cargo", &["test", "$(id)"]);
        pass("pytest", &["-k", "a && b"]);
    }

    #[test]
    fn a_refusal_text_never_echoes_the_call() {
        for why in [
            Refusal::Program,
            Refusal::Verb,
            Refusal::Flag,
            Refusal::Path,
        ] {
            assert!(!why.text().is_empty());
        }
        assert_eq!(Refusal::Path.text(), "path not available");
        assert_ne!(Refusal::Program.text(), Refusal::Verb.text());
        assert_ne!(Refusal::Flag.text(), Refusal::Verb.text());
    }

    // Spec (Refusals before start) and design: an absolute path or `..` in
    // ANY argument is refused, whatever the program and flag.
    #[test]
    fn an_absolute_path_or_dot_dot_in_any_argument_is_refused() {
        for bad in [
            "/etc/passwd",
            "..",
            "../x",
            "a/../b",
            "a/..",
            "..\\x",
            "a\\..\\b",
            "\\\\host\\share",
            "C:\\x",
            "c:/x",
            "--out=/tmp/x",
            "--out=../x",
            "-o/tmp/x",
            "-o..",
        ] {
            assert_eq!(refused("cargo", &["test", bad]), Refusal::Path, "{bad}");
            assert_eq!(refused("pytest", &[bad]), Refusal::Path, "{bad}");
            assert_eq!(refused("go", &["build", bad]), Refusal::Path, "{bad}");
        }
        // Also after `--`, where flags are not judged.
        assert_eq!(refused("cargo", &["run", "--", "/etc/x"]), Refusal::Path);
        assert_eq!(refused("npm", &["run", "b", "--", "../x"]), Refusal::Path);
    }

    #[test]
    fn relative_paths_and_dotted_names_pass() {
        for ok in [
            "src/lib.rs",
            "./src",
            ".",
            "a..b",
            "...",
            "..x",
            "x..",
            "tests/test_a.py::test_one[..a]",
            "--features=a,b",
            "-j4",
            "-",
            "-é",
            "C:",
            "ab:/y",
        ] {
            pass("cargo", &["test", ok]);
            pass("pytest", &[ok]);
        }
    }

    fn req(json: serde_json::Value) -> Result<RunRequest, RunError> {
        parse_run(&json, &scripts())
    }

    // Spec (Process rules): `timeout_secs` defaults to 120 and is clamped at
    // 600 (design D2 pins clamping, never refusing).
    #[test]
    fn the_timeout_defaults_to_120_and_clamps_at_600() {
        use std::time::Duration;
        let run = |extra: serde_json::Value| {
            let mut args = serde_json::json!({"program": "cargo", "args": ["test"]});
            args.as_object_mut()
                .unwrap()
                .extend(extra.as_object().unwrap().clone());
            req(args).unwrap().timeout
        };
        assert_eq!(run(serde_json::json!({})), Duration::from_secs(120));
        assert_eq!(
            run(serde_json::json!({"timeout_secs": 600})),
            Duration::from_secs(600)
        );
        assert_eq!(
            run(serde_json::json!({"timeout_secs": 601})),
            Duration::from_secs(600)
        );
        assert_eq!(
            run(serde_json::json!({"timeout_secs": 900})),
            Duration::from_secs(600)
        );
        assert_eq!(
            run(serde_json::json!({"timeout_secs": 45})),
            Duration::from_secs(45)
        );
        assert_eq!(
            run(serde_json::json!({"timeout_secs": 0})),
            Duration::from_secs(1)
        );
        assert_eq!(
            run(serde_json::json!({"timeout_secs": -5})),
            Duration::from_secs(1)
        );
        assert_eq!(
            run(serde_json::json!({"timeout_secs": 18446744073709551615u64})),
            Duration::from_secs(600)
        );
    }

    #[test]
    fn a_valid_call_parses_into_a_request() {
        let run = req(serde_json::json!({
            "program": "cargo",
            "args": ["test", "--lib"],
            "cwd": "crates/core",
        }))
        .unwrap();
        assert_eq!(run.program, Program::Cargo);
        assert_eq!(run.args, argv(&["test", "--lib"]));
        assert_eq!(run.cwd, std::path::PathBuf::from("crates/core"));
        // No cwd is the root; `.` is the root too.
        let root = req(serde_json::json!({"program": "go", "args": ["vet"]})).unwrap();
        assert_eq!(root.cwd, std::path::PathBuf::new());
        let dot = req(serde_json::json!({"program": "go", "args": ["vet"], "cwd": "./"})).unwrap();
        assert_eq!(dot.cwd, std::path::PathBuf::new());
    }

    // Spec (Refusals before start, "Cwd outside the root"): refused with
    // `path not available`.
    #[test]
    fn a_cwd_outside_the_root_is_path_not_available() {
        for cwd in [
            "..",
            "/tmp",
            "../x",
            "a/../..",
            "~",
            "~/x",
            "",
            "  ",
            "a\u{200b}b",
        ] {
            let why = req(serde_json::json!({"program": "cargo", "args": ["test"], "cwd": cwd}));
            assert_eq!(why, Err(RunError::Refused(Refusal::Path)), "{cwd:?}");
        }
        assert_eq!(Refusal::Path.text(), "path not available");
    }

    #[test]
    fn a_bad_call_is_refused_with_the_table_reason_or_the_shape_error() {
        use super::super::ToolError;
        assert_eq!(
            req(serde_json::json!({"program": "make", "args": []})),
            Err(RunError::Refused(Refusal::Program))
        );
        assert_eq!(
            req(serde_json::json!({"program": "cargo", "args": ["install"]})),
            Err(RunError::Refused(Refusal::Verb))
        );
        assert_eq!(
            req(serde_json::json!({"program": "cargo", "args": ["test", "/x"]})),
            Err(RunError::Refused(Refusal::Path))
        );
        // No shell string: `args` must be a list.
        assert_eq!(
            req(serde_json::json!({"program": "cargo", "args": "test"})),
            Err(RunError::Shape(ToolError::WrongType))
        );
        assert_eq!(
            req(serde_json::json!({"program": "cargo"})),
            Err(RunError::Shape(ToolError::MissingArgument))
        );
        assert_eq!(
            req(serde_json::json!({"args": []})),
            Err(RunError::Shape(ToolError::MissingArgument))
        );
        assert_eq!(
            req(serde_json::json!({"program": "cargo", "args": ["test"], "shell": true})),
            Err(RunError::Shape(ToolError::ExtraArgument))
        );
        assert_eq!(
            req(serde_json::json!(["cargo"])),
            Err(RunError::Shape(ToolError::NotAnObject))
        );
        let many: Vec<String> = (0..65).map(|n| n.to_string()).collect();
        assert_eq!(
            req(serde_json::json!({"program": "cargo", "args": many})),
            Err(RunError::Shape(ToolError::TooLong))
        );
    }
}
