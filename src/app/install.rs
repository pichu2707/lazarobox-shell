//! The llama-server install wizard of the llamacpp picker: each step the
//! user agreed to goes out as an effect, and its reply, its progress and its
//! warning come back as events. The wizard itself lives in the picker; this
//! only carries its messages and makes the engines be detected again once
//! the install changed what is on the disk.

use super::{App, Effect};
use crate::core::ai::picker::{InstallAsk, InstallReply};

impl App {
    /// The effects of an install ask. The newer build check goes out once per
    /// run, and never when `[updates] check = false`: the runtime would make
    /// a network call the user turned off.
    pub(super) fn install_effects(&mut self, ask: InstallAsk) -> Vec<Effect> {
        if ask == InstallAsk::CheckNewer {
            if self.newer_checked || !self.config.update_check {
                return Vec::new();
            }
            self.newer_checked = true;
        }
        vec![Effect::Install(ask)]
    }

    /// The bytes on disk of the archive that downloads.
    pub(super) fn on_install_progress(&mut self, id: u64, done: u64) -> Vec<Effect> {
        if let Some(menu) = self.menu.as_mut() {
            menu.install_progress(id, done);
            self.dirty = true;
        }
        Vec::new()
    }

    /// The result of install request `id`. The wizard drops one that is not
    /// the request in flight; a finished install or remove makes the
    /// engines be detected again either way, since it changed the disk even when
    /// nobody waits for it.
    pub(super) fn on_install_reply(
        &mut self,
        id: u64,
        answer: Result<InstallReply, String>,
    ) -> Vec<Effect> {
        let changed = matches!(answer, Ok(InstallReply::Installed | InstallReply::Removed));
        if let Some(menu) = self.menu.as_mut() {
            menu.install_reply(id, answer);
            self.dirty = true;
        }
        if !changed {
            return Vec::new();
        }
        let effects = self.request_forced_detection();
        self.sync_checking();
        effects
    }

    /// A notice about the install: why the archive's attestation was skipped,
    /// or that a newer build exists. Shown in the footer.
    pub(super) fn on_install_warning(&mut self, warning: String) -> Vec<Effect> {
        self.notice = Some(warning);
        self.dirty = true;
        Vec::new()
    }
}
