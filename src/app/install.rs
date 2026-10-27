//! The llama-server install wizard of the llamacpp picker: each step the
//! user agreed to goes out as an effect, and its reply, its progress and its
//! warning come back as events. The wizard itself lives in the picker; this
//! only carries its messages and makes the engines be detected again once
//! the install changed what is on the disk.

use super::{App, Effect};
use crate::core::ai::picker::InstallReply;

impl App {
    /// The bytes on disk of the archive that downloads.
    pub(super) fn on_install_progress(&mut self, done: u64, total: u64) -> Vec<Effect> {
        if let Some(menu) = self.menu.as_mut() {
            menu.install_progress(done, total);
            self.dirty = true;
        }
        Vec::new()
    }

    /// The result of install request `id`. The wizard drops one that is not
    /// the request in flight; a finished install makes the engines be
    /// detected again either way, since it changed the disk even when
    /// nobody waits for it.
    pub(super) fn on_install_reply(
        &mut self,
        id: u64,
        answer: Result<InstallReply, String>,
    ) -> Vec<Effect> {
        let installed = matches!(answer, Ok(InstallReply::Installed));
        if let Some(menu) = self.menu.as_mut() {
            menu.install_reply(id, answer);
            self.dirty = true;
        }
        if !installed {
            return Vec::new();
        }
        let effects = self.request_forced_detection();
        self.sync_checking();
        effects
    }

    /// Why the archive's attestation was skipped: a footer notice.
    pub(super) fn on_install_warning(&mut self, warning: String) -> Vec<Effect> {
        self.notice = Some(warning);
        self.dirty = true;
        Vec::new()
    }
}
