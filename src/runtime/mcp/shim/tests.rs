use std::{
    io::{BufRead, BufReader, Read, Write},
    os::unix::net::UnixStream,
    thread::{self, JoinHandle},
    time::Duration,
};

use serde_json::{Value, json};

use super::*;
use crate::core::ai::tools::mcp::MAX_LINE_BYTES;

const TOKEN: &str = "0123456789abcdef0123456789abcdef";
const WAIT: Duration = Duration::from_secs(5);

/// The shim running on threads, with the three ends a test drives: the
/// engine's stdin and stdout, and the app's end of the socket.
struct Rig {
    /// Writes what the engine would put on the shim's stdin.
    stdin: UnixStream,
    /// Reads what the shim puts on its stdout.
    stdout: BufReader<UnixStream>,
    /// The session's end.
    app: BufReader<UnixStream>,
    shim: JoinHandle<Result<(), Failure>>,
}

fn rig(wait: Duration) -> Rig {
    let (shim_sock, app) = UnixStream::pair().unwrap();
    let (stdin, shim_in) = UnixStream::pair().unwrap();
    let (shim_out, stdout) = UnixStream::pair().unwrap();
    for end in [&app, &stdout] {
        end.set_read_timeout(Some(WAIT)).unwrap();
    }
    let shim = thread::spawn(move || serve(shim_sock, TOKEN, wait, shim_in, shim_out));
    Rig {
        stdin,
        stdout: BufReader::new(stdout),
        app: BufReader::new(app),
        shim,
    }
}

fn line(reader: &mut impl BufRead) -> String {
    let mut text = String::new();
    reader.read_line(&mut text).expect("a line arrives");
    text
}

impl Rig {
    /// Reads the hello and answers it.
    fn accept(&mut self) -> Value {
        let hello = line(&mut self.app);
        self.app.get_mut().write_all(b"{\"ok\":true}\n").unwrap();
        serde_json::from_str(&hello).expect("the hello is JSON")
    }

    fn finish(self) -> Result<(), Failure> {
        drop(self.stdin);
        self.shim.join().unwrap()
    }
}

#[test]
fn the_first_line_is_exactly_the_handshake() {
    let mut rig = rig(WAIT);
    let hello = rig.accept();
    assert_eq!(hello, json!({"lazarobox": 1, "token": TOKEN}));
}

#[test]
fn lines_are_copied_both_ways_after_the_handshake() {
    let mut rig = rig(WAIT);
    rig.accept();

    let request = "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}\n";
    rig.stdin.write_all(request.as_bytes()).unwrap();
    assert_eq!(line(&mut rig.app), request);

    let reply = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}\n";
    rig.app.get_mut().write_all(reply.as_bytes()).unwrap();
    assert_eq!(line(&mut rig.stdout), reply);
}

#[test]
fn a_line_at_the_cap_goes_through_whole() {
    let mut rig = rig(WAIT);
    rig.accept();
    let body = "a".repeat(MAX_LINE_BYTES);
    let mut stdin = rig.stdin.try_clone().unwrap();
    let sent = body.clone();
    let writer = thread::spawn(move || stdin.write_all(format!("{sent}\n").as_bytes()));
    assert_eq!(line(&mut rig.app), format!("{body}\n"));
    writer.join().unwrap().unwrap();
}

#[test]
fn an_oversized_line_is_answered_not_forwarded_and_the_next_one_passes() {
    let mut rig = rig(WAIT);
    rig.accept();
    let mut stdin = rig.stdin.try_clone().unwrap();
    let writer = thread::spawn(move || {
        let big = "a".repeat(MAX_LINE_BYTES + 1);
        stdin.write_all(big.as_bytes()).unwrap();
        stdin.write_all(b"\n{\"next\":1}\n").unwrap();
    });

    let error: Value = serde_json::from_str(&line(&mut rig.stdout)).unwrap();
    assert_eq!(error["id"], Value::Null);
    assert_eq!(error["error"]["code"], -32600);
    // The app saw the next line and nothing of the big one.
    assert_eq!(line(&mut rig.app), "{\"next\":1}\n");
    writer.join().unwrap();
}

#[test]
fn an_oversized_line_with_no_newline_is_not_buffered_whole() {
    // The cap is applied while reading: a flood with no newline at all never
    // grows a buffer past the cap, and the shim ends cleanly at EOF.
    let mut rig = rig(WAIT);
    rig.accept();
    let mut stdin = rig.stdin.try_clone().unwrap();
    let writer = thread::spawn(move || {
        let chunk = vec![b'a'; 64 * 1024];
        for _ in 0..(3 * MAX_LINE_BYTES / chunk.len()) {
            stdin.write_all(&chunk).unwrap();
        }
    });
    writer.join().unwrap();
    // An unterminated tail has nobody to answer to: it is dropped at EOF.
    assert!(rig.finish().is_ok());
}

#[test]
fn malformed_lines_pass_through_untouched_and_do_not_stop_the_relay() {
    let mut rig = rig(WAIT);
    rig.accept();
    for raw in [
        &b"not json at all\n"[..],
        &b"\xff\xfe\x00broken\n"[..],
        b"\n",
    ] {
        rig.stdin.write_all(raw).unwrap();
        let mut got = Vec::new();
        rig.app.read_until(b'\n', &mut got).unwrap();
        assert_eq!(got, raw);
    }
    rig.stdin.write_all(b"{\"still\":\"alive\"}\n").unwrap();
    assert_eq!(line(&mut rig.app), "{\"still\":\"alive\"}\n");
}

#[test]
fn an_oversized_line_from_the_app_is_answered_to_the_client_instead() {
    let mut rig = rig(WAIT);
    rig.accept();
    let mut app = rig.app.get_ref().try_clone().unwrap();
    let writer = thread::spawn(move || {
        let big = "a".repeat(MAX_LINE_BYTES + 1);
        app.write_all(big.as_bytes()).unwrap();
        app.write_all(b"\n{\"after\":1}\n").unwrap();
    });
    let error: Value = serde_json::from_str(&line(&mut rig.stdout)).unwrap();
    assert_eq!(error["error"]["code"], -32600);
    assert_eq!(line(&mut rig.stdout), "{\"after\":1}\n");
    writer.join().unwrap();
}

#[test]
fn the_shim_ends_when_the_client_closes_stdin() {
    let mut rig = rig(WAIT);
    rig.accept();
    drop(rig.stdin.try_clone());
    let Rig {
        stdin,
        mut app,
        shim,
        ..
    } = rig;
    drop(stdin);
    // The app end sees the connection close.
    let mut rest = Vec::new();
    app.read_to_end(&mut rest).unwrap();
    assert!(shim.join().unwrap().is_ok());
}

#[test]
fn the_shim_ends_when_the_session_closes_the_connection() {
    let mut rig = rig(WAIT);
    rig.accept();
    let Rig {
        stdin, app, shim, ..
    } = rig;
    drop(app);
    assert!(shim.join().unwrap().is_ok());
    drop(stdin);
}

#[test]
fn a_session_that_closes_without_an_ok_is_a_refusal() {
    let mut rig = rig(WAIT);
    let _ = line(&mut rig.app);
    let Rig {
        app,
        shim,
        mut stdout,
        ..
    } = rig;
    drop(app);
    let failure = shim.join().unwrap().unwrap_err();
    assert!(matches!(failure, Failure::Refused), "{failure:?}");
    // Nothing reached the client.
    let mut out = Vec::new();
    stdout.read_to_end(&mut out).unwrap();
    assert!(out.is_empty());
}

#[test]
fn any_answer_but_ok_true_is_a_refusal() {
    for answer in [
        &b"{\"ok\":false}\n"[..],
        b"{\"ok\":1}\n",
        b"{}\n",
        b"nope\n",
        b"\n",
        b"\xff\n",
    ] {
        let mut rig = rig(WAIT);
        let _ = line(&mut rig.app);
        rig.app.get_mut().write_all(answer).unwrap();
        let failure = rig.shim.join().unwrap().unwrap_err();
        assert!(
            matches!(failure, Failure::Refused),
            "{answer:?}: {failure:?}"
        );
    }
}

#[test]
fn an_endless_answer_is_a_refusal_not_a_buffer() {
    let mut rig = rig(WAIT);
    let _ = line(&mut rig.app);
    rig.app.get_mut().write_all(&[b'x'; 4096]).unwrap();
    let failure = rig.shim.join().unwrap().unwrap_err();
    assert!(matches!(failure, Failure::Refused), "{failure:?}");
}

#[test]
fn a_silent_session_times_out() {
    let mut rig = rig(Duration::from_millis(100));
    let _ = line(&mut rig.app);
    // The app says nothing and keeps the connection open.
    let failure = rig.shim.join().unwrap().unwrap_err();
    assert!(matches!(failure, Failure::TimedOut), "{failure:?}");
    drop(rig.app);
}

#[test]
fn no_message_of_a_failure_carries_the_token() {
    let silent = {
        let mut rig = rig(Duration::from_millis(50));
        let _ = line(&mut rig.app);
        rig.shim.join().unwrap().unwrap_err()
    };
    let refused = {
        let mut rig = rig(WAIT);
        let _ = line(&mut rig.app);
        rig.app.get_mut().write_all(b"{\"ok\":false}\n").unwrap();
        rig.shim.join().unwrap().unwrap_err()
    };
    let missing = std::path::Path::new("/nonexistent/lazarobox.sock");
    let connect = run(&["__x".into(), missing.into()][1..], Some(TOKEN)).unwrap_err();
    for failure in [silent, refused, connect, Failure::NoToken, Failure::Usage] {
        let text = format!("{failure} {failure:?}");
        assert!(!text.contains(TOKEN), "{text}");
        assert!(!text.is_empty());
        assert_ne!(failure.exit_code(), 0);
    }
}

#[test]
fn exit_codes_tell_misuse_from_a_refusal() {
    assert_eq!(Failure::Usage.exit_code(), 2);
    assert_eq!(Failure::NoToken.exit_code(), 2);
    assert_eq!(Failure::Refused.exit_code(), 1);
    assert_eq!(Failure::TimedOut.exit_code(), 1);
}

#[test]
fn the_arguments_are_one_socket_path() {
    assert_eq!(
        parse_args(&["/run/s.sock".into()]),
        Some(PathBuf::from("/run/s.sock"))
    );
    assert_eq!(parse_args(&[]), None);
    assert_eq!(parse_args(&["a".into(), "b".into()]), None);
    assert_eq!(parse_args(&["".into()]), None);
}

#[test]
fn a_missing_token_stops_before_touching_the_socket() {
    let failure = run(&["/nonexistent/lazarobox.sock".into()], None).unwrap_err();
    assert!(matches!(failure, Failure::NoToken), "{failure:?}");
    let failure = run(&["/nonexistent/lazarobox.sock".into()], Some("")).unwrap_err();
    assert!(matches!(failure, Failure::NoToken), "{failure:?}");
    let failure = run(&[], Some(TOKEN)).unwrap_err();
    assert!(matches!(failure, Failure::Usage), "{failure:?}");
}

#[test]
fn an_unreachable_socket_is_a_connect_failure() {
    let failure = run(&["/nonexistent/lazarobox.sock".into()], Some(TOKEN)).unwrap_err();
    assert!(matches!(failure, Failure::Connect(_)), "{failure:?}");
}
