//! The worker behind the install wizard: it runs the steps the user agreed
//! to (look up, download, install) in the background and sends what they
//! find to the event loop, always with the id of the request. One download
//! or install at a time; a cancel stops the download that runs.

use std::ffi::OsString;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::{mpsc, oneshot};

use super::{Fetched, activate, archive_path, download_verified, resolve};
use crate::app::AppEvent;
use crate::core::ai::install::{self, Platform, Release, Variant};
use crate::core::ai::picker::{InstallAsk, InstallReply as Reply};
use crate::runtime::hf::Curl;

/// What the reply of a cancelled download says; the wizard ignores the text
/// and goes back to the plan.
pub const CANCELLED: &str = "cancelled";

/// What a step asked while another one runs reads as.
pub const BUSY: &str = "another install step is still running";

/// What the reply of a step that panicked says.
pub const CRASHED: &str = "install step crashed";

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

/// Stops the managed llama-server (if one runs) and returns once it is gone.
pub type StopFn = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// The cancel of the download that runs, shared with its slot.
type CancelSlot = Arc<Mutex<Option<oneshot::Sender<()>>>>;

/// The single place a download or an install runs; freed when it is dropped,
/// whatever way its task ends. Its cancel goes with it.
struct Slot {
    busy: Arc<AtomicBool>,
    cancel: CancelSlot,
}

impl Slot {
    fn take(busy: &Arc<AtomicBool>, cancel: &CancelSlot) -> Option<Slot> {
        busy.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            .then(|| Slot {
                busy: Arc::clone(busy),
                cancel: Arc::clone(cancel),
            })
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        // The cancel first: once the slot is free, a new step owns it.
        if let Ok(mut cancel) = self.cancel.lock() {
            cancel.take();
        }
        self.busy.store(false, Ordering::Release);
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
    cancel: CancelSlot,
    busy: Arc<AtomicBool>,
    /// Run before the owned install is deleted: the managed server may be
    /// running from it.
    stop: StopFn,
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
            cancel: Arc::new(Mutex::new(None)),
            busy: Arc::new(AtomicBool::new(false)),
            stop: Arc::new(|| Box::pin(async {})),
        }
    }

    /// Sets what stops the managed llama-server before a remove deletes it.
    pub fn with_stop(mut self, stop: StopFn) -> Self {
        self.set_stop(stop);
        self
    }

    fn set_stop(&mut self, stop: StopFn) {
        self.stop = stop;
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
            InstallAsk::Remove(id, binary) => self.remove(id, binary),
            InstallAsk::CheckNewer => self.check_newer(),
            InstallAsk::Cancel => {
                let stop = self.cancel.lock().ok().and_then(|mut cancel| cancel.take());
                if let Some(stop) = stop {
                    let _ = stop.send(());
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
            // The step runs on its own task: a panic ends it with a reply
            // instead of leaving the wizard waiting for ever.
            let answer = tokio::spawn(step)
                .await
                .unwrap_or_else(|_| Err(CRASHED.to_owned()));
            drop(slot);
            let _ = events.send(AppEvent::InstallReply(id, answer)).await;
        });
    }

    /// The steps that change the disk run one at a time.
    fn slot(&self, id: u64) -> Option<Slot> {
        let slot = Slot::take(&self.busy, &self.cancel);
        if slot.is_none() {
            self.spawn(id, None, async { Err(BUSY.to_owned()) });
        }
        slot
    }

    fn root(&self) -> Result<PathBuf, String> {
        let home = self.data_home.as_deref().ok_or(NO_HOME)?;
        Ok(install::owned_root(home))
    }

    /// Deletes the owned install `binary` belongs to; the runtime refuses a
    /// system binary.
    fn remove(&mut self, id: u64, binary: PathBuf) {
        let Some(slot) = self.slot(id) else { return };
        let home = self.data_home.clone().ok_or(NO_HOME);
        let stop = Arc::clone(&self.stop);
        self.spawn(id, Some(slot), async move {
            let home = home?;
            // Only an owned install goes, so only then is the server (which
            // runs from it) stopped; a system binary stays as it is.
            if install::is_owned(&binary, &home) {
                stop().await;
            }
            activate::remove(&binary, &home)
                .await
                .map(|()| Reply::Removed)
        });
    }

    /// One lookup of the newest release of the installed build (Vulkan when
    /// that is what was installed); a newer tag than the installed one goes
    /// out as a notice. Every failure (offline, rate limit, no install) is
    /// silent, and nothing is downloaded. No slot: it changes no disk. While
    /// an install or a remove runs the notice is dropped, and the installed
    /// tag is read again right before it goes out.
    fn check_newer(&self) {
        let Some(root) = self.data_home.as_deref().map(install::owned_root) else {
            return;
        };
        let Some(installed) = activate::current_tag(&root) else {
            return;
        };
        let vulkan = activate::installed_vulkan(&root)
            && install::variants(Platform::CURRENT, true).contains(&Variant::Vulkan);
        let variant = if vulkan {
            Variant::Vulkan
        } else {
            match install::variants(Platform::CURRENT, false).first().copied() {
                Some(variant) => variant,
                None => return,
            }
        };
        let (curl, events, busy) = (
            (self.make)(true),
            self.events.clone(),
            Arc::clone(&self.busy),
        );
        tokio::spawn(async move {
            let Ok(release) = resolve(&curl, Platform::CURRENT, variant).await else {
                return;
            };
            if busy.load(Ordering::Acquire)
                || activate::current_tag(&root).as_deref() != Some(&installed)
            {
                return;
            }
            if install::is_newer(&installed, &release.tag) {
                let build = if vulkan { " (Vulkan build)" } else { "" };
                let notice = format!(
                    "llama.cpp {}{build} is available (installed {installed}): open llamacpp in the AI tab to install it",
                    release.tag
                );
                let _ = events.send(AppEvent::InstallNewer(notice)).await;
            }
        });
    }

    fn download(&mut self, id: u64, release: Release) {
        let Some(slot) = self.slot(id) else { return };
        let (stop, mut cancel) = oneshot::channel();
        if let Ok(mut cancel) = self.cancel.lock() {
            *cancel = Some(stop);
        }
        let (curl, path, root) = ((self.make)(false), self.path.clone(), self.root());
        let events = self.events.clone();
        self.spawn(id, Some(slot), async move {
            let progress = |done, total| {
                let _ = events.try_send(AppEvent::InstallProgress { id, done, total });
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
