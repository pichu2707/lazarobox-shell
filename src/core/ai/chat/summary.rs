//! The end-of-conversation summary (engram-memory M8): the request that asks
//! the `chat` engine for it, the run that collects the answer, and the draft
//! the user reviews. Pure: the keys act on the draft and the app does the rest.
//!
//! A summary is never kept without the user's approval. The text that is kept
//! has been through the redaction pass twice: once when the draft opens, once
//! more on approval, where a change keeps the draft open so the user sees
//! what was removed. Approving sends nothing: the kept summary goes out once,
//! with the session end.

use std::time::Instant;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{
    ChatState, Entry, Follow, Phase,
    composer::Composer,
    engine::{
        Availability, ChatError, ChatMessage, ChatRequest, Role, StreamEvent, Tools, build_context,
        history_chars,
    },
    note::REDACTED_AGAIN as NOTE_REDACTED_AGAIN,
    yank,
};
use crate::core::ai::{memory::redact::redact, sanitize::clean};

/// The fixed system prompt of a summary request.
pub const SUMMARY_PROMPT: &str = include_str!("prompts/summary.md");
/// The last message of the request: it asks for the summary itself.
pub const INSTRUCTION: &str =
    "Write the summary of this conversation now, following your instructions.";
/// The most characters of a summary the draft holds.
pub const SUMMARY_CAP: usize = 4_000;
/// The most bytes of the engine's answer kept while it streams. Past it the
/// run ends (the draft is cut at `SUMMARY_CAP` anyway).
pub const COLLECT_CAP: usize = 32 * 1024;

/// Notice for the summary key with nothing to summarize.
pub const NOTHING_TO_SUMMARIZE: &str = "Nothing to summarize";
/// Notice for the summary key while a stream or another summary runs.
pub const SUMMARY_BUSY: &str = "Still answering: wait for it or press Esc to cancel";
/// Notice when the second redaction pass changed the text.
pub const REDACTED_AGAIN: &str = NOTE_REDACTED_AGAIN;
/// Notice for an approval with no text.
pub const SUMMARY_REQUIRED: &str = "Summary is empty";
/// Hint when the engine's summary was longer than the draft.
pub const CUT_NOTICE: &str = "Cut to 4000 characters";
/// Hint when the stream was cut before the model finished.
pub const STREAM_CUT: &str = "stream cut; the summary may be incomplete";
/// System line for a run that ended with no text.
pub const NO_SUMMARY: &str = "the model sent no summary";
/// Notice after an approval.
pub const KEPT: &str = "Summary kept; it is sent when the conversation closes";

/// `Cannot summarize: <reason>`.
pub fn cannot_summarize(reason: &str) -> String {
    format!("Cannot summarize: {reason}")
}

/// What a key did to the draft, for the app to act on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum SummaryOutcome {
    /// Consumed; nothing for the app to do but redraw.
    Handled,
    /// A stable, twice-redacted text to keep.
    Approve(String),
    /// The user dropped the draft.
    Discard,
}

/// The summary the user reviews: one text field.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct SummaryDraft {
    content: Composer,
    /// What the opening did: secrets redacted, text cut.
    hint: Option<String>,
    /// The last refusal; cleared by typing.
    notice: Option<String>,
}

impl SummaryDraft {
    /// A draft on `text`: sanitized, cut to the cap, then redacted.
    pub fn seeded(text: &str) -> Self {
        let clean = clean(text);
        let cut = clean.chars().count() > SUMMARY_CAP;
        let kept: String = clean.chars().take(SUMMARY_CAP).collect();
        let redacted = redact(&kept);
        let mut parts = Vec::new();
        match redacted.hits {
            0 => {}
            1 => parts.push("1 secret redacted".to_owned()),
            n => parts.push(format!("{n} secrets redacted")),
        }
        if cut {
            parts.push(CUT_NOTICE.to_owned());
        }
        Self {
            content: filled(&redacted.text),
            hint: (!parts.is_empty()).then(|| parts.join(" · ")),
            notice: None,
        }
    }

    pub fn content(&self) -> String {
        self.content.text()
    }

    pub fn content_composer(&self) -> &Composer {
        &self.content
    }

    /// What the opening did, like `2 secrets redacted`.
    pub fn hint(&self) -> Option<String> {
        self.hint.clone()
    }

    /// The stream that wrote the text was cut: the hint says it may be short.
    fn mark_cut(&mut self) {
        self.hint = Some(match self.hint.take() {
            Some(hint) => format!("{hint} · {STREAM_CUT}"),
            None => STREAM_CUT.to_owned(),
        });
    }

    /// The last refusal.
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// The approval was refused: the text stays and the reason shows.
    pub fn refused(&mut self, reason: &str) {
        self.notice = Some(reason.to_owned());
    }

    /// One key. Everything is consumed: nothing here reaches the composer
    /// underneath or a PTY, and Ctrl+C never quits. `repeat` marks a held key.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool) -> SummaryOutcome {
        let (code, mods) = (key.code, key.modifiers);
        if code == KeyCode::Esc && mods == KeyModifiers::NONE {
            return if repeat {
                SummaryOutcome::Handled
            } else {
                SummaryOutcome::Discard
            };
        }
        let plain = mods - KeyModifiers::SHIFT == KeyModifiers::NONE;
        match code {
            KeyCode::Char('s') if mods == KeyModifiers::CONTROL => {
                return if repeat {
                    SummaryOutcome::Handled
                } else {
                    self.begin_approve()
                };
            }
            KeyCode::Enter if mods == KeyModifiers::NONE => self.edit(Composer::newline),
            KeyCode::Char(ch) if plain => self.edit(|c| c.insert(ch)),
            KeyCode::Backspace if plain => self.edit(Composer::backspace),
            KeyCode::Delete if plain => self.edit(Composer::delete),
            KeyCode::Left if plain => self.edit(Composer::left),
            KeyCode::Right if plain => self.edit(Composer::right),
            KeyCode::Home if plain => self.edit(Composer::home),
            KeyCode::End if plain => self.edit(Composer::end),
            KeyCode::Up if plain => self.edit(Composer::up),
            KeyCode::Down if plain => self.edit(Composer::down),
            _ => {}
        }
        SummaryOutcome::Handled
    }

    /// A paste into the text.
    pub fn paste(&mut self, text: &str) {
        self.edit(|c| c.paste(text));
    }

    fn edit(&mut self, change: impl FnOnce(&mut Composer)) {
        change(&mut self.content);
        self.notice = None;
    }

    /// The checks of an approval: some text, then the second redaction pass.
    /// Text the pass changes replaces the field and is not kept.
    fn begin_approve(&mut self) -> SummaryOutcome {
        let text = self.content.text();
        if text.trim().is_empty() {
            self.notice = Some(SUMMARY_REQUIRED.to_owned());
            return SummaryOutcome::Handled;
        }
        let again = redact(&text);
        if again.text != text {
            self.content = filled(&again.text);
            self.notice = Some(REDACTED_AGAIN.to_owned());
            return SummaryOutcome::Handled;
        }
        self.notice = None;
        SummaryOutcome::Approve(text)
    }
}

/// A composer of `SUMMARY_CAP` characters holding `text`, the cursor at its end.
fn filled(text: &str) -> Composer {
    let mut composer = Composer::with_cap(SUMMARY_CAP);
    composer.paste(text);
    composer
}

impl ChatState {
    /// Whether a summary request holds the stream slot.
    pub fn summarizing(&self) -> bool {
        self.summarizing.is_some()
    }

    /// The draft, while it is open over the conversation.
    pub fn summary_draft(&self) -> Option<&SummaryDraft> {
        self.draft.as_ref()
    }

    pub fn summary_draft_mut(&mut self) -> Option<&mut SummaryDraft> {
        self.draft.as_mut()
    }

    /// The summary the user approved, kept for the session end.
    pub fn approved_summary(&self) -> Option<&str> {
        self.approved_summary.as_deref()
    }

    /// Starts a summary request: the stream slot is taken and the request to
    /// send comes back with its turn. `None` when the engine is not ready, a
    /// stream (or another summary) runs, or there is no answer to summarize.
    /// The history is the message texts only, capped like a turn's; no
    /// reasoning, no memory block and no workspace block go with it.
    pub fn begin_summary(&mut self, availability: &Availability) -> Option<(u64, ChatRequest)> {
        self.begin_summary_at(availability, Instant::now())
    }

    /// `begin_summary` with the clock injected.
    pub fn begin_summary_at(
        &mut self,
        availability: &Availability,
        now: Instant,
    ) -> Option<(u64, ChatRequest)> {
        let Availability::Ready { engine, model } = availability else {
            return None;
        };
        if self.phase != Phase::Idle || yank::last_answer(self).is_none() {
            return None;
        }
        let all: Vec<&super::Msg> = self.messages().collect();
        let history: Vec<ChatMessage> = super::sendable_indices(&all)
            .into_iter()
            .map(|i| ChatMessage {
                role: all[i].role,
                content: all[i].text.clone(),
            })
            .collect();
        // The newest question may be waiting for its answer (cancelled): the
        // instruction is the user message here, so that one is left out.
        let mut history = history;
        while history.last().is_some_and(|m| m.role == Role::User) {
            history.pop();
        }
        let room = history_chars(engine).saturating_sub(INSTRUCTION.chars().count());
        let mut messages = build_context(&history, room).messages;
        messages.push(ChatMessage {
            role: Role::User,
            content: INSTRUCTION.to_owned(),
        });
        self.compaction.cancel();
        self.turn += 1;
        self.phase = Phase::Waiting { slow: false };
        self.started = Some(now);
        self.request_model = model.clone().unwrap_or_default();
        self.summarizing = Some(String::new());
        self.generation += 1;
        Some((
            self.turn,
            ChatRequest {
                families: crate::core::ai::tools::registry::FamilySet::LEGACY,
                engine: engine.clone(),
                model: model.clone(),
                system: SUMMARY_PROMPT.trim_end().to_owned(),
                memory: None,
                tools: Tools::Off,
                messages,
            },
        ))
    }

    /// One event of the summary run: thinking is dropped, text is collected
    /// and the draft opens when the run ends well.
    pub(super) fn apply_summary(&mut self, event: StreamEvent) -> Follow {
        match event {
            StreamEvent::Slow => {
                if let Phase::Waiting { slow } = &mut self.phase {
                    *slow = true;
                }
                Follow::Nothing
            }
            StreamEvent::ThinkingDelta(_)
            | StreamEvent::Notice(_)
            | StreamEvent::ToolCall { .. }
            | StreamEvent::ToolResult { .. } => Follow::Nothing,
            StreamEvent::Delta(text) => {
                if text.is_empty() {
                    return Follow::Nothing;
                }
                self.phase = Phase::Streaming;
                let Some(collected) = self.summarizing.as_mut() else {
                    return Follow::Nothing;
                };
                let room = COLLECT_CAP - collected.len().min(COLLECT_CAP);
                if text.len() <= room {
                    collected.push_str(&text);
                    return Follow::Nothing;
                }
                let mut cut = room;
                while !text.is_char_boundary(cut) {
                    cut -= 1;
                }
                collected.push_str(&text[..cut]);
                self.finish_summary(None, false);
                // Anything the engine still sends belongs to a turn nobody awaits.
                self.turn += 1;
                Follow::CancelEngine
            }
            StreamEvent::Done(_) => {
                self.finish_summary(None, false);
                Follow::Nothing
            }
            StreamEvent::Error(ChatError::Truncated) => {
                self.finish_summary(None, true);
                Follow::Nothing
            }
            StreamEvent::Error(ChatError::Interrupted) => {
                self.finish_summary(Some(super::INTERRUPTED.to_owned()), false);
                Follow::Nothing
            }
            StreamEvent::Error(ChatError::Failed(message)) => {
                self.finish_summary(Some(message), false);
                Follow::Nothing
            }
            StreamEvent::Error(ChatError::Jail(reason)) => {
                self.finish_summary(Some(super::jail_error(&reason)), false);
                Follow::Nothing
            }
        }
    }

    /// Ends the run: the draft opens on the text, or the failure is said once.
    /// `cut` marks a stream that ended early: the draft says so.
    fn finish_summary(&mut self, failure: Option<String>, cut: bool) {
        let text = self.summarizing.take().unwrap_or_default();
        self.phase = Phase::Idle;
        self.started = None;
        self.request_model.clear();
        self.generation += 1;
        match failure {
            Some(why) => self
                .entries
                .push(Entry::System(format!("summary failed: {why}"))),
            None if text.trim().is_empty() => {
                self.entries.push(Entry::System(NO_SUMMARY.to_owned()));
            }
            None => {
                let mut draft = SummaryDraft::seeded(&text);
                if cut {
                    draft.mark_cut();
                }
                self.draft = Some(draft);
            }
        }
    }

    /// Stops the run in flight; no draft opens.
    pub(super) fn cancel_summary(&mut self) -> Follow {
        self.summarizing = None;
        self.phase = Phase::Idle;
        self.started = None;
        self.request_model.clear();
        self.turn += 1;
        self.generation += 1;
        Follow::CancelEngine
    }

    /// The user approved `text`: it is kept (replacing an earlier one), cut
    /// to the cap, and the draft closes. Nothing is sent.
    pub fn approve_summary(&mut self, text: &str) {
        self.approved_summary = Some(text.chars().take(SUMMARY_CAP).collect());
        self.draft = None;
    }

    /// An approved summary in a chat that wrote no note has no session to end
    /// yet: takes the one `new_id` gives, under the chat's project. A session
    /// the chat already holds (its notes', or the pair the store kept) wins.
    /// Returns whether the session is new. Nothing is sent: the worker
    /// registers it with the end.
    pub fn adopt_summary_session(&mut self, new_id: impl FnOnce() -> Option<String>) -> bool {
        if self.approved_summary.is_none() || self.session().is_some() {
            return false;
        }
        let Some(project) = self.project().map(str::to_owned) else {
            return false;
        };
        match new_id() {
            Some(id) => self.adopt_session(&id, &project),
            None => false,
        }
    }

    /// Forgets the kept summary: it will not go out with the session end.
    pub fn drop_approved_summary(&mut self) {
        self.approved_summary = None;
    }

    /// The user dropped the draft: nothing new is kept; an earlier approval
    /// stays.
    pub fn discard_summary(&mut self) {
        self.draft = None;
    }
}
