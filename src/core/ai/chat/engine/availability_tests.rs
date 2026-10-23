use super::*;
use crate::core::ai::{
    EngineState,
    config::Assignment,
    jail::{Backend, JailOff, JailSupport},
};

const JAIL: JailSupport = JailSupport::Available {
    backend: Backend::Landlock,
    abi: 6,
};
const NO_JAIL: JailSupport = JailSupport::Unavailable(JailOff::NoBackend);

fn cfg(task: Task, engine: Option<&str>, model: Option<&str>) -> AiConfig {
    let mut cfg = AiConfig::default();
    cfg.set(
        task,
        Assignment {
            engine: engine.map(str::to_owned),
            model: model.map(str::to_owned),
        },
    );
    cfg
}

fn state(id: &'static str, status: Status, models: &[&str]) -> EngineState {
    EngineState {
        id,
        status,
        models: models.iter().map(|m| (*m).to_owned()).collect(),
        version: None,
        model_labels: Vec::new(),
        diagnostics: Default::default(),
    }
}

fn detected(states: Vec<EngineState>) -> Detection {
    Detection { engines: states }
}

fn ready(engine: &str, model: Option<&str>) -> Availability {
    Availability::Ready {
        engine: engine.to_owned(),
        model: model.map(str::to_owned),
    }
}

#[test]
fn a_listed_model_on_an_available_server_is_ready() {
    let c = cfg(Task::Chat, Some("ollama"), Some("llama3:8b"));
    let d = detected(vec![state("ollama", Status::Available, &["llama3:8b"])]);
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        ready("ollama", Some("llama3:8b"))
    );
}

#[test]
fn chat_falls_back_to_general() {
    let c = cfg(Task::General, Some("claude"), Some("sonnet"));
    let d = detected(vec![state("claude", Status::Available, &[])]);
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        ready("claude", Some("sonnet"))
    );
}

#[test]
fn nothing_assigned_is_unassigned_even_before_detection() {
    let c = AiConfig::default();
    assert_eq!(availability(&c, None, JAIL), Availability::Unassigned);
    let d = detected(vec![]);
    assert_eq!(availability(&c, Some(&d), JAIL), Availability::Unassigned);
}

#[test]
fn no_detection_yet_is_checking() {
    let c = cfg(Task::Chat, Some("ollama"), Some("llama3:8b"));
    assert_eq!(availability(&c, None, JAIL), Availability::Checking);
}

#[test]
fn an_engine_that_is_not_available_is_undetected_with_its_status() {
    let c = cfg(Task::Chat, Some("ollama"), Some("llama3:8b"));
    let d = detected(vec![state("ollama", Status::NotInstalled, &[])]);
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        Availability::Undetected {
            engine: "ollama".into(),
            status: "not installed".into()
        }
    );
    let d = detected(vec![state("ollama", Status::NotRunning, &[])]);
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        Availability::Undetected {
            engine: "ollama".into(),
            status: "not running".into()
        }
    );
}

#[test]
fn a_failed_detection_run_is_undetected_unknown() {
    let c = cfg(Task::Chat, Some("ollama"), Some("llama3:8b"));
    let d = detected(vec![]);
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        Availability::Undetected {
            engine: "ollama".into(),
            status: "unknown".into()
        }
    );
}

#[test]
fn opencode_has_no_adapter_even_when_available() {
    let c = cfg(Task::Chat, Some("opencode"), Some("default"));
    let d = detected(vec![state("opencode", Status::Available, &[])]);
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        Availability::Unsupported {
            engine: "opencode".into()
        }
    );
    assert_eq!(
        availability(&c, None, JAIL),
        Availability::Unsupported {
            engine: "opencode".into()
        }
    );
}

#[test]
fn codex_and_gemini_need_a_jail() {
    for id in ["codex", "gemini"] {
        let c = cfg(Task::Chat, Some(id), Some("default"));
        let d = detected(vec![state(
            if id == "codex" { "codex" } else { "gemini" },
            Status::Available,
            &[],
        )]);
        assert_eq!(
            availability_with(&c, Some(&d), NO_JAIL, &[], &|_| true),
            Availability::NoJail {
                engine: id.into(),
                reason: JailOff::NoBackend.text_required(),
            }
        );
        assert_eq!(
            availability_with(&c, Some(&d), JAIL, &[], &|_| true),
            ready(id, Some("default"))
        );
    }
}

#[test]
fn the_no_jail_reason_names_the_kernel_abi() {
    let c = cfg(Task::Chat, Some("codex"), Some("default"));
    let d = detected(vec![state("codex", Status::Available, &[])]);
    let old_kernel = JailSupport::Unavailable(JailOff::NoLandlock(2));
    assert_eq!(
        availability(&c, Some(&d), old_kernel),
        Availability::NoJail {
            engine: "codex".into(),
            reason: "Landlock ABI 3+ needed (Linux 6.2); this kernel has ABI 2".into()
        }
    );
}

#[test]
fn engines_without_an_adapter_are_unsupported_whatever_the_jail() {
    for id in ["opencode"] {
        let c = cfg(Task::Chat, Some(id), Some("default"));
        let d = detected(vec![state(id, Status::Available, &["default"])]);
        for jail in [NO_JAIL, JAIL] {
            assert_eq!(
                availability(&c, Some(&d), jail),
                Availability::Unsupported { engine: id.into() },
                "{id}"
            );
        }
    }
}

#[test]
fn codex_is_ready_with_a_jail_and_no_jail_otherwise() {
    let c = cfg(Task::Chat, Some("codex"), Some("default"));
    let d = detected(vec![state("codex", Status::Available, &[])]);
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        ready("codex", Some("default"))
    );
    assert_eq!(
        availability(&c, Some(&d), NO_JAIL),
        Availability::NoJail {
            engine: "codex".into(),
            reason:
                "the CLI jail needs Linux or macOS; on this system use claude or Ollama for chat"
                    .into()
        }
    );
}

#[test]
fn gemini_is_ready_with_a_jail_and_no_jail_otherwise() {
    let c = cfg(Task::Chat, Some("gemini"), Some("default"));
    let d = detected(vec![state("gemini", Status::Available, &[])]);
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        ready("gemini", Some("default"))
    );
    assert_eq!(
        availability(&c, Some(&d), NO_JAIL),
        Availability::NoJail {
            engine: "gemini".into(),
            reason:
                "the CLI jail needs Linux or macOS; on this system use claude or Ollama for chat"
                    .into()
        }
    );
}

#[test]
fn claude_runs_without_a_jail() {
    let c = cfg(Task::Chat, Some("claude"), Some("sonnet"));
    let d = detected(vec![state("claude", Status::Available, &[])]);
    assert_eq!(
        availability(&c, Some(&d), NO_JAIL),
        ready("claude", Some("sonnet"))
    );
}

#[test]
fn a_cli_whose_manual_check_failed_is_gated_off() {
    let c = cfg(Task::Chat, Some("codex"), Some("default"));
    let d = detected(vec![state("codex", Status::Available, &[])]);
    assert_eq!(
        availability_with(&c, Some(&d), JAIL, &["codex"], &|_| true),
        Availability::GatedOff {
            engine: "codex".into()
        }
    );
    assert_eq!(
        availability_with(&c, Some(&d), JAIL, &["gemini"], &|_| true),
        ready("codex", Some("default"))
    );
    assert_eq!(
        availability_with(&c, Some(&d), JAIL, &[], &|_| true),
        ready("codex", Some("default")),
        "nothing is gated off yet"
    );
}

#[test]
fn a_server_needs_a_model() {
    let d = detected(vec![state("ollama", Status::Available, &["llama3:8b"])]);
    for model in [None, Some("")] {
        let c = cfg(Task::Chat, Some("ollama"), model);
        assert_eq!(
            availability(&c, Some(&d), JAIL),
            Availability::NoModel {
                engine: "ollama".into()
            }
        );
    }
}

#[test]
fn a_model_that_is_not_listed_needs_a_download() {
    let c = cfg(Task::Chat, Some("ollama"), Some("old:7b"));
    let d = detected(vec![state("ollama", Status::Available, &["llama3:8b"])]);
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        Availability::ModelMissing {
            engine: "ollama".into(),
            model: "old:7b".into()
        }
    );
}

#[test]
fn latest_is_equivalent_to_the_bare_name_both_ways() {
    let d = detected(vec![state("ollama", Status::Available, &["llama3:latest"])]);
    let c = cfg(Task::Chat, Some("ollama"), Some("llama3"));
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        ready("ollama", Some("llama3"))
    );
    let d = detected(vec![state("ollama", Status::Available, &["llama3"])]);
    let c = cfg(Task::Chat, Some("ollama"), Some("llama3:latest"));
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        ready("ollama", Some("llama3:latest"))
    );
    let c = cfg(Task::Chat, Some("ollama"), Some("llama3:8b"));
    assert!(matches!(
        availability(&c, Some(&d), JAIL),
        Availability::ModelMissing { .. }
    ));
}

#[test]
fn it_never_switches_engine() {
    let c = cfg(Task::Chat, Some("ollama"), Some("llama3:8b"));
    let before = c.clone();
    let d = detected(vec![
        state("ollama", Status::NotRunning, &[]),
        state("claude", Status::Available, &[]),
    ]);
    assert_eq!(
        availability(&c, Some(&d), JAIL),
        Availability::Undetected {
            engine: "ollama".into(),
            status: "not running".into()
        }
    );
    assert_eq!(c, before);
}

#[test]
fn only_opencode_has_no_adapter() {
    let mut without: Vec<_> = CATALOG
        .iter()
        .filter(|e| !has_adapter(e.id))
        .map(|e| e.id)
        .collect();
    without.sort_unstable();
    assert_eq!(without, ["opencode"]);
}

fn claude_at(version: Option<&str>) -> Detection {
    let mut claude = state("claude", Status::Available, &[]);
    claude.version = version.map(str::to_owned);
    detected(vec![claude])
}

#[test]
fn a_claude_below_the_minimum_is_outdated_and_never_ready() {
    let c = cfg(Task::Chat, Some("claude"), Some("sonnet"));
    for jail in [JAIL, NO_JAIL] {
        assert_eq!(
            availability(&c, Some(&claude_at(Some("2.1.258 (Claude Code)"))), jail),
            Availability::Outdated {
                engine: "claude".into(),
                version: "2.1.258".into(),
            }
        );
    }
}

#[test]
fn a_claude_at_the_minimum_or_newer_is_ready() {
    let c = cfg(Task::Chat, Some("claude"), Some("sonnet"));
    for line in ["2.1.259 (Claude Code)", "2.1.302 (Claude Code)"] {
        assert_eq!(
            availability(&c, Some(&claude_at(Some(line))), JAIL),
            ready("claude", Some("sonnet")),
            "{line}"
        );
    }
}

#[test]
fn a_claude_with_no_known_version_fails_open_and_is_ready() {
    let c = cfg(Task::Chat, Some("claude"), Some("sonnet"));
    for version in [None, Some("version unknown"), Some("2.1")] {
        assert_eq!(
            availability(&c, Some(&claude_at(version)), JAIL),
            ready("claude", Some("sonnet")),
            "{version:?}"
        );
    }
}

// The gate is claude's alone: another CLI's version number means something else.
#[test]
fn an_old_looking_version_of_another_cli_is_not_gated() {
    let c = cfg(Task::Chat, Some("codex"), Some("default"));
    let mut codex = state("codex", Status::Available, &[]);
    codex.version = Some("codex-cli 0.1.0".into());
    assert_eq!(
        availability(&c, Some(&detected(vec![codex])), JAIL),
        ready("codex", Some("default"))
    );
}

fn managed_cfg(model: Option<&str>) -> AiConfig {
    cfg(Task::Chat, Some("llamacpp"), model).with_managed(crate::core::ai::config::ManagedServer {
        model: "/m/q.gguf".into(),
    })
}

// Spec: A managed llama-server starts on the first turn (chat gate).
#[test]
fn a_managed_llamacpp_that_is_installed_is_ready_running_or_not() {
    for status in [Status::NotRunning, Status::Available] {
        // A running llama-server lists its model by path, never by the
        // assigned name: llama-server ignores the name anyway.
        let d = detected(vec![state("llamacpp", status, &["/m/q.gguf"])]);
        assert_eq!(
            availability(&managed_cfg(Some("qwen2.5")), Some(&d), JAIL),
            ready("llamacpp", Some("qwen2.5"))
        );
        assert_eq!(
            availability(&managed_cfg(None), Some(&d), NO_JAIL),
            ready("llamacpp", Some("/m/q.gguf"))
        );
    }
}

// Spec: A managed llama-server starts on the first turn (not installed).
#[test]
fn a_managed_llamacpp_that_is_not_installed_is_still_undetected() {
    let d = detected(vec![state("llamacpp", Status::NotInstalled, &[])]);
    assert!(matches!(
        availability(&managed_cfg(Some("qwen2.5")), Some(&d), JAIL),
        Availability::Undetected { .. }
    ));
}

// Spec: A managed llama-server is opt-in (chat gate unchanged without it).
#[test]
fn an_unmanaged_llamacpp_that_is_not_running_is_undetected() {
    let c = cfg(Task::Chat, Some("llamacpp"), Some("qwen2.5"));
    let d = detected(vec![state("llamacpp", Status::NotRunning, &[])]);
    assert!(matches!(
        availability(&c, Some(&d), JAIL),
        Availability::Undetected { .. }
    ));
}
