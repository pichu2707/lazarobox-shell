//! OSC 8 hyperlinks (`ESC ] 8 ; params ; URI ST`), which a program prints
//! around the text a link shows, and `ESC ] 8 ; ; ST` after it.
//!
//! Pure: the payload is untrusted bytes. Only a link a pane may open
//! ([`is_link_url`]) starts anything; every other OSC 8 just ends the link
//! that was open, as a terminal would when the next one starts.

use super::opener::is_link_url;

/// URIs this long or longer are ignored: no web link needs that much.
pub const MAX_URI: usize = 8192;

/// The VT parser keeps at most this many `;`-split params and drops the
/// rest, so a payload that reaches it may have lost the end of its URI.
const MAX_PARAMS: usize = 16;

/// What an OSC 8 sequence does to the link being drawn.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Osc8 {
    /// The text that follows is a link to this URI.
    Open(String),
    /// The text that follows is no link.
    Close,
}

/// Reads an OSC sequence. `params` are the `;`-split OSC parameters, with
/// `params[0]` being `8`; `None` when it is not OSC 8.
pub fn parse(params: &[&[u8]]) -> Option<Osc8> {
    let (cmd, rest) = params.split_first()?;
    if *cmd != b"8" {
        return None;
    }
    // `rest[0]` holds the `key=value` params; the URI may contain `;`.
    let parts = rest.get(1..).unwrap_or_default();
    if parts.is_empty() || params.len() >= MAX_PARAMS {
        return Some(Osc8::Close);
    }
    let raw = parts.join(&b';');
    let uri = (raw.len() < MAX_URI)
        .then(|| String::from_utf8(raw).ok())
        .flatten()
        .filter(|uri| is_link_url(uri));
    Some(uri.map_or(Osc8::Close, Osc8::Open))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn osc(payload: &str) -> Option<Osc8> {
        let params: Vec<&[u8]> = payload.split(';').map(str::as_bytes).collect();
        parse(&params)
    }

    #[test]
    fn a_web_uri_opens_a_link_and_an_empty_one_closes_it() {
        assert_eq!(
            osc("8;;https://a.io/x"),
            Some(Osc8::Open("https://a.io/x".into()))
        );
        assert_eq!(
            osc("8;id=7;http://localhost:3000/"),
            Some(Osc8::Open("http://localhost:3000/".into()))
        );
        assert_eq!(osc("8;;"), Some(Osc8::Close));
        assert_eq!(osc("8"), Some(Osc8::Close));
    }

    #[test]
    fn a_semicolon_in_the_uri_is_kept() {
        assert_eq!(
            osc("8;;https://a.io/x;y;z"),
            Some(Osc8::Open("https://a.io/x;y;z".into()))
        );
    }

    #[test]
    fn a_link_a_pane_never_opens_only_closes() {
        for payload in [
            "8;;file:///etc/passwd",
            "8;;vscode://file/a.rs",
            "8;;javascript:alert(1)",
            "8;;https://",
            "8;;https://a b",
        ] {
            assert_eq!(osc(payload), Some(Osc8::Close), "{payload}");
        }
        let bad = [b"8".as_slice(), b"", b"https://a.io/\xff"];
        assert_eq!(parse(&bad), Some(Osc8::Close));
    }

    #[test]
    fn a_uri_that_may_have_been_cut_or_is_too_long_only_closes() {
        let cut = format!("8;;https://a.io/{}", "a;".repeat(13));
        assert_eq!(osc(&cut), Some(Osc8::Close));
        let fits = format!("8;;https://a.io/{}", "a;".repeat(12));
        assert!(matches!(osc(&fits), Some(Osc8::Open(_))));
        let long = format!("8;;https://a.io/{}", "a".repeat(MAX_URI));
        assert_eq!(osc(&long), Some(Osc8::Close));
    }

    #[test]
    fn other_osc_sequences_are_not_osc8() {
        assert_eq!(osc("7;file:///tmp"), None);
        assert_eq!(osc("52;c;aGk="), None);
        assert_eq!(parse(&[]), None);
    }
}
