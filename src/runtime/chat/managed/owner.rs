//! Whether the socket listening on a loopback port belongs to this user: only
//! then may a llama-server there, which lazarobox did not start, get its
//! prompts. Another user of the machine could listen on the port too.
//!
//! - Linux: every TCP socket listening on the port, IPv4 and IPv6, must be
//!   owned by this user (`/proc/net/tcp`, `/proc/net/tcp6`).
//! - macOS: one of this user's processes must hold a TCP socket listening on
//!   the port (`lsof`). The sockets of other users cannot be read there.
//! - Anywhere else, or when it cannot be read, it is not ours.

/// Whether the socket listening on `port` is this user's.
#[cfg(target_os = "linux")]
pub fn is_ours(port: u16) -> bool {
    // SAFETY: getuid has no arguments and cannot fail.
    let uid = unsafe { libc::getuid() };
    let mut owners = Vec::new();
    for table in ["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(text) = std::fs::read_to_string(table) {
            owners.extend(listeners(&text, port));
        }
    }
    !owners.is_empty() && owners.iter().all(|&owner| owner == uid)
}

/// The owners of the sockets listening on `port` in a `/proc/net/tcp` table.
#[cfg(any(target_os = "linux", test))]
fn listeners(table: &str, port: u16) -> Vec<libc::uid_t> {
    // sl local_address rem_address st tx_queue:rx_queue tr:tm->when retrnsmt uid
    const LISTEN: &str = "0A";
    table
        .lines()
        .skip(1)
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            let (_, hex) = fields.get(1)?.rsplit_once(':')?;
            let listening = u16::from_str_radix(hex, 16).ok()? == port && *fields.get(3)? == LISTEN;
            listening.then(|| fields.get(7)?.parse().ok())?
        })
        .collect()
}

/// Whether the socket listening on `port` is this user's: `lsof`, which comes
/// with macOS, run by its absolute path with no shell, lists the processes of
/// this user listening on it.
#[cfg(target_os = "macos")]
pub fn is_ours(port: u16) -> bool {
    use std::process::{Command, Stdio};
    // SAFETY: getuid has no arguments and cannot fail.
    let uid = unsafe { libc::getuid() };
    Command::new("/usr/sbin/lsof")
        .args(["-nP", "-a", "-sTCP:LISTEN", "-t"])
        .arg(format!("-iTCP:{port}"))
        .arg("-u")
        .arg(uid.to_string())
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|out| out.status.success() && !out.stdout.trim_ascii().is_empty())
}

/// Whether the socket listening on `port` is this user's: never known here.
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
pub fn is_ours(_port: u16) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLE: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:9999 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 1 1 0000000000000000 100 0 0 10 0
   1: 0100007F:9999 0100007F:A000 01 00000000:00000000 00:00000000 00000000  1000        0 2 1 0000000000000000 20 4 30 10 -1
   2: 00000000:9998 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1001        0 3 1 0000000000000000 100 0 0 10 0
";

    // Spec S12 (managed-llama-server-followups): only listening sockets on
    // the port count, with their owner.
    #[test]
    fn the_listeners_of_a_port_are_read_with_their_owner() {
        assert_eq!(listeners(TABLE, 0x9999), [1000]);
        assert_eq!(listeners(TABLE, 0x9998), [1001]);
        assert_eq!(listeners(TABLE, 0x9997), Vec::<libc::uid_t>::new());
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn a_socket_this_user_listens_on_is_ours() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(is_ours(port));
        drop(listener);
        assert!(!is_ours(port), "nothing listens");
    }
}
