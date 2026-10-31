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
//! Limits, on purpose: the lenses are a default pair (`risk`, `reliability`),
//! because the orchestrator has no tool to pick them yet (`Review::pick`
//! accepts any); the review workers are visible to the orchestrator like any
//! worker (a handoff is asked of them, or filled in `unknown`); the review
//! time counts against the waiting worker's own deadline.

use std::collections::{HashMap, HashSet};

use super::*;
use crate::core::ai::{
    chat::prompts::{lens_builtin, refuter_builtin},
    orchestra::{
        rdd::{
            Consent, Finding, Lens, Outcome as Ended, Refutation, Review, ReviewError,
            correction_prompt, lens_prompt, parse_verdicts, refuter_prompt,
        },
        review::{Assessment, Raise, assess},
    },
};

/// The lenses every review runs until the orchestrator can pick them.
const DEFAULT_LENSES: [Lens; 2] = [Lens::Risk, Lens::Reliability];

/// Why the default lenses are picked.
const DEFAULT_WHY: &str = "default lens for a high-risk change";

/// What the refuter is told when it checks a correction.
const VALIDATE: &str = "These claims were reported before a correction was staged. \
                        Uphold a claim only if the staged text still has the problem.\n";

/// A finished review, kept for the receipt (7b.3).
#[derive(Clone, Debug)]
pub struct Reviewed {
    /// The code worker whose change was reviewed.
    pub worker: WorkerId,
    pub review: Review,
    /// `None` when the review stopped before it could conclude.
    pub outcome: Option<Ended>,
}

/// What a run keeps for its reviews.
#[derive(Default)]
pub(super) struct State {
    /// The highest raise of the orchestrator for this run.
    raise: Option<Raise>,
    reviews: Vec<Reviewed>,
    /// The system prompt of a review worker, instead of its kind's.
    systems: HashMap<WorkerId, &'static str>,
    /// Review workers: read-only, with the pane's batch to look at.
    viewers: HashSet<WorkerId>,
    /// The correction worker of a review: it may not propose the batch.
    correcting: HashSet<WorkerId>,
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

    /// The tier of `changes` with the run's raise.
    pub(super) fn assessment(&self, pane: PaneId, run: RunId, changes: &[Change]) -> Assessment {
        let raise = self.on_run(pane, run, |live| live.review.raise.clone());
        assess(changes, raise.flatten().as_ref())
    }

    pub(super) fn is_correction(&self, pane: PaneId, run: RunId, worker: WorkerId) -> bool {
        let found = self.on_run(pane, run, |live| live.review.corrects(worker));
        found.unwrap_or(false)
    }

    fn say(&self, pane: PaneId, run: RunId, text: String) {
        self.send(pane, OrchestraEvent::Review { run, text });
    }

    /// Reviews `changes` of `worker`'s `propose_batch`, whose consent was
    /// answered `consent`. Returns when it ended, whatever it found; the
    /// result is kept for the receipt and said in the transcript.
    pub(super) async fn review(
        self: Arc<Self>,
        pane: PaneId,
        run: RunId,
        worker: WorkerId,
        changes: Vec<Change>,
        assessment: Assessment,
        consent: Consent,
    ) {
        let mut review = Review::new(assessment);
        let _ = review.consent(consent);
        let of = format!("review of worker {}", worker.0);
        let stopped = match consent {
            Consent::Declined => {
                let outcome = review.conclude().ok();
                self.say(
                    pane,
                    run,
                    format!(
                        "{of}: the change was not reviewed (declined); your batch review follows"
                    ),
                );
                self.keep(pane, run, worker, review, outcome);
                return;
            }
            Consent::Granted => {
                self.on_run(pane, run, |live| {
                    live.run.park(now(), worker);
                    self.sync(pane, live);
                });
                let done = self.conduct(pane, run, worker, &changes, &mut review).await;
                self.on_run(pane, run, |live| live.run.unpark(worker));
                done.err()
            }
        };
        let outcome = review.conclude().ok().filter(|_| stopped.is_none());
        let line = match (&stopped, outcome) {
            (Some(why), _) => {
                format!("{of} stopped: {why}; the change goes to your batch review unreviewed")
            }
            (None, Some(Ended::Clean)) => format!("{of}: clean, no blockers"),
            (None, Some(Ended::Corrected)) => {
                format!("{of}: corrected, one correction cleared the blockers")
            }
            (None, Some(Ended::Incomplete)) => format!(
                "{of}: incomplete, findings were omitted or malformed or claims went unanswered"
            ),
            (None, Some(Ended::BlockersOpen)) => {
                let open = review.open();
                let first = open
                    .first()
                    .and_then(|&at| review.findings().get(at))
                    .map_or("", |found| found.text.as_str());
                let first: String = first.chars().take(120).collect();
                format!(
                    "{of}: {} blocker(s) open for your decision: {first}",
                    open.len()
                )
            }
            (None, _) => format!("{of}: the change was not reviewed"),
        };
        self.say(pane, run, line);
        self.keep(pane, run, worker, review, outcome);
    }

    fn keep(
        &self,
        pane: PaneId,
        run: RunId,
        worker: WorkerId,
        review: Review,
        outcome: Option<Ended>,
    ) {
        self.on_run(pane, run, |live| {
            live.review.reviews.push(Reviewed {
                worker,
                review,
                outcome,
            });
        });
    }

    /// The lenses, the refuter and the one correction, in that order. An
    /// `Err` is why the review stopped; what was learned stays in `review`.
    async fn conduct(
        self: &Arc<Self>,
        pane: PaneId,
        run: RunId,
        worker: WorkerId,
        changes: &[Change],
        review: &mut Review,
    ) -> Result<(), String> {
        let wrap = || (self.nonce)().ok_or_else(|| "a prompt cannot be wrapped now".to_owned());
        let profile = self
            .on_run(pane, run, |live| live.profile)
            .ok_or_else(|| ENDED.to_owned())?;
        let mut lenses = Vec::new();
        for lens in DEFAULT_LENSES {
            review.pick(lens.id(), DEFAULT_WHY).map_err(refused)?;
            let prompt = lens_prompt(wrap()?, lens, DEFAULT_WHY, changes);
            let system = Some(lens_builtin(profile, lens));
            let id = self.start_review(pane, run, Role::Reader, prompt, system)?;
            lenses.push((lens, id));
        }
        let ids: Vec<_> = lenses.iter().map(|(_, id)| *id).collect();
        let texts = self.finished(pane, run, &ids).await.ok_or(ENDED)?;
        for ((lens, id), text) in lenses.iter().zip(texts) {
            let text =
                text.ok_or_else(|| format!("lens {} (worker {}) did not finish", lens.id(), id.0))?;
            review.report(*lens, &text).map_err(refused)?;
        }
        let asked = review.to_refute();
        if !asked.is_empty() {
            let text = self
                .refute(pane, run, profile, changes, review, &asked, "", wrap()?)
                .await?;
            let verdicts = parse_verdicts(&text.unwrap_or_default(), asked.len());
            for (at, answer) in asked.iter().zip(verdicts.answers) {
                if let Some(answer) = answer {
                    review.refute(*at, answer).map_err(refused)?;
                }
            }
        }
        review.settle_unanswered().map_err(refused)?;
        let targets = match review.begin_correction() {
            Ok(targets) => targets,
            Err(ReviewError::NoBlockers) => return Ok(()),
            Err(other) => return Err(refused(other)),
        };
        let open = self
            .correct(
                pane,
                run,
                profile,
                changes,
                review,
                &targets,
                wrap()?,
                wrap()?,
            )
            .await;
        review.finish_correction(&open).map_err(refused)?;
        let _ = worker;
        Ok(())
    }

    /// Starts the refuter over `asked` and waits for its text.
    #[allow(clippy::too_many_arguments)]
    async fn refute(
        self: &Arc<Self>,
        pane: PaneId,
        run: RunId,
        profile: Profile,
        changes: &[Change],
        review: &Review,
        asked: &[usize],
        lead: &str,
        nonce: [u8; 8],
    ) -> Result<Option<String>, String> {
        let claims: Vec<&Finding> = asked
            .iter()
            .filter_map(|&at| review.findings().get(at))
            .collect();
        let prompt = format!("{lead}{}", refuter_prompt(nonce, changes, &claims));
        let system = Some(refuter_builtin(profile));
        let id = self.start_review(pane, run, Role::Reader, prompt, system)?;
        let texts = self.finished(pane, run, &[id]).await.ok_or(ENDED)?;
        Ok(texts.into_iter().next().flatten())
    }

    /// The one correction and its check. Returns the targets that are still
    /// open: all of them when the correction or its check could not run.
    #[allow(clippy::too_many_arguments)]
    async fn correct(
        self: &Arc<Self>,
        pane: PaneId,
        run: RunId,
        profile: Profile,
        changes: &[Change],
        review: &Review,
        targets: &[usize],
        nonce: [u8; 8],
        check_nonce: [u8; 8],
    ) -> Vec<usize> {
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
                return targets.to_vec();
            }
        };
        let texts = self.finished(pane, run, &[id]).await;
        self.on_run(pane, run, |live| live.review.correcting.remove(&id));
        if texts.and_then(|t| t.into_iter().next().flatten()).is_none() {
            self.say(pane, run, "the correction did not finish".to_owned());
            return targets.to_vec();
        }
        // The check reads what is staged now, not the listing of before.
        let fresh = self.restaged(pane, run, changes).await;
        match self
            .refute(
                pane,
                run,
                profile,
                &fresh,
                review,
                targets,
                VALIDATE,
                check_nonce,
            )
            .await
        {
            Ok(Some(text)) => {
                let verdicts = parse_verdicts(&text, targets.len());
                let open = targets.iter().zip(verdicts.answers);
                open.filter(|(_, answer)| *answer != Some(Refutation::Refuted))
                    .map(|(at, _)| *at)
                    .collect()
            }
            Ok(None) | Err(_) => {
                self.say(pane, run, "the correction could not be checked".to_owned());
                targets.to_vec()
            }
        }
    }

    /// The staged change set now, or `before` when it cannot be read.
    async fn restaged(&self, pane: PaneId, run: RunId, before: &[Change]) -> Vec<Change> {
        let origin = self.on_run(pane, run, |live| live.origin.clone());
        let (Some(tooling), Some(origin)) = (&self.tooling, origin) else {
            return before.to_vec();
        };
        let now = tooling
            .host
            .staged_changes(StageKey::Pane(pane), &origin)
            .await;
        if now.is_empty() { before.to_vec() } else { now }
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
    /// for one that did not end `done`); `None` when the run is gone.
    async fn finished(
        &self,
        pane: PaneId,
        run: RunId,
        ids: &[WorkerId],
    ) -> Option<Vec<Option<String>>> {
        loop {
            let changed = self.changed.notified();
            let settled = self.on_run(pane, run, |live| {
                let over = |id: &WorkerId| {
                    !matches!(
                        live.run.status(*id),
                        Some(WorkerStatus::Queued | WorkerStatus::Running) | None
                    )
                };
                ids.iter().all(over).then(|| {
                    let text = |id: &WorkerId| live.run.collect(*id).ok().map(str::to_owned);
                    ids.iter().map(text).collect::<Vec<_>>()
                })
            })?;
            if let Some(texts) = settled {
                return Some(texts);
            }
            changed.await;
        }
    }
}

fn refused(why: ReviewError) -> String {
    format!("the review state refused a step ({why:?})")
}
