//! The guided llama-server install of the llamacpp picker: Plan, Resolve,
//! Download, Install and the Hugging Face offer, in turn. Pure: a step the
//! user agreed to leaves as [`InstallAsk`] and its result comes back through
//! [`Wizard::reply`]; nothing here touches the network or the disk.

use super::super::Detection;
use super::super::config::AiConfig;
use super::super::hf;
use super::super::install::{Release, Variant};
use super::hub::{info, item, shown};
use super::{Level, Outcome, PickRow, Picker};

/// The plan row that starts nothing: it copies how to install by hand.
pub const COPY_ROW: &str = "I'll do it myself (copy instructions)";

/// What the "I'll do it myself" row copies.
pub const MANUAL: &str = "Download the llama-server build for your system from \
https://github.com/ggml-org/llama.cpp/releases, unpack it and put llama-server on your PATH.";

pub const HINT_PLAN: &str = "j/k move · Enter choose · Esc back";
pub const HINT_WORKING: &str = "working… · Esc back";
pub const HINT_CONFIRM_STEP: &str = "y continue · n back";
pub const HINT_DOWNLOADING: &str = "Esc cancel the download";
pub const HINT_CANCEL_INSTALL: &str = "cancel the download? (y/n)";
pub const HINT_INSTALLING: &str = "installing…";
pub const HINT_OFFER: &str = "y download a model · n close";
pub const HINT_FAILED: &str = "Esc back";

/// A step the user agreed to; the runtime runs it and replies.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum InstallAsk {
    /// Look up the newest release holding the build of this variant.
    Resolve(Variant),
    /// Download this release and check it against its digest.
    Download(Release),
    /// Unpack, check and activate this downloaded release.
    Install(Release),
    /// Stop the download that runs (the partial file stays for a resume).
    Cancel,
}

/// A step's success. A failure is the `Err` text of the reply.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Reply {
    Resolved(Release),
    Downloaded,
    Installed,
}

/// What a key asks of the picker.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Step {
    Stay,
    Ask(InstallAsk),
    /// Put this text on the clipboard.
    Copy(String),
    /// Open the Hugging Face screens.
    Hub,
    /// Leave the wizard.
    Close,
}

/// One wizard screen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Screen {
    /// The variants to choose from, and the copy-only row.
    Plan,
    Resolving(Variant),
    ConfirmDownload(Release),
    Downloading {
        release: Release,
        done: u64,
        /// `Esc` was pressed: `cancel the download? (y/n)` waits.
        cancel: bool,
    },
    ConfirmInstall(Release),
    Installing(Release),
    OfferHf,
    Failed(String),
}

/// The wizard state: which screen is open and which variants the plan has.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Wizard {
    variants: Vec<Variant>,
    screen: Screen,
}

fn variant_label(variant: Variant) -> &'static str {
    match variant {
        Variant::Cpu => "CPU build",
        Variant::Vulkan => "Vulkan build (GPU)",
        Variant::Metal => "Metal build (macOS)",
    }
}

impl Wizard {
    pub fn new(variants: Vec<Variant>) -> Self {
        Wizard {
            variants,
            screen: Screen::Plan,
        }
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// The request in flight, if any.
    pub fn asking(&self) -> Option<InstallAsk> {
        match &self.screen {
            Screen::Resolving(variant) => Some(InstallAsk::Resolve(*variant)),
            Screen::Downloading { release, .. } => Some(InstallAsk::Download(release.clone())),
            Screen::Installing(release) => Some(InstallAsk::Install(release.clone())),
            _ => None,
        }
    }

    /// Whether `cancel the download? (y/n)` waits for an answer.
    pub fn asks_cancel(&self) -> bool {
        matches!(self.screen, Screen::Downloading { cancel: true, .. })
    }

    pub fn title(&self) -> String {
        match &self.screen {
            Screen::Plan => "Install llama-server".to_owned(),
            Screen::Resolving(_) => "Install llama-server: looking up".to_owned(),
            Screen::ConfirmDownload(_) => "Install llama-server: download".to_owned(),
            Screen::Downloading { .. } => "Install llama-server: downloading".to_owned(),
            Screen::ConfirmInstall(_) | Screen::Installing(_) => {
                "Install llama-server: install".to_owned()
            }
            Screen::OfferHf => "llama-server installed".to_owned(),
            Screen::Failed(_) => "Install llama-server: failed".to_owned(),
        }
    }

    pub fn hint(&self) -> &'static str {
        match &self.screen {
            Screen::Plan => HINT_PLAN,
            Screen::Resolving(_) => HINT_WORKING,
            Screen::ConfirmDownload(_) | Screen::ConfirmInstall(_) => HINT_CONFIRM_STEP,
            Screen::Downloading { cancel: true, .. } => HINT_CANCEL_INSTALL,
            Screen::Downloading { .. } => HINT_DOWNLOADING,
            Screen::Installing(_) => HINT_INSTALLING,
            Screen::OfferHf => HINT_OFFER,
            Screen::Failed(_) => HINT_FAILED,
        }
    }

    pub fn rows(&self) -> Vec<PickRow> {
        let lines = |lines: Vec<String>| {
            lines
                .iter()
                .flat_map(|line| super::wrap_note(&shown(line)))
                .map(info)
                .collect()
        };
        match &self.screen {
            Screen::Plan => self
                .variants
                .iter()
                .map(|v| variant_label(*v).to_owned())
                .chain([COPY_ROW.to_owned()])
                .enumerate()
                .map(|(index, label)| item(index, label))
                .collect(),
            Screen::Resolving(variant) => lines(vec![format!(
                "looking up the newest {} on GitHub…",
                variant_label(*variant)
            )]),
            Screen::ConfirmDownload(release) => lines(vec![
                format!("download llama.cpp {}", release.tag),
                format!("file: {} ({})", release.asset, hf::size_label(release.size)),
                "from: github.com/ggml-org/llama.cpp".to_owned(),
                "the archive is checked against its SHA-256 digest".to_owned(),
            ]),
            Screen::Downloading { release, done, .. } => {
                let percent = if release.size == 0 {
                    0
                } else {
                    u64::try_from(
                        u128::from(*done.min(&release.size)) * 100 / u128::from(release.size),
                    )
                    .unwrap_or(100)
                };
                lines(vec![format!("↓ {percent}% {}", release.asset)])
            }
            Screen::ConfirmInstall(release) => lines(vec![
                format!("{} downloaded and verified", release.tag),
                "unpack it into your data folder and activate it".to_owned(),
            ]),
            Screen::Installing(release) => lines(vec![format!("installing {}…", release.tag)]),
            Screen::OfferHf => lines(vec![
                "llama-server is ready".to_owned(),
                "Download a model from Hugging Face?".to_owned(),
            ]),
            Screen::Failed(message) => lines(vec![message.clone()]),
        }
    }

    /// Enter on row `index`: the plan's choice. Choosing a variant is the
    /// consent to look it up; the copy-only row starts nothing.
    pub fn enter(&mut self, index: usize) -> Step {
        if self.screen != Screen::Plan {
            return Step::Stay;
        }
        if let Some(variant) = self.variants.get(index).copied() {
            self.screen = Screen::Resolving(variant);
            return Step::Ask(InstallAsk::Resolve(variant));
        }
        if index == self.variants.len() {
            return Step::Copy(MANUAL.to_owned());
        }
        Step::Stay
    }

    /// `y`: agree to the step a screen proposes, or confirm a cancel.
    pub fn yes(&mut self) -> Step {
        match std::mem::replace(&mut self.screen, Screen::Plan) {
            Screen::ConfirmDownload(release) => {
                self.screen = Screen::Downloading {
                    release: release.clone(),
                    done: 0,
                    cancel: false,
                };
                Step::Ask(InstallAsk::Download(release))
            }
            Screen::ConfirmInstall(release) => {
                self.screen = Screen::Installing(release.clone());
                Step::Ask(InstallAsk::Install(release))
            }
            Screen::OfferHf => {
                self.screen = Screen::OfferHf;
                Step::Hub
            }
            Screen::Downloading { cancel: true, .. } => Step::Ask(InstallAsk::Cancel),
            other => {
                self.screen = other;
                Step::Stay
            }
        }
    }

    /// `n`: decline the step, or keep the download.
    pub fn no(&mut self) -> Step {
        match &mut self.screen {
            Screen::ConfirmDownload(_) | Screen::ConfirmInstall(_) => {
                self.screen = Screen::Plan;
                Step::Stay
            }
            Screen::Downloading { cancel, .. } => {
                *cancel = false;
                Step::Stay
            }
            Screen::OfferHf => Step::Close,
            _ => Step::Stay,
        }
    }

    /// Esc: back one screen. A download asks first; the install step cannot
    /// be interrupted.
    pub fn back(&mut self) -> Step {
        match &mut self.screen {
            Screen::Plan | Screen::OfferHf => Step::Close,
            Screen::Downloading { cancel, .. } => {
                *cancel = true;
                Step::Stay
            }
            Screen::Installing(_) => Step::Stay,
            _ => {
                self.screen = Screen::Plan;
                Step::Stay
            }
        }
    }

    /// The bytes on disk of the download that runs.
    pub fn progress(&mut self, done: u64, total: u64) {
        if let Screen::Downloading {
            release,
            done: seen,
            ..
        } = &mut self.screen
        {
            if total > 0 {
                release.size = total;
            }
            *seen = done;
        }
    }

    /// The result of the step in flight. Dropped (`false`) when the wizard
    /// no longer waits for that kind of result.
    pub fn reply(&mut self, answer: Result<Reply, String>) -> bool {
        let waiting = matches!(
            self.screen,
            Screen::Resolving(_) | Screen::Downloading { .. } | Screen::Installing(_)
        );
        if !waiting {
            return false;
        }
        self.screen = match (&self.screen, answer) {
            (_, Err(message)) => Screen::Failed(message),
            (Screen::Resolving(_), Ok(Reply::Resolved(release))) => {
                Screen::ConfirmDownload(release)
            }
            (Screen::Downloading { release, .. }, Ok(Reply::Downloaded)) => {
                Screen::ConfirmInstall(release.clone())
            }
            (Screen::Installing(_), Ok(Reply::Installed)) => Screen::OfferHf,
            // A result of another kind: not the one this screen waits for.
            _ => return false,
        };
        true
    }
}

impl Picker {
    /// The install wizard, when it is open.
    pub fn install(&self) -> Option<&Wizard> {
        match &self.level {
            Level::Install(wizard) => Some(wizard),
            _ => None,
        }
    }

    /// Open the wizard over llamacpp's model level, offering `variants`.
    pub fn open_install(&mut self, variants: Vec<Variant>) {
        self.level = Level::Install(Wizard::new(variants));
        self.cursor = 0;
        self.offset = 0;
    }

    /// What a wizard [`Step`] asks of the caller.
    fn install_step(
        &mut self,
        step: Step,
        draft: &AiConfig,
        detection: Option<&Detection>,
    ) -> Outcome {
        match step {
            Step::Stay => Outcome::Stay,
            Step::Ask(ask) => Outcome::Install(ask),
            Step::Copy(text) => Outcome::Copy(text),
            Step::Hub => self.open_hub(),
            Step::Close => {
                self.leave_hub(draft, detection);
                Outcome::Stay
            }
        }
    }

    fn wizard_mut(&mut self) -> Option<&mut Wizard> {
        match &mut self.level {
            Level::Install(wizard) => Some(wizard),
            _ => None,
        }
    }

    /// Enter on a wizard row.
    pub(super) fn install_enter(
        &mut self,
        draft: &AiConfig,
        detection: Option<&Detection>,
    ) -> Outcome {
        let cursor = self.cursor;
        let step = self.wizard_mut().map_or(Step::Stay, |w| w.enter(cursor));
        self.install_step(step, draft, detection)
    }

    /// `y` or `n` on a wizard screen.
    pub fn install_answer(
        &mut self,
        yes: bool,
        draft: &AiConfig,
        detection: Option<&Detection>,
    ) -> Outcome {
        let step = self
            .wizard_mut()
            .map_or(Step::Stay, |w| if yes { w.yes() } else { w.no() });
        self.install_step(step, draft, detection)
    }

    /// Esc on a wizard screen.
    pub(super) fn install_back(
        &mut self,
        draft: &AiConfig,
        detection: Option<&Detection>,
    ) -> Outcome {
        let step = self.wizard_mut().map_or(Step::Stay, Wizard::back);
        self.cursor = 0;
        self.offset = 0;
        self.install_step(step, draft, detection)
    }

    /// The result of the step in flight.
    pub fn install_reply(&mut self, answer: Result<Reply, String>) {
        if let Some(wizard) = self.wizard_mut() {
            wizard.reply(answer);
            self.cursor = 0;
            self.offset = 0;
        }
    }

    /// The bytes on disk of the download that runs.
    pub fn install_progress(&mut self, done: u64, total: u64) {
        if let Some(wizard) = self.wizard_mut() {
            wizard.progress(done, total);
        }
    }
}
