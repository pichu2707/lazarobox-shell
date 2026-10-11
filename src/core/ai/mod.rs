//! AI engine catalog and the pure rules that turn raw probe facts into a
//! `Detection`. No IO lives here: the runtime gathers `Probe`s, `reduce`
//! classifies them.

use std::path::PathBuf;

pub mod http;
pub mod parse;

/// How an engine is detected.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// A local model server probed over HTTP on `127.0.0.1`.
    Server {
        port: u16,
        path: &'static str,
        /// Value `owned_by` must carry in at least one entry, if any.
        owner: Option<&'static str>,
    },
    /// An agent CLI found on PATH.
    Cli,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Engine {
    pub id: &'static str,
    pub kind: Kind,
    pub binary: &'static str,
}

const fn server(
    id: &'static str,
    port: u16,
    path: &'static str,
    owner: Option<&'static str>,
    binary: &'static str,
) -> Engine {
    Engine {
        id,
        kind: Kind::Server { port, path, owner },
        binary,
    }
}

const fn cli(id: &'static str) -> Engine {
    Engine {
        id,
        kind: Kind::Cli,
        binary: id,
    }
}

/// The fixed, ordered set of detectable engines.
pub const CATALOG: &[Engine] = &[
    server("ollama", 11434, "/api/tags", None, "ollama"),
    server("lmstudio", 1234, "/v1/models", None, "lms"),
    server(
        "llamacpp",
        8080,
        "/v1/models",
        Some("llamacpp"),
        "llama-server",
    ),
    server("vllm", 8000, "/v1/models", Some("vllm"), "vllm"),
    cli("claude"),
    cli("codex"),
    cli("gemini"),
    cli("opencode"),
];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    Available,
    NotRunning,
    NotInstalled,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct EngineState {
    pub id: &'static str,
    pub status: Status,
    pub models: Vec<String>,
    pub version: Option<String>,
}

/// One finished run. An empty `Detection` means the run failed.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Detection {
    pub engines: Vec<EngineState>,
}

/// Raw facts gathered by the runtime for one engine.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Probe {
    Server {
        id: &'static str,
        on_path: bool,
        reply: Option<(u16, Vec<u8>)>,
    },
    Cli {
        id: &'static str,
        path: Option<PathBuf>,
        stdout: Option<Vec<u8>>,
    },
}

/// Classifies raw probes into one state per catalog engine, in catalog
/// order. An engine without a probe is `NotInstalled`.
pub fn reduce(probes: &[Probe]) -> Detection {
    let engines = CATALOG
        .iter()
        .map(|engine| {
            let probe = probes.iter().find(|p| match p {
                Probe::Server { id, .. } | Probe::Cli { id, .. } => *id == engine.id,
            });
            match (probe, engine.kind) {
                (Some(Probe::Server { on_path, reply, .. }), Kind::Server { owner, .. }) => {
                    server_state(engine.id, owner, *on_path, reply.as_ref())
                }
                _ => bare(engine.id, Status::NotInstalled),
            }
        })
        .collect();
    Detection { engines }
}

fn bare(id: &'static str, status: Status) -> EngineState {
    EngineState {
        id,
        status,
        models: Vec::new(),
        version: None,
    }
}

fn server_state(
    id: &'static str,
    owner: Option<&str>,
    on_path: bool,
    reply: Option<&(u16, Vec<u8>)>,
) -> EngineState {
    let models = reply
        .filter(|(code, body)| *code == 200 && body.len() <= http::MAX_BODY)
        .and_then(|(_, body)| match id {
            "ollama" => parse::ollama_tags(body),
            _ => parse::openai_models(body, owner),
        });
    match models {
        Some(models) => EngineState {
            models,
            ..bare(id, Status::Available)
        },
        None if on_path => bare(id, Status::NotRunning),
        None => bare(id, Status::NotInstalled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_the_eight_engines_in_order() {
        let ids: Vec<_> = CATALOG.iter().map(|e| e.id).collect();
        assert_eq!(
            ids,
            [
                "ollama", "lmstudio", "llamacpp", "vllm", "claude", "codex", "gemini", "opencode"
            ]
        );
        let bins: Vec<_> = CATALOG.iter().map(|e| e.binary).collect();
        assert_eq!(
            bins,
            [
                "ollama",
                "lms",
                "llama-server",
                "vllm",
                "claude",
                "codex",
                "gemini",
                "opencode"
            ]
        );
    }

    #[test]
    fn catalog_servers_are_local_and_clis_have_no_endpoint() {
        let kinds: Vec<_> = CATALOG.iter().map(|e| e.kind).collect();
        let server = |port, path, owner| Kind::Server { port, path, owner };
        assert_eq!(
            kinds,
            [
                server(11434, "/api/tags", None),
                server(1234, "/v1/models", None),
                server(8080, "/v1/models", Some("llamacpp")),
                server(8000, "/v1/models", Some("vllm")),
                Kind::Cli,
                Kind::Cli,
                Kind::Cli,
                Kind::Cli,
            ]
        );
    }

    const OLLAMA_OK: &str = r#"{"models":[{"name":"llama3:8b"},{"name":"qwen2.5:7b"}]}"#;
    const LLAMA_OK: &str = r#"{"data":[{"id":"m.gguf","owned_by":"llamacpp"}]}"#;

    fn server(id: &'static str, on_path: bool, reply: Option<(u16, &str)>) -> Probe {
        Probe::Server {
            id,
            on_path,
            reply: reply.map(|(code, body)| (code, body.as_bytes().to_vec())),
        }
    }

    fn state(
        id: &'static str,
        status: Status,
        models: &[&str],
        version: Option<&str>,
    ) -> Option<EngineState> {
        Some(EngineState {
            id,
            status,
            models: models.iter().map(|m| m.to_string()).collect(),
            version: version.map(str::to_string),
        })
    }

    fn get(d: &Detection, id: &str) -> Option<EngineState> {
        d.engines.iter().find(|e| e.id == id).cloned()
    }

    /// Runs `reduce` over one server probe and returns its state.
    fn one(id: &'static str, on_path: bool, reply: Option<(u16, &str)>) -> Option<EngineState> {
        get(&reduce(&[server(id, on_path, reply)]), id)
    }

    #[test]
    fn server_matrix() {
        let ok = Some((200, OLLAMA_OK));
        let models = ["llama3:8b", "qwen2.5:7b"];
        assert_eq!(
            one("ollama", false, ok),
            state("ollama", Status::Available, &models, None)
        );
        assert_eq!(
            one("ollama", true, ok),
            state("ollama", Status::Available, &models, None)
        );
        assert_eq!(
            one("ollama", true, Some((200, "<html>"))),
            state("ollama", Status::NotRunning, &[], None)
        );
        assert_eq!(
            one("ollama", false, Some((200, "<html>"))),
            state("ollama", Status::NotInstalled, &[], None)
        );
    }

    #[test]
    fn closed_port_follows_the_binary_rule() {
        assert_eq!(
            one("lmstudio", true, None),
            state("lmstudio", Status::NotRunning, &[], None)
        );
        assert_eq!(
            one("lmstudio", false, None),
            state("lmstudio", Status::NotInstalled, &[], None)
        );
    }

    #[test]
    fn non_200_and_oversize_replies_are_invalid() {
        for code in [404, 503, 301] {
            assert_eq!(
                one("ollama", true, Some((code, OLLAMA_OK))),
                state("ollama", Status::NotRunning, &[], None)
            );
        }
        let big = format!(
            r#"{{"models":[{{"name":"{}"}}]}}"#,
            "x".repeat(2 * 1024 * 1024)
        );
        assert_eq!(
            one("ollama", true, Some((200, &big))),
            state("ollama", Status::NotRunning, &[], None)
        );
    }

    #[test]
    fn owned_servers_need_their_marker() {
        let foreign = r#"{"data":[{"id":"gpt-x","owned_by":"localai"}]}"#;
        let d = reduce(&[
            server("llamacpp", true, Some((200, foreign))),
            server("vllm", false, Some((200, LLAMA_OK))),
        ]);
        assert_eq!(
            get(&d, "llamacpp"),
            state("llamacpp", Status::NotRunning, &[], None)
        );
        assert_eq!(
            get(&d, "vllm"),
            state("vllm", Status::NotInstalled, &[], None)
        );
        assert_eq!(
            one("llamacpp", false, Some((200, LLAMA_OK))),
            state("llamacpp", Status::Available, &["m.gguf"], None)
        );
    }

    #[test]
    fn foreign_json_with_a_binary_is_not_running() {
        for body in [
            r#"{"status":"ok"}"#,
            r#"{"data":"x"}"#,
            r#"{"data":[]}"#,
            "[]",
        ] {
            assert_eq!(
                one("llamacpp", true, Some((200, body))),
                state("llamacpp", Status::NotRunning, &[], None)
            );
        }
        assert_eq!(
            one(
                "llamacpp",
                false,
                Some((200, "<html><body>It works</body></html>"))
            ),
            state("llamacpp", Status::NotInstalled, &[], None)
        );
    }
}
