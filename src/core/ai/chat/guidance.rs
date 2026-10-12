//! The text a chat pane shows when its engine cannot be used yet.

use super::engine::Availability;

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
        Availability::NoJail { engine } => format!("{engine}: not available on this system yet"),
        Availability::GatedOff { engine } => format!("{engine}: gated off"),
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
    fn no_jail_is_the_exact_line() {
        assert_eq!(
            guidance(&Availability::NoJail {
                engine: s("gemini")
            })
            .as_deref(),
            Some("gemini: not available on this system yet")
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
}
