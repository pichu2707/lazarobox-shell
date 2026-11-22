# Proposal: The chat runs approved commands itself (chat-runs-commands, #845)

## Intent

Installing a program from the chat takes three manual steps today: the model shows a command, the user sends it to a pane and presses Enter (`y` in `ask` only types it), then the user tells the chat that output is ready. The owner wants one action, at most one approval: "si tenemos el run es para que trabaje solo excepto si hay riesgo".

Root causes (verified in exploration):
- `run` mode runs only a tiny read grammar (`exec::parse`, `src/core/ai/tools/exec.rs`); everything else prompts.
- `y` in `ask` (the default) types without Enter (`RUN_KEYS`, `approve_run` in `src/app/run.rs`).
- Settle ends a sudo password prompt as `TimedOut` (10 s) or `UserTyped`, so the chat never resumes on its own.
- `tools.md` and the `run_in_pane` description tell the model the user runs commands, so it prints shell blocks (`RUN_HINT`).

## Users and situations

| Situation | Need |
|---|---|
| Install a tool (`apt`, `brew`, `cargo install`) | One `y`, then the chat reads the result |
| `sudo` command | Password typed in the visible pane, never seen by the model; chat resumes when the command ends |
| Read-only exploration in `run` | No prompt at all |
| Dev server or watcher | Chat goes on after ~30 s with a partial capture |
| Chat that read GitHub text | Unchanged: commands are only typed |

## Scope

### In scope
- Two approval modes: `ask` (every command asks one `y`; `y` runs) and `run` (read-only runs unprompted; everything else asks one `y`; `y` runs). Default stays `ask`.
- `auto` removed: a config with `auto` maps to `ask` with a visible notice.
- Read-only classifier: allow-list per program and per segment (today's grammar plus `cat`, `head`, `git status/diff/log`, `--version`, ...). Unknown, writing or installing commands ask, inside the project included. `sudo` always asks.
- Approved commands run in the visible pane; the chat captures output by itself.
- Settle waits for interactive input (password prompt) until the command ends; typed input is redacted from the capture.
- Long-running commands: partial capture after ~30 s plus a notice that the command keeps running (Ctrl+C in the pane stops it).
- 5-per-turn cap counts only unprompted runs.
- Model guidance rewritten: `tools.md`, `run_in_pane` description, `RUN_KEYS`, `RUN_HINT`; the model calls the tool, never prints a command for the user.
- README.md and README.es.md updated together.

### Non-goals
- Unprompted runs of anything that writes, even inside the project.
- Changing the jail, the GitHub-text typed-only rule, or orchestrator worker approvals.
- Implementing #657/#658/#659; only the policy hook they need.
- Hidden or background execution: every run stays in a visible pane.

## Business rules
- Same flow for local and cloud engines.
- Workers never inherit the new `y` semantics.
- A single policy hook decides whether `y` may run; #657/#658/#659 can disable or restrict it later.
- Every refusal and fallback shows a visible line.

## Capabilities

### New Capabilities
- None (the classifier extends `agent-tools`).

### Modified Capabilities
- `agent-tools`: the `run` grammar becomes the per-program allow-list; `y` runs in both modes; cap counts unprompted runs only; tool description.
- `pane-execution`: settle gains a waiting-for-input state, the ~30 s partial capture, and input redaction.
- `chat-pane`: `RUN_KEYS`/`RUN_HINT` text, notices, resume without a user message.
- `command-runner`: approved runs press Enter and follow the command to its end.
- `configuration` / `config-menu`: `auto` removed, mapped to `ask` with a notice.
- `agent-orchestrator`: workers keep today's approval behavior.

## Approach

Exploration approach A: the machinery exists (`run_approved` in `src/app/auto_run.rs` already runs on `y` in `run`). The change is policy plus one settle state:
1. Pure classifier: split into segments; each segment must match a program allow-list entry; any metacharacter, pipe to a shell, `sudo` or unknown program means ask.
2. `approve_run` runs on `y` in both modes, behind a `may_run_on_y` policy hook that returns false for GitHub-text chats and workers.
3. Settle: foreground not the shell plus no output change means `WaitingForInput`, not `TimedOut`; keys typed by the user while waiting do not end the follow; the capture drops input echoed after a password prompt.
4. Prompt text rewrite.

## Affected Areas

| Area | Impact | Description |
|---|---|---|
| `src/core/ai/tools/exec.rs` | Modified | Allow-list classifier |
| `src/core/ai/chat/approval.rs` | Modified | Routing, `RUN_KEYS`, fallbacks, policy hook |
| `src/app/run.rs`, `src/app/auto_run.rs`, `src/app/typed_run.rs` | Modified | `y` runs, follow to end, cap |
| `src/core/ai/tools/settle.rs` | Modified | Waiting-for-input, 30 s partial, redaction |
| `src/core/ai/tools/mod.rs`, `tools.md`, `transcript.rs` | Modified | Model guidance, `RUN_HINT` |
| `src/core/ai/config.rs`, config menu | Modified | `auto` removal |
| `openspec/specs/*` (above) | Delta | Requirement changes |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Classifier bypass (`cat x > y`, `git -c alias...`, `$(...)`, `find -exec`) | High | Allow-list only; reject every metacharacter; per-program flag allow-lists; table tests with known bypasses |
| Settle regressions in #819/#824/#841 tests | Med | New state is additive; existing settle tests kept green before each slice merges |
| Password reaches the model | Med | Redact input after a password prompt; capture only output; test with a fake sudo prompt |
| Servers never end and block the turn | Med | ~30 s partial capture plus visible notice; pane keeps running |
| Users relied on `auto` | Low | Visible notice on mapping; README note |

## Rollback Plan

Each slice is one revertable PR, chained stacked-to-main. Reverting S2 restores type-only `y` in `ask`; the classifier (S1) is pure and can stay. The policy hook can force `may_run_on_y = false` as a kill switch without reverting code. No data migration; a config still holding `auto` keeps loading.

## Delivery slices (each 400 production lines or less)

1. S1: classifier allow-list and `auto` removal with notice.
2. S2: `y` runs in both modes, policy hook, cap change, GitHub-text and worker exclusions.
3. S3: settle waiting-for-input, password redaction, 30 s partial capture.
4. S4: model guidance rewrite and both READMEs.

Between S2 and S3, a password prompt still ends as today's timeout with its visible notice; design may swap S2 and S3 to avoid that gap.

## Dependencies
- Existing settle, jail and per-turn limits; #819 typed-run follow.

## Success Criteria
- [ ] Asking the chat to install a program needs at most one `y`; the chat reports the result with no user message.
- [ ] In `run`, allow-listed reads run unprompted; `mkdir`, `git commit`, `cargo build`, `npm install` ask.
- [ ] Bypass table tests all classify as ask.
- [ ] A `sudo` run accepts the password in the pane, resumes by itself, and the password is absent from the model input.
- [ ] A server run returns a partial capture and notice after ~30 s.
- [ ] GitHub-text chats and workers behave as today.
- [ ] A config with `auto` loads as `ask` with a visible notice.
- [ ] #819/#824/#841 settle tests stay green.

## Proposal question round
Two owner rounds (2026-11-22) settled the decisions above. Assumptions for owner review:
1. The ~30 s threshold is fixed, not configurable, in this change.
2. Commands typed with Ctrl+Space e by hand keep today's behavior.
