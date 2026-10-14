//! Engine adapters as pure builders: a `ChatRequest` in, the description of
//! what to run out. Nothing here touches the network; the runtime executes the
//! `Invocation`.

use serde_json::{Value, json};

use super::engine::{ChatMessage, ChatRequest, Role};
use crate::core::ai::{
    CATALOG, Kind,
    jail::{Cli, JailPolicy},
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

/// The engines that can chat. More arrive with their adapters.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Adapter {
    Ollama,
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
    /// A file the runtime writes into the turn's private temp folder, and
    /// names to the CLI with a flag appended after `args`.
    pub tmp_file: Option<TmpFile>,
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
            Adapter::Ollama | Adapter::Claude | Adapter::Codex | Adapter::Gemini => true,
        }
    }

    /// The invocation for `request`. The body holds the model, the system
    /// prompt, the history and the stream flag, and nothing else: no `think`
    /// option (a reasoning model streams its `thinking` without it, see the
    /// recorded fixtures) and no earlier reasoning.
    pub fn build(self, request: &ChatRequest) -> Invocation {
        match self {
            Adapter::Ollama => Invocation::Http {
                wire: Wire::Ndjson,
                port: catalog_port("ollama"),
                path: "/api/chat",
                body: ollama_body(request),
            },
            Adapter::Claude => claude_invocation(request),
            Adapter::Codex => codex_invocation(request),
            Adapter::Gemini => gemini_invocation(request),
        }
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

fn ollama_body(request: &ChatRequest) -> Vec<u8> {
    let system = system_text(request);
    let system = (!system.is_empty()).then(|| json!({"role": "system", "content": system}));
    let messages: Vec<Value> = system
        .into_iter()
        .chain(request.messages.iter().map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            json!({"role": role, "content": m.content})
        }))
        .collect();
    let body = json!({
        "model": request.model.as_deref().unwrap_or(""),
        "messages": messages,
        "stream": true,
    });
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

/// The claude run: the pinned argv (text only, no tools, nothing from the
/// user's setup), the profile prompt as the system prompt, and the memory block
/// and the history on stdin. argv is visible in `ps`, so only the prompt goes
/// there.
fn claude_invocation(request: &ChatRequest) -> Invocation {
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
        jail: JailPolicy::Layered,
        tmp_file: None,
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
fn codex_invocation(request: &ChatRequest) -> Invocation {
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
        jail: JailPolicy::Required,
        tmp_file: None,
    })
}

/// The `-p` text of a gemini turn. gemini appends it to what it reads on
/// stdin, so it is fixed: the conversation is on stdin and never in argv.
const GEMINI_PROMPT: &str = "Answer the last user message of the conversation given on stdin.";

/// The policy gemini gets: one rule that denies every tool, above every rule
/// of the CLI's own policies and of the approval mode. A denied tool is not
/// offered to the model at all. `plan` mode alone is not enough (the model
/// can leave it), and neither is `tools.core` (the system settings file is
/// skipped unless root owns it).
const GEMINI_POLICY: &str = "[[rule]]\ntoolName = \"*\"\ndecision = \"deny\"\npriority = 999\ndenyMessage = \"This chat has no tools.\"\n";

/// The gemini run: the pinned argv (text only, no extension, no MCP server, no
/// tool), and on stdin the profile prompt in an `Instructions:` block, then the
/// memory block and the history. gemini has no system-prompt flag, and argv is
/// visible in `ps`, so nothing but fixed flags and the model goes there. The
/// runtime appends `--policy <file>` after these flags.
fn gemini_invocation(request: &ChatRequest) -> Invocation {
    let model = match cli_model(request.model.as_deref()) {
        Ok(model) => model,
        Err(reason) => return Invocation::Refused(reason),
    };
    let mut args: Vec<String> = [
        // The folder is our private empty one, and an untrusted folder makes
        // the CLI drop out of `plan` mode and refuse to run headless.
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
        jail: JailPolicy::Required,
        tmp_file: Some(TmpFile {
            flag: "--policy",
            name: "gemini-policy.toml",
            content: GEMINI_POLICY,
        }),
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
