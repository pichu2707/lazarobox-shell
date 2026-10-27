//! The worker behind the install wizard: it runs the steps the user agreed
//! to (look up, download, install) in the background and sends what they
//! find to the event loop, always with the id of the request. One download
//! or install at a time; a cancel stops the download that runs.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{mpsc, oneshot};

use super::{Fetched, activate, archive_path, download_verified, resolve};
use crate::app::AppEvent;
use crate::core::ai::install::{self, Platform, Release};
use crate::core::ai::picker::{InstallAsk, InstallReply as Reply};
use crate::runtime::hf::Curl;

/// What the reply of a cancelled download says; the wizard ignores the text
/// and goes back to the plan.
pub const CANCELLED: &str = "cancelled";

/// What a step asked while another one runs reads as.
pub const BUSY: &str = "another install step is running; wait for it to end";

/// What a step without a data folder reads as.
pub const NO_HOME: &str = "no data folder was found to install llama.cpp into";

/// The folders whose files make a Vulkan build usable: one ICD file per
/// driver.
const ICD_DIRS: [&str; 2] = ["/usr/share/vulkan/icd.d", "/etc/vulkan/icd.d"];

/// Whether a Vulkan driver is registered on this machine.
pub fn has_vulkan_icd() -> bool {
    has_icd_in(&ICD_DIRS.map(Path::new))
}

fn has_icd_in(dirs: &[&Path]) -> bool {
    dirs.iter().any(|dir| {
        std::fs::read_dir(dir).is_ok_and(|entries| {
            entries.flatten().any(|entry| {
                entry.path().extension().is_some_and(|ext| ext == "json")
                    && entry.file_type().is_ok_and(|kind| kind.is_file())
            })
        })
    })
}

/// Builds the curl of one request: the GitHub API (`true`) or the download
/// host.
type MakeCurl = Arc<dyn Fn(bool) -> Curl + Send + Sync>;

/// The single place a download or an install runs; freed when it is dropped,
/// whatever way its task ends.
struct Slot(Arc<AtomicBool>);

impl Slot {
    fn take(flag: &Arc<AtomicBool>) -> Option<Slot> {
        flag.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then(|| Slot(Arc::clone(flag)))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Runs the wizard's steps and replies with `AppEvent::InstallReply`.
pub struct Installer {
    make: MakeCurl,
    /// The login `PATH`: `curl`, `gh` and `tar` are looked up in it.
    path: OsString,
    data_home: Option<PathBuf>,
    events: mpsc::Sender<AppEvent>,
    /// The cancel of the download that runs.
    cancel: Option<oneshot::Sender<()>>,
    busy: Arc<AtomicBool>,
}

impl Installer {
    pub fn new(path: OsString, data_home: Option<PathBuf>, events: mpsc::Sender<AppEvent>) -> Self {
        let curl_path = path.clone();
        Installer {
            make: Arc::new(move |api| {
                if api {
                    Curl::github_api(curl_path.clone())
                } else {
                    Curl::github(curl_path.clone())
                }
            }),
            path,
            data_home,
            events,
            cancel: None,
            busy: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Tests: every request goes through the curl `make` builds.
    #[cfg(test)]
    fn with_curl(mut self, make: impl Fn(bool) -> Curl + Send + Sync + 'static) -> Self {
        self.make = Arc::new(make);
        self
    }

    /// Runs one step; one `AppEvent::InstallReply` with the same id answers
    /// it. Returns at once.
    pub fn ask(&mut self, ask: InstallAsk) {
        match ask {
            InstallAsk::Resolve(id, variant) => {
                let curl = (self.make)(true);
                self.spawn(id, None, async move {
                    resolve(&curl, Platform::CURRENT, variant)
                        .await
                        .map(Reply::Resolved)
                });
            }
            InstallAsk::Download(id, release) => self.download(id, release),
            InstallAsk::Install(id, release) => self.install(id, release),
            InstallAsk::Cancel => {
                if let Some(cancel) = self.cancel.take() {
                    let _ = cancel.send(());
                }
            }
        }
    }

    /// Runs `step` on its own task and sends its result. `slot` is released
    /// before the reply goes out, so the next step is never refused.
    fn spawn(
        &self,
        id: u64,
        slot: Option<Slot>,
        step: impl Future<Output = Result<Reply, String>> + Send + 'static,
    ) {
        let events = self.events.clone();
        tokio::spawn(async move {
            let answer = step.await;
            drop(slot);
            let _ = events.send(AppEvent::InstallReply(id, answer)).await;
        });
    }

    /// The steps that change the disk run one at a time.
    fn slot(&self, id: u64) -> Option<Slot> {
        let slot = Slot::take(&self.busy);
        if slot.is_none() {
            self.spawn(id, None, async { Err(BUSY.to_owned()) });
        }
        slot
    }

    fn root(&self) -> Result<PathBuf, String> {
        let home = self.data_home.as_deref().ok_or(NO_HOME)?;
        Ok(install::owned_root(home))
    }

    fn download(&mut self, id: u64, release: Release) {
        let Some(slot) = self.slot(id) else { return };
        let (stop, mut cancel) = oneshot::channel();
        self.cancel = Some(stop);
        let (curl, path, root) = ((self.make)(false), self.path.clone(), self.root());
        let events = self.events.clone();
        self.spawn(id, Some(slot), async move {
            let progress = |done, total| {
                let _ = events.try_send(AppEvent::InstallProgress { done, total });
            };
            let fetched =
                download_verified(&curl, &path, &root?, &release, progress, &mut cancel).await;
            match fetched {
                Fetched::Done { warning, .. } => {
                    if let Some(warning) = warning {
                        let _ = events.send(AppEvent::InstallWarning(warning)).await;
                    }
                    Ok(Reply::Downloaded)
                }
                Fetched::Cancelled => Err(CANCELLED.to_owned()),
                Fetched::Failed(message) => Err(message),
            }
        });
    }

    fn install(&mut self, id: u64, release: Release) {
        let Some(slot) = self.slot(id) else { return };
        let (path, root) = (self.path.clone(), self.root());
        self.spawn(id, Some(slot), async move {
            let root = root?;
            let archive = archive_path(&root, &release);
            activate::install(&root, &release, &archive, &path)
                .await
                .map(|()| Reply::Installed)
        });
    }
}

#[cfg(test)]
mod tests;
