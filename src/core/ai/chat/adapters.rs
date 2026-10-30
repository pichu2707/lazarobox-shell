//! Engine adapters as pure builders: a `ChatRequest` in, the description of
//! what to run out. Nothing here touches the network; the runtime executes the
//! `Invocation`.

use serde_json::{Value, json};

use super::{
    engine::{ChatMessage, ChatRequest, MCP_VERIFIED, Role, Tools},
    prompts,
};
use crate::core::ai::{
    CATALOG, Kind,
    jail::{Cli, JailPolicy, Os, jail_policy},
    parse,
    tools::{registry_with, wiring::McpWire},
};

/// The wire format of a local server's reply.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wire {
    Ndjson,
    Sse,
    /// The stream-json output of the claude CLI.
    ClaudeStreamJson,
    /// The `--json` output of `codex exec`.
    CodexJsonl,
    /// The `-o stream-json` output of the gemini CLI.
    GeminiStreamJson,
}

/// The engines that can chat. opencode has no adapter yet.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Adapter {
    Ollama,
    /// The OpenAI-compatible servers: `/v1/chat/completions` over SSE.
    Lmstudio,
    Llamacpp,
    Vllm,
    Claude,
    Codex,
    Gemini,
}

/// What the runtime runs for one turn.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Invocation {
    /// One POST to `127.0.0.1:port`, the reply decoded as `wire`.
    Http {
        wire: Wire,
        port: u16,
        path: &'static str,
        body: Vec<u8>,
    },
    /// One run of a CLI: the transcript on stdin (written, then closed), the
    /// reply decoded as `wire`. The runtime finds `binary` on `PATH`, and
    /// `jail` says what happens when the system cannot confine it.
    Process(Job),
    /// The request cannot be sent as it is; nothing runs and the turn fails
    /// with this message.
    Refused(&'static str),
}

/// One CLI run.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Job {
    pub wire: Wire,
    pub binary: &'static str,
    pub cli: Cli,
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
    pub jail: JailPolicy,
    /// Files the runtime writes into the turn's private temp folder, each
    /// named to the CLI with its flag, appended after `args` in this order.
    pub tmp_files: Vec<TmpFile>,
    /// The tool session the CLI connects to (tools on, engine verified). The
    /// runtime turns it into the engine's config and flags; if that fails the
    /// turn runs as text.
    pub mcp: Option<McpWire>,
    /// What the turn sends instead of `args` and `stdin` when it really runs
    /// wired: the same with the tools prompt. Only `Some` with `mcp`; the
    /// runtime takes it on the attempt that carries the wiring and on no other,
    /// so a text-only turn never hears about tools it does not have.
    pub tools: Option<Box<ToolsText>>,
}

/// The argv and stdin of a wired turn: those of the text-only job, with the
/// tools prompt as the system prompt. `args` has the length of `Job::args`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToolsText {
    pub args: Vec<String>,
    pub stdin: Vec<u8>,
}

/// A small fixed file a CLI is told to read. The content is a constant of the
/// adapter, never user text.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TmpFile {
    /// The flag that takes the file's path.
    pub flag: &'static str,
    /// The file name inside the turn's private temp folder.
    pub name: &'static str,
    pub content: &'static str,
}

/// The adapter of an engine id, if it has one.
pub fn adapter_for(id: &str) -> Option<Adapter> {
    match id {
        "ollama" => Some(Adapter::Ollama),
        "lmstudio" => Some(Adapter::Lmstudio),
        "llamacpp" => Some(Adapter::Llamacpp),
        "vllm" => Some(Adapter::Vllm),
        "claude" => Some(Adapter::Claude),
        "codex" => Some(Adapter::Codex),
        "gemini" => Some(Adapter::Gemini),
        _ => None,
    }
}

/// The catalog port of a server engine.
fn catalog_port(id: &str) -> u16 {
    CATALOG
        .iter()
        .find_map(|e| match e.kind {
            Kind::Server { port, .. } if e.id == id => Some(port),
            _ => None,
        })
        .expect("a server adapter has a server catalog entry")
}

/// Whether the adapter of engine `id` carries the memory block. An engine
/// without an adapter cannot be chatted with, so it carries nothing.
pub fn carries_memory(id: &str) -> bool {
    adapter_for(id).is_some_and(Adapter::carries_memory)
}

impl Adapter {
    /// Whether `build` puts `ChatRequest::memory` in the request. The chat
    /// injects the block, counts it against the history cap and announces it
    /// (`▸ memory`) only for an adapter that says yes, so the UI never claims
    /// the model got notes it did not. A CLI adapter that writes the block on
    /// stdin flips its own answer when it arrives.
    pub fn carries_memory(self) -> bool {
        match self {
            Adapter::Ollama
            | Adapter::Lmstudio
            | Adapter::Llamacpp
            | Adapter::Vllm
            | Adapter::Claude
            | Adapter::Codex
            | Adapter::Gemini => true,
        }
    }

    /// The invocation for `request`. The body holds the model, the system
    /// prompt, the history and the stream flag, and nothing else: no `think`
    /// option (a reasoning model streams its `thinking` without it, see the
    /// recorded fixtures) and no earlier reasoning.
    pub fn build(self, request: &ChatRequest) -> Invocation {
        self.build_with(request, MCP_VERIFIED)
    }

    /// [`build`](Self::build) with the engines whose tool wiring passed its
    /// manual check given (D12). An engine outside `verified` gets exactly the
    /// invocation of a request without tools.
    pub fn build_with(self, request: &ChatRequest, verified: &[&str]) -> Invocation {
        let built = self.build_text(request, verified);
        let Invocation::Process(mut job) = built else {
            return built;
        };
        if job.mcp.is_some() {
            // The same job once more with the tools prompt, to keep what
            // each engine does with its system prompt in one place.
            let mut with_tools = request.clone();
            with_tools.system = prompts::with_tools(&request.system);
            if let Invocation::Process(other) = self.build_text(&with_tools, verified) {
                job.tools = Some(Box::new(ToolsText {
                    args: other.args,
                    stdin: other.stdin,
                }));
            }
        }
        Invocation::Process(job)
    }

    fn build_text(self, request: &ChatRequest, verified: &[&str]) -> Invocation {
        match self {
            Adapter::Ollama => Invocation::Http {
                wire: Wire::Ndjson,
                port: catalog_port("ollama"),
                path: "/api/chat",
                body: ollama_body(request),
            },
            Adapter::Lmstudio => openai_invocation(request, "lmstudio"),
            Adapter::Llamacpp => openai_invocation(request, "llamacpp"),
            Adapter::Vllm => openai_invocation(request, "vllm"),
            Adapter::Claude => claude_invocation(request, wire_for(request, verified)),
            Adapter::Codex => codex_invocation(request, wire_for(request, verified)),
            Adapter::Gemini => gemini_invocation(request, wire_for(request, verified)),
        }
    }
}

/// One chat-completions turn of the OpenAI-compatible server `id` on its
/// catalog port. The body is the one a tool-less Ollama turn sends (model,
/// messages, stream flag, nothing else); the recorded llama.cpp server
/// streams no reasoning field, so none is asked for or read.
fn openai_invocation(request: &ChatRequest, id: &str) -> Invocation {
    Invocation::Http {
        wire: Wire::Sse,
        port: catalog_port(id),
        path: "/v1/chat/completions",
        body: ollama_body(request),
    }
}

/// The system text a server gets: the pinned prompt, then the memory block
/// (the prompt, the workspace block and the memory block, in that order).
/// Empty when there is neither.
pub fn system_text(request: &ChatRequest) -> String {
    match (request.system.is_empty(), request.memory.as_deref()) {
        (_, None) => request.system.clone(),
        (true, Some(memory)) => memory.to_owned(),
        (false, Some(memory)) => format!("{}\n\n{memory}", request.system),
    }
}

/// One tool the model called and what it got back, to replay in a follow-up.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToolExchange {
    pub name: String,
    /// The call `id` the server sent, echoed as `tool_call_id`. Left out
    /// when absent, so older servers see the same bytes.
    pub id: Option<String>,
    /// The `function.index` the server sent.
    pub index: Option<u64>,
    /// The arguments object the model sent.
    pub arguments: Value,
    /// The tool's result text, already sanitized and capped by the host.
    pub result: String,
}

/// One round of a tool loop: what the assistant said, then the tools it
/// called. Rounds live for one turn only; they are never kept in history.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ToolRound {
    /// The reasoning of a thinking model, echoed on the assistant message.
    pub thinking: Option<String>,
    pub text: String,
    pub exchanges: Vec<ToolExchange>,
    /// A user message sent after the round: the reminder to a model that
    /// wrote a tool call as text (local-model-writes D3). Such a round has
    /// no exchanges.
    pub nudge: Option<String>,
}

/// The request `Adapter::build` returns: never with tools, whatever
/// `request.tools` says. The tool loop (slice 3b) builds its own bodies with
/// [`ollama_round_body`], so a build without the loop is byte-identical.
fn ollama_body(request: &ChatRequest) -> Vec<u8> {
    ollama_round_body(request, &[], false)
}

/// The messages every chat body starts with: the system text (when there is
/// any), then the history. Shared by every OpenAI-shaped and Ollama body.
fn base_messages(request: &ChatRequest) -> Vec<Value> {
    let system = system_text(request);
    let system = (!system.is_empty()).then(|| json!({"role": "system", "content": system}));
    system
        .into_iter()
        .chain(request.messages.iter().map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            json!({"role": role, "content": m.content})
        }))
        .collect()
}

/// The registry as the `tools` array both wires send; empty when the request
/// has no tools on or nothing to offer.
fn offered_tools(request: &ChatRequest) -> Vec<Value> {
    let external = match &request.tools {
        Tools::On { external, .. } => external.external(),
        Tools::Off => Vec::new(),
    };
    registry_with(&external, request.families)
        .into_iter()
        .map(|t| {
            json!({"type": "function", "function": {
                "name": t.name, "description": t.description, "parameters": t.schema,
            }})
        })
        .collect()
}

/// The request after `rounds` of tool calls. `tools` says whether to offer the
/// registry (the loop passes `false` for the retry on a model that rejects
/// tools). Each round adds the assistant message with its `tool_calls`, then
/// one `role: "tool"` message per call carrying `tool_name`.
pub fn ollama_round_body(request: &ChatRequest, rounds: &[ToolRound], tools: bool) -> Vec<u8> {
    let mut messages = base_messages(request);
    for round in rounds {
        let calls: Vec<Value> = round
            .exchanges
            .iter()
            .map(|x| {
                let mut function = json!({"name": x.name, "arguments": x.arguments});
                if let Some(index) = x.index {
                    function["index"] = index.into();
                }
                let mut call = json!({"function": function});
                if let Some(id) = &x.id {
                    call["id"] = json!(id);
                }
                call
            })
            .collect();
        let mut assistant = json!({"role": "assistant", "content": round.text});
        if !calls.is_empty() {
            assistant["tool_calls"] = Value::Array(calls);
        }
        if let Some(thinking) = &round.thinking {
            assistant["thinking"] = json!(thinking);
        }
        messages.push(assistant);
        messages.extend(round.exchanges.iter().map(|x| {
            let mut message = json!({"role": "tool", "content": x.result, "tool_name": x.name});
            if let Some(id) = &x.id {
                message["tool_call_id"] = json!(id);
            }
            message
        }));
        messages.extend(nudge(round));
    }
    finish_body(request, messages, tools)
}

/// The user message that follows a reminder round, if any.
fn nudge(round: &ToolRound) -> Option<Value> {
    (round.nudge.as_ref()).map(|text| json!({"role": "user", "content": text}))
}

/// The names of the tools `request` offers, as the `tools` array lists them.
pub fn offered_names(request: &ChatRequest) -> Vec<String> {
    offered_tools(request)
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_owned))
        .collect()
}

/// The id of call `n` of round `round` (both from 0) when the server sent
/// none. One helper feeds the assistant entry and its tool message, so the
/// two always match.
fn call_id(round: usize, n: usize) -> String {
    format!("call_{round}_{n}")
}

/// The id for call `n` of round `round`: the server's when it is new to the
/// body, else a synthesized one that no earlier id uses (a server may repeat
/// ids, and `echo` can cut two long ones to the same prefix).
fn unique_id(
    used: &mut std::collections::HashSet<String>,
    server: Option<&str>,
    round: usize,
    n: usize,
) -> String {
    let mut id = server.map_or_else(String::new, str::to_owned);
    let mut tries = 0;
    while id.is_empty() || used.contains(&id) {
        id = call_id(round, n);
        if tries > 0 {
            id = format!("{id}_{tries}");
        }
        tries += 1;
    }
    used.insert(id.clone());
    id
}

/// The request after `rounds` of tool calls on the OpenAI wire (llama.cpp):
/// the assistant message holds `tool_calls` with string arguments, then one
/// `role: "tool"` message per call carrying its `tool_call_id`. No reasoning
/// is echoed, and no `parallel_tool_calls` or `tool_choice` is set.
pub fn openai_round_body(request: &ChatRequest, rounds: &[ToolRound], tools: bool) -> Vec<u8> {
    let mut messages = base_messages(request);
    let mut used = std::collections::HashSet::new();
    for (r, round) in rounds.iter().enumerate() {
        let ids: Vec<String> = (round.exchanges.iter().enumerate())
            .map(|(n, x)| unique_id(&mut used, x.id.as_deref(), r, n))
            .collect();
        let calls: Vec<Value> = round
            .exchanges
            .iter()
            .zip(&ids)
            .map(|(x, id)| {
                let function = json!({"name": x.name, "arguments": x.arguments.to_string()});
                json!({"id": id, "type": "function", "function": function})
            })
            .collect();
        let mut assistant = json!({"role": "assistant", "content": round.text});
        if !calls.is_empty() {
            assistant["tool_calls"] = Value::Array(calls);
        }
        messages.push(assistant);
        messages.extend(
            round
                .exchanges
                .iter()
                .zip(&ids)
                .map(|(x, id)| json!({"role": "tool", "tool_call_id": id, "content": x.result})),
        );
        messages.extend(nudge(round));
    }
    finish_body(request, messages, tools)
}

/// The body both round wires end with: model, messages, stream flag and, when
/// asked, the registry.
fn finish_body(request: &ChatRequest, messages: Vec<Value>, tools: bool) -> Vec<u8> {
    let mut body = json!({
        "model": request.model.as_deref().unwrap_or(""),
        "messages": messages,
        "stream": true,
    });
    if tools {
        // An empty list would read as "no tools" to some models and as an
        // error to others: with nothing to offer the key is left out.
        let offered = offered_tools(request);
        if !offered.is_empty() {
            body["tools"] = Value::Array(offered);
        }
    }
    serde_json::to_vec(&body).expect("a JSON value always serializes")
}

/// The shown reason when the model name cannot be passed to a CLI.
const INVALID_MODEL: &str = "invalid model name";

/// The model flag value, if any: `None` for no model or `default`, and an
/// error for a name that could be read as another option or that holds a
/// space or a control character.
fn cli_model(model: Option<&str>) -> Result<Option<&str>, &'static str> {
    match model {
        None | Some("" | "default") => Ok(None),
        Some(m) if m.starts_with('-') || m.chars().any(|c| c.is_control() || c.is_whitespace()) => {
            Err(INVALID_MODEL)
        }
        Some(m) => Ok(Some(m)),
    }
}

/// The session of this turn, when the request has tools on and its engine is
/// verified.
fn wire_for(request: &ChatRequest, verified: &[&str]) -> Option<McpWire> {
    match &request.tools {
        Tools::On { mcp, .. } if verified.contains(&request.engine.as_str()) => {
            mcp.as_deref().cloned()
        }
        _ => None,
    }
}

/// The oldest claude the pinned argv of [`claude_invocation`] runs on. The
/// newest flag in it sets the number: `--permission-prompts none` arrived in
/// 2.1.259 (changelog: "Added `--permission-prompts none` for unattended
/// headless hosts"). An older claude exits with `error: unknown option`.
///
/// We NEVER drop or weaken the text-only flags (`--permission-prompts none`,
/// `--safe-mode`, `--tools ""`, `--strict-mcp-config`) to fit an old claude:
/// a turn that cannot run text-only does not run. The fix for an old claude is
/// to gate it (see [`claude_too_old`]), not to degrade the argv. The adapter
/// tests pin every flag to a version, so a new flag must raise this number.
pub const CLAUDE_MIN_VERSION: (u32, u32, u32) = (2, 1, 259);

/// [`CLAUDE_MIN_VERSION`] as text, `2.1.259`.
pub fn claude_min_version_text() -> String {
    let (major, minor, patch) = CLAUDE_MIN_VERSION;
    format!("{major}.{minor}.{patch}")
}

/// The `x.y.z` of the detected claude `version` line when it is known and
/// below [`CLAUDE_MIN_VERSION`]. A missing or unreadable version is `None`: it
/// fails open, because a failed `--version` probe must not lock a user with a
/// working claude out of the chat. Failing open is also safe: an old claude
/// rejects the unknown flag before it does anything, so it can never run with
/// weaker flags; fail-open can only reproduce the original error.
pub fn claude_old_version(version: Option<&str>) -> Option<String> {
    let found = version.and_then(parse::version_triplet)?;
    (found < CLAUDE_MIN_VERSION).then(|| format!("{}.{}.{}", found.0, found.1, found.2))
}

/// Whether [`claude_old_version`] found a version below the minimum.
pub fn claude_too_old(version: Option<&str>) -> bool {
    claude_old_version(version).is_some()
}

/// The claude run: the pinned argv (text only, no tools, nothing from the
/// user's setup), the profile prompt as the system prompt, and the memory block
/// and the history on stdin. argv is visible in `ps`, so only the prompt goes
/// there.
fn claude_invocation(request: &ChatRequest, mcp: Option<McpWire>) -> Invocation {
    let model = match cli_model(request.model.as_deref()) {
        Ok(model) => model,
        Err(reason) => return Invocation::Refused(reason),
    };
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "stream-json",
        // stream-json with --print needs it (M1, claude 2.1.299).
        "--verbose",
        "--include-partial-messages",
        "--tools",
        "",
        "--strict-mcp-config",
        "--safe-mode",
        "--disable-slash-commands",
        "--permission-prompts",
        "none",
        "--no-session-persistence",
        // Always passed, even empty, so the CLI's own agent prompt never applies.
        "--system-prompt",
    ]
    .map(str::to_owned)
    .into();
    args.push(request.system.clone());
    args.extend(model.map(|m| format!("--model={m}")));
    Invocation::Process(Job {
        wire: Wire::ClaudeStreamJson,
        binary: "claude",
        cli: Cli::Claude,
        args,
        stdin: cli_stdin(request).into_bytes(),
        jail: jail_policy(Cli::Claude, Os::CURRENT),
        tmp_files: Vec::new(),
        mcp,
        tools: None,
    })
}

/// Features codex runs with switched off. Together with the config keys in
/// [`codex_invocation`] they remove every tool the model could call: the
/// shell, patching, web search, image tools, sub-agents, connectors, plugins
/// and hooks. A name the installed codex does not know makes it exit with
/// `Unknown feature flag`, which the chat shows as the failure.
const CODEX_DISABLED: [&str; 14] = [
    "shell_tool",
    "unified_exec",
    "apps",
    "browser_use",
    "browser_use_external",
    "computer_use",
    "image_generation",
    "in_app_browser",
    "multi_agent",
    "plugins",
    "hooks",
    "goals",
    "tool_suggest",
    "shell_snapshot",
];

/// The codex run: the pinned argv (text only, nothing from the user's setup,
/// nothing kept on disk), and on stdin the profile prompt in an `Instructions:`
/// block, then the memory block and the history. codex has no system-prompt
/// flag, and argv is visible in `ps`, so nothing but fixed flags and the model
/// goes there. The child starts in the private empty folder, which is also
/// codex's working root, so no `-C` is passed.
fn codex_invocation(request: &ChatRequest, mcp: Option<McpWire>) -> Invocation {
    let model = match cli_model(request.model.as_deref()) {
        Ok(model) => model,
        Err(reason) => return Invocation::Refused(reason),
    };
    let mut args: Vec<String> = [
        "exec",
        "--json",
        "-s",
        "read-only",
        "--ephemeral",
        "--ignore-user-config",
        "--ignore-rules",
        "--skip-git-repo-check",
    ]
    .map(str::to_owned)
    .into();
    for feature in CODEX_DISABLED {
        args.extend(["--disable".to_owned(), feature.to_owned()]);
    }
    args.extend(
        [
            "-c",
            "web_search=\"disabled\"",
            "-c",
            "include_apply_patch_tool=false",
            "-c",
            "tools.view_image=false",
        ]
        .map(str::to_owned),
    );
    args.extend(model.map(|m| format!("--model={m}")));
    // The prompt is read from stdin.
    args.push("-".to_owned());
    Invocation::Process(Job {
        wire: Wire::CodexJsonl,
        binary: "codex",
        cli: Cli::Codex,
        args,
        stdin: instructed_stdin(request).into_bytes(),
        jail: jail_policy(Cli::Codex, Os::CURRENT),
        tmp_files: Vec::new(),
        mcp,
        tools: None,
    })
}

/// The `-p` text of a gemini turn. gemini appends it to what it reads on
/// stdin, so it is fixed: the conversation is on stdin and never in argv.
const GEMINI_PROMPT: &str = "Answer the last user message of the conversation given on stdin.";

/// The policy gemini gets: one rule that denies every tool, at the highest
/// priority the policy tiers document. On gemini 0.63.0 it was verified
/// empirically that it works (zero tool calls, and `exit_plan_mode` is not
/// offered to the model); precedence over every other rule is not guaranteed
/// by the CLI's docs, so the decoder (it ends the turn on any tool event) and
/// the jail remain the backstops. `plan` mode alone is not enough (the model
/// can leave it), and neither is `tools.core` (the system settings file is
/// skipped unless root owns it). `--allowed-mcp-server-names none` would let a
/// server literally named "none" through; this rule covers it.
const GEMINI_POLICY: &str = "[[rule]]\ntoolName = \"*\"\ndecision = \"deny\"\npriority = 999\ndenyMessage = \"This chat has no tools.\"\n";

/// The gemini run: the pinned argv (text only, no extension, no MCP server, no
/// tool), and on stdin the profile prompt in an `Instructions:` block, then the
/// memory block and the history. gemini has no system-prompt flag, and argv is
/// visible in `ps`, so nothing but fixed flags and the model goes there. The
/// runtime appends `--policy <file>` after these flags, and for a turn with
/// tools it adds its own (`--allowed-mcp-server-names=lazarobox`, an admin
/// policy that allows the server) and runs gemini in a work folder that holds
/// the workspace settings.
fn gemini_invocation(request: &ChatRequest, mcp: Option<McpWire>) -> Invocation {
    let model = match cli_model(request.model.as_deref()) {
        Ok(model) => model,
        Err(reason) => return Invocation::Refused(reason),
    };
    let mut args: Vec<String> = [
        // The folder is our private one (on a wired turn it holds
        // `.gemini/settings.json`), and an untrusted folder makes the CLI
        // drop out of `plan` mode and refuse to run headless.
        "--skip-trust",
        "-p",
        GEMINI_PROMPT,
        "-o",
        "stream-json",
        "--approval-mode",
        "plan",
        "-e",
        "none",
        "--allowed-mcp-server-names",
        "none",
    ]
    .map(str::to_owned)
    .into();
    args.extend(model.map(|m| format!("--model={m}")));
    Invocation::Process(Job {
        wire: Wire::GeminiStreamJson,
        binary: "gemini",
        cli: Cli::Gemini,
        args,
        stdin: instructed_stdin(request).into_bytes(),
        jail: jail_policy(Cli::Gemini, Os::CURRENT),
        tmp_files: vec![TmpFile {
            flag: "--policy",
            name: "gemini-policy.toml",
            content: GEMINI_POLICY,
        }],
        mcp,
        tools: None,
    })
}

/// What a CLI without a system-prompt flag reads on stdin: the profile prompt
/// in an `Instructions:` block (when there is one), then the memory block and
/// the conversation.
fn instructed_stdin(request: &ChatRequest) -> String {
    let conversation = cli_stdin(request);
    if request.system.is_empty() {
        conversation
    } else {
        format!("Instructions:\n{}\n\n{conversation}", request.system)
    }
}

/// What a CLI reads on stdin: the memory block, then the conversation. One
/// message goes as it is; a longer history is labelled turn by turn.
fn cli_stdin(request: &ChatRequest) -> String {
    let transcript = match request.messages.as_slice() {
        [only] if only.role == Role::User => only.content.clone(),
        messages => messages
            .iter()
            .map(labelled)
            .collect::<Vec<_>>()
            .join("\n\n"),
    };
    match request.memory.as_deref() {
        Some(memory) => format!("{memory}\n\n{transcript}"),
        None => transcript,
    }
}

fn labelled(message: &ChatMessage) -> String {
    let label = match message.role {
        Role::User => "User",
        Role::Assistant => "Assistant",
    };
    // A content line that is exactly a label would read as a turn boundary the
    // user never wrote: a leading space keeps it text.
    let content = message
        .content
        .split('\n')
        .map(|line| match line {
            "User:" | "Assistant:" => format!(" {line}"),
            _ => line.to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!("{label}:\n{content}")
}

#[cfg(test)]
mod tests;
