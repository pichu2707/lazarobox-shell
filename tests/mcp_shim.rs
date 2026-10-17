//! `lazarobox-shell __mcp-serve`, the real binary: it is dispatched before the
//! nesting guard and the token comes from the environment, never from argv.

#![cfg(unix)]

use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixListener,
    process::{Command, Stdio},
    thread,
};

const TOKEN: &str = "0123456789abcdef0123456789abcdef";

/// A folder only we can enter, like the session's: `tempdir` alone follows the
/// umask.
fn private_dir() -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    dir
}

/// Accepts one connection, failing the test instead of hanging when the shim
/// never comes.
fn accept_within(listener: &UnixListener) -> std::os::unix::net::UnixStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                stream.set_nonblocking(false).unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(10)))
                    .unwrap();
                return stream;
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                assert!(std::time::Instant::now() < deadline, "the shim never came");
                thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => panic!("{e}"),
        }
    }
}

fn shim(socket: &std::path::Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lazarobox-shell"));
    cmd.arg("__mcp-serve")
        .arg(socket)
        // The shim runs inside an engine launched from a lazarobox pane.
        .env("LAZAROBOX", "1")
        .env_remove("LAZAROBOX_MCP_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

#[test]
fn the_nesting_marker_does_not_block_the_shim() {
    let dir = tempfile::tempdir().unwrap();
    let out = shim(&dir.path().join("none.sock")).output().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    // It got as far as asking for its token: the nesting guard never ran.
    assert_eq!(out.status.code(), Some(2), "{err}");
    assert!(err.contains("LAZAROBOX_MCP_TOKEN"), "{err}");
    assert!(out.stdout.is_empty());
}

#[test]
fn the_shim_hands_the_token_over_the_socket_and_relays_lines() {
    let dir = private_dir();
    let socket = dir.path().join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let app = thread::spawn(move || {
        let stream = accept_within(&listener);
        let mut reader = BufReader::new(stream);
        let mut hello = String::new();
        reader.read_line(&mut hello).unwrap();
        reader.get_mut().write_all(b"{\"ok\":true}\n").unwrap();
        let mut request = String::new();
        reader.read_line(&mut request).unwrap();
        reader.get_mut().write_all(b"{\"echo\":true}\n").unwrap();
        (hello, request)
    });

    let mut child = shim(&socket)
        .env("LAZAROBOX_MCP_TOKEN", TOKEN)
        .spawn()
        .unwrap();
    // The token is in the environment, not on the command line.
    let cmdline = std::fs::read(format!("/proc/{}/cmdline", child.id())).unwrap_or_default();
    assert!(!String::from_utf8_lossy(&cmdline).contains(TOKEN));

    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(b"{\"ping\":1}\n").unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut reply = String::new();
    stdout.read_line(&mut reply).unwrap();
    assert_eq!(reply, "{\"echo\":true}\n");
    drop(stdin);
    let mut rest = String::new();
    stdout.read_to_string(&mut rest).unwrap();
    assert!(rest.is_empty(), "{rest:?}");
    assert!(child.wait().unwrap().success());

    let (hello, request) = app.join().unwrap();
    assert_eq!(
        hello,
        format!("{{\"lazarobox\":1,\"token\":\"{TOKEN}\"}}\n")
    );
    assert_eq!(request, "{\"ping\":1}\n");
}

#[test]
fn a_refused_shim_exits_non_zero_and_never_prints_the_token() {
    let dir = private_dir();
    let socket = dir.path().join("s.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let app = thread::spawn(move || {
        let stream = accept_within(&listener);
        let mut line = String::new();
        BufReader::new(stream).read_line(&mut line).unwrap();
        // Dropped: the session refuses by closing, with no reply.
    });
    let out = shim(&socket)
        .env("LAZAROBOX_MCP_TOKEN", TOKEN)
        .output()
        .unwrap();
    app.join().unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(!err.is_empty());
    assert!(!err.contains(TOKEN), "{err}");
    assert!(out.stdout.is_empty());
}
