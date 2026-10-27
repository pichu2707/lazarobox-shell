//! A static stand-in for a recording sandbox. No in-process hook can see a
//! spawn or a socket, so the non-test code of the tool files is tokenized and
//! checked for the names of the APIs that start a process, open a socket,
//! create a file or write one. The ban list lives here, in a file that is not
//! itself audited, so the check never matches its own words.
//!
//! What it sees: identifiers and `::` paths. Comments and string literals are
//! dropped by the tokenizer. Every item under `#[cfg(test)]` is dropped too,
//! wherever it sits (a `mod tests`, a nested module, a field, a statement),
//! up to its closing brace, or to the next `;` or `,` outside any bracket.
//! It also flags `extern` blocks, `use libc::*`, an aliased `std::fs`, and a
//! `libc::syscall` whose number is not the one the file is allowed.
//!
//! Every `.rs` file under `src/runtime/tools/` is scanned, whatever its name.
//! The bans come in two sets. `PROCESS_NET` (spawn, sockets, `extern`) holds
//! for every file. `FS_WRITE` (create, write, remove, rename) holds for every
//! file except those in `MAY_WRITE`, and even those have a `libc` allow-list of
//! their own. `runner.rs` alone (`MAY_SPAWN`) may start a process, and only
//! that: no socket, no write, and a `libc` list of its own. Naming the `write` module is allowed only in `MAY_CALL_WRITE`.
//!
//! A best-effort tripwire against accidental use, not a guarantee. It cannot
//! see a macro that builds a call from tokens, a call made through a path the
//! ban list does not name, or code reached through another crate.

/// Identifiers that start a process, open a socket or declare foreign code.
/// They hold for every file, the write module included.
const PROCESS_NET: &[&str] = &[
    "Command",
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "UnixStream",
    "UnixListener",
    "UnixDatagram",
    "extern",
];

/// A segment that is banned when it is part of a longer path (`PROCESS_NET`).
const BANNED_IN_PATH: &[&str] = &["process", "net"];

/// Identifiers that create, write, resize or remove (`FS_WRITE`).
const FS_WRITE: &[&str] = &[
    "OpenOptions",
    "O_TRUNC",
    "O_CREAT",
    "O_WRONLY",
    "O_RDWR",
    "O_APPEND",
    "O_TMPFILE",
    "create_dir",
    "create_dir_all",
    "create_new",
    "remove_file",
    "remove_dir",
    "remove_dir_all",
    "hard_link",
    "set_permissions",
    "set_len",
    "write_all",
    "write_vectored",
];

/// `fs::<name>` calls that change the tree (`FS_WRITE`).
const BANNED_FS: &[&str] = &["write", "copy", "rename", "symlink", "truncate"];

/// The files (relative to `src/runtime/tools/`) that may use `FS_WRITE` names.
const MAY_WRITE: &[&str] = &["write.rs", "scratch.rs"];

/// The files that may start a process: the spawn core of `run_command`. The
/// socket bans, the write bans and the `libc` list still hold for them.
const MAY_SPAWN: &[&str] = &["runner.rs"];

/// The files that may name the `write` module. `apply.rs` drives it through
/// `WriteOps`, and has no write API of its own.
const MAY_CALL_WRITE: &[&str] = &["write.rs", "mod.rs", "audit.rs", "apply.rs"];

/// The files that may name the `apply` module, the one entry to a disk write.
/// `stage.rs` is the store that applies a proposed batch; later wiring slices
/// add the file that calls the store.
const MAY_CALL_APPLY: &[&str] = &["apply.rs", "mod.rs", "audit.rs", "stage.rs"];

/// The only `libc::` names the read side may use.
const LIBC_ALLOWED: &[&str] = &[
    "openat",
    "syscall",
    "SYS_openat2",
    "AT_FDCWD",
    "O_RDONLY",
    "O_PATH",
    "O_DIRECTORY",
    "O_CLOEXEC",
    "O_NOFOLLOW",
    "O_NONBLOCK",
    "O_NOCTTY",
    "fstat",
    "close",
    "getrandom",
    // macOS reads: the real path of a descriptor and a folder listing.
    "fcntl",
    "F_GETPATH",
    "MAXPATHLEN",
    "F_DUPFD_CLOEXEC",
    "dirfd",
    "fstatat",
    "stat",
    "AT_SYMLINK_NOFOLLOW",
    "S_IFMT",
    "S_IFDIR",
    "S_IFREG",
    "S_IFLNK",
    "DT_UNKNOWN",
    "fdopendir",
    "rewinddir",
    "readdir",
    "closedir",
    "DIR",
    "DT_DIR",
    "DT_REG",
    "DT_LNK",
    "__error",
    "ENOSYS",
    "EPERM",
    "EINVAL",
    "E2BIG",
];

/// The only `libc::` names `write.rs` may use: exactly what it calls.
const WRITE_LIBC_ALLOWED: &[&str] = &[
    "openat",
    "mkdirat",
    "unlinkat",
    "renameat",
    "fsync",
    "syscall",
    "SYS_renameat2",
    "RENAME_NOREPLACE",
    "getrandom",
    "GRND_NONBLOCK",
    "getpid",
    "O_WRONLY",
    "O_CREAT",
    "O_EXCL",
    "O_NOFOLLOW",
    "O_CLOEXEC",
    "AT_REMOVEDIR",
    "EEXIST",
    "EINTR",
    "c_uint",
    "c_long",
];

/// The only `libc::` names `runner.rs` may use.
const SPAWN_LIBC_ALLOWED: &[&str] = &["pid_t", "ETXTBSY"];

/// What one file may do.
pub struct Allow {
    /// Whether `FS_WRITE` names are allowed.
    fs_write: bool,
    /// Whether a process may be started.
    spawn: bool,
    /// The `libc::` names.
    libc: &'static [&'static str],
    /// The only name `libc::syscall` may be called with.
    syscall: &'static str,
}

/// The read side: no write, `openat2` only.
pub const READ_SIDE: Allow = Allow {
    spawn: false,
    fs_write: false,
    libc: LIBC_ALLOWED,
    syscall: "SYS_openat2",
};

/// The write module: may write, with its own `libc` list.
pub const WRITE_SIDE: Allow = Allow {
    spawn: false,
    fs_write: true,
    libc: WRITE_LIBC_ALLOWED,
    syscall: "SYS_renameat2",
};

/// The spawn core: may start a process, nothing else.
pub const SPAWN_SIDE: Allow = Allow {
    spawn: true,
    fs_write: false,
    libc: SPAWN_LIBC_ALLOWED,
    syscall: "",
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Path,
    Punct(char),
}

/// Identifiers, `::` and single punctuation, with comments, strings and
/// character literals gone.
fn tokenize(src: &str) -> Vec<Tok> {
    let b: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        if c == '/' && next == Some('/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            let mut depth = 0;
            while i < b.len() {
                if b[i] == '/' && b.get(i + 1) == Some(&'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == '*' && b.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if c == '"' {
            i = skip_string(&b, i + 1, 0);
        } else if (c == 'r' || c == 'b') && is_raw_string(&b, i) {
            let mut j = i + 1;
            if b[j] == 'r' {
                j += 1;
            }
            let hashes = b[j..].iter().take_while(|&&h| h == '#').count();
            i = skip_string(&b, j + hashes + 1, hashes + 1);
        } else if c == 'b' && next == Some('"') {
            i = skip_string(&b, i + 2, 0);
        } else if c == '\'' {
            // A character literal is one char or one escape; a lifetime is not.
            if next == Some('\\') {
                // Skip the escaped char, then run to the closing quote.
                let mut j = i + 3;
                while j < b.len() && b[j] != '\'' {
                    j += 1;
                }
                i = j + 1;
            } else if b.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(b[start..i].iter().collect()));
        } else if c == ':' && next == Some(':') {
            out.push(Tok::Path);
            i += 2;
        } else {
            if !c.is_whitespace() {
                out.push(Tok::Punct(c));
            }
            i += 1;
        }
    }
    out
}

/// Whether `r"`, `r#"`, `br"` or `br#"` starts at `i`.
fn is_raw_string(b: &[char], i: usize) -> bool {
    let mut j = i;
    if b[j] == 'b' {
        j += 1;
    }
    if b.get(j) != Some(&'r') {
        return false;
    }
    // Not the tail of a longer identifier.
    if i > 0 && (b[i - 1].is_alphanumeric() || b[i - 1] == '_') {
        return false;
    }
    j += 1;
    while b.get(j) == Some(&'#') {
        j += 1;
    }
    b.get(j) == Some(&'"')
}

/// The index after the closing quote. `hashes` is 0 for an ordinary string,
/// or one more than the number of `#` of a raw one.
fn skip_string(b: &[char], mut i: usize, hashes: usize) -> usize {
    while i < b.len() {
        if hashes == 0 && b[i] == '\\' {
            i += 2;
        } else if b[i] == '"' {
            let marks = hashes.saturating_sub(1);
            if b[i + 1..].iter().take(marks).filter(|&&h| h == '#').count() == marks {
                return i + 1 + marks;
            }
            i += 1;
        } else {
            i += 1;
        }
    }
    b.len()
}

/// The tokens that remain after every `#[cfg(test)]` item is dropped.
fn non_test_tokens(src: &str) -> Vec<Tok> {
    let toks = tokenize(src);
    let mut out = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        if is_cfg_test(&toks, i) {
            i = skip_item(&toks, i);
        } else {
            out.push(toks[i].clone());
            i += 1;
        }
    }
    out
}

/// `# [ cfg ( test ) ]` at `i`.
fn is_cfg_test(t: &[Tok], i: usize) -> bool {
    let want = [
        Tok::Punct('#'),
        Tok::Punct('['),
        Tok::Ident("cfg".into()),
        Tok::Punct('('),
        Tok::Ident("test".into()),
        Tok::Punct(')'),
        Tok::Punct(']'),
    ];
    t.get(i..i + want.len()) == Some(&want[..])
}

/// The index after the item that starts at the attribute at `i`: after the
/// matching `}` of its first brace block, or after a `;` or `,` that is
/// outside any bracket. Further attributes before the item are skipped too.
fn skip_item(t: &[Tok], mut i: usize) -> usize {
    let mut depth = 0usize;
    let mut in_attr = false;
    while i < t.len() {
        match &t[i] {
            Tok::Punct('#') if depth == 0 && t.get(i + 1) == Some(&Tok::Punct('[')) => {
                in_attr = true;
            }
            Tok::Punct('(' | '[') => depth += 1,
            Tok::Punct(')' | ']') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    in_attr = false;
                }
            }
            Tok::Punct('{') if depth == 0 && !in_attr => {
                let mut braces = 0usize;
                while i < t.len() {
                    match t[i] {
                        Tok::Punct('{') => braces += 1,
                        Tok::Punct('}') => {
                            braces -= 1;
                            if braces == 0 {
                                return i + 1;
                            }
                        }
                        _ => {}
                    }
                    i += 1;
                }
                return i;
            }
            Tok::Punct(';' | ',') if depth == 0 && !in_attr => return i + 1,
            // The enclosing block closes: the item was its last one.
            Tok::Punct('}') if depth == 0 => return i,
            _ => {}
        }
        i += 1;
    }
    i
}

/// The names that carry the approval mode. The model must never be able to
/// reach them, so no runtime code may name them (design D19).
const MODE_NAMES: &[&str] = &[
    "ApprovalMode",
    "set_approval_mode",
    "approval_mode",
    "set_approval",
];

/// Every use of a mode name in the non-test code of `src`.
pub fn mode_violations(src: &str) -> Vec<String> {
    non_test_tokens(src)
        .into_iter()
        .filter_map(|tok| match tok {
            Tok::Ident(name) if MODE_NAMES.contains(&name.as_str()) => Some(format!("`{name}`")),
            _ => None,
        })
        .collect()
}

/// Every banned use in the non-test code of `src` under the read-side rules.
pub fn violations(src: &str) -> Vec<String> {
    scan(src, &READ_SIDE)
}

/// Every banned use in the non-test code of `src`, one line each.
pub fn scan(src: &str, allow: &Allow) -> Vec<String> {
    let t = non_test_tokens(src);
    let mut found = Vec::new();
    let ident = |k: usize| match t.get(k) {
        Some(Tok::Ident(name)) => Some(name.as_str()),
        _ => None,
    };
    for (k, tok) in t.iter().enumerate() {
        let Tok::Ident(name) = tok else { continue };
        let name = name.as_str();
        let after_path = k > 0 && t[k - 1] == Tok::Path;
        let before_path = t.get(k + 1) == Some(&Tok::Path);
        // Starting a process is the one thing a spawn file earns.
        let spawning = allow.spawn && name == "Command";
        if (PROCESS_NET.contains(&name) && !spawning)
            || (!allow.fs_write && FS_WRITE.contains(&name))
        {
            found.push(format!("`{name}`"));
        }
        // `tokio::net::unix::pipe` is the async end of an anonymous pipe, not
        // a socket: a spawn file may name it.
        let pipe = allow.spawn
            && name == "net"
            && t.get(k + 1) == Some(&Tok::Path)
            && ident(k + 2) == Some("unix")
            && t.get(k + 3) == Some(&Tok::Path)
            && ident(k + 4) == Some("pipe");
        if BANNED_IN_PATH.contains(&name)
            && !(allow.spawn && name == "process")
            && !pipe
            && (after_path || before_path)
        {
            found.push(format!("path segment `{name}`"));
        }
        if let (true, true, Some(next), false) =
            (name == "fs", before_path, ident(k + 2), allow.fs_write)
            && (BANNED_FS.contains(&next) || next.starts_with("remove_"))
        {
            found.push(format!("`fs::{next}`"));
        }
        // `File::create`, `File::create_new`, `File::options`.
        if name == "File"
            && !allow.fs_write
            && before_path
            && matches!(ident(k + 2), Some("create" | "create_new" | "options"))
        {
            found.push(format!("`File::{}`", ident(k + 2).unwrap_or_default()));
        }
        // `use std::fs as x` hides every later `x::write`.
        if name == "fs" && !allow.fs_write && ident(k + 1) == Some("as") {
            found.push("`fs` aliased".to_string());
        }
        // `use std::fs::{write, self as x}`: look inside the group.
        if name == "fs" && !allow.fs_write && before_path && t.get(k + 2) == Some(&Tok::Punct('{'))
        {
            let group = t[k + 3..].iter().take_while(|x| **x != Tok::Punct('}'));
            let mut prev_as = false;
            for tok in group {
                if let Tok::Ident(n) = tok {
                    if prev_as {
                        found.push("`fs` aliased in a group".to_string());
                    } else if BANNED_FS.contains(&n.as_str()) || n.starts_with("remove_") {
                        found.push(format!("`fs::{{{n}}}`"));
                    }
                    prev_as = n == "as";
                }
            }
        }
        if name == "libc" && before_path && t.get(k + 2) == Some(&Tok::Punct('*')) {
            found.push("`use libc::*`".to_string());
        }
        // Only the file's own syscall may be called through `libc::syscall`.
        if name == "libc" && before_path && ident(k + 2) == Some("syscall") {
            let arg = (t.get(k + 3), ident(k + 4), t.get(k + 5), ident(k + 6));
            let ok = matches!(
                arg,
                (
                    Some(Tok::Punct('(')),
                    Some("libc"),
                    Some(Tok::Path),
                    Some(number)
                ) if number == allow.syscall
            );
            if !ok && t.get(k + 3) == Some(&Tok::Punct('(')) {
                found.push(format!(
                    "`libc::syscall` with a number other than {}",
                    allow.syscall
                ));
            }
        }
        if name == "libc" && before_path {
            match t.get(k + 2) {
                Some(Tok::Ident(next)) if !allow.libc.contains(&next.as_str()) => {
                    found.push(format!("`libc::{next}`"));
                }
                // `libc::{a, b}`: every name inside the braces.
                Some(Tok::Punct('{')) => {
                    for tok in t[k + 3..].iter().take_while(|x| **x != Tok::Punct('}')) {
                        if let Tok::Ident(n) = tok
                            && !allow.libc.contains(&n.as_str())
                        {
                            found.push(format!("`libc::{n}`"));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    found
}

/// Every reference to the `write` module in the non-test code of `src`: the
/// name beside a `::`, or anywhere inside a `use` item (a group, an alias).
/// `write!`, `Write` and `.write(` are other things and do not match.
pub fn write_refs(src: &str) -> Vec<String> {
    module_refs(src, "write")
}

/// The same for the `apply` module: only the files in `MAY_CALL_APPLY` may
/// name it. A method `.apply(` or a `fn apply` is another thing.
pub fn apply_refs(src: &str) -> Vec<String> {
    module_refs(src, "apply")
}

fn module_refs(src: &str, module: &str) -> Vec<String> {
    let t = non_test_tokens(src);
    let mut in_use = false;
    let mut found = Vec::new();
    for (k, tok) in t.iter().enumerate() {
        match tok {
            Tok::Ident(name) if name == "use" => in_use = true,
            Tok::Punct(';') => in_use = false,
            Tok::Ident(name) if name == module => {
                let beside = (k > 0 && t[k - 1] == Tok::Path) || t.get(k + 1) == Some(&Tok::Path);
                if in_use || beside {
                    found.push(format!("a reference to the `{module}` module"));
                }
            }
            _ => {}
        }
    }
    found
}

/// Every problem of the file `rel` (relative to the tools folder, `/`
/// separated) under the rules its name earns.
pub fn file_problems(rel: &str, src: &str) -> Vec<String> {
    let allow = if MAY_WRITE.contains(&rel) {
        &WRITE_SIDE
    } else if MAY_SPAWN.contains(&rel) {
        &SPAWN_SIDE
    } else {
        &READ_SIDE
    };
    let mut found = scan(src, allow);
    if !MAY_CALL_WRITE.contains(&rel) {
        found.extend(write_refs(src));
    }
    if !MAY_CALL_APPLY.contains(&rel) {
        found.extend(apply_refs(src));
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The out-of-line modules that are declared `#[cfg(test)]` in `src`.
    fn test_only_mods(src: &str) -> Vec<String> {
        let t = tokenize(src);
        let mut out = Vec::new();
        for i in 0..t.len() {
            if !is_cfg_test(&t, i) {
                continue;
            }
            let mut k = i + 7;
            if t.get(k) == Some(&Tok::Ident("pub".into())) {
                k += 1;
            }
            if let (Some(Tok::Ident(kw)), Some(Tok::Ident(name)), Some(Tok::Punct(';'))) =
                (t.get(k), t.get(k + 1), t.get(k + 2))
                && kw == "mod"
            {
                out.push(name.clone());
            }
        }
        out
    }

    /// Every file under `root` with its problems, and how many files were
    /// walked and how many were skipped as test-only modules (they never ship).
    fn tree_problems(root: &std::path::Path) -> (usize, usize, Vec<String>) {
        let mut files = Vec::new();
        rust_files(root, &mut files);
        files.sort();
        let mut test_only = Vec::new();
        for file in &files {
            let src = std::fs::read_to_string(file).unwrap();
            let stem = file.file_stem().unwrap().to_string_lossy().into_owned();
            let base = if stem == "mod" {
                file.parent().unwrap().to_path_buf()
            } else {
                file.with_extension("")
            };
            test_only.extend(
                test_only_mods(&src)
                    .into_iter()
                    .map(|name| base.join(format!("{name}.rs"))),
            );
        }
        let mut found = Vec::new();
        for file in &files {
            if test_only.contains(file) {
                continue;
            }
            let rel = file.strip_prefix(root).unwrap().to_string_lossy();
            let src = std::fs::read_to_string(file).unwrap();
            // A tokenizer gone wrong would pass everything: it must see code.
            assert!(non_test_tokens(&src).len() > 5, "{rel} yields no tokens");
            found.extend(
                file_problems(&rel, &src)
                    .into_iter()
                    .map(|problem| format!("{rel}: {problem}")),
            );
        }
        (files.len(), test_only.len(), found)
    }

    #[test]
    fn every_tool_file_starts_no_process_opens_no_socket_and_writes_only_in_write_rs() {
        let tools = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime/tools");
        let (walked, skipped, found) = tree_problems(&tools);
        assert!(walked >= 11, "the walk found {walked} files");
        // `host/tests.rs`, `host/external_tests.rs` and `audit.rs`.
        assert!(skipped >= 3, "{skipped} test-only files skipped");
        assert_eq!(found, Vec::<String>::new());
    }

    fn planted(files: &[(&str, &str)]) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        for (rel, src) in files {
            let path = dir.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, src).unwrap();
        }
        tree_problems(dir.path()).2
    }

    #[test]
    fn a_planted_violation_in_any_file_of_the_tree_is_caught() {
        let clean = "fn f() { let _ = 1 + 2 + 3; }";
        for (rel, src) in [
            ("search.rs", "fn f() { std::fs::write(p, b) }"),
            ("host/deep/new.rs", "fn f() { std::fs::write(p, b) }"),
            ("write.rs", "fn f() { Command::new(\"ls\") }"),
            ("write.rs", "fn f() { libc::unlink(p) }"),
            ("write.rs", "fn f() { libc::open(p, 0) }"),
            ("write.rs", "fn f() { libc::syscall(libc::SYS_openat2, a) }"),
            ("write.rs", "use std::net::TcpStream;"),
            (
                "host.rs",
                "use crate::runtime::tools::{write::create_file};",
            ),
            ("host.rs", "use crate::runtime::tools::{self, write};"),
            ("host.rs", "fn f() { super::write::make_dir(r, p) }"),
            ("fs.rs", "use crate::runtime::tools::write as w;"),
            ("fs.rs", "use super::apply::{apply, Disk};"),
            (
                "fs.rs",
                "fn f() { crate::runtime::tools::apply::apply(o, e) }",
            ),
            (
                "host/x.rs",
                "fn f() { crate::runtime::tools::write::make_dir(r, p) }",
            ),
        ] {
            let found = planted(&[(rel, src), ("ok.rs", clean)]);
            assert!(!found.is_empty(), "{rel}: {src}");
            assert!(found.iter().all(|line| line.starts_with(rel)), "{found:?}");
        }
    }

    #[test]
    fn apply_may_name_the_write_module_but_never_a_write_api_itself() {
        let tools = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime/tools");
        let src = std::fs::read_to_string(tools.join("apply.rs")).unwrap();
        // It is on the list for a reason: it does reference `write`.
        assert!(!write_refs(&src).is_empty());
        assert_eq!(file_problems("apply.rs", &src), Vec::<String>::new());
        // ...and it earns nothing else: every write token is still flagged.
        for bad in [
            "fn f() { std::fs::write(p, b) }",
            "fn f() { std::fs::remove_file(p) }",
            "fn f() { libc::unlinkat(a, p, 0) }",
            "fn f() { let _ = OpenOptions::new(); }",
        ] {
            assert!(!file_problems("apply.rs", bad).is_empty(), "{bad}");
        }
        let named = "use super::write::{self, WriteError};";
        assert_eq!(planted(&[("apply.rs", named)]), Vec::<String>::new());
        assert!(!planted(&[("host/apply.rs", named)]).is_empty());
    }

    #[test]
    fn stage_may_name_the_apply_module_but_holds_no_write_api_and_not_the_write_module() {
        let tools = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime/tools");
        let src = std::fs::read_to_string(tools.join("stage.rs")).unwrap();
        // It is on the list for a reason: it does reference `apply`.
        assert!(!apply_refs(&src).is_empty());
        assert_eq!(file_problems("stage.rs", &src), Vec::<String>::new());
        // ...and it earns nothing else: every write token is still flagged,
        // and so is the `write` module itself.
        for bad in [
            "fn f() { std::fs::write(p, b) }",
            "fn f() { std::fs::remove_file(p) }",
            "fn f() { libc::unlinkat(a, p, 0) }",
            "fn f() { let _ = OpenOptions::new(); }",
            "use super::write::create_file;",
        ] {
            assert!(!file_problems("stage.rs", bad).is_empty(), "{bad}");
        }
        let named = "use super::apply::{self, ApplyResult};";
        assert_eq!(planted(&[("stage.rs", named)]), Vec::<String>::new());
        assert!(!planted(&[("host/stage.rs", named)]).is_empty());
    }

    #[test]
    fn the_runner_may_start_a_process_and_nothing_else_and_others_may_not() {
        let spawns = "fn f() { tokio::process::Command::new(p).spawn(); libc::pid_t::MAX; \
                      tokio::net::unix::pipe::Receiver::from_file(f); }";
        assert_eq!(planted(&[("runner.rs", spawns)]), Vec::<String>::new());
        assert!(!planted(&[("host/runner.rs", spawns)]).is_empty());
        assert!(!planted(&[("search.rs", spawns)]).is_empty());
        // It earns no socket, no write, no other `libc` name, no `net` path.
        for bad in [
            "fn f() { TcpStream::connect(a) }",
            "fn f() { std::net::TcpListener::bind(a) }",
            "fn f() { tokio::net::UnixStream::connect(a) }",
            "fn f() { tokio::net::unix::SocketAddr::x() }",
            "fn f() { std::fs::write(p, b) }",
            "fn f() { let _ = OpenOptions::new(); }",
            "fn f() { libc::kill(1, 9) }",
            "fn f() { libc::killpg(1, 9) }",
        ] {
            assert!(!file_problems("runner.rs", bad).is_empty(), "{bad}");
        }
    }

    #[test]
    fn the_write_file_may_write_and_name_the_write_module_and_others_may_not() {
        let writes = "fn f() { std::fs::write(p, b); libc::unlinkat(a, p, 0); \
                      libc::syscall(libc::SYS_renameat2, a) }";
        assert_eq!(planted(&[("write.rs", writes)]), Vec::<String>::new());
        let named = "pub mod write; use super::write::make_dir;";
        assert_eq!(planted(&[("mod.rs", named)]), Vec::<String>::new());
        // The same text, one folder down, earns nothing.
        assert!(!planted(&[("host/write.rs", writes)]).is_empty());
        assert!(!planted(&[("host/mod.rs", named)]).is_empty());
    }

    #[test]
    fn other_senses_of_the_word_write_are_not_the_module() {
        for good in [
            "use std::fmt::Write; fn f(s: &mut String) { write!(s, \"x\").ok(); }",
            "fn f(s: &mut String) { writeln!(s).ok(); w.write(b); let write = 1; }",
            "use std::io::{Read, Write};",
        ] {
            assert_eq!(write_refs(good), Vec::<String>::new(), "{good}");
        }
        for good in [
            "fn apply(o: &dyn Ops) {} fn f() { x.apply(y); let apply = 1; }",
            "pub mod apply; type R = ApplyResult;",
        ] {
            assert_eq!(apply_refs(good), Vec::<String>::new(), "{good}");
        }
        assert!(!apply_refs("use a::{b, apply};").is_empty());
        assert!(!write_refs("use a::{b, write};").is_empty());
        assert!(!write_refs("fn f() { write::go() }").is_empty());
        assert_eq!(
            write_refs("#[cfg(test)] mod t { use super::write::go; }"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn test_only_files_are_found_from_their_declaration() {
        assert_eq!(
            test_only_mods(
                "#[cfg(test)]\nmod a;\n#[cfg(test)] pub mod b;\nmod c;\n#[cfg(test)] mod d { }"
            ),
            ["a", "b"]
        );
        // A file declared test-only is not scanned; a sibling is.
        let found = planted(&[
            (
                "host.rs",
                "#[cfg(test)]\nmod tests;\nfn f() { let _ = 1 + 2 + 3; }",
            ),
            ("host/tests.rs", "fn t() { std::fs::write(p, b) }"),
            ("host/other.rs", "fn t() { std::fs::write(p, b) }"),
        ]);
        assert!(!found.is_empty(), "{found:?}");
        assert!(
            found.iter().all(|f| f.starts_with("host/other.rs")),
            "{found:?}"
        );
    }

    /// Every `.rs` file under `dir`, recursively.
    fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    // Design D19: the model can never switch the approval mode, because no
    // runtime code can even name it.
    #[test]
    fn the_runtime_never_names_the_approval_mode() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![src.join("runtime.rs")];
        rust_files(&src.join("runtime"), &mut files);
        assert!(files.len() > 20, "the walk found {} files", files.len());
        for file in files {
            let text = std::fs::read_to_string(&file).unwrap();
            assert_eq!(mode_violations(&text), Vec::<String>::new(), "{file:?}");
        }
    }

    #[test]
    fn each_mode_name_is_seen_and_strings_comments_and_tests_are_not() {
        for bad in [
            "use crate::core::ai::config::ApprovalMode;",
            "fn f(c: &mut Chat) { c.set_approval_mode(m) }",
            "fn f(a: &mut AiConfig) { a.set_approval(m) }",
            "fn f(c: &Chat) { let _ = c.approval_mode(); }",
            "fn f() { let _ = crate::ApprovalMode::Run; }",
        ] {
            assert!(!mode_violations(bad).is_empty(), "{bad}");
        }
        for good in [
            "// ApprovalMode\nfn f() {}",
            "fn f() { let _ = \"set_approval_mode\"; }",
            "#[cfg(test)]\nmod tests { use ApprovalMode; }",
            "fn approval() {}",
        ] {
            assert_eq!(mode_violations(good), Vec::<String>::new(), "{good}");
        }
    }

    #[test]
    fn each_banned_use_is_seen() {
        for bad in [
            "fn f() { std::process::exit(1) }",
            "use std::net::TcpStream;",
            "fn f() { std::fs::write(p, b) }",
            "fn f() { fs::remove_dir_all(p) }",
            "fn f() { fs::copy(a, b) }",
            "fn f() { fs::rename(a, b) }",
            "fn f() { let o = OpenOptions::new(); }",
            "fn f() { libc::open(p, libc::O_TRUNC) }",
            "fn f() { libc::unlink(p) }",
            "use libc::{openat, unlink};",
            "fn f() { std::fs::create_dir(p) }",
            "fn f() { Command::new(\"ls\") }",
            "fn f() { let _ = libc::O_RDWR; }",
            "fn f() { File::create(p) }",
            "fn f() { File::create_new(p) }",
            "fn f() { File::options().write(true) }",
            "use std::fs::{read, write};",
            "use std::fs::{self as x};",
            "use std::fs as x;",
            "use std::{fs as x, io};",
            "use libc::*;",
            "extern \"C\" { fn unlink(p: *const u8) -> i32; }",
            "fn f() { libc::syscall(libc::SYS_unlink, p) }",
            "fn f() { libc::syscall(num, p) }",
            "fn f() { let _ = ['\\\\', '\"']; fs::write(a, b) }",
        ] {
            assert!(!violations(bad).is_empty(), "{bad}");
        }
    }

    #[test]
    fn allowed_uses_comments_strings_and_test_code_are_not_flagged() {
        for good in [
            "fn f() { libc::openat(a, p, libc::O_RDONLY | libc::O_NOFOLLOW) }",
            "fn f() { libc::syscall(libc::SYS_openat2, a, p, how, size) }",
            "use std::fs::{read, read_dir};",
            "fn f() { let _ = ['\\\\', '\"', '\\'', '\\u{1F600}', 'x']; }",
            "use libc::{close, fstat};",
            "// std::process::exit\nfn f() {}",
            "/* /* nested */ fs::write */ fn f() {}",
            "fn f() { let _ = \"std::process fs::write\"; }",
            "fn f() { let _ = r#\"libc::unlink \" fs::write\"#; }",
            "fn f<'a>(x: &'a str) { let _ = '\"'; let _ = std::fs::read(x); }",
            "#[cfg(test)]\nmod tests { fn t() { std::fs::write(p, b); mod inner { fn u() { fs::copy(a, b) } } } }",
            "fn f() { #[cfg(test)] { fs::write(a, b); } }",
            "fn f() { #[cfg(test)] fs::write(a, b); g() }",
            "struct S { #[cfg(test)] x: Command, y: u8 }",
            "#[cfg(test)]\n#[allow(dead_code)]\nfn t() { fs::write(a, b) }",
        ] {
            assert_eq!(violations(good), Vec::<String>::new(), "{good}");
        }
    }

    #[test]
    fn code_after_a_nested_test_module_is_still_checked() {
        let src = "mod a { #[cfg(test)] mod tests { fn t() {} } fn bad() { fs::write(a, b) } }";
        assert_eq!(violations(src), vec!["`fs::write`".to_string()]);
        let not_test = "#[cfg(not(test))] fn bad() { fs::write(a, b) }";
        assert!(!violations(not_test).is_empty());
    }
}
