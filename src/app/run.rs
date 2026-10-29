//! Run in pane: the confirm that types a command of a chat into a shell pane.
//! The confirm only types; Enter is sent only by a run (see `auto_run`).

use super::{
    tool::{CANCELLED, DECLINED},
    *,
};
use crate::core::ai::{
    chat::{
        approval::{
            Approval, ApprovalAsk, ApprovalPrompt, EXTERNAL_KEYS, FOLDER_CHECKING_KEYS,
            FOLDER_KEYS, Origin, Press, READ_KEYS, RUN_CANCELLED, RUN_KEYS, Reply, SEARCH_KEYS,
            exec_notice, run_sent_line,
        },
        approval_rows::fits,
        engine::{CallId, Decision},
        yank,
    },
    sanitize,
    tools::command::{CONTINUATION, validate},
};

/// Notices for the refusals of `Prefix e`.
const CHAT_ONLY: &str = "Run in pane works from a chat";
const CONVERSATION_ONLY: &str = "Run in pane works in the conversation view";
const TOOLS_OFF: &str = "Tools are off: nothing can run from the chat";
const NO_COMMAND: &str = "No runnable command in the last answer";
const NO_SHELL: &str = "No shell pane in this tab";
const TARGET_GONE: &str = "That pane is gone: press p to pick another";
const NO_PASTE_MODE: &str = "The shell cannot take a multi-line command safely";
const ANSWER_FIRST: &str = "Answer the pending confirm first";
const ALREADY_OPEN: &str = "A confirm is already open: answer it first";
const TOOLS_OFF_REASON: &str = "tools are off";
const PROMPT_TOO_SMALL: &str = "Pane too small to show it all: enlarge it or press n";

/// The mark of a line typed from a block (`Prefix e`): no tool call made it.
const BLOCK_LINE: CallId = CallId(u64::MAX);

/// A command that was not typed: what the user sees, and why, in the short
/// words that go back to the model when a tool call asked for it.
pub(super) struct Refused {
    notice: String,
    pub(super) reason: &'static str,
}

impl App {
    /// Brings the chats in line with `[ai.tools]`: tools off ends every
    /// pending confirm (nothing is written) and hides the hint under shell
    /// blocks; tools on shows it again.
    pub(super) fn sync_tools(&mut self) -> Vec<Effect> {
        let on = self.ai.tools_enabled();
        let mut effects = Vec::new();
        for (&id, state) in &mut self.panes {
            let Some(chat) = state.chat_mut() else {
                continue;
            };
            chat.set_run_hint(on);
            if !on && let Some(approval) = chat.approvals().pending().cloned() {
                chat.approvals_mut().withdraw(approval.id());
                chat.set_run_target(None);
                // A tool call that waited for it is told, so it ends now.
                effects.extend(Self::answer(
                    id,
                    &approval,
                    Decision::Denied(TOOLS_OFF_REASON),
                ));
            }
            // The session goes too, so free tools stop at once and the socket
            // closes; the turn itself keeps running as text.
            if !on && chat.take_tool_session() {
                effects.push(Effect::EndToolSession(id));
            }
        }
        if !on {
            // The session ends and answers its calls: no run is left to wait for.
            self.drop_run_slots(|_| true);
        }
        self.dirty = true;
        effects
    }

    /// Whether chat `id` has an approval waiting for an answer.
    pub(super) fn approval_pending(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.approvals().pending().is_some())
    }

    /// Whether the approval of chat `id` takes the keys: it is pending and
    /// the conversation view, the one that draws the prompt, is the one
    /// shown. Under the list, an editor or a draft the keys belong to that
    /// view, so a hidden prompt is never answered blind.
    pub(super) fn has_approval(&self, id: PaneId) -> bool {
        self.approval_pending(id)
            && !(self.list_open(id) || self.memory_view_open(id) || self.tag_editor_open(id))
    }

    /// `Prefix T`, `n` or `s` on a chat whose confirm waits: they would
    /// replace the conversation and hide the prompt, so they refuse.
    pub(super) fn refuse_over_approval(&mut self) -> bool {
        let pending = self.focused_is_chat() && self.approval_pending(self.tab().focus);
        if pending {
            self.notice = Some(ANSWER_FIRST.to_owned());
        }
        pending
    }

    /// A key in a chat with a pending approval. It is routed before the chat
    /// key table: `y` approves, `n` and Esc decline, `p` cycles the target,
    /// Ctrl+C cancels the stream and denies, and every other key is swallowed
    /// (no composer edit, no write, no quit). Only a Press answers.
    pub(super) fn on_approval_key(
        &mut self,
        id: PaneId,
        key: KeyEvent,
        repeat: bool,
    ) -> Vec<Effect> {
        if self.batch_pending(id) {
            return self.on_review_key(id, key, repeat);
        }
        if repeat {
            return Vec::new();
        }
        if self.folder_ask_pending(id) {
            return self.on_folder_key(id, key);
        }
        // A prompt the model just opened takes no answer yet: the key may have
        // been typed for the composer. Ctrl+C always works.
        let answers = matches!(
            (key.code, key.modifiers),
            (KeyCode::Char('y' | 'n') | KeyCode::Esc, KeyModifiers::NONE)
        );
        if answers && !self.approval_armed(id) {
            return Vec::new();
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('y'), KeyModifiers::NONE) => self.approve(id),
            (KeyCode::Char('n') | KeyCode::Esc, KeyModifiers::NONE) => self.decline(id, DECLINED),
            (KeyCode::Char('p'), KeyModifiers::NONE) => {
                self.cycle_run_target(id);
                Vec::new()
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                let mut effects = self.decline(id, CANCELLED);
                effects.extend(self.chat_cancel_key(id));
                effects
            }
            _ => Vec::new(),
        }
    }

    /// Ends the pending approval of chat `id` with `reply`, writing nothing.
    pub(super) fn close_approval(&mut self, id: PaneId, reply: Reply) -> Option<Approval> {
        let chat = self.panes.get_mut(&id)?.chat_mut()?;
        let approval_id = chat.approvals().pending()?.id();
        let (approval, _) = chat
            .approvals_mut()
            .answer(approval_id, reply, Press::Press)?;
        chat.set_run_target(None);
        chat.set_orphan(None);
        // A tool call has its own result row, and a batch review its notice;
        // only the user's own action leaves a line behind.
        let batch = matches!(
            approval.ask(),
            ApprovalAsk::Batch(_) | ApprovalAsk::Commit(_) | ApprovalAsk::Folder { .. }
        );
        if reply == Reply::Decline && approval.call().is_none() && !batch {
            chat.push_system_line(RUN_CANCELLED.to_owned());
        }
        Some(approval)
    }

    /// `n`, Esc and Ctrl+C: ends the approval and, for a tool call, tells it
    /// why nothing was done.
    pub(super) fn decline(&mut self, id: PaneId, reason: &'static str) -> Vec<Effect> {
        self.close_approval(id, Reply::Decline)
            .and_then(|approval| Self::answer(id, &approval, Decision::Denied(reason)))
            .into_iter()
            .collect()
    }

    /// Whether the prompt of chat `id` shows all of what `y` would approve in
    /// the pane as it is now: the same rows the renderer draws.
    fn prompt_fits(&self, id: PaneId) -> bool {
        let Some(prompt) = self.approval_prompt(id) else {
            return true;
        };
        let size = self.panes.get(&id).map(|state| state.size);
        size.is_none_or(|size| fits(&prompt, usize::from(size.cols), usize::from(size.rows)))
    }

    /// `y`: a run types the command; a read (or another tool) is allowed. Only
    /// when the prompt shows the whole path or command: a partly hidden text
    /// is never approved. The prompt stays, and `n` still denies.
    fn approve(&mut self, id: PaneId) -> Vec<Effect> {
        let is_batch = (self.panes.get(&id))
            .and_then(PaneState::chat_state)
            .and_then(|chat| chat.approvals().pending())
            .is_some_and(|approval| {
                matches!(
                    approval.ask(),
                    ApprovalAsk::Batch(_) | ApprovalAsk::Commit(_)
                )
            });
        // Defence in depth: a batch is answered only by its review, behind the
        // coverage gate.
        if is_batch {
            return Vec::new();
        }
        if !self.prompt_fits(id) {
            self.notice = Some(PROMPT_TOO_SMALL.to_owned());
            return Vec::new();
        }
        let is_run = (self.panes.get(&id))
            .and_then(PaneState::chat_state)
            .and_then(|chat| chat.approvals().pending())
            .is_some_and(|approval| matches!(approval.ask(), ApprovalAsk::Run { .. }));
        if is_run {
            return self.approve_run(id);
        }
        self.close_approval(id, Reply::Approve)
            .and_then(|approval| Self::answer(id, &approval, Decision::Sent))
            .into_iter()
            .collect()
    }

    /// `y` on a run: types the command into the target pane. The target is
    /// rechecked now, not when the confirm opened: it must still be a shell
    /// pane of this tab and not run a full-screen app, and the paste framing
    /// follows its present state. One accept is one write; nothing ever sends
    /// Enter. A tool call is answered `Sent` only when the command was written;
    /// a check that fails answers it `Denied` with the reason.
    fn approve_run(&mut self, id: PaneId) -> Vec<Effect> {
        let Some(target) = self
            .panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .and_then(ChatState::run_target)
            .filter(|target| self.shell_panes().contains(target))
        else {
            // The target is gone: the confirm stays so `p` can pick another.
            self.notice = Some(TARGET_GONE.to_owned());
            return Vec::new();
        };
        let Some(approval) = self.close_approval(id, Reply::Approve) else {
            return Vec::new();
        };
        let ApprovalAsk::Run { command } = approval.ask() else {
            return Vec::new();
        };
        let mut effects = Vec::new();
        let decision = match self.typed_bytes(target, command) {
            Ok(bytes) => {
                let number = self.pane_number(target).unwrap_or(0);
                if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
                    chat.push_system_line(run_sent_line(number));
                }
                // Enter is never sent: the line stays unsent, and an auto
                // call must not land on it.
                if let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) {
                    pty.line_pending = Some(approval.call().map_or(BLOCK_LINE, |(_, call)| call));
                }
                effects.push(Effect::WritePty(target, bytes));
                // A run being settled in this pane is no longer the only thing
                // in it.
                self.note_typed(target);
                Decision::Sent
            }
            Err(refused) => {
                self.notice = Some(refused.notice);
                Decision::Denied(refused.reason)
            }
        };
        effects.extend(Self::answer(id, &approval, decision));
        effects
    }

    /// The accept-time checks of a command bound for `target`, and the bytes
    /// to write when they pass. Shared by every way a command gets typed.
    pub(super) fn typed_bytes(
        &mut self,
        target: PaneId,
        command: &str,
    ) -> Result<Vec<u8>, Refused> {
        let command = validate(command).map_err(|refusal| Refused {
            notice: format!("Command refused: {}", refusal.reason()),
            reason: if CONTINUATION.contains(&refusal.reason()) {
                refusal.reason()
            } else {
                "the command was refused"
            },
        })?;
        self.frame_bytes(target, command.as_str())
    }

    /// The post-validation half of [`typed_bytes`](Self::typed_bytes): the
    /// pane checks and the bytes for `text`, already validated, framed by the
    /// state of the pane now. A run appends its `\r` to these.
    pub(super) fn frame_bytes(&mut self, target: PaneId, text: &str) -> Result<Vec<u8>, Refused> {
        let number = self.pane_number(target).unwrap_or(0);
        let Some(pty) = self.panes.get_mut(&target).and_then(PaneState::pty_mut) else {
            return Err(Refused {
                notice: TARGET_GONE.to_owned(),
                reason: "the pane is gone",
            });
        };
        if pty.emu.mouse_facts().alt_screen {
            return Err(Refused {
                notice: format!("Pane {number} runs a full-screen app"),
                reason: "the pane runs a full-screen app",
            });
        }
        let modes = pty.emu.modes();
        let bytes = if modes.bracketed_paste {
            encode_paste(text, modes)
        } else if text.contains(['\n', '\t']) {
            return Err(Refused {
                notice: NO_PASTE_MODE.to_owned(),
                reason: "the shell cannot take a multi-line command safely",
            });
        } else {
            text.as_bytes().to_vec()
        };
        pty.emu.set_scrollback(0);
        Ok(bytes)
    }

    /// `p`: the next shell pane of the tab becomes the target, with wrap.
    fn cycle_run_target(&mut self, id: PaneId) {
        let shells = self.shell_panes();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return;
        };
        // Only a command has a target pane to pick.
        if !matches!(
            chat.approvals().pending().map(Approval::ask),
            Some(ApprovalAsk::Run { .. })
        ) {
            return;
        }
        let next = match chat
            .run_target()
            .and_then(|t| shells.iter().position(|p| *p == t))
        {
            Some(at) => shells.get((at + 1) % shells.len()),
            None => shells.first(),
        };
        match next {
            Some(next) => chat.set_run_target(Some(*next)),
            None => self.notice = Some(NO_SHELL.to_owned()),
        }
    }

    /// The 1-based number of `pane` in the active tab.
    pub(super) fn pane_number(&self, pane: PaneId) -> Option<usize> {
        Some(self.tab().tree.leaves().iter().position(|p| *p == pane)? + 1)
    }

    /// `Prefix e`: opens the confirm for the last shell block of the focused
    /// chat's last answer. Nothing is written; only the confirm can.
    pub(super) fn run_in_pane(&mut self) -> Vec<Effect> {
        let id = self.tab().focus;
        if !self.focused_is_chat() {
            self.notice = Some(CHAT_ONLY.to_owned());
            return Vec::new();
        }
        if self.list_open(id) || self.memory_view_open(id) || self.tag_editor_open(id) {
            self.notice = Some(CONVERSATION_ONLY.to_owned());
            return Vec::new();
        }
        if !self.ai.tools_enabled() {
            self.notice = Some(TOOLS_OFF.to_owned());
            return Vec::new();
        }
        let Some(block) = self
            .focused_state()
            .chat_state()
            .and_then(yank::last_shell_block)
        else {
            self.notice = Some(NO_COMMAND.to_owned());
            return Vec::new();
        };
        let command = match validate(&block) {
            Ok(command) => command,
            Err(refusal) => {
                self.notice = Some(format!("Command refused: {}", refusal.reason()));
                return Vec::new();
            }
        };
        let Some(target) = self.default_run_target() else {
            self.notice = Some(NO_SHELL.to_owned());
            return Vec::new();
        };
        let Some(chat) = self.focused_state_mut().chat_mut() else {
            return Vec::new();
        };
        let ask = ApprovalAsk::Run {
            command: command.as_str().to_owned(),
        };
        match chat.approvals_mut().request(ask, Origin::Block) {
            Ok(_) => chat.set_run_target(Some(target)),
            Err(_) => self.notice = Some(ALREADY_OPEN.to_owned()),
        }
        Vec::new()
    }

    /// The shell panes of the active tab, in layout order.
    fn shell_panes(&self) -> Vec<PaneId> {
        self.shell_panes_in(self.tab())
    }

    /// Whether `pane` runs the `cargo install` of an update: a PTY, but its
    /// stdin is cargo's, never a place for a command.
    fn is_update(&self, pane: PaneId) -> bool {
        (self.panes.get(&pane).and_then(PaneState::pty)).is_some_and(|pty| pty.update.is_some())
    }

    fn shell_panes_in(&self, tab: &Tab) -> Vec<PaneId> {
        tab.tree
            .leaves()
            .into_iter()
            .filter(|pane| self.is_shell(*pane) && !self.is_update(*pane))
            .collect()
    }

    /// Where a command goes unless the user cycles: the last shell that had
    /// the focus in this tab, else the first shell of the tab. A chat or a
    /// pane of another tab is never chosen.
    fn default_run_target(&self) -> Option<PaneId> {
        self.default_run_target_in(self.tab())
    }

    /// `default_run_target` for any tab: a chat that is not on screen types
    /// into the shells of its own tab.
    pub(super) fn default_run_target_in(&self, tab: &Tab) -> Option<PaneId> {
        let shells = self.shell_panes_in(tab);
        tab.last_shell
            .filter(|pane| shells.contains(pane))
            .or_else(|| shells.first().copied())
    }

    /// What chat `id` shows for its pending run-in-pane confirm: the target
    /// pane by its number in the tab and its cwd, and the exact command.
    pub fn approval_prompt(&self, id: PaneId) -> Option<ApprovalPrompt> {
        let chat = self.panes.get(&id)?.chat_state()?;
        let armed = self.approval_armed(id);
        let approval = chat.approvals().pending()?;
        let why = approval.why().map(str::to_owned);
        match approval.ask() {
            ApprovalAsk::Run { command } => {
                let target = chat.run_target().and_then(|pane| self.pane_label(pane));
                Some(ApprovalPrompt {
                    target: target.unwrap_or_else(|| "target pane is gone".to_owned()),
                    command: command.clone(),
                    keys: RUN_KEYS,
                    armed,
                    why,
                    notice: None,
                })
            }
            // The path was resolved and made safe to show before it was asked.
            ApprovalAsk::Read { path, .. } => Some(ApprovalPrompt {
                target: "send a file's text to the engine".to_owned(),
                command: format!("read {path}"),
                keys: READ_KEYS,
                armed,
                why: None,
                notice: None,
            }),
            // A worker's read outside its scope: the label names the worker,
            // its kind and `engine>model`, already sanitized.
            ApprovalAsk::WorkerRead { path, label } => Some(ApprovalPrompt {
                target: format!("worker {label}: send a file's text to the engine"),
                command: format!("read {path}"),
                keys: READ_KEYS,
                armed,
                why: None,
                notice: None,
            }),
            // The pattern and the folder were made one clean line when asked.
            ApprovalAsk::Search { pattern, dir, .. } => Some(ApprovalPrompt {
                target: "send matching lines of your files to the engine".to_owned(),
                command: format!("search {pattern} in {dir}"),
                keys: SEARCH_KEYS,
                armed,
                why: None,
                notice: None,
            }),
            // The path was made one clean line when asked.
            ApprovalAsk::GitDiff { staged, path } => Some(ApprovalPrompt {
                target: "send a diff of your repository to the engine".to_owned(),
                command: format!("git diff{} {path}", if *staged { " --staged" } else { "" }),
                keys: READ_KEYS,
                armed,
                why: None,
                notice: None,
            }),
            // The paths were made one clean line when asked, and the host
            // refused an add whose paths would not show whole.
            ApprovalAsk::GitAdd { paths, files } => Some(ApprovalPrompt {
                target: "stage files in your repository's index".to_owned(),
                command: match files {
                    Some(n) => format!(
                        "git add {paths} ({n} file{})",
                        if *n == 1 { "" } else { "s" }
                    ),
                    None => format!("git add {paths}"),
                },
                keys: EXTERNAL_KEYS,
                armed,
                why: None,
                notice: None,
            }),
            // The program, arguments and folder are what will run, and the host
            // refused text that would not show as it is. The notice is part of
            // what is approved, so the pane must fit it (it is not `why`).
            ApprovalAsk::Exec {
                program,
                args,
                cwd,
                script,
                timeout_secs,
                cloud,
                earlier,
            } => {
                // One logical line per fact, so the folder and the script are
                // rows `fits` checks, not a clipped head. Arguments are quoted
                // where the text alone would read as another argv.
                let words: Vec<String> = std::iter::once(program.as_str())
                    .chain(args.iter().map(String::as_str))
                    .map(display_word)
                    .collect();
                let mut command = format!("{}\nin {}", words.join(" "), display_word(cwd));
                for line in script.iter().flat_map(|text| text.split('\n')) {
                    command.push_str("\nscript ");
                    command.push_str(line);
                }
                Some(ApprovalPrompt {
                    target: "run a command in the sandbox".to_owned(),
                    command,
                    keys: EXTERNAL_KEYS,
                    armed,
                    why: None,
                    notice: Some(exec_notice(
                        program,
                        args,
                        *timeout_secs,
                        *cloud,
                        earlier.as_deref(),
                    )),
                })
            }
            // The review view draws a batch; it has no overlay.
            ApprovalAsk::Batch(_) | ApprovalAsk::Commit(_) => None,
            ApprovalAsk::Folder {
                from,
                to,
                note,
                checking,
            } => Some(ApprovalPrompt {
                target: "Change folder?".to_owned(),
                command: if *checking {
                    format!("checking {}…", sanitize::clean(&to.to_string_lossy()))
                } else {
                    format!("{from} -> {}", sanitize::clean(&to.to_string_lossy()))
                },
                keys: if *checking {
                    FOLDER_CHECKING_KEYS
                } else {
                    FOLDER_KEYS
                },
                armed,
                why: None,
                notice: note.clone(),
            }),
            // Server, tool and arguments are sanitized and capped when the ask
            // is made; `y` approves exactly this.
            ApprovalAsk::External { server, tool, args } => Some(ApprovalPrompt {
                target: format!("call a tool of the MCP server {server}"),
                command: format!("{server} {tool} {args}"),
                keys: EXTERNAL_KEYS,
                armed,
                why: None,
                notice: None,
            }),
        }
    }

    /// `run in pane N (<cwd>)` for a shell pane of the active tab; `None`
    /// when it is not one. The cwd is cleaned: it is text from the shell.
    fn pane_label(&self, pane: PaneId) -> Option<String> {
        let number = self.pane_number(pane)?;
        let state = self.panes.get(&pane).filter(|s| s.pty().is_some())?;
        Some(match state.effective_cwd() {
            Some(cwd) => format!(
                "run in pane {number} ({})",
                sanitize::clean(&self.home_label(cwd))
            ),
            None => format!("run in pane {number}"),
        })
    }
}

/// `word` as a shell would need it written to be one word, for display only:
/// quoted when it is empty or has a space, a quote or another character a
/// shell reads. Nothing is run through a shell.
fn display_word(word: &str) -> String {
    let plain = !word.is_empty()
        && !word
            .chars()
            .any(|c| c.is_whitespace() || "'\"\\$`;|&<>()*?#~{}![]^".contains(c));
    if plain {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}
