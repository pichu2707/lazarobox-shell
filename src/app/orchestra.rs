//! The orchestrator in a chat pane (agent-orchestrator D2, D5): the pane's
//! ORCH mode, its Ctrl+O toggle, the task a send runs on, the one-time
//! provider notice (D12), the run's events and its failure alert. State and
//! keys only; the badge, the hint, the notice and the alert are drawn by the
//! UI.

use super::*;
use crate::core::ai::{
    chat::{
        Entry, OrchAlert, OrchNote,
        engine::{Availability, OrchSend, Tools, availability_for, tools_for},
        transcript::worker_label,
    },
    config::Assignment,
    orchestra::{
        Toolset, WorkerKey,
        event::Continuation,
        ledger::{Unit, Verdict},
        overrides::{Change, FailurePicker, Picked},
        run::{Roster, RunLimits},
        worker::WorkerStatus,
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

/// The keys of the failure alert after `r retry`: `m` and `p` open the
/// detached picker, `c` or Esc cancels.
const ALERT_KEYS: &str = "m change model \u{b7} p change provider \u{b7} c/Esc cancel";
const RETRY_KEY: &str = "r retry";
/// Said when `r` is pressed again after a retry that failed.
const RETRY_AGAIN: &str = "r try again";
/// Said when the run manager is not attached: the answer was not applied.
const NO_ORCHESTRA: &str = "orchestration is not available: your answer was not applied";
/// Said when the run manager dropped an answer: its run is over.
const ANSWER_LOST: &str = "that worker already ended: your answer was not applied";
const MOVED_ON: &str = "an earlier run already moved on: your answer was not applied";

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
    fn orch_gate(&self) -> bool {
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
            event => event,
        };
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        // A worker that is over takes its waiting asks with it.
        let mut over = None;
        match event {
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
                chat.set_orch_run(Some(row.run));
                chat.keep_override();
                let menu = self.ai.resolve(row.kind).0.engine;
                row.overridden = chat.orch_overrides().is_some_and(|over| {
                    over.is_on(row.kind, &row.engine, row.model.as_deref(), menu.as_deref())
                });
                chat.worker_row(row);
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
                }
            }
            // Handled before the pane is borrowed.
            OrchestraEvent::Continue(_) => {}
            OrchestraEvent::Unavailable => {
                chat.undo_override();
                chat.push_system_line(NO_ORCHESTRA.to_owned());
            }
            // The ledger lives in the run manager; the transcript hears the
            // verdicts that need the user's eye (S4). Persisting the handoff
            // (engram, degraded mode) is the next slice's seam, right here.
            OrchestraEvent::Handoff { handoff, .. } => {
                if matches!(handoff.verdict, Verdict::OffCourse | Verdict::Blocked) {
                    let Unit::Worker(worker) = handoff.unit;
                    chat.push_system_line(format!(
                        "handoff worker {} {} {}: {}",
                        worker.0,
                        handoff.kind.id(),
                        handoff.verdict.label(),
                        handoff.reason
                    ));
                }
            }
            // Only the run it names ends: a replaced run ends after the new
            // one started.
            OrchestraEvent::Ended { run } => {
                chat.end_orch_run(run);
                if chat.orch_run() == Some(run) {
                    chat.set_orch_run(None);
                }
            }
        }
        if let Some(key) = over {
            self.withdraw_worker(id, key);
        }
        self.dirty = true;
        Vec::new()
    }

    /// The orchestrator turn of the run of pane `id` ended with work pending
    /// (S4, D8): the next turn starts when the pane is idle, which the turn's
    /// own end may not have made it yet.
    fn on_continue(&mut self, id: PaneId, next: Continuation) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        if chat.phase() != Phase::Idle {
            chat.park_continue(next);
            return Vec::new();
        }
        self.start_continuation(id, next)
    }

    /// Starts the continuation turn on the open run, after a line that says
    /// so. A turn that cannot go ends the run, said: never a silent stop.
    pub(super) fn start_continuation(&mut self, id: PaneId, next: Continuation) -> Vec<Effect> {
        let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) else {
            return Vec::new();
        };
        chat.push_system_line(format!(
            "orchestrator turn {} of {}: continuing the run from its ledger",
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
        let picker = failed.map(|row| {
            let config = chat
                .orch_overrides()
                .map_or_else(|| self.ai.clone(), |over| over.apply(&self.ai));
            let in_use = (row.engine.as_str(), row.model.as_deref());
            FailurePicker::open(row.kind, &config, in_use, self.engines.clone(), change)
        });
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
            (notice, alert) => notice.or(alert).or_else(|| self.worker_keys(chat)),
        }
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
                "worker {} selected \u{b7} Ctrl+C cancels it \u{b7} Alt+Up/Down change \u{b7} Ctrl+C twice cancels the run",
                worker.0
            ),
            None => "Alt+Up/Down select a worker \u{b7} Ctrl+C cancels the run".to_owned(),
        })
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
        Some(if self.orch_run_active(id) {
            OrchHint::CancelRunFirst
        } else {
            OrchHint::Toggle
        })
    }

    /// Ctrl+O on chat `id`: flips its ORCH mode. With the gate off it does
    /// nothing, like any unbound key, and while a run is active the mode
    /// stays (the hint says to cancel the run first).
    pub(super) fn chat_toggle_orch(&mut self, id: PaneId) -> Vec<Effect> {
        if !self.orch_gate() || self.orch_run_active(id) {
            return Vec::new();
        }
        if let Some(chat) = self.panes.get_mut(&id).and_then(PaneState::chat_mut) {
            chat.set_orch(!chat.orch());
            self.dirty = true;
        }
        Vec::new()
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
            return Effect::ChatSend {
                pane: id,
                turn,
                request,
            };
        }
        // The orchestrator session: the read tools and the four orchestration
        // tools, never `run_in_pane`.
        request.families = Toolset::Orchestrator.families();
        // This turn replaces the run: it starts from the menu values, not from
        // what the user picked in the old run's alert. Those overrides stay
        // with the old run, which lives on if this one cannot start; the new
        // run's first event ends them.
        let roster = self.roster(&self.ai);
        let deadline = Duration::from_secs(u64::from(self.ai.orchestrator.worker_timeout_secs));
        let limits = self.run_limits();
        // The goal is what the user typed; a continuation keeps the run's.
        let goal = match attach {
            Some(_) => String::new(),
            None => request
                .messages
                .last()
                .map(|m| m.content.clone())
                .unwrap_or_default(),
        };
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
                goal,
                attach,
            }),
        }
    }
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
    let mut line = format!("run paused: worker {worker} {ended}: {text}");
    let retry = match chat.orch_note() {
        Some(OrchNote::RetryFailed(reason)) => {
            line.push_str(&format!(" \u{b7} retry failed: {reason}"));
            RETRY_AGAIN
        }
        None => RETRY_KEY,
    };
    format!("{line} \u{b7} {retry} \u{b7} {ALERT_KEYS}")
}
