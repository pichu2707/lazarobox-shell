//! The guided llama-server install of the llamacpp picker: Plan, Resolve,
//! Download, Install and the Hugging Face offer, in turn. Pure: a step the
//! user agreed to leaves as [`InstallAsk`] and its result comes back through
//! [`Wizard::reply`]; nothing here touches the network or the disk.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

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
pub const HINT_CANCELLING: &str = "cancelling…";
pub const HINT_INSTALLING: &str = "installing…";
pub const HINT_OFFER: &str = "y download a model · n close";
pub const HINT_REMOVING: &str = "removing…";
pub const HINT_FAILED: &str = "Esc back";

/// The last request id handed out by any wizard. Process-wide: the installer
/// outlives the menu, so a late reply of a closed wizard must never match the
/// id a new wizard waits on.
static LAST_ID: AtomicU64 = AtomicU64::new(0);

/// A step the user agreed to; the runtime runs it and replies with the same
/// request id.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum InstallAsk {
    /// Look up the newest release holding the build of this variant.
    Resolve(u64, Variant),
    /// Download this release and check it against its digest.
    Download(u64, Release),
    /// Unpack, check and activate this downloaded release.
    Install(u64, Release),
    /// Stop the download that runs (the partial file stays for a resume).
    Cancel,
    /// Delete the owned install; the binary is the one the user chose to
    /// remove (the runtime refuses anything outside the owned folder).
    Remove(u64, PathBuf),
    /// Look up, once per run, whether a newer build exists. No id: nothing
    /// waits for the answer, a newer build shows up as a notice and
    /// anything else stays silent.
    CheckNewer,
}

/// A step's success. A failure is the `Err` text of the reply.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Reply {
    Resolved(Release),
    Downloaded,
    Installed,
    Removed,
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
        /// The cancel was confirmed and the download is still stopping.
        cancelling: bool,
    },
    ConfirmInstall(Release),
    Installing(Release),
    OfferHf,
    Failed(String),
    /// `remove the llama.cpp install? (y/n)`.
    ConfirmRemove,
    Removing,
    Removed,
}

/// The wizard state: which screen is open and which variants the plan has.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Wizard {
    variants: Vec<Variant>,
    /// The binary a remove wizard deletes; `None` on the install wizard.
    binary: Option<PathBuf>,
    screen: Screen,
    /// The id of the request the wizard waits on; any other reply is stale.
    in_flight: Option<u64>,
    /// Whether the confirm screen was drawn since a reply opened it. A key
    /// queued for the previous screen must not answer a prompt never seen.
    armed: bool,
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
            binary: None,
            screen: Screen::Plan,
            in_flight: None,
            armed: true,
        }
    }

    /// A wizard that asks to delete the owned install `binary` belongs to.
    /// Disarmed until its prompt is drawn, like every confirm screen.
    pub fn remove(binary: PathBuf) -> Self {
        Wizard {
            variants: Vec::new(),
            binary: Some(binary),
            screen: Screen::ConfirmRemove,
            in_flight: None,
            armed: false,
        }
    }

    /// The builds the plan offers.
    pub fn variants(&self) -> &[Variant] {
        &self.variants
    }

    /// A new request id, now in flight.
    fn issue(&mut self) -> u64 {
        let id = LAST_ID.fetch_add(1, Ordering::Relaxed) + 1;
        self.in_flight = Some(id);
        id
    }

    /// Whether a download is in flight: closing the wizard now must stop it.
    pub fn downloading(&self) -> bool {
        self.in_flight.is_some() && matches!(self.screen, Screen::Downloading { .. })
    }

    /// The screen was drawn: its `y`/`n` now answer what the user saw. The
    /// picker calls this from its draw/tick path.
    pub fn arm(&mut self) {
        self.armed = true;
    }

    pub fn screen(&self) -> &Screen {
        &self.screen
    }

    /// The request in flight, if any.
    pub fn asking(&self) -> Option<InstallAsk> {
        let id = self.in_flight?;
        match &self.screen {
            Screen::Resolving(variant) => Some(InstallAsk::Resolve(id, *variant)),
            Screen::Downloading { release, .. } => Some(InstallAsk::Download(id, release.clone())),
            Screen::Installing(release) => Some(InstallAsk::Install(id, release.clone())),
            Screen::Removing => Some(InstallAsk::Remove(id, self.binary.clone()?)),
            _ => None,
        }
    }

    /// Whether `cancel the download? (y/n)` waits for an answer.
    pub fn asks_cancel(&self) -> bool {
        matches!(
            self.screen,
            Screen::Downloading {
                cancel: true,
                cancelling: false,
                ..
            }
        )
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
            Screen::Failed(_) if self.binary.is_some() => "Remove llama-server: failed".to_owned(),
            Screen::Failed(_) => "Install llama-server: failed".to_owned(),
            Screen::ConfirmRemove | Screen::Removing | Screen::Removed => {
                "Remove llama-server".to_owned()
            }
        }
    }

    pub fn hint(&self) -> &'static str {
        match &self.screen {
            Screen::Plan => HINT_PLAN,
            Screen::Resolving(_) => HINT_WORKING,
            Screen::ConfirmDownload(_) | Screen::ConfirmInstall(_) => HINT_CONFIRM_STEP,
            Screen::Downloading {
                cancelling: true, ..
            } => HINT_CANCELLING,
            Screen::Downloading { cancel: true, .. } => HINT_CANCEL_INSTALL,
            Screen::Downloading { .. } => HINT_DOWNLOADING,
            Screen::Installing(_) => HINT_INSTALLING,
            Screen::OfferHf => HINT_OFFER,
            Screen::Failed(_) | Screen::Removed => HINT_FAILED,
            Screen::ConfirmRemove => HINT_CONFIRM_STEP,
            Screen::Removing => HINT_REMOVING,
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
            Screen::Downloading {
                cancelling: true, ..
            } => lines(vec!["cancelling…".to_owned()]),
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
            Screen::ConfirmRemove => lines(vec![
                "remove the llama.cpp install of lazarobox?".to_owned(),
                "every downloaded build is deleted; a llama-server of your own is not touched"
                    .to_owned(),
            ]),
            Screen::Removing => lines(vec!["removing…".to_owned()]),
            Screen::Removed => lines(vec!["the llama.cpp install was removed".to_owned()]),
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
            return Step::Ask(InstallAsk::Resolve(self.issue(), variant));
        }
        if index == self.variants.len() {
            return Step::Copy(MANUAL.to_owned());
        }
        Step::Stay
    }

    /// `y`: agree to the step a screen proposes, or confirm a cancel. A
    /// confirm screen not drawn yet ignores it.
    pub fn yes(&mut self) -> Step {
        match std::mem::replace(&mut self.screen, Screen::Plan) {
            Screen::ConfirmDownload(release) if self.armed => {
                self.screen = Screen::Downloading {
                    release: release.clone(),
                    done: 0,
                    cancel: false,
                    cancelling: false,
                };
                Step::Ask(InstallAsk::Download(self.issue(), release))
            }
            Screen::ConfirmInstall(release) if self.armed => {
                self.screen = Screen::Installing(release.clone());
                Step::Ask(InstallAsk::Install(self.issue(), release))
            }
            Screen::OfferHf if self.armed => {
                self.screen = Screen::OfferHf;
                Step::Hub
            }
            Screen::ConfirmRemove if self.armed && self.binary.is_some() => {
                self.screen = Screen::Removing;
                let binary = self.binary.clone().unwrap_or_default();
                Step::Ask(InstallAsk::Remove(self.issue(), binary))
            }
            Screen::Downloading {
                release,
                done,
                cancel: true,
                cancelling: false,
            } => {
                // Stays on screen as "cancelling…" until the reply arrives.
                self.screen = Screen::Downloading {
                    release,
                    done,
                    cancel: true,
                    cancelling: true,
                };
                Step::Ask(InstallAsk::Cancel)
            }
            other => {
                self.screen = other;
                Step::Stay
            }
        }
    }

    /// `n`: decline the step, or keep the download.
    pub fn no(&mut self) -> Step {
        match &mut self.screen {
            Screen::ConfirmDownload(_)
            | Screen::ConfirmInstall(_)
            | Screen::OfferHf
            | Screen::ConfirmRemove
                if !self.armed => {}
            Screen::ConfirmRemove => return Step::Close,
            Screen::ConfirmDownload(_) | Screen::ConfirmInstall(_) => {
                self.screen = Screen::Plan;
            }
            Screen::Downloading {
                cancel,
                cancelling: false,
                ..
            } => *cancel = false,
            Screen::OfferHf => return Step::Close,
            _ => {}
        }
        Step::Stay
    }

    /// Esc: back one screen. A download asks first; the install step cannot
    /// be interrupted.
    pub fn back(&mut self) -> Step {
        match &mut self.screen {
            Screen::Plan | Screen::OfferHf | Screen::ConfirmRemove | Screen::Removed => Step::Close,
            Screen::Downloading {
                cancelling: true, ..
            }
            | Screen::Installing(_)
            | Screen::Removing => Step::Stay,
            Screen::Downloading { cancel, .. } => {
                *cancel = true;
                Step::Stay
            }
            // A remove wizard has no plan to go back to.
            Screen::Failed(_) if self.binary.is_some() => Step::Close,
            screen => {
                // A lookup left behind: its reply is dropped by id. The
                // lookup is idempotent, so nothing needs cancelling.
                if matches!(screen, Screen::Resolving(_)) {
                    self.in_flight = None;
                }
                self.screen = Screen::Plan;
                Step::Stay
            }
        }
    }

    /// The bytes on disk of the download of request `id`. Progress of any
    /// other request is dropped; the size shown stays the release's.
    pub fn progress(&mut self, id: u64, done: u64) {
        if self.in_flight != Some(id) {
            return;
        }
        if let Screen::Downloading { done: seen, .. } = &mut self.screen {
            *seen = done;
        }
    }

    /// The result of request `id`. Dropped (`false`) when it is not the
    /// request in flight or not the kind of result the screen waits for.
    /// Opening a confirm screen disarms it until it is drawn.
    pub fn reply(&mut self, id: u64, answer: Result<Reply, String>) -> bool {
        if self.in_flight != Some(id) {
            return false;
        }
        let cancelling = matches!(
            self.screen,
            Screen::Downloading {
                cancelling: true,
                ..
            }
        );
        let next = match (&self.screen, answer) {
            // The user cancelled: whatever the download ended with, back to the plan.
            _ if cancelling => Screen::Plan,
            (
                Screen::Resolving(_)
                | Screen::Downloading { .. }
                | Screen::Installing(_)
                | Screen::Removing,
                Err(m),
            ) => Screen::Failed(m),
            (Screen::Removing, Ok(Reply::Removed)) => Screen::Removed,
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
        self.armed = !matches!(
            next,
            Screen::ConfirmDownload(_) | Screen::ConfirmInstall(_) | Screen::OfferHf
        );
        self.in_flight = None;
        self.screen = next;
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
            // A Hugging Face download that runs owns the hub: the offer
            // must not open its cancel prompt.
            Step::Hub if self.download.is_some() => Outcome::Stay,
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

    /// The wizard screen was drawn: its confirm prompts accept `y`/`n`.
    pub fn install_arm(&mut self) {
        if let Some(wizard) = self.wizard_mut() {
            wizard.arm();
        }
    }

    /// The result of request `id`; a stale one changes nothing.
    pub fn install_reply(&mut self, id: u64, answer: Result<Reply, String>) {
        if let Some(wizard) = self.wizard_mut()
            && wizard.reply(id, answer)
        {
            self.cursor = 0;
            self.offset = 0;
        }
    }

    /// The bytes on disk of the download of request `id`.
    pub fn install_progress(&mut self, id: u64, done: u64) {
        if let Some(wizard) = self.wizard_mut() {
            wizard.progress(id, done);
        }
    }
}
