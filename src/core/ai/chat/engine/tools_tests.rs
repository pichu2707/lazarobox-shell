use super::*;

fn origin() -> Option<PathBuf> {
    Some(PathBuf::from("/home/u/dev"))
}

#[test]
fn the_four_engines_are_verified() {
    assert_eq!(MCP_VERIFIED, ["claude", "codex", "gemini", "ollama"]);
    for engine in MCP_VERIFIED {
        assert_eq!(
            tools_for(engine, true, origin(), MCP_VERIFIED),
            Tools::On {
                origin: origin(),
                mcp: None
            },
            "{engine}"
        );
        assert_eq!(
            tools_for(engine, false, origin(), MCP_VERIFIED),
            Tools::Off,
            "{engine}"
        );
    }
}

#[test]
fn with_an_empty_verified_set_every_engine_stays_off() {
    for engine in ["claude", "codex", "gemini", "ollama"] {
        assert_eq!(tools_for(engine, true, origin(), &[]), Tools::Off);
    }
}

#[test]
fn a_verified_engine_with_tools_enabled_gets_them_with_the_origin() {
    assert_eq!(
        tools_for("claude", true, origin(), &["claude"]),
        Tools::On {
            origin: origin(),
            mcp: None
        }
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
