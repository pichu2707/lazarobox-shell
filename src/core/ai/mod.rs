//! AI engine catalog and the pure rules that turn raw probe facts into a
//! `Detection`. No IO lives here: the runtime gathers `Probe`s, `reduce`
//! classifies them.

use std::path::PathBuf;

pub mod catalog;
pub mod chat;
pub mod config;
pub mod gguf;
pub mod http;
pub mod jail;
pub mod memory;
pub mod menu;
pub mod parse;
pub mod picker;
pub mod sanitize;
pub mod tools;

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

impl Kind {
    /// Where the engine's adapter sends what a tool returns: a loopback
    /// server keeps it on the machine, a CLI hands it to its provider. Comes
    /// from the kind alone, so no model name can change it.
    ///
    /// Known limit: this is the engine kind, not the route. The Ollama URL is
    /// hard-coded to loopback today, but an Ollama `*-cloud` model goes
    /// through the local daemon to ollama.com and still counts as `Local`.
    /// Accepted. If a host or URL setting is ever added, `Local` must mean
    /// "loopback AND not forwarding".
    pub fn locality(self) -> tools::Locality {
        match self {
            Kind::Server { .. } => tools::Locality::Local,
            Kind::Cli => tools::Locality::Cloud,
        }
    }
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

/// How to start the server engine `id` by hand so that it listens on
/// `127.0.0.1:port`; `None` for an engine that is not a server. Shown, never
/// run: lazarobox does not start servers. `<model…>` is for the user to fill.
pub fn start_command(id: &str, port: u16) -> Option<String> {
    let command = match id {
        "ollama" if port == 11434 => "ollama serve".to_owned(),
        "ollama" => format!("OLLAMA_HOST=127.0.0.1:{port} ollama serve"),
        "lmstudio" => format!("lms server start --port {port}"),
        "llamacpp" => format!("llama-server --port {port} -m <model.gguf>"),
        "vllm" => format!("vllm serve <model> --host 127.0.0.1 --port {port}"),
        _ => return None,
    };
    Some(command)
}

/// The loopback ports of the server engines: each catalog port, unless
/// `[ai.servers.<id>] port` replaced it. Detection and chat read the same
/// value, so an engine is used where it was detected. The host is always
/// `127.0.0.1`.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub struct ServerPorts(Vec<(&'static str, u16)>);

impl ServerPorts {
    /// `port` for the server engine `id`; an id that is not a catalog server
    /// is ignored.
    pub fn with(mut self, id: &str, port: u16) -> Self {
        let server = CATALOG
            .iter()
            .find(|e| e.id == id && matches!(e.kind, Kind::Server { .. }));
        if let Some(engine) = server {
            self.0.retain(|(other, _)| *other != engine.id);
            self.0.push((engine.id, port));
        }
        self
    }

    /// The same `port` for every server engine.
    pub fn every(port: u16) -> Self {
        CATALOG
            .iter()
            .filter(|e| matches!(e.kind, Kind::Server { .. }))
            .fold(Self::default(), |ports, e| ports.with(e.id, port))
    }

    /// Whether `id` has its own port here, not the catalog's.
    pub fn is_set(&self, id: &str) -> bool {
        self.0.iter().any(|(other, _)| *other == id)
    }

    /// The port of the server engine `id`; `None` for any other id.
    pub fn port(&self, id: &str) -> Option<u16> {
        let catalog = CATALOG.iter().find_map(|e| match e.kind {
            Kind::Server { port, .. } if e.id == id => Some(port),
            _ => None,
        })?;
        Some(
            self.0
                .iter()
                .find_map(|(other, port)| (*other == id).then_some(*port))
                .unwrap_or(catalog),
        )
    }

    /// The catalog with these ports in place of the catalog ones.
    pub fn catalog(&self) -> Vec<Engine> {
        CATALOG
            .iter()
            .map(|e| match e.kind {
                Kind::Server { path, owner, .. } => Engine {
                    kind: Kind::Server {
                        port: self.port(e.id).expect("a catalog server has a port"),
                        path,
                        owner,
                    },
                    ..*e
                },
                Kind::Cli => *e,
            })
            .collect()
    }
}

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
    /// `(label, value)` pairs of a CLI model list read from the account
    /// (codex today), best first and without `default`. Empty when the CLI
    /// has no listing or the listing failed.
    pub model_labels: Vec<(String, String)>,
    pub version: Option<String>,
    /// Why something about this engine is degraded, for the menu.
    pub diagnostics: Diagnostics,
}

/// Short, already sanitized reasons the menu shows next to a CLI engine.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Diagnostics {
    /// Why the model list could not be read (`None`: it was, or there is none).
    pub models: Option<String>,
    /// Why this CLI would run without its jail here (`None`: it is jailed,
    /// or it has no jail).
    pub jail: Option<String>,
}

/// One finished run. An empty `Detection` means the run failed.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Detection {
    pub engines: Vec<EngineState>,
    /// The llama.cpp model files found on this machine, sorted by file name;
    /// looked for only when `llama-server` is installed.
    pub local_models: Vec<std::path::PathBuf>,
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
        /// The CLI's model listing (`codex debug models`): its raw output, or
        /// why it failed. `None` when the CLI has no listing.
        listing: Option<Result<Vec<u8>, String>>,
        /// A few words on why the CLI cannot be jailed here; `None` when it
        /// can, or when the CLI has no jail.
        jail: Option<String>,
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
                (
                    Some(Probe::Cli {
                        path: Some(_),
                        stdout,
                        listing,
                        jail,
                        ..
                    }),
                    Kind::Cli,
                ) => cli_state(engine.id, stdout.as_deref(), listing.as_ref(), jail.clone()),
                _ => bare(engine.id, Status::NotInstalled),
            }
        })
        .collect();
    Detection {
        engines,
        ..Default::default()
    }
}

/// What the menu says when the listing's output is not a model catalog.
pub const BAD_LISTING: &str = "unexpected output format";

/// What the menu says when the catalog lists no model.
pub const EMPTY_LISTING: &str = "the list is empty";

/// An available CLI. Only codex has a listing (`codex debug models`); its
/// models come after `default`, and a failed or empty listing leaves just
/// `default` and the reason.
fn cli_state(
    id: &'static str,
    stdout: Option<&[u8]>,
    listing: Option<&Result<Vec<u8>, String>>,
    jail: Option<String>,
) -> EngineState {
    let (labels, note) = match (id, listing) {
        ("codex", Some(Ok(raw))) => {
            match std::str::from_utf8(raw).ok().and_then(parse::codex_catalog) {
                Some(labels) if labels.is_empty() => (labels, Some(EMPTY_LISTING.to_owned())),
                Some(labels) => (labels, None),
                None => (Vec::new(), Some(BAD_LISTING.to_owned())),
            }
        }
        ("codex", Some(Err(why))) => (Vec::new(), Some(why.clone())),
        _ => (Vec::new(), None),
    };
    let models = std::iter::once("default".to_string())
        .chain(labels.iter().map(|(_, value)| value.clone()))
        .collect();
    EngineState {
        models,
        model_labels: labels,
        version: stdout.and_then(parse::parse_version),
        diagnostics: Diagnostics { models: note, jail },
        ..bare(id, Status::Available)
    }
}

fn bare(id: &'static str, status: Status) -> EngineState {
    EngineState {
        id,
        status,
        models: Vec::new(),
        version: None,
        model_labels: Vec::new(),
        diagnostics: Diagnostics::default(),
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

/// The row text of a detected engine that has no chat adapter yet.
const DETECTED_ONLY: &str = "detected · not in chat yet";

/// Display text of an engine: `detecting…` before the first result,
/// `not detected` for an id outside the catalog, `unknown` when the run
/// produced nothing for it.
pub fn status_text(detection: Option<&Detection>, id: &str) -> String {
    let Some(detection) = detection else {
        return "detecting…".to_string();
    };
    let Some(engine) = CATALOG.iter().find(|e| e.id == id) else {
        return "not detected".to_string();
    };
    let Some(state) = detection.engines.iter().find(|s| s.id == id) else {
        return "unknown".to_string();
    };
    match (state.status, engine.kind) {
        (Status::NotRunning, _) => "not running".to_string(),
        (Status::NotInstalled, _) => "not installed".to_string(),
        // Detected, but chat refuses it: say so instead of reading as usable.
        // Asked of the adapter table, so the note goes away with the adapter.
        (Status::Available, _) if chat::adapters::adapter_for(id).is_none() => {
            DETECTED_ONLY.to_string()
        }
        (Status::Available, Kind::Server { .. }) => match state.models.len() {
            1 => "available · 1 model".to_string(),
            n => format!("available · {n} models"),
        },
        (Status::Available, Kind::Cli) => {
            let version = state.version.as_deref().unwrap_or("version unknown");
            // The reason replaces the version: a narrow row clips the tail,
            // and the reason matters more than the version.
            if id == "claude" && chat::adapters::claude_too_old(state.version.as_deref()) {
                return format!(
                    "available · update needed: {}+",
                    chat::adapters::claude_min_version_text()
                );
            }
            match &state.diagnostics.jail {
                Some(why) => format!("available · no jail: {why}"),
                None => format!("available · {version}"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn server_ports_replace_only_the_overridden_catalog_ports() {
        let ports = ServerPorts::default().with("llamacpp", 39080);
        let catalog = ports.catalog();
        assert_eq!(catalog.len(), CATALOG.len());
        for (got, base) in catalog.iter().zip(CATALOG) {
            match (got.id, got.kind, base.kind) {
                (
                    "llamacpp",
                    Kind::Server { port, path, owner },
                    Kind::Server {
                        path: p, owner: o, ..
                    },
                ) => {
                    assert_eq!((port, path, owner), (39080, p, o));
                }
                _ => assert_eq!(got, base),
            }
        }
        assert_eq!(ServerPorts::default().catalog(), CATALOG);
    }

    #[test]
    fn each_server_has_a_loopback_start_command_on_its_port() {
        assert_eq!(
            start_command("ollama", 11434).as_deref(),
            Some("ollama serve")
        );
        assert_eq!(
            start_command("ollama", 11500).as_deref(),
            Some("OLLAMA_HOST=127.0.0.1:11500 ollama serve")
        );
        assert_eq!(
            start_command("lmstudio", 1234).as_deref(),
            Some("lms server start --port 1234")
        );
        assert_eq!(
            start_command("llamacpp", 39080).as_deref(),
            Some("llama-server --port 39080 -m <model.gguf>")
        );
        assert_eq!(
            start_command("vllm", 8000).as_deref(),
            Some("vllm serve <model> --host 127.0.0.1 --port 8000")
        );
        assert_eq!(start_command("claude", 1), None);
    }

    #[test]
    fn a_port_override_for_an_engine_that_is_not_a_server_is_ignored() {
        let ports = ServerPorts::default().with("claude", 9).with("nope", 9);
        assert_eq!(ports, ServerPorts::default());
        assert_eq!(ports.port("claude"), None);
    }

    use super::*;
    use crate::core::ai::tools::Locality;

    // Spec (agent-tools, locality): from the adapter kind, whatever the model.
    #[test]
    fn a_loopback_server_is_local_and_every_cli_is_cloud() {
        for engine in CATALOG {
            let want = match engine.kind {
                Kind::Server { .. } => Locality::Local,
                Kind::Cli => Locality::Cloud,
            };
            assert_eq!(engine.kind.locality(), want, "{}", engine.id);
        }
        let ids = |l: Locality| -> Vec<&str> {
            CATALOG
                .iter()
                .filter(|e| e.kind.locality() == l)
                .map(|e| e.id)
                .collect()
        };
        assert_eq!(
            ids(Locality::Local),
            ["ollama", "lmstudio", "llamacpp", "vllm"]
        );
        assert_eq!(
            ids(Locality::Cloud),
            ["claude", "codex", "gemini", "opencode"]
        );
    }

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
            model_labels: Vec::new(),
            diagnostics: Default::default(),
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
    fn lmstudio_valid_reply_is_available_with_exact_models() {
        let body = r#"{"object":"list","data":[{"id":"gemma-3-4b"},{"id":"qwen3"}]}"#;
        for on_path in [true, false] {
            assert_eq!(
                one("lmstudio", on_path, Some((200, body))),
                state(
                    "lmstudio",
                    Status::Available,
                    &["gemma-3-4b", "qwen3"],
                    None
                )
            );
        }
    }

    #[test]
    fn llamacpp_status_matrix() {
        let ok = Some((200, LLAMA_OK));
        for on_path in [true, false] {
            assert_eq!(
                one("llamacpp", on_path, ok),
                state("llamacpp", Status::Available, &["m.gguf"], None)
            );
        }
        assert_eq!(
            one("llamacpp", true, Some((500, LLAMA_OK))),
            state("llamacpp", Status::NotRunning, &[], None)
        );
        assert_eq!(
            one("llamacpp", false, Some((500, LLAMA_OK))),
            state("llamacpp", Status::NotInstalled, &[], None)
        );
        assert_eq!(
            one("llamacpp", true, None),
            state("llamacpp", Status::NotRunning, &[], None)
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

    fn cli(id: &'static str, path: Option<&str>, stdout: Option<&str>) -> Probe {
        Probe::Cli {
            id,
            path: path.map(PathBuf::from),
            stdout: stdout.map(|s| s.as_bytes().to_vec()),
            listing: None,
            jail: None,
        }
    }

    #[test]
    fn cli_on_path_is_available_with_version_and_default_model() {
        let d = reduce(&[cli("codex", Some("/bin/codex"), Some("codex 0.5.0\n"))]);
        assert_eq!(
            get(&d, "codex"),
            state(
                "codex",
                Status::Available,
                &["default"],
                Some("codex 0.5.0")
            )
        );
    }

    #[test]
    fn codex_listing_becomes_labeled_models_after_default() {
        let raw = include_str!("../../../tests/fixtures/codex/debug-models.json");
        let d = reduce(&[Probe::Cli {
            id: "codex",
            path: Some(PathBuf::from("/bin/codex")),
            stdout: Some(b"codex-cli 0.162.1\n".to_vec()),
            listing: Some(Ok(raw.as_bytes().to_vec())),
            jail: None,
        }]);
        let codex = get(&d, "codex").unwrap();
        assert_eq!(codex.status, Status::Available);
        assert_eq!(codex.model_labels.len(), 7);
        assert_eq!(
            codex.model_labels[0],
            ("GPT-6.1-Sol".to_string(), "gpt-6.1-sol".to_string())
        );
        assert_eq!(codex.models.len(), 8);
        assert_eq!(codex.models[0], "default");
        assert_eq!(codex.models[1], "gpt-6.1-sol");
        assert_eq!(codex.diagnostics.models, None, "a good list needs no note");
    }

    fn codex_with(listing: Option<Result<&[u8], &str>>) -> EngineState {
        let d = reduce(&[Probe::Cli {
            id: "codex",
            path: Some(PathBuf::from("/bin/codex")),
            stdout: None,
            listing: listing.map(|l| l.map(<[u8]>::to_vec).map_err(str::to_owned)),
            jail: None,
        }]);
        get(&d, "codex").unwrap()
    }

    #[test]
    fn a_failed_or_foreign_listing_leaves_just_default_and_says_why() {
        let none = codex_with(None);
        assert_eq!(none.models, ["default"]);
        assert_eq!(none.diagnostics.models, None, "no listing, nothing to say");
        let cases: [(Result<&[u8], &str>, &str); 5] = [
            (
                Err("codex exited 1: not logged in"),
                "codex exited 1: not logged in",
            ),
            (Ok(b"not json"), BAD_LISTING),
            (Ok(b"{\"data\":[]}"), BAD_LISTING),
            (Ok(&[0xff, 0xfe, 0x00]), BAD_LISTING),
            (Ok(b"{\"models\":[]}"), EMPTY_LISTING),
        ];
        for (listing, note) in cases {
            let codex = codex_with(Some(listing));
            assert_eq!(codex.models, ["default"], "{note}");
            assert!(codex.model_labels.is_empty(), "{note}");
            assert_eq!(codex.diagnostics.models.as_deref(), Some(note));
            assert_eq!(codex.status, Status::Available);
        }
    }

    #[test]
    fn only_codex_reads_a_listing() {
        let d = reduce(&[Probe::Cli {
            id: "claude",
            path: Some(PathBuf::from("/bin/claude")),
            stdout: None,
            listing: Some(Ok(
                br#"{"models":[{"slug":"x","visibility":"list"}]}"#.to_vec()
            )),
            jail: None,
        }]);
        assert_eq!(
            get(&d, "claude"),
            state("claude", Status::Available, &["default"], None)
        );
    }

    #[test]
    fn cli_off_path_is_not_installed() {
        let d = reduce(&[cli("opencode", None, None)]);
        assert_eq!(
            get(&d, "opencode"),
            state("opencode", Status::NotInstalled, &[], None)
        );
    }

    #[test]
    fn cli_without_version_output_is_still_available() {
        for stdout in [None, Some(""), Some(" \n")] {
            let d = reduce(&[cli("claude", Some("/bin/claude"), stdout)]);
            assert_eq!(
                get(&d, "claude"),
                state("claude", Status::Available, &["default"], None)
            );
        }
    }

    #[test]
    fn reduce_returns_eight_entries_in_catalog_order() {
        let d = reduce(&[
            cli("opencode", None, None),
            server("vllm", false, None),
            cli("codex", Some("/bin/codex"), None),
        ]);
        let ids: Vec<_> = d.engines.iter().map(|e| e.id).collect();
        assert_eq!(
            ids,
            [
                "ollama", "lmstudio", "llamacpp", "vllm", "claude", "codex", "gemini", "opencode"
            ]
        );
    }

    fn text(d: &Detection, id: &str) -> String {
        status_text(Some(d), id)
    }

    #[test]
    fn status_text_formats_every_status() {
        let models = |n: usize| vec!["m".to_string(); n];
        let mk = |id, status, models, version: Option<&str>| EngineState {
            models,
            version: version.map(str::to_string),
            model_labels: Vec::new(),
            diagnostics: Default::default(),
            ..bare(id, status)
        };
        let d = Detection {
            engines: vec![
                mk("ollama", Status::Available, models(2), None),
                mk("lmstudio", Status::Available, models(1), None),
                mk("llamacpp", Status::Available, models(0), None),
                mk("vllm", Status::NotRunning, vec![], None),
                mk("claude", Status::Available, models(1), None),
                mk("codex", Status::Available, models(1), Some("0.5.0")),
                mk("gemini", Status::NotInstalled, vec![], None),
            ],
            ..Default::default()
        };
        assert_eq!(text(&d, "ollama"), "available · 2 models");
        assert_eq!(text(&d, "lmstudio"), "available · 1 model");
        assert_eq!(text(&d, "llamacpp"), "available · 0 models");
        assert_eq!(text(&d, "vllm"), "not running");
        assert_eq!(text(&d, "claude"), "available · version unknown");
        assert_eq!(text(&d, "codex"), "available · 0.5.0");
        assert_eq!(text(&d, "gemini"), "not installed");
    }

    #[test]
    fn a_cli_without_a_jail_puts_the_reason_where_a_narrow_row_keeps_it() {
        let mut codex = state_of("codex", Some("codex-cli 0.130.0"));
        codex.diagnostics.jail = Some("Landlock ABI 2, needs 3+".to_owned());
        let d = Detection {
            engines: vec![codex],
            ..Default::default()
        };
        assert_eq!(
            text(&d, "codex"),
            "available · no jail: Landlock ABI 2, needs 3+"
        );
        let mut unknown = state_of("gemini", None);
        unknown.diagnostics.jail = Some("~/.nvm too broad".to_owned());
        let d = Detection {
            engines: vec![unknown],
            ..Default::default()
        };
        assert_eq!(text(&d, "gemini"), "available · no jail: ~/.nvm too broad");
    }

    fn state_of(id: &'static str, version: Option<&str>) -> EngineState {
        EngineState {
            version: version.map(str::to_owned),
            ..bare(id, Status::Available)
        }
    }

    #[test]
    fn a_jail_note_of_the_probe_reaches_the_state_of_a_cli_only() {
        let probe = |id, jail: Option<&str>| Probe::Cli {
            id,
            path: Some(PathBuf::from("/bin/x")),
            stdout: None,
            listing: None,
            jail: jail.map(str::to_owned),
        };
        let d = reduce(&[probe("codex", Some("needs Linux")), probe("claude", None)]);
        assert_eq!(
            get(&d, "codex").unwrap().diagnostics.jail.as_deref(),
            Some("needs Linux")
        );
        assert_eq!(get(&d, "claude").unwrap().diagnostics.jail, None);
    }

    #[test]
    fn status_text_covers_pending_failed_and_foreign_ids() {
        assert_eq!(status_text(None, "ollama"), "detecting…");
        assert_eq!(text(&Detection::default(), "ollama"), "unknown");
        assert_eq!(text(&reduce(&[]), "mystery"), "not detected");
    }

    #[test]
    fn a_detected_engine_without_a_chat_adapter_says_it_is_detected_only() {
        let d = Detection {
            engines: CATALOG
                .iter()
                .map(|e| EngineState {
                    models: vec!["m".to_owned()],
                    version: Some("1.19.0".to_owned()),
                    ..bare(e.id, Status::Available)
                })
                .collect(),
            ..Default::default()
        };
        let (mut with, mut without) = (0, 0);
        for e in CATALOG {
            let row = status_text(Some(&d), e.id);
            if chat::adapters::adapter_for(e.id).is_some() {
                with += 1;
                assert!(!row.contains("not in chat"), "{}: {row}", e.id);
            } else {
                without += 1;
                assert_eq!(row, "detected · not in chat yet", "{}", e.id);
            }
        }
        // Both branches ran: the loop is not vacuous.
        assert!(with > 0 && without > 0);
    }

    #[test]
    fn an_engine_without_an_adapter_that_is_not_detected_keeps_its_plain_status() {
        let d = Detection {
            engines: vec![
                bare("opencode", Status::NotInstalled),
                bare("vllm", Status::NotRunning),
            ],
            ..Default::default()
        };
        assert_eq!(text(&d, "opencode"), "not installed");
        assert_eq!(text(&d, "vllm"), "not running");
    }

    #[test]
    fn an_outdated_claude_row_says_an_update_is_needed() {
        let mk = |id, version: Option<&str>| EngineState {
            id,
            status: Status::Available,
            models: vec!["default".to_owned()],
            version: version.map(str::to_owned),
            model_labels: Vec::new(),
            diagnostics: Diagnostics::default(),
        };
        let d = Detection {
            engines: vec![
                mk("claude", Some("2.1.200 (Claude Code)")),
                mk("codex", Some("codex-cli 0.1.0")),
            ],
            ..Default::default()
        };
        assert_eq!(
            status_text(Some(&d), "claude"),
            "available · update needed: 2.1.259+"
        );
        // Only claude is gated.
        assert_eq!(
            status_text(Some(&d), "codex"),
            "available · codex-cli 0.1.0"
        );
        for version in [Some("2.1.259 (Claude Code)"), None, Some("weird")] {
            let d = Detection {
                engines: vec![mk("claude", version)],
                ..Default::default()
            };
            let want = format!("available · {}", version.unwrap_or("version unknown"));
            assert_eq!(status_text(Some(&d), "claude"), want, "{version:?}");
        }
    }
}
