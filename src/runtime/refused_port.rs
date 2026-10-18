//! Test support: a loopback port that refuses connections.
//!
//! A refusal needs a port nothing listens on that no parallel test can take in
//! between. Linux gives both with a socket that is bound and never listens: it
//! holds the port, and a connect to it gets a reset at once. macOS does not
//! reset it: the connect hangs until its time limit. There the port is a
//! listener that is closed again; its number is not handed out again soon, as
//! the ephemeral ports of macOS are picked at random.

/// A port on `127.0.0.1` whose connections are refused, for as long as it lives.
pub(crate) struct RefusedPort {
    port: u16,
    /// Keeps the port on Linux; nothing to keep elsewhere.
    _held: Option<tokio::net::TcpSocket>,
}

impl RefusedPort {
    pub(crate) fn new() -> Self {
        #[cfg(target_os = "linux")]
        {
            let socket = tokio::net::TcpSocket::new_v4().expect("a socket");
            socket
                .bind("127.0.0.1:0".parse().expect("an address"))
                .expect("binds");
            let port = socket.local_addr().expect("an address").port();
            Self {
                port,
                _held: Some(socket),
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
            let port = listener.local_addr().expect("an address").port();
            drop(listener);
            Self { port, _held: None }
        }
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }
}

#[cfg(test)]
mod tests {
    use std::{net::TcpStream, time::Duration};

    use super::*;

    fn connect(port: u16) -> std::io::Result<TcpStream> {
        TcpStream::connect_timeout(&([127, 0, 0, 1], port).into(), Duration::from_secs(5))
    }

    #[test]
    fn a_refused_port_refuses_at_once_instead_of_hanging() {
        let refused = RefusedPort::new();
        let err = connect(refused.port()).expect_err("nothing accepts here");
        assert_eq!(err.kind(), std::io::ErrorKind::ConnectionRefused);
    }

    // Only the held port of Linux is promised to stay taken.
    #[cfg(target_os = "linux")]
    #[test]
    fn two_refused_ports_at_the_same_time_are_different_ports() {
        let (a, b) = (RefusedPort::new(), RefusedPort::new());
        assert_ne!(a.port(), b.port());
        assert!(connect(a.port()).is_err());
        assert!(connect(b.port()).is_err());
    }
}
