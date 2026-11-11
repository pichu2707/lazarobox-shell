//! The text a chat pane shows when its engine cannot be used yet.

use super::{adapters, engine::Availability, wrap::wrap_words};
use crate::core::ai::orchestra::steps::{Gate, Step};

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

/// The hint that suggests `/github` in `cols` columns, ASCII and one line:
/// first with the repository, then without it, then shorter, and at the
/// narrowest the bare command. The word `/github` is never cut.
pub fn github_hint(repo: &str, cols: usize) -> String {
    let forms = [
        format!("Type /github to let this chat read {repo}'s issues and PRs"),
        "Type /github to let this chat read this repo's issues and PRs".to_owned(),
        "Type /github to read this repo's issues and PRs".to_owned(),
        "Type /github for GitHub issues".to_owned(),
    ];
    forms
        .into_iter()
        .find(|form| form.chars().count() <= cols)
        .unwrap_or_else(|| "/github".to_owned())
}

/// The notice that no worker ran, in `cols` columns, ASCII and one line with
/// two columns of margin: the full form, then a shorter one. It names no key:
/// the keys are named by [`delegate_hint`], only while they can act.
pub fn undelegated_hint(cols: usize) -> String {
    let full = "no worker ran, nothing written";
    let form = if full.len() + 2 <= cols {
        full
    } else {
        "no worker ran"
    };
    form.to_owned()
}

/// The line offered while the delegate key can act, in `cols` columns, ASCII
/// and one line with two columns of margin. The keys come first and stay
/// whole; the explanation and then Esc go as the width shrinks.
pub fn delegate_hint(cols: usize) -> String {
    let forms = [
        "Enter delegate | Esc dismiss | no worker ran, nothing written",
        "Enter delegate | Esc dismiss | no worker ran",
        "Enter delegate | no worker ran",
    ];
    forms
        .into_iter()
        .find(|form| form.len() + 2 <= cols)
        .unwrap_or("Enter delegate")
        .to_owned()
}

/// The line offered while `Enter gates off` can act (W3, D24), in `cols`
/// columns, ASCII and one line with two columns of margin. The key comes first
/// and stays whole; the step and then the gate go as the width shrinks.
pub fn gates_hint(cols: usize, gate: Gate, step: Step) -> String {
    let gate = gate.label();
    let forms = [
        format!("Enter gates off | {gate} gate: step {}", step.label()),
        format!("Enter gates off | {gate} gate"),
    ];
    let last = "Enter gates off".to_owned();
    forms
        .into_iter()
        .find(|form| form.len() + 2 <= cols)
        .unwrap_or(last)
}

/// The most served ids the not-served guidance lists, and the most characters of each.
const MAX_SERVED_SHOWN: usize = 3;
const SERVED_ID_CHARS: usize = 48;

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
        Availability::ModelNotServed {
            engine,
            model,
            served,
            port,
        } => {
            let serves = if served.is_empty() {
                "it serves no model".to_owned()
            } else {
                let ids: Vec<String> = served
                    .iter()
                    .take(MAX_SERVED_SHOWN)
                    .map(|id| {
                        format!(
                            "\"{}\"",
                            id.chars().take(SERVED_ID_CHARS).collect::<String>()
                        )
                    })
                    .collect();
                let more = served.len().saturating_sub(MAX_SERVED_SHOWN);
                let more = if more > 0 {
                    format!(" (+{more} more)")
                } else {
                    String::new()
                };
                format!("it serves {}{more}", ids.join(", "))
            };
            format!(
                "{engine}: the server at 127.0.0.1:{port} does not serve \"{model}\": \
                 start llama-server with --alias {model}, or set the model to the served name; \
                 {serves}"
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Spec (chat-pane, Hint at 80 columns and TERM=linux): one line, the word
    // `/github` never cut, the repository dropped before anything else, ASCII.
    #[test]
    fn the_github_hint_drops_the_repository_name_before_it_cuts_the_command() {
        const LONG: &str = "owner/a-fairly-long-repository-name";
        assert_eq!(
            github_hint("o/r", 80),
            "Type /github to let this chat read o/r's issues and PRs"
        );
        // The long name does not fit one line: the name goes first.
        assert_eq!(
            github_hint(LONG, 80),
            "Type /github to let this chat read this repo's issues and PRs"
        );
        for cols in [80_usize, 60, 50, 40, 30, 20, 10, 7] {
            for repo in ["o/r", LONG] {
                let text = github_hint(repo, cols);
                assert!(text.is_ascii(), "{text:?}");
                assert!(text.contains("/github"), "{cols}: {text:?}");
                assert!(text.chars().count() <= cols, "{cols}: {text:?}");
            }
        }
        // Narrower still: the shorter forms keep the command whole.
        assert_eq!(
            github_hint(LONG, 47),
            "Type /github to read this repo's issues and PRs"
        );
        assert_eq!(github_hint(LONG, 30), "Type /github for GitHub issues");
        assert_eq!(github_hint(LONG, 7), "/github");
    }

    // Spec (chat-pane, Worker-flow notices are transcript notices): ASCII,
    // two columns of margin so the line never wraps, shorter at narrow widths.
    // No key is named: PR4 adds the key text with the keys.
    #[test]
    fn the_undelegated_hint_has_a_form_for_each_width() {
        let full = "no worker ran, nothing written";
        let short = "no worker ran";
        assert_eq!(undelegated_hint(200), full);
        assert_eq!(undelegated_hint(full.len() + 2), full);
        assert_eq!(undelegated_hint(full.len() + 1), short);
        assert_eq!(undelegated_hint(short.len() + 2), short);
        assert_eq!(undelegated_hint(3), short);
        for cols in 1..=80 {
            let text = undelegated_hint(cols);
            assert!(text.is_ascii() && !text.contains("Enter"));
        }
    }

    // D27: the keys come first, shorter forms by width, ASCII, two columns of
    // margin, and the Enter key is never cut.
    #[test]
    fn the_delegate_hint_puts_the_keys_first_with_a_form_for_each_width() {
        let full = "Enter delegate | Esc dismiss | no worker ran, nothing written";
        assert_eq!(delegate_hint(200), full);
        assert_eq!(delegate_hint(full.len() + 2), full);
        assert_eq!(
            delegate_hint(full.len() + 1),
            "Enter delegate | Esc dismiss | no worker ran"
        );
        assert_eq!(
            delegate_hint(48),
            "Enter delegate | Esc dismiss | no worker ran"
        );
        assert_eq!(delegate_hint(40), "Enter delegate | no worker ran");
        assert_eq!(delegate_hint(20), "Enter delegate");
        assert_eq!(delegate_hint(3), "Enter delegate");
        for cols in 1..=80 {
            let text = delegate_hint(cols);
            assert!(text.is_ascii() && text.starts_with("Enter delegate"));
        }
    }

    // D24: the key first, the gate and then the step shed by width, ASCII, two
    // columns of margin.
    #[test]
    fn the_gates_hint_names_the_key_in_a_form_for_each_width() {
        let full = "Enter gates off | spawn gate: step explore";
        assert_eq!(gates_hint(200, Gate::Spawn, Step::Explore), full);
        assert_eq!(gates_hint(full.len() + 2, Gate::Spawn, Step::Explore), full);
        assert_eq!(
            gates_hint(full.len() + 1, Gate::Spawn, Step::Explore),
            "Enter gates off | spawn gate"
        );
        assert_eq!(
            gates_hint(60, Gate::Finish, Step::Implement),
            "Enter gates off | finish gate: step implement"
        );
        assert_eq!(
            gates_hint(10, Gate::Spawn, Step::Explore),
            "Enter gates off"
        );
        for cols in 1..=90 {
            let text = gates_hint(cols, Gate::Finish, Step::Implement);
            assert!(text.is_ascii() && text.starts_with("Enter gates off"));
            assert!(
                text.len() + 2 <= cols || text == "Enter gates off",
                "{cols}: {text}"
            );
        }
    }

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
    fn a_model_under_another_served_name_names_both_and_the_fix() {
        let text = guidance(&Availability::ModelNotServed {
            engine: s("llamacpp"),
            model: s("qwen"),
            served: vec![s("/models/a.gguf"), s("b.gguf")],
            port: 8080,
        })
        .unwrap();
        assert!(!text.contains("download"), "{text}");
        for part in [
            "127.0.0.1:8080",
            "\"/models/a.gguf\", \"b.gguf\"",
            "\"qwen\"",
            "--alias qwen",
        ] {
            assert!(text.contains(part), "{part}: {text}");
        }
        assert!(!text.contains("more)"), "{text}");
    }

    #[test]
    fn a_server_that_serves_nothing_says_so() {
        let text = guidance(&Availability::ModelNotServed {
            engine: s("llamacpp"),
            model: s("qwen"),
            served: Vec::new(),
            port: 8080,
        })
        .unwrap();
        assert!(
            text.contains("serves no model") && text.contains("--alias qwen"),
            "{text}"
        );
    }

    // Many long ids never push the fix out: it comes first, at most three ids
    // are listed, each clipped, and the rest are counted.
    #[test]
    fn many_long_served_ids_keep_the_fix_and_a_bounded_line() {
        let served: Vec<String> = (0..256)
            .map(|i| format!("{i}{}", "x".repeat(127)))
            .collect();
        let text = guidance(&Availability::ModelNotServed {
            engine: s("llamacpp"),
            model: s("qwen"),
            served,
            port: 8080,
        })
        .unwrap();
        assert!(text.contains("--alias qwen"), "{text}");
        assert!(text.contains("(+253 more)"), "{text}");
        assert!(text.chars().count() < 400, "{}", text.chars().count());
        let alias = text.find("--alias qwen").unwrap();
        let first_id = text.find("0xxx").unwrap();
        assert!(alias < first_id, "the fix comes before the list: {text}");
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
