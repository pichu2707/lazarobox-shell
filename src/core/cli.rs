//! Classifies the command line of the plain start (program name excluded),
//! after the subcommands have been dispatched by `main`.

/// What the leading arguments ask for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Action {
    /// Start the TUI.
    Run,
    Version,
    Help,
    /// An unknown flag, verbatim.
    UnknownOption(String),
    /// An unknown first non-flag argument, verbatim.
    UnknownCommand(String),
    /// A known subcommand that is not the first argument.
    MisplacedCommand(String),
}

/// The text of `--help`.
pub const USAGE: &str = "\
usage: lazarobox-shell [--force] [--dir <folder> | --dir=<folder>]
       lazarobox-shell install-desktop [--terminal <cmd>] [--dir <folder>] [--uninstall]
       lazarobox-shell --version | --help

  --force        start even inside a lazarobox pane
  --dir <folder> start in this folder (also --dir=<folder>)
  install-desktop  must be the first argument
  -V, --version  print the version
  -h, --help     print this help";

const KNOWN: [&str; 4] = ["install-desktop", "__jail-exec", "__mcp-serve", "__watch"];

/// The one-line stderr message of a rejected command line.
pub fn rejection(action: &Action) -> Option<String> {
    match action {
        Action::UnknownOption(x) => Some(format!(
            "lazarobox-shell: unknown option '{x}' (try --help)"
        )),
        Action::UnknownCommand(x) => Some(format!(
            "lazarobox-shell: unknown command '{x}' (try --help)"
        )),
        Action::MisplacedCommand(x) => Some(format!(
            "lazarobox-shell: '{x}' must be the first argument (try --help)"
        )),
        _ => None,
    }
}

/// The first decisive argument wins; `--dir` takes the next argument as its
/// value unless it starts with `-` (as `start::parse_args` does).
pub fn classify(argv: &[String]) -> Action {
    let mut it = argv.iter().peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--version" | "-V" => return Action::Version,
            "--help" | "-h" => return Action::Help,
            "--force" => {}
            "--dir" => {
                it.next_if(|v| !v.starts_with('-'));
            }
            a if a.starts_with("--dir=") => {}
            a if a.starts_with('-') => return Action::UnknownOption(a.to_owned()),
            a if KNOWN.contains(&a) => return Action::MisplacedCommand(a.to_owned()),
            a => return Action::UnknownCommand(a.to_owned()),
        }
    }
    Action::Run
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(list: &[&str]) -> Action {
        classify(&list.iter().map(|a| a.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn known_flags_run_the_tui() {
        assert_eq!(c(&[]), Action::Run);
        assert_eq!(c(&["--force"]), Action::Run);
        assert_eq!(c(&["--dir", "/a", "--force"]), Action::Run);
        assert_eq!(c(&["--dir=/a"]), Action::Run);
    }

    #[test]
    fn version_and_help_in_both_spellings() {
        assert_eq!(c(&["--version"]), Action::Version);
        assert_eq!(c(&["-V"]), Action::Version);
        assert_eq!(c(&["--help"]), Action::Help);
        assert_eq!(c(&["-h"]), Action::Help);
        assert_eq!(c(&["--force", "--help"]), Action::Help);
    }

    #[test]
    fn the_dir_value_never_starts_with_a_dash() {
        assert_eq!(c(&["--dir", "--version"]), Action::Version);
        assert_eq!(
            c(&["--dir", "--bogus"]),
            Action::UnknownOption("--bogus".into())
        );
        assert_eq!(c(&["--dir", "x", "-h"]), Action::Help);
        assert_eq!(c(&["--dir=", "--force"]), Action::Run);
        assert_eq!(c(&["--dir=-x"]), Action::Run);
    }

    #[test]
    fn odd_tokens_are_rejected() {
        assert_eq!(c(&["--"]), Action::UnknownOption("--".into()));
        assert_eq!(c(&["-"]), Action::UnknownOption("-".into()));
        assert_eq!(c(&[""]), Action::UnknownCommand(String::new()));
    }

    #[test]
    fn a_known_subcommand_must_come_first() {
        for cmd in ["install-desktop", "__jail-exec", "__mcp-serve", "__watch"] {
            let a = c(&["--force", cmd]);
            assert_eq!(a, Action::MisplacedCommand(cmd.into()));
            assert_eq!(
                rejection(&a).unwrap(),
                format!("lazarobox-shell: '{cmd}' must be the first argument (try --help)")
            );
        }
        assert_eq!(
            c(&["--dir", "x", "install-desktop"]),
            Action::MisplacedCommand("install-desktop".into())
        );
    }

    #[test]
    fn the_known_list_matches_the_dispatched_subcommands() {
        assert!(KNOWN.contains(&crate::runtime::watch::SUBCOMMAND));
        assert!(KNOWN.contains(&crate::runtime::mcp::shim::SUBCOMMAND));
        assert!(KNOWN.contains(&crate::core::ai::jail::SUBCOMMAND));
        assert!(KNOWN.contains(&crate::core::desktop::SUBCOMMAND));
    }

    #[test]
    fn unknown_input_is_rejected_with_one_line() {
        let opt = c(&["--forse"]);
        assert_eq!(opt, Action::UnknownOption("--forse".into()));
        assert_eq!(
            rejection(&opt).unwrap(),
            "lazarobox-shell: unknown option '--forse' (try --help)"
        );
        let cmd = c(&["bogus"]);
        assert_eq!(cmd, Action::UnknownCommand("bogus".into()));
        assert!(rejection(&cmd).unwrap().contains("unknown command 'bogus'"));
        assert_eq!(rejection(&Action::Run), None);
    }
}
