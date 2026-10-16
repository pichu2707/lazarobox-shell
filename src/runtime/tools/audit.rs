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
//! What it cannot see: a macro that builds a call from tokens, or a call made
//! through a path the ban list does not name.

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
            // A character literal closes within a few chars; a lifetime does not.
            let close = (i + 2..b.len().min(i + 12)).find(|&j| b[j] == '\'' && b[j - 1] != '\\');
            let is_char = next == Some('\\') || (b.get(i + 2) == Some(&'\''));
            match (is_char, close) {
                (true, Some(j)) => i = j + 1,
                _ => i += 1,
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
            assert_eq!(violations(source), Vec::<String>::new(), "{name}");
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
        ] {
            assert!(!violations(bad).is_empty(), "{bad}");
        }
    }

    #[test]
    fn allowed_uses_comments_strings_and_test_code_are_not_flagged() {
        for good in [
            "fn f() { libc::openat(a, p, libc::O_RDONLY | libc::O_NOFOLLOW) }",
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
