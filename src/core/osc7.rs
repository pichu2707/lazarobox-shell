//! OSC 7 (`ESC ] 7 ; file://host/path`) working-directory reports.
//!
//! Pure: the payload is untrusted bytes, so everything is validated here and
//! nothing touches the filesystem (no canonicalisation).

use std::{ffi::OsStr, os::unix::ffi::OsStrExt, path::PathBuf};

/// Payloads (`7;<uri>`) of this many bytes or more are ignored: the VT parser
/// may have truncated them.
const MAX_PAYLOAD: usize = 1024;

/// Validates an OSC 7 report. `params` are the `;`-split OSC parameters, with
/// `params[0]` being `7`. Returns the reported path, or `None` when anything
/// is malformed (the caller then keeps its previous cwd).
pub fn parse(params: &[&[u8]], local_host: Option<&str>) -> Option<PathBuf> {
    let _ = (params, local_host, MAX_PAYLOAD);
    let _ = (OsStr::from_bytes(b""), percent_decode(b""));
    None
}

/// Decodes `%XX` escapes. `None` on a truncated or non-hex escape.
fn percent_decode(bytes: &[u8]) -> Option<Vec<u8>> {
    let _ = bytes;
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn p(uri: &str, host: Option<&str>) -> Option<PathBuf> {
        parse(&[b"7", uri.as_bytes()], host)
    }

    #[test]
    fn valid_path_empty_host() {
        assert_eq!(p("file:///tmp", None), Some(PathBuf::from("/tmp")));
    }

    #[test]
    fn matching_hostname_accepted() {
        assert_eq!(
            p("file://myhost/home/u/proj", Some("myhost")),
            Some(PathBuf::from("/home/u/proj"))
        );
    }

    #[test]
    fn local_hosts_accepted_case_insensitive() {
        for uri in [
            "file:///a",
            "file://localhost/a",
            "file://LOCALHOST/a",
            "file://MyHost/a",
        ] {
            assert_eq!(p(uri, Some("myhost")), Some(PathBuf::from("/a")), "{uri}");
        }
    }

    #[test]
    fn remote_host_rejected() {
        assert_eq!(p("file://other.example/etc", Some("myhost")), None);
        assert_eq!(p("file://myhost/etc", None), None);
    }

    #[test]
    fn percent_escapes_decoded() {
        assert_eq!(
            p("file:///tmp/my%20dir", None),
            Some(PathBuf::from("/tmp/my dir"))
        );
        assert_eq!(p("file:///a%3Bb", None), Some(PathBuf::from("/a;b")));
    }

    #[test]
    fn semicolon_params_rejoined() {
        let params: [&[u8]; 3] = [b"7", b"file:///tmp/a", b"b"];
        assert_eq!(parse(&params, None), Some(PathBuf::from("/tmp/a;b")));
    }

    #[test]
    fn bad_percent_escape_rejected() {
        assert_eq!(p("file:///a%zz", None), None);
        assert_eq!(p("file:///a%4", None), None);
        assert_eq!(p("file:///a%", None), None);
    }

    #[test]
    fn nul_and_control_bytes_rejected() {
        assert_eq!(p("file:///a%00b", None), None);
        assert_eq!(p("file:///a%0ab", None), None);
        assert_eq!(p("file:///a%7fb", None), None);
        assert_eq!(parse(&[b"7", b"file:///a\x01b"], None), None);
    }

    #[test]
    fn non_utf8_bytes_accepted() {
        let got = p("file:///bad%ff", None).expect("accepted");
        assert_eq!(got.as_os_str().as_bytes(), b"/bad\xff");
    }

    #[test]
    fn relative_or_missing_path_rejected() {
        assert_eq!(p("file://relative/path", None), None);
        assert_eq!(p("file://host", Some("host")), None);
        assert_eq!(p("file://", None), None);
    }

    #[test]
    fn non_file_scheme_rejected() {
        assert_eq!(p("http:///x", None), None);
        assert_eq!(p("/tmp", None), None);
    }

    #[test]
    fn wrong_command_or_missing_uri_rejected() {
        assert_eq!(parse(&[b"8", b"file:///tmp"], None), None);
        assert_eq!(parse(&[b"7"], None), None);
        assert_eq!(parse(&[], None), None);
    }

    #[test]
    fn payload_of_1024_bytes_or_more_rejected() {
        // "7;" + uri: 1023 bytes is the largest accepted payload.
        let ok = format!("file:///{}", "a".repeat(1023 - 2 - 8));
        assert_eq!(2 + ok.len(), 1023);
        assert_eq!(p(&ok, None), Some(Path::new(&ok[7..]).to_path_buf()));
        let too_long = format!("file:///{}", "a".repeat(1024 - 2 - 8));
        assert_eq!(2 + too_long.len(), 1024);
        assert_eq!(p(&too_long, None), None);
    }

    #[test]
    fn percent_decode_unit() {
        assert_eq!(percent_decode(b"a%20b"), Some(b"a b".to_vec()));
        assert_eq!(percent_decode(b"%FF"), Some(vec![0xFF]));
        assert_eq!(percent_decode(b"%g0"), None);
        assert_eq!(percent_decode(b"abc"), Some(b"abc".to_vec()));
    }
}
