//! The orchestrator in a chat pane (agent-orchestrator D2, D5): the pane's
//! ORCH mode, its Ctrl+O toggle, the task a send runs on, the one-time
//! provider notice (D12), the run's events and its failure alert. State and
//! keys only; the badge, the hint, the notice and the alert are drawn by the
//! UI.

use super::*;
use crate::core::ai::{
    chat::{
        Entry, GateOffer, OrchAlert, OrchNote,
        engine::{Availability, OrchSend, Tools, availability_for, tools_for},
        guidance::{delegate_hint, gates_hint, undelegated_hint},
        transcript::{kind_label, worker_label},
    },
    config::Assignment,
    orchestra::{
        Toolset, WorkerId, WorkerKey,
        changes::Disclosure,
        event::{Continuation, Degrade},
        ledger::{NO_HANDOFF, Unit, Verdict},
        overrides::{Change, FailurePicker, Picked},
        persist::Item,
        run::{Roster, RunLimits},
        worker::{ErrorClass, WorkerStatus},
    },
};

/// Why an orchestrator turn is refused when the user's tools are off: it
/// could not offer the orchestration tools.
const TOOLS_OFF: &str =
    "ORCH needs tools: turn them on in the menu (Ctrl+Space m), or press Ctrl+O to chat";

/// The provider notice (agent-orchestrator D12) with the keys that answer
/// it: Enter accepts, Esc dismisses (`y` is typed).
const NOTICE: &str = "Providers recommend API keys for automation; \
     consumer-account logins carry risk. Enter accept \u{b7} Esc dismiss";

/// What the delegate key sends: lazarobox's own words, never the model's or a
/// tool result's. The run keeps the user's request as its goal.
const DELEGATE_ASK: &str =
    "Please delegate my previous request to a worker with spawn_agent, then wait for its result.";
/// Said when the delegate key is pressed with no undelegated request.
const NOTHING_TO_DELEGATE: &str = "nothing to delegate";
/// Said when the delegate key found the turn could not go.
const DELEGATE_NOT_SENT: &str = "the delegation was not sent; press Enter to try again";

/// Said when `m` opens the engines because the worker has no engine to take
/// the models from.
const NO_ENGINE_FOR_MODEL: &str =
    "This worker has no engine and none is configured for its task; pick an engine first";
/// The keys of the failure alert after `r retry`: `m` and `p` open the
/// detached picker, `c` or Esc cancels.
const ALERT_KEYS: &str = "m change model \u{b7} p change provider \u{b7} c/Esc cancel";
const RETRY_KEY: &str = "r retry";
/// Said when `r` is pressed again after a retry that failed.
const RETRY_AGAIN: &str = "r try again";
/// Said of a `code` worker that ended `done` and staged nothing
/// (agent-orchestrator [W1]); ASCII and neutral. [`disclosure_line`] names
/// the worker before it.
const CHANGED_NO_FILE: &str = "ended done and changed no file";
/// What a worker's reply holds when it changed no file and wrote tool calls
/// as text, after [`CHANGED_NO_FILE`]: a likely cause, not a certain one.
const TOOL_CALLS_AS_TEXT: &str =
    "its reply holds tool calls written as text: the model may lack tool support";
/// Said of a worker whose staged batch was never proposed (agent-orchestrator
/// [W1]), after the count of files.
const NEVER_PROPOSED: &str = "and never proposed the batch; the pane holds it as an orphan";
/// Said when the run manager is not attached: the answer was not applied.
const NO_ORCHESTRA: &str = "orchestration is not available: your answer was not applied";
/// Said when the run manager dropped an answer: its run is over.
const ANSWER_LOST: &str = "that worker already ended: your answer was not applied";
const MOVED_ON: &str = "an earlier run already moved on: your answer was not applied";

/// The footer hint of the ORCH toggle (modal-input, ORCH toggle key): what
/// Ctrl+O does now, or why it does nothing while a run is active.
const HINT_ON: &str = "Ctrl+O: orchestrate this pane";
const HINT_OFF: &str = "Ctrl+O: stop orchestrating";
pub(super) const HINT_RUN: &str =
    "A run is active: cancel the run first (Ctrl+C), then Ctrl+O changes the mode";

/// Said in the pane when ORCH turns on. The coordinator is offered the read,
/// pane-read and orchestration tools only, whatever the saved Write, Exec and GitHub
/// toggles say, so without these lines the model's "I have no tools" has no
/// visible cause. The head is short on purpose: the ORCH badge covers the end
/// of the first row of an empty chat, and the body must not lose its key name.
const ORCH_ON_HEAD: &str = "ORCH on";
const ORCH_ON_BODY: &str = "This chat cannot write files, run commands or read GitHub itself. \
    A code worker writes, through the batch review. Ctrl+O goes back to a plain chat.";
/// Said after the body while the menu has Write off: a `code` worker follows
/// that toggle and is refused with a notice (agent-orchestrator, W2).
const ORCH_ON_WRITE: &str =
    "code workers need Write: type /write, or turn it on in the AI menu (Ctrl+Space m)";
/// The same with the master tools switch off too.
const ORCH_ON_TOOLS_WRITE: &str = "code workers need tools and Write: turn tools on in the AI menu (Ctrl+Space m), then type /write";

/// Said by the first Ctrl+C of a selected worker.
const CANCEL_AGAIN: &str = "Ctrl+C again cancels the run";
const NO_WORKER: &str = "No worker to select";
/// How long a second Ctrl+C escalates to the whole run.
const ESCALATE: Duration = Duration::from_secs(2);

/// What the chat footer says about ORCH while the gate is on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OrchHint {
    /// Ctrl+O toggles the pane's mode.
    Toggle,
    /// A run is active in the pane: it must be cancelled before the mode
    /// can change.
    CancelRunFirst,
}

impl App {
    /// The global orchestrator gate (`[ai.orchestrator] enabled`), read from
    /// the live configuration every time.
    pub(super) fn orch_gate(&self) -> bool {
        self.ai.orchestrator.enabled
    }

    /// Ctrl+C on chat `id`. With a worker row selected the first press
    /// cancels only that worker and says so; a second one within
    /// [`ESCALATE`], or one with no worker selected, is the chat cancel: the
    /// turn and the whole run (modal-input, Cancel keys for workers).
    pub(super) fn chat_ctrl_c(&mut self, id: PaneId) -> Vec<Effect> {
        let now = (self.clock)();
        let gate = self.orch_gate();
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let target = chat.orch_run().zip(chat.selected_worker());
        let escalate = chat.cancel_is_armed(now);
        let live = chat.live_workers();
        let Some((run, worker)) = target.filter(|(_, worker)| gate && live.contains(worker)) else {
            return self.chat_cancel_key(id);
        };
        if escalate {
            return self.chat_cancel_key(id);
        }
        chat.select_worker(None);
        chat.arm_cancel(Some(now + ESCALATE));
        self.notice = Some(CANCEL_AGAIN.to_owned());
        self.dirty = true;
        vec![Effect::OrchCancelWorker {
            pane: id,
            run,
            worker,
        }]
    }

    /// Alt+Down and Alt+Up: select the next or the previous worker of the
    /// run that has not ended, wrapping; said when there is none.
    pub(super) fn select_worker(&mut self, id: PaneId, forward: bool) -> Vec<Effect> {
        if !self.orch_gate() {
            return Vec::new();
        }
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let live = chat.live_workers();
        if live.is_empty() {
            self.notice = Some(NO_WORKER.to_owned());
            self.dirty = true;
            return Vec::new();
        }
        let at = chat
            .selected_worker()
            .and_then(|worker| live.iter().position(|w| *w == worker));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => live.len() - 1,
            (Some(i), true) => (i + 1) % live.len(),
            (Some(i), false) => (i + live.len() - 1) % live.len(),
        };
        chat.select_worker(Some(live[next]));
        self.dirty = true;
        Vec::new()
    }

    /// Whether pane `id` orchestrates: the global gate and the pane's mode
    /// both on.
    pub fn orchestrating(&self, id: PaneId) -> bool {
        self.orch_gate()
            && self
                .panes
                .get(&id)
                .and_then(PaneState::chat_state)
                .is_some_and(ChatState::orch)
    }

    /// Whether a run is active in pane `id`: from its orchestrator turn
    /// until the run's `Ended`, since a worker can outlive the turn.
    pub(super) fn orch_run_active(&self, id: PaneId) -> bool {
        self.panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(|chat| chat.orch_turn_in_flight() || chat.orch_run().is_some())
    }

    /// What the run manager said about the run of chat `id`. A failure shows
    /// its alert, which stays until it is answered or the run ends: rows,
    /// other output and time leave it as it is. Events of a pane that is not
    /// a chat any more are dropped.
    pub(super) fn on_orchestra(&mut self, id: PaneId, event: OrchestraEvent) -> Vec<Effect> {
        let event = match event {
            OrchestraEvent::Continue(next) => return self.on_continue(id, next),
            OrchestraEvent::Undelegated { run } => return self.on_undelegated(id, run),
            event => event,
        };
        // A worker's step is a redraw only when its transcript is on view.
        if let OrchestraEvent::Trace { run, worker, item } = event {
            if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
                let shown = chat
                    .run_view()
                    .is_some_and(|v| (v.run, v.worker) == (run, worker));
                chat.trace(run, worker, item);
                self.dirty |= shown;
            }
            return Vec::new();
        }
        // A run that ended stores nothing more, whether its pane is still a
        // chat or not.
        let mut kept = Vec::new();
        if let OrchestraEvent::Ended { run } = &event {
            self.end_store(*run);
            kept = self.keep_end(*run);
        }
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            // The spend happened: with history on it is stored unlinked.
            // There is no pane to tell if that write fails.
            if let OrchestraEvent::Usage {
                run,
                worker,
                engine,
                usage,
            } = event
                && self.history
            {
                kept.push(Effect::Store(StoreCmd::RecordUsage {
                    pane: id,
                    conversation: None,
                    engine,
                    role: "worker".to_owned(),
                    worker: Some(format!("{}.{}", run.0, worker.0)),
                    usage,
                }));
            }
            return kept;
        };
        // A worker that is over takes its waiting asks with it.
        let mut over = None;
        // The run that ended takes every ask of its workers with it.
        let mut ended = None;
        // What the run stores in engram once the pane is not borrowed.
        let mut store = None;
        // A worker turn's usage waits in the pane for the usage store.
        let mut usage_saved = false;
        match event {
            OrchestraEvent::Usage {
                run,
                worker,
                engine,
                usage,
            } => {
                chat.record_worker_usage(WorkerKey { run, worker }, &engine, *usage);
                usage_saved = true;
            }
            OrchestraEvent::Worker(mut row) => {
                let key = WorkerKey {
                    run: row.run,
                    worker: row.id,
                };
                let is_over = !matches!(row.status, WorkerStatus::Queued | WorkerStatus::Running);
                chat.set_worker_over(key, is_over);
                if is_over && chat.selected_worker() == Some(row.id) {
                    chat.select_worker(None);
                }
                if is_over {
                    over = Some(key);
                }
                // A writing worker exists (`code` or `simple`): both gates
                // pass, nothing is left to lift.
                if matches!(
                    row.kind,
                    crate::core::ai::config::Task::Code | crate::core::ai::config::Task::Simple
                ) {
                    chat.drop_gate_offer();
                }
                chat.set_orch_run(Some(row.run));
                chat.keep_override();
                let menu = self.ai.resolve(row.kind).0.engine;
                row.overridden = chat.orch_overrides().is_some_and(|over| {
                    over.is_on(row.kind, &row.engine, row.model.as_deref(), menu.as_deref())
                });
                chat.worker_row(row.clone());
                kept = self.keep_worker(id, &row);
            }
            OrchestraEvent::Halted {
                run,
                worker,
                class,
                text,
            } => {
                chat.set_orch_run(Some(run));
                chat.keep_override();
                chat.drop_orch_alert();
                // The alert is the user's to answer: the transcript gives way.
                chat.close_run_view();
                chat.set_orch_alert(Some(OrchAlert {
                    run,
                    worker,
                    class,
                    text,
                }));
            }
            // The answer did not go through: the alert comes back and says why.
            OrchestraEvent::Declined {
                run,
                worker,
                class,
                text,
                reason,
            } => {
                chat.set_orch_run(Some(run));
                chat.drop_orch_alert();
                chat.close_run_view();
                chat.set_orch_note(Some(OrchNote::RetryFailed(reason)));
                chat.set_orch_alert(Some(OrchAlert {
                    run,
                    worker,
                    class,
                    text,
                }));
            }
            // The run it names is over: the pane's own alert is not touched.
            // Always said (never a silent no-op); the wording is neutral so it
            // is not read as part of a newer run.
            OrchestraEvent::Dropped { .. } => {
                chat.undo_override();
                chat.push_system_line(MOVED_ON.to_owned());
            }
            OrchestraEvent::AnswerLost => {
                chat.push_system_line(ANSWER_LOST.to_owned());
            }
            // A newer run of the pane is not this run's business.
            OrchestraEvent::Expired { run, after } => {
                if chat.orch_run().is_none_or(|current| current == run) {
                    let mins = after.as_secs() / 60;
                    chat.push_system_line(format!(
                        "run stopped: it reached run_timeout_mins ({mins}); running workers were cancelled"
                    ));
                }
            }
            // The turn cap, or a ledger that cannot be wrapped (S4): said, in
            // the key's own name, for the run it names.
            OrchestraEvent::Stopped { run, reason } => {
                if chat.orch_run().is_none_or(|current| current == run) {
                    chat.push_system_line(reason);
                    // The run is ending: no key is offered for it.
                    chat.drop_gate_offer();
                }
            }
            // Where a worker's review stands (S7b): lazarobox's own words.
            OrchestraEvent::Review { run, text } => {
                if chat.orch_run().is_none_or(|current| current == run) {
                    chat.push_system_line(text);
                }
            }
            // A fact about a worker's end: a system line, never history.
            OrchestraEvent::Disclosed {
                run,
                worker,
                workers,
                kind,
                engine,
                model,
                what,
            } => {
                if chat.orch_run().is_none_or(|current| current == run) {
                    let who = kind_label(kind, &engine, model.as_deref());
                    chat.push_system_line(disclosure_line(worker, workers, &who, what));
                }
            }
            // A spawn that made no worker, with the coordinator's own line.
            OrchestraEvent::SpawnRefused { run, kind, reason } => {
                if chat.orch_run().is_none_or(|current| current == run) {
                    chat.push_system_line(ascii(format!(
                        "spawn of a {} worker refused: {reason}",
                        kind.id()
                    )));
                }
            }
            // A step gate refused a spawn or the finish: its line, as told.
            OrchestraEvent::GateRefused {
                run,
                gate,
                step,
                reason,
            } => {
                if chat.orch_run().is_none_or(|current| current == run) {
                    chat.push_system_line(ascii(reason));
                    // The refusal makes the run known: its offer outlives the turn.
                    chat.set_orch_run(Some(run));
                    // The offer belongs to this run: a row of another one drops it.
                    chat.offer_gates(GateOffer { run, gate, step });
                }
            }
            // The turn ended with the spawn gate standing: the run stays open
            // for the reply, and the pane says why (W3, option B).
            OrchestraEvent::Waits { run, step } => {
                // A `Waits` that comes after the run ended or was cancelled
                // marks nothing.
                if chat.orch_run() == Some(run) && !chat.run_cancelled(run) {
                    chat.set_waiting_run(Some(run));
                    chat.push_system_line(format!(
                        "run waits for your reply: step {} is not closed (Enter gates off)",
                        step.label()
                    ));
                }
            }
            // A worker turn that runs as text: said before it starts.
            OrchestraEvent::Degraded {
                run,
                worker,
                workers,
                kind,
                engine,
                model,
                why,
            } => {
                if chat.orch_run().is_none_or(|current| current == run) {
                    let who = kind_label(kind, &engine, model.as_deref());
                    let said = format!("{RAN_WITHOUT_TOOLS}: {}", degrade_reason(why));
                    chat.push_system_line(worker_notice(worker, workers, &who, &said));
                }
            }
            // Handled before the pane is borrowed.
            OrchestraEvent::Trace { .. } => {}
            // Handled before the pane is borrowed.
            OrchestraEvent::Continue(_) | OrchestraEvent::Undelegated { .. } => {}
            OrchestraEvent::Notice { text } => chat.push_system_line(text),
            OrchestraEvent::Unavailable => {
                chat.undo_override();
                chat.push_system_line(NO_ORCHESTRA.to_owned());
            }
            // The transcript hears the verdicts that need the user's eye (S4);
            // every handoff is stored in engram (D13), or kept in the ledger
            // when engram cannot be used.
            OrchestraEvent::Handoff { run, handoff } => {
                store = Some((run, Item::of_handoff(&handoff)));
                if matches!(handoff.verdict, Verdict::OffCourse | Verdict::Blocked) {
                    let subject = match handoff.unit {
                        Unit::Worker(worker) => {
                            format!("worker {} {}", worker.0, handoff.kind.id())
                        }
                        Unit::Step(step) => format!("step {}", step.label()),
                        Unit::Receipt(worker, n) => format!("receipt {n} of worker {}", worker.0),
                    };
                    chat.push_system_line(format!(
                        "handoff {subject} {}: {}",
                        handoff.verdict.label(),
                        handoff.reason
                    ));
                }
            }
            // The feature document of a large goal: one note, one key, stored
            // again when a correction rewrites it (S5, D15).
            OrchestraEvent::Feature { run, doc } => store = Some((run, Item::of_feature(&doc))),
            // The finished run's ledger and summary, stored like a handoff.
            OrchestraEvent::Closed {
                run,
                ledger,
                summary,
            } => {
                // The run finished: its gates passed or no longer matter.
                chat.drop_gate_offer();
                store = Some((run, Item::of_ledger(run, &ledger, &summary)));
            }
            // Never a silent fallback: say which handoffs lazarobox wrote.
            OrchestraEvent::Filled { workers, .. } => {
                let ids: Vec<_> = workers.iter().map(|id| id.0.to_string()).collect();
                let noun = if workers.len() == 1 {
                    "worker"
                } else {
                    "workers"
                };
                chat.push_system_line(format!("{NO_HANDOFF} for {noun} {}", ids.join(", ")));
            }
            // Only the run it names ends: a replaced run ends after the new
            // one started.
            OrchestraEvent::Ended { run } => {
                ended = Some(run);
                chat.end_orch_run(run);
                if chat.waiting_run() == Some(run) {
                    chat.set_waiting_run(None);
                    chat.push_system_line(
                        "run no longer waits for your reply: the run ended".to_owned(),
                    );
                }
                // No live run is left to take a lift.
                if chat.gate_offer().is_some_and(|offer| offer.run == run) {
                    chat.drop_gate_offer();
                }
                if chat.orch_run() == Some(run) {
                    chat.set_orch_run(None);
                }
            }
        }
        if let Some(key) = over {
            self.withdraw_worker(id, key);
        }
        if let Some(run) = ended {
            self.withdraw_run(id, run);
        }
        self.dirty = true;
        let mut effects = kept;
        if usage_saved {
            effects.extend(self.store_effects(id));
        }
        if let Some((run, item)) = store {
            effects.extend(self.store_item(id, run, item));
        }
        effects
    }

    /// The orchestrator turn of the run of pane `id` ended with work pending
    /// (S4, D8): the next turn starts when the pane is idle, which the turn's
    /// own end may not have made it yet.
    fn on_continue(&mut self, id: PaneId, next: Continuation) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        if chat.run_cancelled(next.run) {
            return Vec::new();
        }
        if chat.phase() != Phase::Idle {
            chat.park_continue(next);
            return Vec::new();
        }
        self.start_continuation(id, next)
    }

    /// The orchestrator turn of pane `id` ended with no worker made (D27): the
    /// notice shows when the pane is idle and the turn did not fail, and waits
    /// for the turn's end otherwise. Nothing starts by itself.
    fn on_undelegated(&mut self, id: PaneId, run: RunId) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        if chat.run_cancelled(run) {
            return Vec::new();
        }
        if chat.phase() != Phase::Idle {
            chat.park_undelegated(run);
            return Vec::new();
        }
        if chat.turn_clean() && chat.orch_run().is_none_or(|live| live == run) {
            self.say_undelegated(id);
        }
        Vec::new()
    }

    /// The turn of pane `id` ended: a parked undelegated notice shows if it
    /// ended well (`done`) and its run is still the pane's, or the one that
    /// just ended; it goes otherwise.
    pub(super) fn flush_undelegated(&mut self, id: PaneId, done: bool) {
        let parked = (self.panes.get_mut(&id))
            .and_then(PaneState::chat_mut)
            .and_then(|chat| Some((chat.take_undelegated()?, chat.orch_run())));
        if let Some((run, live)) = parked
            && done
            && live.is_none_or(|live| live == run)
        {
            self.say_undelegated(id);
        }
    }

    /// Adds the "no worker ran" line, unless pane `id` is a resumed run that
    /// waits for the user's reply: it says so itself.
    fn say_undelegated(&mut self, id: PaneId) {
        if self.resume_waits(id) {
            return;
        }
        let cols = self.panes.get(&id).map_or(80, |p| usize::from(p.size.cols));
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(undelegated_hint(cols));
            // The key acts on the user's request, not on its own earlier ask.
            if let Some(goal) = chat.last_request(DELEGATE_ASK) {
                chat.offer_delegate(goal);
            }
            self.dirty = true;
        }
    }

    /// Whether the delegate key can act in pane `id` now: a request is
    /// offered, the pane is orchestrating, idle, with an empty composer and
    /// ready, and nothing that answers Enter first (D24) is on screen.
    fn delegate_ready(&self, id: PaneId) -> bool {
        let Some(chat) = self.panes.get(&id).and_then(PaneState::chat_state) else {
            return false;
        };
        chat.delegate_offered()
            && self.orchestrating(id)
            && chat.phase() == Phase::Idle
            && chat.composer().text().is_empty()
            && self.server_offer_prompt(id).is_none()
            && self.orch_guidance(id).is_none()
            && self.orch_refusal(id).is_none()
            && matches!(self.turn_availability(id), Availability::Ready { .. })
    }

    /// The line that names the delegate key, only while it can act.
    pub(super) fn delegate_line(&self, id: PaneId) -> Option<String> {
        let cols = usize::from(self.panes.get(&id)?.size.cols);
        self.delegate_ready(id).then(|| delegate_hint(cols))
    }

    /// Enter and Esc on the delegate line (D27): only a fresh, plain press on
    /// an empty composer of an idle, orchestrating pane. Enter sends
    /// lazarobox's fixed text and the run keeps the user's request; with
    /// nothing offered it says so. `None` leaves the key to the chat.
    pub(super) fn answer_delegate(
        &mut self,
        id: PaneId,
        key: &KeyEvent,
        repeat: bool,
    ) -> Option<Vec<Effect>> {
        let enter = key.code == KeyCode::Enter;
        if !(enter || key.code == KeyCode::Esc)
            || repeat
            || key.kind != KeyEventKind::Press
            || key.modifiers != KeyModifiers::NONE
            || !self.orchestrating(id)
        {
            return None;
        }
        let chat = self.panes.get_mut(&id).and_then(PaneState::chat_mut)?;
        if chat.phase() != Phase::Idle || !chat.composer().text().is_empty() {
            return None;
        }
        if !self.delegate_ready(id) {
            // Nothing offered: an Enter that did nothing says why.
            let offered = (self.panes.get(&id).and_then(PaneState::chat_state))
                .is_some_and(ChatState::delegate_offered);
            // Said only when true and nothing else explains the pane: no
            // guidance, no live run or worker, engine ready. A key already clears the
            // status notice, so none can be overwritten here.
            if enter
                && !offered
                && !self.orch_run_active(id)
                && self.server_offer_prompt(id).is_none()
                && self.orch_guidance(id).is_none()
                && self.orch_refusal(id).is_none()
                && matches!(self.turn_availability(id), Availability::Ready { .. })
            {
                self.notice = Some(NOTHING_TO_DELEGATE.to_owned());
                self.dirty = true;
                return Some(Vec::new());
            }
            return None;
        }
        let chat = self.panes.get_mut(&id).and_then(PaneState::chat_mut)?;
        let goal = chat.take_delegate()?;
        self.dirty = true;
        if !enter {
            return Some(Vec::new());
        }
        self.run_keep.resume_goal = Some(goal.clone());
        let availability = self.turn_availability(id);
        let effects = self.send_text(id, DELEGATE_ASK, &availability);
        if effects.is_empty() {
            self.run_keep.resume_goal = None;
            if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
                chat.offer_delegate(goal);
            }
            self.notice = Some(DELEGATE_NOT_SENT.to_owned());
        }
        Some(effects)
    }

    /// Starts the continuation turn on the open run, after a line that says
    /// so. A turn that cannot go ends the run, said: never a silent stop.
    pub(super) fn start_continuation(&mut self, id: PaneId, next: Continuation) -> Vec<Effect> {
        let failure = self
            .panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .map(|chat| (chat.turn_failure().map(str::to_owned), chat.orch_run()));
        let Some((failure, run)) = failure else {
            return Vec::new();
        };
        // Nothing is sent when the run cannot go on: the pane stopped
        // orchestrating, the run is not its live one, or its last turn failed.
        let why = if !self.orchestrating(id) {
            Some("the pane is no longer orchestrating".to_owned())
        } else if run.is_some_and(|live| live != next.run) {
            Some("the run was replaced".to_owned())
        } else {
            failure.map(|cause| format!("the orchestrator turn failed ({cause})"))
        };
        if let Some(why) = why {
            if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
                chat.push_system_line(format!("run stopped: {why}"));
            }
            self.dirty = true;
            return self.cancel_chat(id);
        }
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        chat.push_system_line(format!(
            "orchestrator turn {} of {}: starting from the run ledger",
            next.turn, next.max
        ));
        self.dirty = true;
        let availability = self.turn_availability(id);
        let effects = self.send_with(id, &next.text, &availability, Some(next.run));
        if effects
            .iter()
            .any(|effect| matches!(effect, Effect::OrchestrateSend { .. }))
        {
            return effects;
        }
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.push_system_line(
                "the continuation turn could not start; the run was cancelled".to_owned(),
            );
        }
        let mut effects = effects;
        effects.extend(self.cancel_chat(id));
        effects
    }

    /// The effects that end the runs of the chats in `ids` when they close:
    /// a run does not outlive its pane.
    pub(super) fn close_run_effects(&self, ids: &[PaneId]) -> Vec<Effect> {
        let live = |id: &&PaneId| {
            let chat = self.panes.get(*id).and_then(PaneState::chat_state);
            chat.is_some_and(|c| c.orch_run().is_some() || c.orch_turn_in_flight())
        };
        ids.iter()
            .filter(live)
            .map(|id| Effect::ChatCancel(*id))
            .collect()
    }

    /// A key on chat `id` for the failure alert shown there: `r` retries the
    /// worker, `c` or Esc cancels the run. The letters act only with an
    /// empty composer, so text being typed never picks an option (key audit,
    /// 2026-10-29). Only a fresh, unmodified press answers it; `None` leaves
    /// every other key to the chat.
    pub(super) fn answer_orch_alert(
        &mut self,
        id: PaneId,
        key: &KeyEvent,
        repeat: bool,
    ) -> Option<Vec<Effect>> {
        if key.kind == KeyEventKind::Release {
            return None;
        }
        let chat = self.panes.get_mut(&id).and_then(PaneState::chat_mut)?;
        let alert = chat.orch_alert()?;
        // The open picker keeps every key but Ctrl+C: none reaches the hidden
        // composer. A held Enter or Esc is ignored on purpose (the keys line
        // stays visible), so the picker needs no notice for it.
        if chat.orch_picker().is_some() {
            let cancel = key.code == KeyCode::Char('c') && key.modifiers == KeyModifiers::CONTROL;
            return (!cancel).then(|| self.on_failure_picker_key(id, key, repeat));
        }
        if key.modifiers != KeyModifiers::NONE {
            return None;
        }
        let empty = chat.composer().text().is_empty();
        let letter = matches!(key.code, KeyCode::Char('r' | 'c' | 'm' | 'p'));
        // A held key answers once: its repeats are not typed either.
        if repeat && empty && letter {
            return Some(Vec::new());
        }
        if repeat || key.kind != KeyEventKind::Press {
            return None;
        }
        let choice = match key.code {
            KeyCode::Char('r') if empty => UserChoice::Retry,
            KeyCode::Char('c') if empty => UserChoice::Cancel,
            KeyCode::Esc => UserChoice::Cancel,
            KeyCode::Char('m') if empty => {
                return Some(self.open_failure_picker(id, Change::Model));
            }
            KeyCode::Char('p') if empty => {
                return Some(self.open_failure_picker(id, Change::Provider));
            }
            _ => return None,
        };
        let run = alert.run;
        chat.drop_orch_alert();
        self.dirty = true;
        Some(vec![Effect::OrchDecide {
            pane: id,
            run,
            choice,
            ceiling: self.orch_ceiling(id),
        }])
    }

    /// The roster of `config`, as the menu or an override has it, never
    /// saved. The menu part is read now: a menu save made during a run
    /// changes the rosters of later relaunches for the tasks the user did not
    /// override.
    fn roster(&self, config: &AiConfig) -> Roster {
        Roster::new(
            Task::ALL
                .iter()
                .map(|&task| {
                    let engines = self.engines.as_ref();
                    (task, availability_for(task, config, engines, self.jail))
                })
                .collect(),
        )
    }

    /// `m` or `p` on the alert: the menu picker of the failed worker's task,
    /// detached from the menu. A worker whose row is gone says so.
    fn open_failure_picker(&mut self, id: PaneId, change: Change) -> Vec<Effect> {
        self.dirty = true;
        let Some(chat) = self.panes.get(&id).and_then(PaneState::chat_state) else {
            return Vec::new();
        };
        let failed = chat.orch_alert().and_then(|alert| {
            chat.entries().iter().find_map(|entry| match entry {
                Entry::Worker(row) if (row.run, row.id) == (alert.run, alert.worker) => Some(row),
                _ => None,
            })
        });
        let mut unresolved = false;
        let picker = failed.map(|row| {
            let config = chat
                .orch_overrides()
                .map_or_else(|| self.ai.clone(), |over| over.apply(&self.ai));
            let mut in_use = (row.engine.as_str(), row.model.as_deref());
            // A refused worker never got an engine: the models offered are
            // those of the engine its class is configured with.
            let configured = config.resolve(row.kind).0;
            if in_use.0.is_empty()
                && let Some(engine) = configured.engine.as_deref()
            {
                in_use = (engine, None);
            }
            unresolved = change == Change::Model && in_use.0.is_empty();
            FailurePicker::open(row.kind, &config, in_use, self.engines.clone(), change)
        });
        if unresolved {
            self.notice = Some(NO_ENGINE_FOR_MODEL.to_owned());
        }
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            if picker.is_none() {
                chat.push_system_line("cannot change: that worker is not shown any more".into());
            }
            chat.set_orch_picker(picker);
        }
        Vec::new()
    }

    /// A key while the failure picker is open: it takes every plain key; Esc
    /// returns to the alert, an Enter on a leaf is the override and relaunches
    /// the worker with it.
    fn on_failure_picker_key(&mut self, id: PaneId, key: &KeyEvent, repeat: bool) -> Vec<Effect> {
        self.dirty = true;
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        let Some(picker) = chat.orch_picker_mut() else {
            return Vec::new();
        };
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => picker.step(true),
            KeyCode::Char('k') | KeyCode::Up => picker.step(false),
            KeyCode::Enter if !repeat => {
                let task = picker.task();
                if let Picked::Chosen(assignment) = picker.enter() {
                    return self.choose_override(id, task, assignment);
                }
            }
            KeyCode::Esc if !repeat => chat.set_orch_picker(None),
            _ => {}
        }
        Vec::new()
    }

    /// The user confirmed `assignment` for `task`: it becomes the run's
    /// override and the failed worker is relaunched with the new roster.
    fn choose_override(&mut self, id: PaneId, task: Task, assignment: Assignment) -> Vec<Effect> {
        let Some(chat) = self.panes.get(&id).and_then(PaneState::chat_state) else {
            return Vec::new();
        };
        let Some(run) = chat.orch_alert().map(|alert| alert.run) else {
            return Vec::new();
        };
        // A choice that could not run is refused here, not after a relaunch.
        let mut config =
            (chat.orch_overrides()).map_or_else(|| self.ai.clone(), |o| o.apply(&self.ai));
        config.set(task, assignment.clone());
        let engines = self.engines.as_ref();
        let ready = availability_for(task, &config, engines, self.jail);
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        if !matches!(ready, Availability::Ready { .. }) {
            chat.set_orch_picker(None);
            chat.push_system_line("engine not available".into());
            return Vec::new();
        }
        chat.override_task(run, task, assignment);
        chat.drop_orch_alert();
        let choice = UserChoice::Relaunch(self.roster(&config));
        vec![Effect::OrchDecide {
            pane: id,
            run,
            choice,
            ceiling: self.orch_ceiling(id),
        }]
    }

    /// What pane `id` shows above its composer about its run, in the order
    /// the keys answer: the provider notice, then the failure alert. `None`
    /// when neither is up. Like the `engram serve` offer it is guidance, so
    /// paging counts its rows.
    pub(super) fn orch_guidance(&self, id: PaneId) -> Option<String> {
        let chat = self.panes.get(&id)?.chat_state()?;
        let notice = self.orch_notice(id).then(|| NOTICE.to_owned());
        // The picker belongs to the alert: without one it is not shown.
        let alert = chat.orch_alert().map(|alert| match chat.orch_picker() {
            Some(picker) => picker.view(),
            None => alert_text(chat, alert),
        });
        match (notice, alert) {
            (Some(notice), Some(alert)) => Some(format!("{notice} | {alert}")),
            (notice, alert) => notice
                .or(alert)
                .or_else(|| self.gates_line(id))
                .or_else(|| self.worker_keys(chat))
                .or_else(|| self.run_hint(id)),
        }
    }

    /// Whether `Enter gates off` can act in pane `id` now: a refusal is
    /// offered, the pane is orchestrating, the composer is empty and nothing
    /// that answers Enter first (D24) is on screen. The provider notice needs
    /// no check here: it is answered before this key, and `orch_guidance`
    /// names it first.
    fn gates_ready(&self, id: PaneId) -> bool {
        let Some(chat) = self.panes.get(&id).and_then(PaneState::chat_state) else {
            return false;
        };
        chat.gate_offer().is_some()
            && self.orchestrating(id)
            && self.orch_run_active(id)
            && chat.composer().text().is_empty()
            && chat.orch_alert().is_none()
            && self.server_offer_prompt(id).is_none()
    }

    /// The line that names the override key, only while it can act (D24).
    fn gates_line(&self, id: PaneId) -> Option<String> {
        let cols = usize::from(self.panes.get(&id)?.size.cols);
        let offer = self.panes.get(&id)?.chat_state()?.gate_offer()?;
        self.gates_ready(id)
            .then(|| gates_hint(cols, offer.gate, offer.step))
    }

    /// Enter on the gates line (W3, D24): only a fresh, plain press on an
    /// empty composer of an orchestrating pane, and also while a turn runs:
    /// the lift targets the live run, and the runtime says what it did.
    /// `None` leaves the key to the chat.
    pub(super) fn answer_gates_off(
        &mut self,
        id: PaneId,
        key: &KeyEvent,
        repeat: bool,
    ) -> Option<Vec<Effect>> {
        if key.code != KeyCode::Enter
            || repeat
            || key.kind != KeyEventKind::Press
            || key.modifiers != KeyModifiers::NONE
            || !self.gates_ready(id)
        {
            return None;
        }
        let chat = self.panes.get_mut(&id).and_then(PaneState::chat_mut)?;
        let offer = chat.take_gate_offer()?;
        self.dirty = true;
        Some(vec![Effect::OrchGatesOff {
            pane: id,
            run: offer.run,
        }])
    }

    /// The cancel keys, while a run has a worker that has not ended and
    /// nothing else needs the line.
    fn worker_keys(&self, chat: &ChatState) -> Option<String> {
        if !self.orch_gate() {
            return None;
        }
        let live = chat.live_workers();
        if live.is_empty() {
            return None;
        }
        Some(match chat.selected_worker() {
            Some(worker) => format!(
                "worker {} \u{b7} Ctrl+Space W transcript \u{b7} Ctrl+C cancels it \u{b7} Alt+Up/Down change \u{b7} Ctrl+C twice cancels the run",
                worker.0
            ),
            None => "Alt+Up/Down select a worker \u{b7} Ctrl+C cancels the run".to_owned(),
        })
    }

    /// The hint of a pane whose run is active: the mode cannot change.
    fn run_hint(&self, id: PaneId) -> Option<String> {
        (self.orch_hint(id) == Some(OrchHint::CancelRunFirst)).then(|| HINT_RUN.to_owned())
    }

    /// The footer hint of an idle, ready pane while the gate is on: what
    /// Ctrl+O does. `None` with the gate off.
    pub(super) fn toggle_hint(&self, id: PaneId) -> Option<String> {
        (self.orch_hint(id)? == OrchHint::Toggle).then(|| {
            let on =
                (self.panes.get(&id).and_then(PaneState::chat_state)).is_some_and(ChatState::orch);
            (if on { HINT_OFF } else { HINT_ON }).to_owned()
        })
    }

    /// Whether what pane `id` shows above its composer is only the quiet
    /// Ctrl+O hint: it names a key, it is not a problem to fix, so it is
    /// drawn muted and does not stand in for the transcript.
    pub fn guidance_is_hint(&self, id: PaneId) -> bool {
        let shown = self.chat_guidance(id);
        [
            self.delegate_line(id),
            self.github_hint(id),
            self.toggle_hint(id),
        ]
        .into_iter()
        .flatten()
        .any(|hint| shown.as_deref() == Some(hint.as_str()))
    }

    /// Whether the provider notice is shown in pane `id`.
    pub fn orch_notice(&self, id: PaneId) -> bool {
        self.orch_notice == Some(id)
    }

    /// A key on chat `id` for the provider notice shown there: Enter accepts
    /// it (written at once; `y` is typed, user decision 2026-10-29), Esc dismisses it (it shows again at the
    /// next run). Only a fresh, unmodified press with an empty composer
    /// answers it; `None` leaves every other key to the chat.
    pub(super) fn answer_orch_notice(
        &mut self,
        id: PaneId,
        key: &KeyEvent,
        repeat: bool,
    ) -> Option<Vec<Effect>> {
        let fresh = !repeat && key.kind == KeyEventKind::Press;
        if !self.orch_notice(id) || !fresh || key.modifiers != KeyModifiers::NONE {
            return None;
        }
        let chat = self.panes.get(&id).and_then(PaneState::chat_state)?;
        if !chat.composer().text().is_empty() {
            return None;
        }
        let accepted = match key.code {
            KeyCode::Enter => true,
            KeyCode::Esc => false,
            _ => return None,
        };
        self.orch_notice = None;
        self.dirty = true;
        if !accepted {
            return Some(Vec::new());
        }
        self.ai.orchestrator.acknowledged = true;
        self.orch_saves += 1;
        let edit = ConfigEdit::orchestrator_flag("acknowledged", true);
        Some(vec![Effect::SaveConfig(vec![edit])])
    }

    /// The result of the write of an accepted notice. A failed one persisted
    /// nothing: the notice is not accepted, so the next run shows it again,
    /// and the error is told.
    pub(super) fn on_notice_saved(&mut self, failed: Option<String>) -> Vec<Effect> {
        self.orch_saves = self.orch_saves.saturating_sub(1);
        if let Some(message) = failed {
            self.ai.orchestrator.acknowledged = false;
            self.notice = Some(message);
            self.dirty = true;
        }
        Vec::new()
    }

    /// The run limits of the configuration.
    fn run_limits(&self) -> RunLimits {
        let orch = &self.ai.orchestrator;
        RunLimits {
            parallel: usize::from(orch.max_parallel),
            workers: usize::from(orch.max_workers),
            wall: Duration::from_secs(u64::from(orch.run_timeout_mins) * 60),
            turns: usize::from(orch.max_turns),
        }
    }

    /// The ORCH hint of the chat footer of pane `id`; `None` with the gate
    /// off or on a pane that is not a chat.
    pub fn orch_hint(&self, id: PaneId) -> Option<OrchHint> {
        self.panes.get(&id)?.chat_state()?;
        if !self.orch_gate() {
            return None;
        }
        let waits = self
            .panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(ChatState::only_waits);
        Some(if self.orch_run_active(id) && !waits {
            OrchHint::CancelRunFirst
        } else {
            OrchHint::Toggle
        })
    }

    /// Ctrl+O on chat `id`: flips its ORCH mode. With the gate off it does
    /// nothing, like any unbound key, and while a run is active the mode
    /// stays (the hint says to cancel the run first).
    pub(super) fn chat_toggle_orch(&mut self, id: PaneId) -> Vec<Effect> {
        if !self.orch_gate() {
            return Vec::new();
        }
        // A run that only waits for the reply gives way: the toggle ends it.
        let mut effects = Vec::new();
        let waiting = self
            .panes
            .get(&id)
            .and_then(PaneState::chat_state)
            .is_some_and(ChatState::only_waits);
        if waiting {
            if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
                if let Some(run) = chat.waiting_run() {
                    chat.cancel_run(run);
                }
                chat.set_waiting_run(None);
                chat.push_system_line(
                    "run no longer waits for your reply: ORCH was toggled off".to_owned(),
                );
            }
            effects.push(Effect::ChatCancel(id));
        } else if self.orch_run_active(id) {
            self.notice = Some(HINT_RUN.to_owned());
            self.dirty = true;
            return Vec::new();
        }
        let mut now_on = false;
        let tools_off = !self.ai.tools_enabled();
        let write_off = !self
            .orch_ceiling(id)
            .contains(crate::core::ai::tools::registry::Family::Write);
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.set_orch(!chat.orch());
            now_on = chat.orch();
            if now_on {
                chat.push_system_line(ORCH_ON_HEAD.to_owned());
                chat.push_system_line(ORCH_ON_BODY.to_owned());
                if write_off {
                    let line = if tools_off {
                        ORCH_ON_TOOLS_WRITE
                    } else {
                        ORCH_ON_WRITE
                    };
                    chat.push_system_line(line.to_owned());
                }
            }
            // The grant stays, but an orchestrated turn never offers it.
            if now_on && chat.github_granted() {
                chat.push_system_line(github::ORCH_KEEPS_GRANT.to_owned());
            }
            self.dirty = true;
        }
        // A reply to a resumed run is an ORCH turn: toggling gives it back.
        self.release_wait(id, false);
        // The first ORCH of a start asks which runs a restart interrupted.
        if now_on {
            effects.extend(self.recover_runs(id));
        }
        effects
    }

    /// The engine a turn of pane `id` goes to: the effective `orchestrate`
    /// assignment (own, else `general`) in an orchestrating pane, never
    /// replaced by another; the `chat` one everywhere else.
    pub(super) fn turn_availability(&self, id: PaneId) -> Availability {
        if self.orchestrating(id) {
            availability_for(
                Task::Orchestrate,
                &self.ai,
                self.engines.as_ref(),
                self.jail,
            )
        } else {
            self.chat_availability()
        }
    }

    /// Why an orchestrator turn of pane `id` on a ready engine cannot go: it
    /// could not offer the orchestration tools. `None` for any other pane.
    pub(super) fn orch_refusal(&self, id: PaneId) -> Option<String> {
        if !self.orchestrating(id) {
            return None;
        }
        let Availability::Ready { engine, .. } = self.turn_availability(id) else {
            return None;
        };
        if !self.ai.tools_enabled() {
            return Some(TOOLS_OFF.to_owned());
        }
        (tools_for(&engine, true, None, self.tool_engines) == Tools::Off).then(|| {
            format!(
                "{engine} cannot use tools in lazarobox yet, so it cannot orchestrate: \
                 assign another engine to orchestrate (Ctrl+Space m), or press Ctrl+O to chat"
            )
        })
    }

    /// The effect that starts turn `turn` of pane `id`: an orchestrator turn
    /// with the roster in an orchestrating pane, a chat turn as before
    /// everywhere else.
    pub(super) fn turn_effect(
        &mut self,
        id: PaneId,
        turn: u64,
        mut request: ChatRequest,
        attach: Option<RunId>,
    ) -> Effect {
        if !self.orchestrating(id) {
            // A plain send is not the reply a resumed run waits for.
            self.release_wait(id, false);
            return Effect::ChatSend {
                pane: id,
                turn,
                request,
            };
        }
        // The orchestrator session: the read tools, the pane reads and the
        // orchestration tools, never `run_in_pane`.
        request.families = Toolset::Orchestrator.families();
        // A turn sent while a resumed run waits is the user's reply.
        self.reply_started(id);
        // This turn replaces the run: it starts from the menu values, not from
        // what the user picked in the old run's alert. Those overrides stay
        // with the old run, which lives on if this one cannot start; the new
        // run's first event ends them.
        let roster = self.roster(&self.ai);
        let deadline = Duration::from_secs(u64::from(self.ai.orchestrator.worker_timeout_secs));
        let limits = self.run_limits();
        // The goal is what the user typed; a continuation keeps the run's.
        // A resumed run's goal is the stored one, not the seed that carries it.
        let goal = match attach {
            Some(_) => String::new(),
            None => self.run_keep.resume_goal.take().unwrap_or_else(|| {
                (request.messages.last())
                    .map(|m| m.content.clone())
                    .unwrap_or_default()
            }),
        };
        if attach.is_none() {
            self.keep_goal(id, &goal);
        }
        let chat = self.panes.get_mut(&id).and_then(PaneState::chat_mut);
        let profile = chat
            .as_ref()
            .map_or_else(|| self.ai.profile(), |c| c.profile());
        if let Some(chat) = chat {
            chat.set_orch_turn(turn);
        }
        // The run starts now, whatever the notice's answer (it never blocks).
        if !self.ai.orchestrator.acknowledged {
            self.orch_notice = Some(id);
            self.dirty = true;
        }
        Effect::OrchestrateSend {
            pane: id,
            turn,
            request,
            orch: Box::new(OrchSend {
                roster,
                profile,
                deadline,
                limits,
                // The menu as it is now: a toggle changed since the last turn
                // reaches the next spawn, never a running worker.
                ceiling: self.orch_ceiling(id),
                goal,
                attach,
            }),
        }
    }
}

/// Said of a worker turn whose tools could not be offered, before the reason.
const RAN_WITHOUT_TOOLS: &str = "runs as plain text, without tools";

/// Why a worker turn has no tools, in lazarobox's words (ASCII, neutral).
fn degrade_reason(why: Degrade) -> &'static str {
    match why {
        Degrade::NoTooling => "the tool host is not available",
        Degrade::ToolsOff => "its engine is not verified for tools here",
        Degrade::NoLocality => "its engine is unknown to lazarobox",
        Degrade::NoHub => "the tool server could not start",
    }
}

/// The one shape of a notice about a worker: `worker 2/3 code ollama>qwen3`
/// (its number of the workers the run has made) and what is said of it.
fn worker_notice(worker: WorkerId, workers: usize, who: &str, said: &str) -> String {
    ascii(format!("worker {}/{workers} {who} {said}", worker.0))
}

/// `text` with every non-ASCII character shown as `?`: a model or engine name
/// can carry any, and these notices are ASCII.
fn ascii(text: String) -> String {
    if text.is_ascii() {
        return text;
    }
    text.chars()
        .map(|c| if c.is_ascii() { c } else { '?' })
        .collect()
}

/// A disclosure as one transcript line: the worker and what its end left
/// unsaid.
fn disclosure_line(worker: WorkerId, workers: usize, who: &str, what: Disclosure) -> String {
    let said = match what {
        Disclosure::EmptyLedger => CHANGED_NO_FILE.to_owned(),
        Disclosure::ToolCallAsText => format!("{CHANGED_NO_FILE}; {TOOL_CALLS_AS_TEXT}"),
        Disclosure::StagedNotProposed { files } => {
            let noun = if files == 1 { "file" } else { "files" };
            format!("staged {files} {noun} {NEVER_PROPOSED}")
        }
        Disclosure::NeedsSpecification => format!(
            "needs specification: brief {} needs an answer from the coordinator or you",
            worker.0
        ),
    };
    worker_notice(worker, workers, who, &said)
}

/// The failure alert as one paragraph: the run is paused, the worker by its
/// row's label and how it ended, the error as it came (a control character,
/// a line break included, drawn as a space) and the keys.
fn alert_text(chat: &ChatState, alert: &OrchAlert) -> String {
    let row = chat.entries().iter().find_map(|entry| match entry {
        Entry::Worker(row) if (row.run, row.id) == (alert.run, alert.worker) => Some(row),
        _ => None,
    });
    let (worker, ended) = row.map_or_else(
        || (format!("{}", alert.worker.0), "failed"),
        |row| (worker_label(row), row.status.label()),
    );
    let text: String = alert
        .text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    // `other` says nothing more than the error does.
    let class = match alert.class {
        ErrorClass::Other => String::new(),
        class => format!(" ({})", class.id()),
    };
    let mut line = format!("run paused: worker {worker} {ended}{class}: {text}");
    let retry = match chat.orch_note() {
        Some(OrchNote::RetryFailed(reason)) => {
            line.push_str(&format!(" \u{b7} retry failed: {reason}"));
            RETRY_AGAIN
        }
        None => RETRY_KEY,
    };
    format!("{line} \u{b7} {retry} \u{b7} {ALERT_KEYS}")
}
