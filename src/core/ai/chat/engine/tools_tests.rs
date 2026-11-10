use super::*;

fn origin() -> Option<PathBuf> {
    Some(PathBuf::from("/home/u/dev"))
}

#[test]
fn the_five_engines_are_verified() {
    assert_eq!(
        MCP_VERIFIED,
        ["claude", "codex", "gemini", "ollama", "llamacpp"]
    );
    for engine in MCP_VERIFIED {
        assert_eq!(
            tools_for(engine, true, origin(), MCP_VERIFIED),
            Tools::On {
                origin: origin(),
                mcp: None,
                external: Default::default(),
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
            mcp: None,
            external: Default::default(),
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

#[test]
fn the_text_only_servers_and_opencode_stay_off() {
    for engine in ["lmstudio", "vllm", "opencode"] {
        assert_eq!(
            tools_for(engine, true, origin(), MCP_VERIFIED),
            Tools::Off,
            "{engine}"
        );
    }
}

#[test]
fn the_readme_states_llamacpp_tools_and_their_template_requirement() {
    let readme = include_str!("../../../../../README.md");
    assert!(
        !readme.contains("Ninguno de los tres muestra razonamiento ni ofrece herramientas"),
        "the old limit still says llama.cpp offers no tools"
    );
    let section = readme
        .split("### Tools with llama.cpp")
        .nth(1)
        .expect("a `### Tools with llama.cpp` section");
    let section = section.split("\n### ").next().unwrap();
    for needle in ["--jinja", "Qwen2.5", "texto"] {
        assert!(section.contains(needle), "{needle}");
    }
}

// The screen the user approved never reaches a log: Debug shows its size only.
#[test]
fn a_screen_answer_debugs_as_a_size() {
    let answer = Answer::Screen(ScreenText("password: hunter2".to_owned()));
    let shown = format!("{answer:?}");
    assert_eq!(shown, "Screen(ScreenText(17 bytes))");
    assert!(!shown.contains("hunter2"));
    assert_ne!(answer, Answer::Decided(Decision::Sent));
}
