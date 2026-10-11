//! Tests of the AI section of the menu, at the `MenuState` level.

use super::*;
use crate::core::ai::{CATALOG, EngineState, Status};

fn engine(id: &'static str, status: Status, models: &[&str], version: Option<&str>) -> EngineState {
    EngineState {
        id,
        status,
        models: models.iter().map(|m| (*m).to_owned()).collect(),
        version: version.map(str::to_owned),
    }
}

/// All eight catalog engines in order; the ones not listed are
/// `NotInstalled`.
fn detection(found: &[EngineState]) -> Detection {
    Detection {
        engines: CATALOG
            .iter()
            .map(|e| {
                found
                    .iter()
                    .find(|s| s.id == e.id)
                    .cloned()
                    .unwrap_or_else(|| engine(e.id, Status::NotInstalled, &[], None))
            })
            .collect(),
    }
}

fn machine() -> Detection {
    detection(&[
        engine(
            "ollama",
            Status::Available,
            &["llama3:8b", "qwen2:7b"],
            None,
        ),
        engine("lmstudio", Status::NotRunning, &[], None),
        engine("claude", Status::Available, &["default"], Some("2.1.0")),
        engine("codex", Status::Available, &["default"], Some("0.5.0")),
    ])
}

fn saved(engine: Option<&str>, model: Option<&str>) -> AiConfig {
    AiConfig::general(engine, model)
}

fn menu(engine: Option<&str>, model: Option<&str>, found: Option<Detection>) -> MenuState {
    MenuState::open(&Config::default()).with_ai(saved(engine, model), found)
}

// Spec: Unchanged AI draft, Undetected saved engine survives (edit part).
#[test]
fn an_unchanged_ai_draft_adds_no_edits() {
    for m in [
        menu(Some("ollama"), Some("llama3:8b"), Some(machine())),
        menu(Some("codex"), Some("gone"), Some(detection(&[]))),
        menu(Some("claude"), None, None),
        menu(None, None, Some(machine())),
    ] {
        assert_eq!(m.edits(&Config::default()), vec![]);
        let mut live = Config::default();
        live.bars.statusline = BarPosition::Top;
        assert_eq!(
            m.edits(&live),
            vec![ConfigEdit {
                table: "statusline",
                key: "position",
                value: "top".into()
            }]
        );
    }
}
