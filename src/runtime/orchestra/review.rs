//! The runtime of a receipt-driven review (agent-orchestrator S7b, D16): the
//! lens workers, the refuter and the one correction, all ordinary workers of
//! the run, so the S3 limits (`max_parallel`, `max_workers`, the wall clock),
//! the routes of S4b and the worker rows apply to them as to any other.
//!
//! The worker whose change is under review is waiting in `propose_batch`: it
//! is parked, so its slot goes to the review workers. A lens and the refuter
//! are read-only workers that see the staged view; the correction is a `code`
//! worker that stages its fixes in the same batch and may not propose it.
//! Nothing here approves a batch: the user's batch review still decides.
//!
//! Seams for 7b.3: [`RunManager::reviews`] holds each finished [`Review`]
//! with its outcome, from which the receipt handoff is built.
//!
//! Every worker a review starts ends when the review does: a [`Guard`]
//! cancels what is still queued or running when the review ends or is
//! dropped (the proposer's deadline, an aborted turn), and gives the waiting
//! worker its slot back. A review cannot hold the proposer for ever: a run
//! that halts, a proposer that is cancelled or ended, or a run that is gone
//! ends it, said in the transcript, and the batch goes on to the user.
//!
//! Limits, on purpose: the lenses are a default pair (`risk`, `reliability`),
//! because the orchestrator has no tool to pick them yet (`Review::pick`
//! accepts any); the review workers are visible to the orchestrator like any
//! worker (a handoff is asked of them, or filled in `unknown`); the review
//! time counts against the waiting worker's own deadline.

use std::collections::{HashMap, HashSet};

use super::*;
use crate::core::ai::{
    chat::prompts::{lens_builtin, refuter_builtin, validator_builtin},
    orchestra::{
        rdd::{
            Consent, Finding, Lens, Outcome as Ended, Refutation, Review, ReviewError,
            correction_prompt, lens_prompt, parse_verdicts, refuter_prompt, validation_prompt,
        },
        review::{Assessment, Raise, Tier, assess},
    },
};

/// The lenses every review runs until the orchestrator can pick them.
const DEFAULT_LENSES: [Lens; 2] = [Lens::Risk, Lens::Reliability];

/// Why the default lenses are picked.
const DEFAULT_WHY: &str = "default lens for a high-risk change";

/// A finished review, kept for the receipt (7b.3).
#[derive(Clone, Debug)]
pub struct Reviewed {
    /// The code worker whose change was reviewed.
    pub worker: WorkerId,
    pub review: Review,
    /// `None` when the review stopped before it could conclude.
    pub outcome: Option<Ended>,
    /// Paths staged after the set that was assessed or consented to: they
    /// were not reviewed.
    pub unreviewed: Vec<String>,
    /// Nobody answered the consent (cancelled, expired), and why: not a decline.
    pub unanswered: Option<String>,
    /// Staged paths a review worker could not read (outside the run's scope).
    pub unread: Vec<String>,
}

impl Reviewed {
    fn new(worker: WorkerId, review: Review, outcome: Option<Ended>) -> Self {
        Self {
            worker,
            review,
            outcome,
            unreviewed: Vec::new(),
            unanswered: None,
            unread: Vec::new(),
        }
    }
}

/// What a run keeps for its reviews.
#[derive(Default)]
pub(super) struct State {
    /// The highest raise of the orchestrator for this run.
    pub(super) raise: Option<Raise>,
    pub(super) reviews: Vec<Reviewed>,
    /// The system prompt of a review worker, instead of its kind's.
    pub(super) systems: HashMap<WorkerId, &'static str>,
    /// Review workers: read-only, with the pane's batch to look at.
    pub(super) viewers: HashSet<WorkerId>,
    /// The correction worker of a review: it may not propose the batch. Kept
    /// for the life of the run, so a retry or a relaunch of it is no
    /// proposer and no second correction.
    pub(super) correcting: HashSet<WorkerId>,
    /// What review workers could not read, by worker.
    pub(super) unread: HashMap<WorkerId, Vec<String>>,
}

impl State {
    pub(super) fn system(&self, id: WorkerId) -> Option<&'static str> {
        self.systems.get(&id).copied()
    }

    pub(super) fn views(&self, id: WorkerId) -> bool {
        self.viewers.contains(&id)
    }

    pub(super) fn corrects(&self, id: WorkerId) -> bool {
        self.correcting.contains(&id)
    }
}

pub(super) fn reviews(manager: &RunManager, pane: PaneId, run: RunId) -> Vec<Reviewed> {
    let kept = manager.on_run(pane, run, |live| live.review.reviews.clone());
    kept.unwrap_or_default()
}

/// What kind of review worker to start.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Reader,
    Correction,
}

/// Why a review stopped before it concluded.
enum Stop {
    /// No worker could start.
    Start(String),
    /// The run halted (a lens failed): the review is incomplete.
    Halted(String),
    /// Anything else, said as it is.
    Other(String),
}

/// How the wait for review workers ended.
enum Waited {
    Texts(Vec<Option<String>>),
    Halted(String),
    /// The waiting worker is no longer running.
    ProposerOver,
    Gone,
}

/// The workers a review started: when it ends, or is dropped, whatever of
/// them still runs is cancelled and the waiting worker gets its slot back.
struct Guard {
    manager: Arc<RunManager>,
    pane: PaneId,
    run: RunId,
    worker: WorkerId,
    spawned: Vec<WorkerId>,
    armed: bool,
}

impl Guard {
    fn release(&mut self) {
        if std::mem::take(&mut self.armed) {
            self.manager
                .release(self.pane, self.run, self.worker, &self.spawned);
        }
    }
}

impl Drop for Guard {
    /// A review dropped midway. The drop may run while the manager's lock is
    /// held (an abort inside it), so the release is deferred to a task.
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let (manager, pane, run, worker) = (self.manager.clone(), self.pane, self.run, self.worker);
        let spawned = std::mem::take(&mut self.spawned);
        match tokio::runtime::Handle::try_current() {
            Ok(runtime) => {
                runtime.spawn(async move { manager.release(pane, run, worker, &spawned) });
            }
            // Nothing can cancel them here: say so, never fail silently.
            Err(_) => manager.say(
                pane,
                run,
                format!(
                    "review of worker {}: its workers could not be cancelled (no runtime)",
                    worker.0
                ),
            ),
        }
    }
}

/// The paths of `changes` as one short line.
fn paths_line(changes: &[Change]) -> String {
    let mut shown: Vec<String> = changes.iter().take(5).map(|c| c.path.clone()).collect();
    if changes.len() > 5 {
        shown.push(format!("+{} more", changes.len() - 5));
    }
    shown.join(", ")
}

impl RunManager {
    /// The orchestrator's `raise`: kept if it is above what it raised before.
    pub(super) fn raise(&self, pane: PaneId, run: RunId, tier: &str, reason: &str) -> Outcome {
        let raised = match Raise::new(tier, reason) {
            Ok(raised) => raised,
            Err(why) => return error(why.to_string()),
        };
        let kept = self.on_run(pane, run, |live| {
            let keep = (live.review.raise.as_ref()).is_some_and(|old| old.tier() >= raised.tier());
            if !keep {
                live.review.raise = Some(raised);
            }
            live.review.raise.as_ref().map(|now| now.tier().as_str())
        });
        match kept.flatten() {
            Some(tier) => fine(format!(
                "staged changes of this run are reviewed at least at {tier}"
            )),
            None => error(ENDED),
        }
    }

    /// The tier of `changes` with the run's raise. A run that is gone cannot
    /// say what was raised, so the answer is `High`, not the rules alone.
    pub(super) fn assessment(&self, pane: PaneId, run: RunId, changes: &[Change]) -> Assessment {
        match self.on_run(pane, run, |live| live.review.raise.clone()) {
            Some(raise) => assess(changes, raise.as_ref()),
            None => gone(),
        }
    }

    pub(super) fn is_correction(&self, pane: PaneId, run: RunId, worker: WorkerId) -> bool {
        let found = self.on_run(pane, run, |live| live.review.corrects(worker));
        found.unwrap_or(false)
    }

    /// A review worker could not read `path`: its review cannot be clean.
    pub(super) fn unread(&self, pane: PaneId, run: RunId, worker: WorkerId, path: &str) {
        let path: String = path.chars().filter(|c| !c.is_control()).take(120).collect();
        self.on_run(pane, run, |live| {
            let seen = live.review.unread.entry(worker).or_default();
            if seen.len() < 8 {
                seen.push(path);
            }
        });
    }

    /// Paths staged after the set that was assessed or consented to.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn late(
        &self,
        pane: PaneId,
        run: RunId,
        worker: WorkerId,
        changes: Vec<Change>,
        assessment: Assessment,
        reviewed: bool,
        kind: Late,
    ) {
        let after = if reviewed {
            "consent"
        } else {
            "the assessment"
        };
        let what = match kind {
            Late::Staged => format!("staged after {after}"),
            Late::Changed => "changed after review".to_owned(),
        };
        let line = format!(
            "review of worker {}: {what}, not reviewed: {}",
            worker.0,
            paths_line(&changes)
        );
        let paths: Vec<String> = changes.iter().map(|c| c.path.clone()).collect();
        self.on_run(pane, run, |live| {
            let kept = live.review.reviews.iter_mut().rev();
            match kept.into_iter().find(|kept| kept.worker == worker) {
                Some(kept) => kept.unreviewed.extend(paths),
                None => {
                    let mut kept = Reviewed::new(worker, Review::new(assessment), None);
                    kept.unreviewed = paths;
                    live.review.reviews.push(kept);
                }
            }
        });
        self.say(pane, run, line);
    }

    fn say(&self, pane: PaneId, run: RunId, text: String) {
        self.send(pane, OrchestraEvent::Review { run, text });
    }

    /// Ends the workers of a review and unparks its proposer.
    fn release(self: &Arc<Self>, pane: PaneId, run: RunId, worker: WorkerId, spawned: &[WorkerId]) {
        for id in spawned {
            // One that already ended answers "already ended".
            self.cancel_worker(pane, run, *id);
        }
        self.on_run(pane, run, |live| {
            live.run.unpark(worker);
            for id in spawned {
                live.review.viewers.remove(id);
                live.review.systems.remove(id);
                live.review.unread.remove(id);
            }
        });
    }

    /// Reviews `changes` of `worker`'s `propose_batch`, whose consent was
    /// answered `answered`. Returns when it ended, whatever it found; the
    /// result is kept for the receipt and said in the transcript. `Err` is a
    /// review that could not even begin (its run is gone).
    pub(super) async fn review(
        self: Arc<Self>,
        pane: PaneId,
        run: RunId,
        worker: WorkerId,
        changes: Vec<Change>,
        assessment: Assessment,
        answered: Answered,
    ) -> Result<Vec<Change>, String> {
        let of = format!("review of worker {}", worker.0);
        let mut review = Review::new(assessment);
        let unanswered = match &answered {
            Answered::Granted => None,
            Answered::Declined => {
                let _ = review.consent(Consent::Declined);
                self.say(
                    pane,
                    run,
                    format!(
                        "{of}: the change was not reviewed (declined); your batch review follows"
                    ),
                );
                self.keep(
                    pane,
                    run,
                    Reviewed::new(worker, review, Some(Ended::NotReviewed)),
                );
                return Ok(changes);
            }
            Answered::Unanswered(why) => Some(why.clone()),
        };
        if let Some(why) = unanswered {
            let _ = review.consent(Consent::Declined);
            self.say(
                pane,
                run,
                format!(
                    "{of}: consent not answered ({why}); the change was not reviewed, your batch review follows"
                ),
            );
            let mut kept = Reviewed::new(worker, review, Some(Ended::NotReviewed));
            kept.unanswered = Some(why);
            self.keep(pane, run, kept);
            return Ok(changes);
        }
        let _ = review.consent(Consent::Granted);
        let mut guard = Guard {
            manager: self.clone(),
            pane,
            run,
            worker,
            spawned: Vec::new(),
            armed: true,
        };
        let parked = self.on_run(pane, run, |live| {
            let parked = live.run.park(now(), worker);
            self.sync(pane, live);
            parked
        });
        let parked = parked.ok_or_else(|| ENDED.to_owned())?;
        // The set as last reviewed: a corrected and checked file moves to its
        // post-correction hash.
        let mut reviewed = changes.clone();
        let stopped = if parked {
            self.conduct(
                pane,
                run,
                worker,
                &changes,
                &mut reviewed,
                &mut review,
                &mut guard,
            )
            .await
            .err()
        } else {
            Some(Stop::Start(format!("worker {} is not running", worker.0)))
        };
        let unread = self.take_unread(pane, run, &guard.spawned);
        guard.release();
        let (outcome, line) = match stopped {
            Some(Stop::Start(why)) => (
                None,
                format!(
                    "{of} could not start: {why}; the change goes to your batch review unreviewed"
                ),
            ),
            Some(Stop::Halted(why)) => (
                Some(Ended::Incomplete),
                format!("{of} is incomplete: {why}; your batch review follows"),
            ),
            Some(Stop::Other(why)) => (
                None,
                format!("{of} stopped: {why}; the change goes to your batch review unreviewed"),
            ),
            None => self.verdict(&of, &review, &unread),
        };
        self.say(pane, run, line);
        let mut kept = Reviewed::new(worker, review, outcome);
        kept.unread = unread;
        self.keep(pane, run, kept);
        Ok(reviewed)
    }

    /// What the review concluded and the line that says it. A staged file a
    /// reader could not read keeps a clean review from being clean.
    fn verdict(&self, of: &str, review: &Review, unread: &[String]) -> (Option<Ended>, String) {
        let outcome = review.conclude().ok();
        let outcome = match outcome {
            Some(Ended::Clean | Ended::Corrected) if !unread.is_empty() => Some(Ended::Incomplete),
            other => other,
        };
        let note = if unread.is_empty() {
            String::new()
        } else {
            format!(" ({} staged file(s) could not be read)", unread.len())
        };
        let line = match outcome {
            Some(Ended::Clean) => format!("{of}: clean, no blockers"),
            Some(Ended::Corrected) => {
                format!("{of}: corrected, one correction cleared the blockers")
            }
            Some(Ended::Incomplete) => format!(
                "{of}: incomplete, findings were omitted or malformed, claims went unanswered or files could not be read{note}"
            ),
            Some(Ended::BlockersOpen) => {
                let open = review.open();
                let first = open
                    .first()
                    .and_then(|&at| review.findings().get(at))
                    .map_or("", |found| found.text.as_str());
                // The text is a lens worker's, clipped; the receipt (7b.3)
                // names every open blocker whole.
                let first: String = first.chars().take(120).collect();
                format!(
                    "{of}: {} blocker(s) open for your decision: {first}",
                    open.len()
                )
            }
            Some(Ended::NotReviewed) | None => format!("{of}: the change was not reviewed"),
        };
        (outcome, line)
    }

    fn take_unread(&self, pane: PaneId, run: RunId, ids: &[WorkerId]) -> Vec<String> {
        let taken = self.on_run(pane, run, |live| {
            let mut all = Vec::new();
            for id in ids {
                all.extend(live.review.unread.remove(id).unwrap_or_default());
            }
            all
        });
        taken.unwrap_or_default()
    }

    fn keep(&self, pane: PaneId, run: RunId, kept: Reviewed) {
        self.on_run(pane, run, |live| live.review.reviews.push(kept));
    }

    /// The lenses, the refuter and the one correction, in that order. A
    /// `Stop` is why the review ended early; what was learned stays in
    /// `review`.
    #[allow(clippy::too_many_arguments)]
    async fn conduct(
        self: &Arc<Self>,
        pane: PaneId,
        run: RunId,
        worker: WorkerId,
        changes: &[Change],
        reviewed: &mut [Change],
        review: &mut Review,
        guard: &mut Guard,
    ) -> Result<(), Stop> {
        let other = |why: String| Stop::Other(why);
        let wrap = || {
            (self.nonce)().ok_or_else(|| Stop::Start("a prompt cannot be wrapped now".to_owned()))
        };
        let (profile, room) = self
            .on_run(pane, run, |live| {
                let limits = live.run.limits();
                let room = limits.workers.saturating_sub(live.run.worker_count());
                (live.profile, (room, limits.workers))
            })
            .ok_or_else(|| Stop::Start(ENDED.to_owned()))?;
        // Two lenses and the refuter: all or none, so no lens is left
        // running while the next cannot start.
        let need = DEFAULT_LENSES.len() + 1;
        if room.0 < need {
            return Err(Stop::Start(format!(
                "max_workers is {} and leaves room for {} more, a review needs {need}",
                room.1, room.0
            )));
        }
        let mut lenses = Vec::new();
        for lens in DEFAULT_LENSES {
            review
                .pick(lens.id(), DEFAULT_WHY)
                .map_err(|e| other(refused(e)))?;
            let prompt = lens_prompt(wrap()?, lens, DEFAULT_WHY, changes);
            let system = Some(lens_builtin(profile, lens));
            let id = self
                .start_review(pane, run, Role::Reader, prompt, system)
                .map_err(Stop::Start)?;
            guard.spawned.push(id);
            lenses.push((lens, id));
        }
        let ids: Vec<_> = lenses.iter().map(|(_, id)| *id).collect();
        let texts = self.finished(pane, run, worker, &ids).await?;
        for ((lens, id), text) in lenses.iter().zip(texts) {
            let text = text.ok_or_else(|| {
                other(format!(
                    "lens {} (worker {}) did not finish",
                    lens.id(),
                    id.0
                ))
            })?;
            review.report(*lens, &text).map_err(|e| other(refused(e)))?;
        }
        let asked = review.to_refute();
        if !asked.is_empty() {
            let text = self
                .refute(
                    pane,
                    run,
                    worker,
                    guard,
                    changes,
                    review,
                    &asked,
                    Check::Refute,
                )
                .await?;
            let verdicts = parse_verdicts(&text.unwrap_or_default(), asked.len());
            for (at, answer) in asked.iter().zip(verdicts.answers) {
                if let Some(answer) = answer {
                    review.refute(*at, answer).map_err(|e| other(refused(e)))?;
                }
            }
        }
        review.settle_unanswered().map_err(|e| other(refused(e)))?;
        let targets = match review.begin_correction() {
            Ok(targets) => targets,
            Err(ReviewError::NoBlockers) => return Ok(()),
            Err(e) => return Err(other(refused(e))),
        };
        let open = self
            .correct(
                pane, run, worker, guard, changes, reviewed, review, &targets,
            )
            .await?;
        review
            .finish_correction(&open)
            .map_err(|e| other(refused(e)))?;
        Ok(())
    }

    /// Starts the refuter (or the correction check) over `asked` and waits
    /// for its text.
    #[allow(clippy::too_many_arguments)]
    async fn refute(
        self: &Arc<Self>,
        pane: PaneId,
        run: RunId,
        worker: WorkerId,
        guard: &mut Guard,
        changes: &[Change],
        review: &Review,
        asked: &[usize],
        check: Check,
    ) -> Result<Option<String>, Stop> {
        let nonce = (self.nonce)()
            .ok_or_else(|| Stop::Start("a prompt cannot be wrapped now".to_owned()))?;
        let profile = self
            .on_run(pane, run, |live| live.profile)
            .ok_or(Stop::Start(ENDED.to_owned()))?;
        let claims: Vec<&Finding> = asked
            .iter()
            .filter_map(|&at| review.findings().get(at))
            .collect();
        let (prompt, system) = match check {
            Check::Refute => (
                refuter_prompt(nonce, changes, &claims),
                refuter_builtin(profile),
            ),
            Check::Validate => (
                validation_prompt(nonce, changes, &claims),
                validator_builtin(profile),
            ),
        };
        let id = self
            .start_review(pane, run, Role::Reader, prompt, Some(system))
            .map_err(Stop::Start)?;
        guard.spawned.push(id);
        let texts = self.finished(pane, run, worker, &[id]).await?;
        Ok(texts.into_iter().next().flatten())
    }

    /// The one correction and its check. Returns the targets that are still
    /// open: all of them when the correction staged nothing new, or when it
    /// or its check could not run or could not settle a claim.
    #[allow(clippy::too_many_arguments)]
    async fn correct(
        self: &Arc<Self>,
        pane: PaneId,
        run: RunId,
        worker: WorkerId,
        guard: &mut Guard,
        changes: &[Change],
        reviewed: &mut [Change],
        review: &Review,
        targets: &[usize],
    ) -> Result<Vec<usize>, Stop> {
        // The correction and its check: both or neither, so a correction is
        // never left unchecked for want of a slot.
        let room = self.on_run(pane, run, |live| {
            let workers = live.run.limits().workers;
            workers.saturating_sub(live.run.worker_count())
        });
        let room = room.ok_or_else(|| Stop::Other(ENDED.to_owned()))?;
        if room < 2 {
            self.say(
                pane,
                run,
                format!(
                    "the correction was skipped: max_workers leaves room for {room} more worker(s), a correction and its check need 2; the blockers stay open"
                ),
            );
            return Ok(targets.to_vec());
        }
        let nonce = (self.nonce)()
            .ok_or_else(|| Stop::Start("a prompt cannot be wrapped now".to_owned()))?;
        let blockers: Vec<&Finding> = targets
            .iter()
            .filter_map(|&at| review.findings().get(at))
            .collect();
        let prompt = correction_prompt(nonce, changes, &blockers);
        let started = self.start_review(pane, run, Role::Correction, prompt, None);
        let id = match started {
            Ok(id) => id,
            Err(why) => {
                self.say(pane, run, format!("the correction could not start: {why}"));
                return Ok(targets.to_vec());
            }
        };
        guard.spawned.push(id);
        let texts = self.finished(pane, run, worker, &[id]).await?;
        if texts.into_iter().next().flatten().is_none() {
            self.say(pane, run, "the correction did not finish".to_owned());
            return Ok(targets.to_vec());
        }
        // The check reads what is staged now, not the listing of before; a
        // correction that staged nothing new fixed nothing.
        let Some(fresh) = self.restaged(pane, run, changes).await else {
            self.say(
                pane,
                run,
                "the staged changes could not be re-read: the correction is not checked".to_owned(),
            );
            return Ok(targets.to_vec());
        };
        // The reviewed files themselves must have changed: a new unrelated
        // path fixes nothing.
        let moved = changes.iter().any(|was| {
            fresh
                .iter()
                .any(|now| now.path == was.path && now.hash != was.hash)
        });
        if !moved {
            self.say(
                pane,
                run,
                "the correction did not change the reviewed files".to_owned(),
            );
            return Ok(targets.to_vec());
        }
        let checked = self
            .refute(
                pane,
                run,
                worker,
                guard,
                &fresh,
                review,
                targets,
                Check::Validate,
            )
            .await;
        let text = match checked {
            Ok(text) => text,
            Err(Stop::Start(why)) => {
                self.say(
                    pane,
                    run,
                    format!("the correction could not be checked: {why}; the blockers stay open"),
                );
                return Ok(targets.to_vec());
            }
            Err(other) => return Err(other),
        };
        // What the check looked at is what was last reviewed.
        for kept in reviewed.iter_mut() {
            if let Some(now) = fresh.iter().find(|now| now.path == kept.path) {
                kept.hash.clone_from(&now.hash);
            }
        }
        let verdicts = parse_verdicts(&text.unwrap_or_default(), targets.len());
        // Only an explicit "refuted" clears a blocker: upheld and unsettled
        // (or no answer) leave it open.
        let open = targets.iter().zip(verdicts.answers);
        Ok(open
            .filter(|(_, answer)| *answer != Some(Refutation::Refuted))
            .map(|(at, _)| *at)
            .collect())
    }

    /// The staged change set now; `None` when it cannot be read.
    async fn restaged(&self, pane: PaneId, run: RunId, _before: &[Change]) -> Option<Vec<Change>> {
        #[cfg(test)]
        if let Some(restage) = self.restage {
            return restage(_before);
        }
        let origin = self.on_run(pane, run, |live| live.origin.clone())?;
        let tooling = self.tooling.as_ref()?;
        tooling
            .host
            .staged_changes(StageKey::Pane(pane), &origin)
            .await
    }

    /// Starts one review worker as a worker of the run.
    fn start_review(
        self: &Arc<Self>,
        pane: PaneId,
        run: RunId,
        role: Role,
        prompt: String,
        system: Option<&'static str>,
    ) -> Result<WorkerId, String> {
        let task = if role == Role::Correction {
            Task::Code
        } else {
            Task::Review
        };
        let started = self.on_run(pane, run, |live| {
            let id = live
                .run
                .spawn(now(), task, &prompt)
                .map_err(|why| match why {
                    Refusal::Paused => "the run is paused".to_owned(),
                    Refusal::Ended => ENDED.to_owned(),
                    Refusal::ReadOnly => READ_ONLY.to_owned(),
                    Refusal::WorkerCap(cap) => format!("max_workers is {cap}"),
                })?;
            if let Some(system) = system {
                live.review.systems.insert(id, system);
            }
            match role {
                Role::Reader => live.review.viewers.insert(id),
                Role::Correction => live.review.correcting.insert(id),
            };
            self.sync(pane, live);
            Ok(id)
        });
        started.unwrap_or_else(|| Err(ENDED.to_owned()))
    }

    /// The text of each worker once none of them is queued or running (`None`
    /// for one that did not end `done`). Ends early, as a `Stop`, when the
    /// run halts, the waiting worker is no longer running or the run is gone.
    async fn finished(
        &self,
        pane: PaneId,
        run: RunId,
        proposer: WorkerId,
        ids: &[WorkerId],
    ) -> Result<Vec<Option<String>>, Stop> {
        let waited = loop {
            let changed = self.changed.notified();
            let settled = self.on_run(pane, run, |live| {
                let over = |id: &WorkerId| {
                    !matches!(
                        live.run.status(*id),
                        Some(WorkerStatus::Queued | WorkerStatus::Running) | None
                    )
                };
                if live.run.status(proposer) != Some(WorkerStatus::Running) {
                    return Some(Waited::ProposerOver);
                }
                if ids.iter().all(over) {
                    let text = |id: &WorkerId| live.run.collect(*id).ok().map(str::to_owned);
                    return Some(Waited::Texts(ids.iter().map(text).collect()));
                }
                match live.run.state() {
                    RunState::Halted { worker, class, .. } => Some(Waited::Halted(format!(
                        "the run halted: worker {} failed ({})",
                        worker.0,
                        class.id()
                    ))),
                    _ => None,
                }
            });
            match settled {
                None => break Waited::Gone,
                Some(Some(waited)) => break waited,
                Some(None) => changed.await,
            }
        };
        match waited {
            Waited::Texts(texts) => Ok(texts),
            Waited::Halted(why) => Err(Stop::Halted(why)),
            Waited::ProposerOver => Err(Stop::Other(format!(
                "worker {} is no longer waiting",
                proposer.0
            ))),
            Waited::Gone => Err(Stop::Other(ENDED.to_owned())),
        }
    }
}

/// Which prompt a refuter-shaped worker gets.
#[derive(Clone, Copy)]
enum Check {
    /// The first pass over inferred blockers.
    Refute,
    /// The check of a correction.
    Validate,
}

/// The assessment of a run that cannot be asked: `High`, never the rules
/// alone, because a raise may have been lost with it.
pub(super) fn gone() -> Assessment {
    Assessment {
        tier: Tier::High,
        source: crate::core::ai::orchestra::review::Source::Failed(
            "the run is not available, so its raise is unknown".to_owned(),
        ),
    }
}

fn refused(why: ReviewError) -> String {
    format!("the review state refused a step ({why:?})")
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    // A review dropped where no runtime can cancel its workers says so.
    #[test]
    fn a_guard_dropped_without_a_runtime_says_it_could_not_cancel() {
        let (tx, mut events) = mpsc::unbounded_channel();
        let runner: Runner = Arc::new(|_, _| Box::pin(async {}));
        let manager = Arc::new(RunManager::new(runner, None, tx));
        let guard = Guard {
            manager,
            pane: PaneId::for_test(1),
            run: RunId(1),
            worker: WorkerId(1),
            spawned: vec![WorkerId(2)],
            armed: true,
        };
        drop(guard);
        let (_, event) = events.try_recv().expect("a line");
        let OrchestraEvent::Review { text, .. } = event else {
            panic!("{event:?}");
        };
        assert!(text.contains("could not be cancelled"), "{text}");
    }
}
