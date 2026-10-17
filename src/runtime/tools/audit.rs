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
//! `libc::syscall` whose number is not `SYS_openat2`.
//!
//! A best-effort tripwire against accidental use, not a guarantee. It cannot
//! see a macro that builds a call from tokens, a call made through a path the
//! ban list does not name, or code reached through another crate.

/// A path segment that may not appear at all.
const BANNED_IDENTS: &[&str] = &[
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
    "Command",
    "TcpStream",
    "TcpListener",
    "UdpSocket",
    "UnixStream",
    "UnixListener",
    "UnixDatagram",
    "extern",
    "hard_link",
    "set_permissions",
    "set_len",
    "write_all",
    "write_vectored",
];

/// A segment that is banned when it is part of a longer path.
const BANNED_IN_PATH: &[&str] = &["process", "net"];

/// `fs::<name>` calls that change the tree.
const BANNED_FS: &[&str] = &["write", "copy", "rename", "symlink", "truncate"];

/// The only `libc::` names the tool code may use.
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
    "ENOSYS",
    "EPERM",
    "EINVAL",
    "E2BIG",
];

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
const MODE_NAMES: &[&str] = &["ApprovalMode", "set_approval_mode"];

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

/// Every banned use in the non-test code of `src`, one line each.
pub fn violations(src: &str) -> Vec<String> {
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
        if BANNED_IDENTS.contains(&name) {
            found.push(format!("`{name}`"));
        }
        if BANNED_IN_PATH.contains(&name) && (after_path || before_path) {
            found.push(format!("path segment `{name}`"));
        }
        if let (true, true, Some(next)) = (name == "fs", before_path, ident(k + 2))
            && (BANNED_FS.contains(&next) || next.starts_with("remove_"))
        {
            found.push(format!("`fs::{next}`"));
        }
        // `File::create`, `File::create_new`, `File::options`.
        if name == "File"
            && before_path
            && matches!(ident(k + 2), Some("create" | "create_new" | "options"))
        {
            found.push(format!("`File::{}`", ident(k + 2).unwrap_or_default()));
        }
        // `use std::fs as x` hides every later `x::write`.
        if name == "fs" && ident(k + 1) == Some("as") {
            found.push("`fs` aliased".to_string());
        }
        // `use std::fs::{write, self as x}`: look inside the group.
        if name == "fs" && before_path && t.get(k + 2) == Some(&Tok::Punct('{')) {
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
        // Only `openat2` may be called through `libc::syscall`.
        if name == "libc" && before_path && ident(k + 2) == Some("syscall") {
            let arg = (t.get(k + 3), ident(k + 4), t.get(k + 5), ident(k + 6));
            let ok = matches!(
                arg,
                (
                    Some(Tok::Punct('(')),
                    Some("libc"),
                    Some(Tok::Path),
                    Some("SYS_openat2")
                )
            );
            if !ok && t.get(k + 3) == Some(&Tok::Punct('(')) {
                found.push("`libc::syscall` with a number other than SYS_openat2".to_string());
            }
        }
        if name == "libc" && before_path {
            match t.get(k + 2) {
                Some(Tok::Ident(next)) if !LIBC_ALLOWED.contains(&next.as_str()) => {
                    found.push(format!("`libc::{next}`"));
                }
                // `libc::{a, b}`: every name inside the braces.
                Some(Tok::Punct('{')) => {
                    for tok in t[k + 3..].iter().take_while(|x| **x != Tok::Punct('}')) {
                        if let Tok::Ident(n) = tok
                            && !LIBC_ALLOWED.contains(&n.as_str())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tool_code_starts_no_process_opens_no_socket_and_creates_no_file() {
        for (name, source) in [
            ("fs.rs", include_str!("fs.rs")),
            ("open.rs", include_str!("open.rs")),
            ("mod.rs", include_str!("mod.rs")),
        ] {
            // A tokenizer gone wrong would pass everything: it must see code.
            assert!(
                non_test_tokens(source).len() > 50,
                "{name} yields no tokens"
            );
            assert_eq!(violations(source), Vec::<String>::new(), "{name}");
        }
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
