//! The Hugging Face download of the llamacpp picker, the Ollama pull of the
//! ollama picker and the account of the `Hugging Face` row (config-menu):
//! the requests go out as effects, the replies and the progress come back as
//! events. One download at a time, from either source; its progress is a
//! status bar mark and its end a notice.

use super::{App, Effect};
use crate::core::ai::hf::{AccountReply, Answer, Ask, End, Job};
use crate::core::ai::ollama_lib::{self, Pull};
use crate::core::ai::picker::{Running, Source, shown};

/// The notice of a cancelled download.
const CANCELLED: &str = "download cancelled · run it again to resume";

impl App {
    /// The status bar mark of the running download: `↓ 42%`.
    pub fn status_download(&self) -> Option<String> {
        self.download
            .as_ref()
            .map(|download| format!("↓ {}%", download.percent()))
    }

    /// Tells the open menu about the running download, if any.
    pub(super) fn sync_download(&mut self) {
        if let Some(menu) = self.menu.as_mut() {
            menu.set_download(self.download.clone());
        }
        self.dirty = true;
    }

    /// `y` on the Confirm screen. Ignored while a download runs: the picker
    /// offers to cancel that one instead.
    pub(super) fn start_download(&mut self, job: Box<Job>) -> Vec<Effect> {
        if self.download.is_some() {
            return Vec::new();
        }
        self.download = Some(Running {
            source: Source::HuggingFace,
            file: job.choice.name().to_owned(),
            license: job.license.id.clone(),
            done: 0,
            total: job.choice.size,
        });
        self.sync_download();
        vec![Effect::Download(job)]
    }

    /// `y` on the Ollama Confirm screen. Ignored while a download runs, and
    /// with no Ollama port configured.
    pub(super) fn start_pull(&mut self, pull: Box<Pull>) -> Vec<Effect> {
        if self.download.is_some() {
            return Vec::new();
        }
        let Some(port) = self.ai.servers().port("ollama") else {
            return Vec::new();
        };
        self.download = Some(Running {
            source: Source::Ollama,
            file: pull.name(),
            license: pull.license.clone(),
            done: 0,
            total: pull.size.unwrap_or(0),
        });
        self.sync_download();
        vec![Effect::Pull { pull, port }]
    }

    /// A reply of the Ollama library, for the menu's picker. With the menu
    /// closed nobody waits for it.
    pub(super) fn on_ollama_reply(
        &mut self,
        ask: ollama_lib::Ask,
        answer: Result<ollama_lib::Answer, String>,
    ) -> Vec<Effect> {
        if let Some(menu) = self.menu.as_mut() {
            menu.ollama_reply(ask, answer);
            self.dirty = true;
        }
        Vec::new()
    }

    /// `y` on the cancel prompt.
    pub(super) fn cancel_download(&mut self) -> Vec<Effect> {
        match self.download {
            Some(_) => vec![Effect::CancelDownload],
            None => Vec::new(),
        }
    }

    /// A reply of the Hub, for the menu's picker. With the menu closed
    /// nobody waits for it.
    pub(super) fn on_hub_reply(&mut self, ask: Ask, answer: Result<Answer, String>) -> Vec<Effect> {
        if let Some(menu) = self.menu.as_mut() {
            menu.hub_reply(ask, answer);
            self.dirty = true;
        }
        Vec::new()
    }

    /// The bytes on disk of the running download; dropped once it ended.
    pub(super) fn on_download_progress(&mut self, done: u64, total: u64) -> Vec<Effect> {
        if let Some(download) = self.download.as_mut() {
            download.done = done;
            download.total = total;
            self.sync_download();
        }
        Vec::new()
    }

    /// The running download ended: a notice says how, and a finished one
    /// makes the engines be detected again, so its file is listed.
    pub(super) fn on_download_end(&mut self, end: End) -> Vec<Effect> {
        let Some(download) = self.download.take() else {
            return Vec::new();
        };
        self.sync_download();
        match end {
            End::Done(_) => {
                self.notice = Some(format!("downloaded {}", shown(&download.file)));
                let effects = self.request_forced_detection();
                self.sync_checking();
                effects
            }
            End::Cancelled => {
                self.notice = Some(CANCELLED.to_owned());
                Vec::new()
            }
            End::Failed(message) => {
                self.notice = Some(message);
                Vec::new()
            }
        }
    }

    /// What the runtime found for an account request of the `Hugging Face`
    /// row.
    /// The account is kept for the session, so a menu opened later shows
    /// it; the open menu learns it at once.
    pub(super) fn on_account_reply(&mut self, reply: AccountReply) -> Vec<Effect> {
        match &reply {
            AccountReply::Connected(Ok(name)) | AccountReply::Checked(Ok(name)) => {
                self.hf_account = Some(name.clone());
            }
            AccountReply::Disconnected(Ok(())) => self.hf_account = None,
            _ => {}
        }
        if let Some(menu) = self.menu.as_mut() {
            menu.account_reply(reply);
            self.dirty = true;
        }
        Vec::new()
    }
}
