//! The text a chat pane shows when its engine cannot be used yet.

use super::{adapters, engine::Availability, wrap::wrap_words};

/// The rows `text` takes above the composer in a transcript area `width`
/// wide and `height` high: its wrapped lines, never more than the area has.
/// The one count both the drawing and the paging use.
pub fn guidance_rows(text: &str, width: u16, height: u16) -> u16 {
    if width == 0 || height == 0 {
        return 0;
    }
    let rows = wrap_words(text, usize::from(width)).len();
    u16::try_from(rows).unwrap_or(u16::MAX).min(height)
}

/// The guidance for `availability`, or `None` when the engine is ready.
pub fn guidance(availability: &Availability) -> Option<String> {
    Some(match availability {
        Availability::Ready { .. } => return None,
        Availability::Unassigned => {
            "No engine assigned to chat: open the menu with Ctrl+Space m".to_owned()
        }
        Availability::Checking => "checking engines…".to_owned(),
        Availability::Undetected { engine, status } => format!(
            "{engine}: {status}; start or install it, then refresh from the menu \
             (Ctrl+Space m, then r)"
        ),
        Availability::Unsupported { engine } => format!("{engine}: not supported in chat yet"),
        Availability::NoJail { engine, reason } => {
            format!("{engine}: not available on this system yet: {reason}")
        }
        Availability::Refused { engine, reason } => {
            format!("{engine}: the jail refuses to start it: {reason}")
        }
        Availability::GatedOff { engine } => format!("{engine}: gated off"),
        Availability::Outdated { engine, version } => format!(
            "{engine} {version} is too old for lazarobox (it needs {} or newer); \
             update it: claude update, brew upgrade claude-code, or \
             npm i -g @anthropic-ai/claude-code@latest; then refresh from the menu \
             (Ctrl+Space m, then r)",
            adapters::claude_min_version_text()
        ),
        Availability::NoModel { engine } => {
            format!("{engine}: no model chosen: pick one in the menu (Ctrl+Space m)")
        }
        Availability::ModelMissing { engine, model } => {
            format!("{engine}: {model} is not downloaded: download required")
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guidance_rows_are_the_wrapped_lines_capped_by_the_area() {
        assert_eq!(guidance_rows("alpha beta gamma", 12, 9), 2);
        assert_eq!(guidance_rows("alpha beta gamma", 12, 1), 1);
        assert_eq!(guidance_rows("alpha", 0, 9), 0);
        assert_eq!(guidance_rows("alpha", 12, 0), 0);
    }

    fn s(x: &str) -> String {
        x.to_owned()
    }

    #[test]
    fn a_ready_engine_needs_no_guidance() {
        let ready = Availability::Ready {
            engine: s("ollama"),
            model: Some(s("llama3:8b")),
        };
        assert_eq!(guidance(&ready), None);
    }

    #[test]
    fn unassigned_names_the_task_and_the_menu_key() {
        let text = guidance(&Availability::Unassigned).unwrap();
        assert!(text.starts_with("No engine assigned to chat"), "{text}");
        assert!(text.contains("Ctrl+Space m"), "{text}");
    }

    #[test]
    fn checking_is_a_fixed_line() {
        assert_eq!(
            guidance(&Availability::Checking).as_deref(),
            Some("checking engines…")
        );
    }

    #[test]
    fn undetected_names_the_engine_its_status_and_the_refresh_key() {
        let text = guidance(&Availability::Undetected {
            engine: s("ollama"),
            status: s("not running"),
        })
        .unwrap();
        assert!(
            text.contains("ollama") && text.contains("not running"),
            "{text}"
        );
        assert!(text.contains("start or install"), "{text}");
        assert!(text.contains("refresh") && text.contains('r'), "{text}");
    }

    #[test]
    fn unsupported_says_so() {
        let text = guidance(&Availability::Unsupported {
            engine: s("opencode"),
        })
        .unwrap();
        assert!(text.contains("opencode"), "{text}");
        assert!(text.contains("not supported in chat yet"), "{text}");
    }

    #[test]
    fn a_refused_layout_is_the_exact_line() {
        assert_eq!(
            guidance(&Availability::Refused {
                engine: s("codex"),
                reason: s("codex (standalone) cannot be isolated"),
            })
            .as_deref(),
            Some("codex: the jail refuses to start it: codex (standalone) cannot be isolated")
        );
    }

    #[test]
    fn no_jail_is_the_exact_line() {
        assert_eq!(
            guidance(&Availability::NoJail {
                engine: s("gemini"),
                reason: s("Landlock ABI 3+ needed (Linux 6.2); this kernel has ABI 2"),
            })
            .as_deref(),
            Some(
                "gemini: not available on this system yet: Landlock ABI 3+ needed \
                 (Linux 6.2); this kernel has ABI 2"
            )
        );
    }

    #[test]
    fn no_jail_on_another_system_points_codex_to_claude_or_ollama() {
        use crate::core::ai::jail::JailOff;
        assert_eq!(
            guidance(&Availability::NoJail {
                engine: s("codex"),
                reason: JailOff::NoBackend.text_required(),
            })
            .as_deref(),
            Some(
                "codex: not available on this system yet: the CLI jail needs Linux or macOS; \
                 on this system use claude or Ollama for chat"
            )
        );
    }

    #[test]
    fn no_jail_on_macos_names_the_sandbox_not_linux() {
        use crate::core::ai::jail::{JailOff, SeatbeltFault};
        assert_eq!(
            guidance(&Availability::NoJail {
                engine: s("gemini"),
                reason: JailOff::NoSeatbelt(SeatbeltFault::Leaked).text_required(),
            })
            .as_deref(),
            Some(
                "gemini: not available on this system yet: the macOS jail is unavailable: \
                 the sandbox did not block a test action; \
                 on this system use claude or Ollama for chat"
            )
        );
    }

    #[test]
    fn gated_off_names_the_engine() {
        let text = guidance(&Availability::GatedOff { engine: s("codex") }).unwrap();
        assert!(
            text.contains("codex") && text.contains("gated off"),
            "{text}"
        );
    }

    #[test]
    fn a_server_without_a_model_says_no_model() {
        let text = guidance(&Availability::NoModel {
            engine: s("ollama"),
        })
        .unwrap();
        assert!(
            text.contains("ollama") && text.contains("no model"),
            "{text}"
        );
    }

    #[test]
    fn a_missing_model_says_download_required() {
        let text = guidance(&Availability::ModelMissing {
            engine: s("ollama"),
            model: s("old:7b"),
        })
        .unwrap();
        assert!(
            text.contains("ollama")
                && text.contains("old:7b")
                && text.contains("download required"),
            "{text}"
        );
    }

    #[test]
    fn outdated_names_the_version_the_minimum_and_how_to_update() {
        let text = guidance(&Availability::Outdated {
            engine: s("claude"),
            version: s("2.1.200"),
        })
        .unwrap();
        assert_eq!(
            text,
            "claude 2.1.200 is too old for lazarobox (it needs 2.1.259 or newer); \
             update it: claude update, brew upgrade claude-code, or \
             npm i -g @anthropic-ai/claude-code@latest; then refresh from the menu \
             (Ctrl+Space m, then r)"
        );
    }
}
