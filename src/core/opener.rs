//! Which program opens a link in the browser, and which links it may be
//! handed. Pure: the runtime looks the programs up on PATH and runs the
//! first one found.

/// The programs that open a URL in the default browser, best first. WSL
/// prefers `wslview` (wslu), then the Windows `rundll32.exe` through the
/// interop, then a Linux `xdg-open`; macOS has `open`.
pub fn candidates(os: &str, wsl: bool) -> &'static [&'static str] {
    match (os, wsl) {
        ("macos", _) => &["open"],
        (_, true) => &["wslview", "rundll32.exe", "xdg-open"],
        _ => &["xdg-open"],
    }
}

/// The arguments `program` takes before the link: `rundll32.exe` opens it
/// through `url.dll`, which hands it to the default browser. (`explorer.exe`
/// opens some links, such as the Hugging Face token page, in the file
/// manager instead.)
pub fn leading_args(program: &str) -> &'static [&'static str] {
    match program {
        "rundll32.exe" => &["url.dll,FileProtocolHandler"],
        _ => &[],
    }
}

/// Whether `url` may be handed to an opener: an `https://` link of one
/// line with no spaces or control characters. Anything else (a file, a
/// `javascript:` link, a flag-looking word) is never opened.
pub fn is_web_url(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        !rest.is_empty() && !rest.chars().any(|c| c.is_whitespace() || c.is_control())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_host_gets_its_openers_best_first() {
        assert_eq!(candidates("linux", false), ["xdg-open"]);
        assert_eq!(
            candidates("linux", true),
            ["wslview", "rundll32.exe", "xdg-open"]
        );
        assert_eq!(candidates("macos", false), ["open"]);
    }

    // Bug (WSL): `explorer.exe` opens some links, such as the Hugging Face
    // token page, in the file manager; `rundll32.exe` hands them to the
    // default browser through `url.dll`.
    #[test]
    fn rundll32_gets_the_url_handler_before_the_link() {
        assert_eq!(
            leading_args("rundll32.exe"),
            ["url.dll,FileProtocolHandler"]
        );
        for program in ["wslview", "xdg-open", "open"] {
            assert!(leading_args(program).is_empty(), "{program}");
        }
    }

    #[test]
    fn only_one_line_https_links_are_opened() {
        assert!(is_web_url(
            "https://huggingface.co/settings/tokens/new?tokenType=read"
        ));
        assert!(is_web_url(
            "https://github.com/o/r/issues/new?title=a%20b&x=1"
        ));
        for bad in [
            "http://example.com",
            "https://",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "--help",
            "https://a b",
            "https://a\nb",
            "https://a\u{7}",
        ] {
            assert!(!is_web_url(bad), "{bad:?}");
        }
    }
}
