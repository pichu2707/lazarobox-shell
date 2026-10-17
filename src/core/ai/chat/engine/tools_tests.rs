use super::*;

fn origin() -> Option<PathBuf> {
    Some(PathBuf::from("/home/u/dev"))
}

#[test]
fn nothing_is_verified_yet() {
    assert!(MCP_VERIFIED.is_empty());
    for engine in ["claude", "codex", "gemini", "ollama"] {
        assert_eq!(tools_for(engine, true, origin(), MCP_VERIFIED), Tools::Off);
    }
}

#[test]
fn a_verified_engine_with_tools_enabled_gets_them_with_the_origin() {
    assert_eq!(
        tools_for("claude", true, origin(), &["claude"]),
        Tools::On { origin: origin() }
    );
}

#[test]
fn tools_off_or_an_unverified_engine_stay_off() {
    assert_eq!(
        tools_for("claude", false, origin(), &["claude"]),
        Tools::Off
    );
    assert_eq!(tools_for("codex", true, origin(), &["claude"]), Tools::Off);
}
