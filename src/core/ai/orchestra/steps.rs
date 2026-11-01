//! The ODD steps of a run and what closes them (agent-orchestrator S5, D15).
//! Pure: a step is closed only with its handoff, kept in the ledger whether
//! or not engram stores it.

use super::WorkerId;
use super::feature::FeatureError;
use super::ledger::Handoff;

/// The ODD steps, in the order a run takes them.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    Authorize,
    Explore,
    Resolve,
    Classify,
    /// Large goals only.
    Track,
    Implement,
    Close,
}

impl Step {
    pub const ALL: [Step; 7] = [
        Self::Authorize,
        Self::Explore,
        Self::Resolve,
        Self::Classify,
        Self::Track,
        Self::Implement,
        Self::Close,
    ];

    /// The spec's id of the step, also its topic-key segment.
    pub fn label(self) -> &'static str {
        match self {
            Self::Authorize => "authorize",
            Self::Explore => "explore",
            Self::Resolve => "resolve",
            Self::Classify => "classify",
            Self::Track => "track",
            Self::Implement => "implement",
            Self::Close => "close",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|step| step.label() == text)
    }

    fn at(self) -> usize {
        Self::ALL.iter().position(|step| *step == self).unwrap_or(0)
    }
}

/// The reason a `code` worker is refused on a read-only goal.
pub const READ_ONLY: &str = "code workers are not spawned for a read-only goal";

/// How big `classify` found the goal.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Size {
    Small,
    Large,
}

impl Size {
    pub fn label(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Large => "large",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::Small, Self::Large]
            .into_iter()
            .find(|size| size.label() == text)
    }
}

/// What `authorize` allows the run to do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Grant {
    ReadOnly,
    Implement,
}

impl Grant {
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Implement => "implement",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        [Self::ReadOnly, Self::Implement]
            .into_iter()
            .find(|grant| grant.label() == text)
    }
}

/// What a step's handoff decides besides the record: the grant of
/// `authorize` or the size of `classify`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mark {
    None,
    Grant(Grant),
    Size(Size),
}

/// Why a step was not closed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepError {
    /// The run is over: its record is closed.
    Ended,
    /// The step has no handoff yet.
    NoHandoff(Step),
    /// An earlier step is still open.
    Order { step: Step, open: Step },
    /// `track` is for large goals only.
    NotLarge,
    /// The feature document refused (S5): `track` closes with it.
    Doc(FeatureError),
    /// `authorize` cannot make the goal read-only while this `code` worker
    /// runs: cancel it first.
    CodeRunning(WorkerId),
    /// `classify` needs a reason (its one line).
    NoReason,
    /// A read-only goal cannot be authorized to implement again.
    Widen,
    /// `step` cannot be handed off again: `later` is already closed.
    Locked { step: Step, later: Step },
}

/// The steps of one run.
#[derive(Clone, Debug, Default)]
pub struct Steps {
    handoffs: [Option<Handoff>; 7],
    closed: [bool; 7],
    size: Option<(Size, String)>,
    grant: Option<Grant>,
    read_only: bool,
}

impl Steps {
    /// Whether `authorize` made the goal read-only. Once it did, a later
    /// handoff cannot lift it.
    pub fn read_only(&self) -> bool {
        self.read_only
    }

    pub fn size(&self) -> Option<Size> {
        self.size.as_ref().map(|(size, _)| *size)
    }

    pub fn is_closed(&self, step: Step) -> bool {
        self.closed[step.at()]
    }

    pub fn handoff(&self, step: Step) -> Option<&Handoff> {
        self.handoffs[step.at()].as_ref()
    }

    /// Whether `step` may be closed now: every step before it is, except
    /// `track` for a goal not found large.
    pub fn check(&self, step: Step) -> Result<(), StepError> {
        let small = self.size() == Some(Size::Small);
        if step == Step::Classify && self.is_closed(step) {
            let later = Step::ALL.into_iter().skip(step.at() + 1);
            if let Some(later) = later.into_iter().find(|s| self.is_closed(*s)) {
                return Err(StepError::Locked { step, later });
            }
        }
        if step == Step::Track && small {
            return Err(StepError::NotLarge);
        }
        let before = Step::ALL.into_iter().take(step.at());
        for open in before.filter(|s| !(*s == Step::Track && small)) {
            if !self.is_closed(open) {
                return Err(StepError::Order { step, open });
            }
        }
        Ok(())
    }

    /// Whether `mark` may replace what `step` stored: a read-only grant is
    /// never widened, so the record and the ledger agree.
    pub fn check_mark(&self, step: Step, mark: Mark) -> Result<(), StepError> {
        match (step, mark) {
            (Step::Authorize, Mark::Grant(Grant::Implement)) if self.read_only => {
                Err(StepError::Widen)
            }
            _ => Ok(()),
        }
    }

    /// Keeps the handoff of `step`, replacing an earlier one.
    pub fn record(&mut self, step: Step, handoff: Handoff) {
        self.handoffs[step.at()] = Some(handoff);
    }

    /// Closes `step`: refused without its handoff or out of order. `mark`
    /// sets the grant or the size; a read-only grant is never lifted.
    pub fn close(&mut self, step: Step, mark: Mark) -> Result<(), StepError> {
        let Some(handoff) = self.handoff(step) else {
            return Err(StepError::NoHandoff(step));
        };
        self.check(step)?;
        let reason = handoff.reason.clone();
        match (step, mark) {
            (Step::Authorize, Mark::Grant(grant)) => {
                self.read_only |= grant == Grant::ReadOnly;
                self.grant = Some(grant);
            }
            (Step::Classify, Mark::Size(size)) => self.size = Some((size, reason)),
            _ => {}
        }
        self.closed[step.at()] = true;
        Ok(())
    }

    /// The ledger's line without grants, sizes and reasons.
    pub fn short_line(&self) -> String {
        let closed = Step::ALL.into_iter().filter(|s| self.is_closed(*s)).count();
        match closed {
            0 => String::new(),
            n => format!("Steps: {n} closed"),
        }
    }

    /// The ledger's line: each closed step, with its grant or size.
    pub fn line(&self) -> String {
        let closed: Vec<String> = Step::ALL
            .into_iter()
            .filter(|step| self.is_closed(*step))
            .map(|step| match (step, self.grant, &self.size) {
                (Step::Authorize, Some(grant), _) => {
                    format!("{} ({})", step.label(), grant.label())
                }
                (Step::Classify, _, Some((size, reason))) => {
                    format!("{} ({}: {reason})", step.label(), size.label())
                }
                _ => step.label().to_owned(),
            })
            .collect();
        if closed.is_empty() {
            String::new()
        } else {
            format!("Steps: {}", closed.join(", "))
        }
    }
}
