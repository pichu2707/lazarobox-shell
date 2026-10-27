//! The Hugging Face screens of the llamacpp picker (config-menu): Query,
//! Repos, Files and Confirm, in turn. Pure: a request leaves as
//! [`Outcome::Ask`] and its reply comes back through [`Picker::hub_reply`];
//! the download leaves as [`Outcome::Download`] on `y`, and nothing else
//! here starts one or edits the config.

use std::path::PathBuf;

use super::super::Detection;
use super::super::config::AiConfig;
use super::super::hf::{self, Answer, Ask, Job, Listing, RepoHit};
use super::super::sanitize::is_format;
use super::{Level, Mark, Outcome, PickKey, PickRow, Picker, wrap_note};

/// The entry row on llamacpp's model level.
pub const HF_ROW: &str = "download from Hugging Face…";

/// The hint under the Query field.
pub const QUERY_HINT: &str = "search the Hub or owner/repo";

/// Longest search text, in characters.
const MAX_QUERY: usize = 128;

/// The footer while a request runs.
pub const HINT_ASKING: &str = "asking Hugging Face…";
pub const HINT_QUERY: &str = "Enter search · Esc back";
pub const HINT_LIST: &str = "j/k move · Enter choose · Esc back";
pub const HINT_POPULAR: &str = "j/k move · Enter choose · Esc search";

/// The mark of a repo or a file too big for this PC's memory.
pub const TOO_BIG: &str = "too big for this PC";
pub const HINT_CONFIRM: &str = "y download · n back";
pub const HINT_CANCEL: &str = "cancel the download? (y/n)";

/// `text` for a picker row: control and format characters drawn as `?`.
pub fn shown(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || is_format(c) {
                '?'
            } else {
                c
            }
        })
        .collect()
}

/// A download the runtime is running, as the llamacpp row shows it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Running {
    pub file: String,
    pub license: String,
    /// Bytes on disk.
    pub done: u64,
    pub total: u64,
}

impl Running {
    /// The share done, `0..=100`.
    pub fn percent(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        let done = u128::from(self.done.min(self.total));
        u64::try_from(done * 100 / u128::from(self.total)).unwrap_or(100)
    }

    /// The llamacpp row while it runs: `↓ 42% a-Q4_K_M.gguf · apache-2.0`.
    pub fn label(&self) -> String {
        format!(
            "↓ {}% {} · {}",
            self.percent(),
            shown(&self.file),
            shown(&self.license)
        )
    }
}

/// The confirmation of one choice: everything the user agrees to with `y`.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Confirm {
    pub job: Job,
    pub hub: Option<PathBuf>,
    pub free: Option<u64>,
    /// The disk space error when the free space does not cover the bytes
    /// still missing; `y` does nothing then.
    pub short: Option<String>,
}

/// One Hugging Face screen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Screen {
    /// The text field.
    Query(String),
    /// The repos a search found for `text`, or the popular ones that fit
    /// this PC (`popular`, with no text).
    Repos {
        text: String,
        hits: Vec<RepoHit>,
        popular: bool,
        /// This machine's memory, when known.
        ram: Option<u64>,
    },
    /// The GGUF choices of a repo.
    Files(Box<Listing>),
    Confirm(Box<Confirm>),
}

/// A screen and where its cursor was when a later one opened.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Frame {
    screen: Screen,
    cursor: usize,
    offset: usize,
}

/// The Hugging Face screens opened so far, the current one last.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Hub {
    frames: Vec<Frame>,
    /// The request in flight: keys other than Esc wait for its reply.
    asking: Option<Ask>,
}

impl Hub {
    fn new() -> Self {
        Hub {
            frames: vec![Frame {
                screen: Screen::Query(String::new()),
                cursor: 0,
                offset: 0,
            }],
            asking: None,
        }
    }

    pub fn screen(&self) -> &Screen {
        &self.frames.last().expect("a hub has a screen").screen
    }

    fn screen_mut(&mut self) -> &mut Screen {
        &mut self.frames.last_mut().expect("a hub has a screen").screen
    }

    /// The request in flight, if any.
    pub fn asking(&self) -> Option<&Ask> {
        self.asking.as_ref()
    }

    /// The Query text, when that is the current screen.
    pub fn text(&self) -> Option<&str> {
        match self.screen() {
            Screen::Query(text) => Some(text),
            _ => None,
        }
    }

    /// The title of the current screen.
    pub fn title(&self) -> String {
        match self.screen() {
            Screen::Query(_) => "Download from Hugging Face".to_owned(),
            Screen::Repos { popular: true, .. } => {
                "Hugging Face: most downloaded for this PC".to_owned()
            }
            Screen::Repos { text, .. } => format!("Hugging Face: \"{}\"", shown(text)),
            Screen::Files(listing) => format!("Hugging Face: {}", shown(&listing.files.repo.id)),
            Screen::Confirm(_) => "Hugging Face: download".to_owned(),
        }
    }

    /// The footer of the current screen.
    pub fn hint(&self) -> &'static str {
        if self.asking.is_some() {
            return HINT_ASKING;
        }
        match self.screen() {
            Screen::Query(_) => HINT_QUERY,
            Screen::Repos { popular: true, .. } => HINT_POPULAR,
            Screen::Repos { .. } | Screen::Files(_) => HINT_LIST,
            Screen::Confirm(_) => HINT_CONFIRM,
        }
    }

    /// The rows of the current screen; the Query screen has none (the menu
    /// draws its field).
    pub fn rows(&self) -> Vec<PickRow> {
        match self.screen() {
            Screen::Query(_) => Vec::new(),
            Screen::Repos {
                text,
                hits,
                popular,
                ram,
            } => {
                if hits.is_empty() {
                    return vec![info(if *popular {
                        "no popular model fits this PC; Esc to search".to_owned()
                    } else {
                        format!("no GGUF models found for \"{text}\"")
                    })];
                }
                hits.iter()
                    .enumerate()
                    .map(|(index, hit)| item(index, repo_label(hit, *ram)))
                    .collect()
            }
            Screen::Files(listing) => {
                let files = &listing.files;
                let mut rows = vec![info(format!("license: {}", files.license.id))];
                if files.choices.is_empty() {
                    rows.push(info(format!("no .gguf files in {}", files.repo.id)));
                }
                rows.extend(files.choices.iter().enumerate().map(|(index, choice)| {
                    let big = listing.ram.is_some_and(|ram| !hf::fits(choice.size, ram));
                    item(index, marked(choice.label(), big))
                }));
                rows
            }
            Screen::Confirm(confirm) => {
                let choice = &confirm.job.choice;
                let hub = confirm
                    .hub
                    .as_ref()
                    .map_or_else(|| "no cache folder".to_owned(), |h| h.display().to_string());
                let free = confirm
                    .free
                    .map_or_else(|| "unknown".to_owned(), hf::size_label);
                [
                    format!(
                        "download {} ({})",
                        choice.name(),
                        hf::size_label(choice.size)
                    ),
                    format!("from {}", confirm.job.id),
                    format!("license: {}", confirm.job.license.id),
                    format!("read it: {}", confirm.job.license.link),
                    format!("to: {hub}"),
                    format!("free: {free}"),
                ]
                .iter()
                // A long link or folder is wrapped, not cut.
                .flat_map(|line| wrap_note(&shown(line)))
                .map(info)
                .collect()
            }
        }
    }
}

/// A repo row: `o/r  1200 ↓  ~2.5 GB`, the size estimated from its
/// parameters, marked when it does not fit `ram`.
fn repo_label(hit: &RepoHit, ram: Option<u64>) -> String {
    let label = format!("{}  {} ↓", hit.id, hit.downloads);
    let Some(bytes) = hit.params.map(hf::model_bytes) else {
        return label;
    };
    let big = ram.is_some_and(|ram| !hf::fits(bytes, ram));
    marked(format!("{label}  ~{}", hf::size_label(bytes)), big)
}

/// `label`, with the [`TOO_BIG`] mark when `big`.
fn marked(label: String, big: bool) -> String {
    if big {
        format!("{label} · {TOO_BIG}")
    } else {
        label
    }
}

/// A row that cannot be chosen, with a Hub label.
pub(super) fn info(label: String) -> PickRow {
    PickRow {
        key: PickKey::Info,
        label: shown(&label),
        mark: Mark::Plain,
        enabled: false,
        current: false,
    }
}

/// A repo or a choice, by its index in the reply.
pub(super) fn item(index: usize, label: String) -> PickRow {
    PickRow {
        key: PickKey::Hub(index),
        label: shown(&label),
        mark: Mark::Plain,
        enabled: true,
        current: false,
    }
}

/// Whether `c` may be typed into the Query field: spaces may, control and
/// format characters may not.
fn accepts(c: char) -> bool {
    !c.is_control() && !is_format(c)
}

impl Picker {
    /// The Hugging Face screens, when one is open.
    pub fn hub(&self) -> Option<&Hub> {
        match &self.level {
            Level::Hub(hub) => Some(hub),
            _ => None,
        }
    }

    fn hub_mut(&mut self) -> Option<&mut Hub> {
        match &mut self.level {
            Level::Hub(hub) => Some(hub),
            _ => None,
        }
    }

    /// The download that runs, shown on the llamacpp row. Its end closes a
    /// cancel prompt.
    pub fn set_download(&mut self, download: Option<Running>) {
        if download.is_none() {
            self.cancel_prompt = false;
        }
        self.download = download;
    }

    /// Whether `cancel the download? (y/n)` waits for an answer.
    pub fn asks_cancel(&self) -> bool {
        self.cancel_prompt
    }

    /// The answer to the cancel prompt: `y` cancels, anything else keeps
    /// the download.
    pub fn answer_cancel(&mut self, yes: bool) -> Outcome {
        let asked = std::mem::take(&mut self.cancel_prompt);
        if asked && yes && self.download.is_some() {
            Outcome::CancelDownload
        } else {
            Outcome::Stay
        }
    }

    /// Enter on the llamacpp entry row: the cancel prompt while a download
    /// runs, else the Query screen, with the popular models asked for to
    /// open over it.
    pub(super) fn open_hub(&mut self) -> Outcome {
        if self.download.is_some() {
            self.cancel_prompt = true;
            return Outcome::Stay;
        }
        let mut hub = Hub::new();
        hub.asking = Some(Ask::Popular);
        self.level = Level::Hub(hub);
        self.cursor = 0;
        self.offset = 0;
        Outcome::Ask(Ask::Popular)
    }

    /// Enter on a Hugging Face screen.
    pub(super) fn hub_enter(&mut self) -> Outcome {
        /// What Enter leads to.
        enum Next {
            Ask(Ask),
            Open(Screen),
        }
        let cursor = self.cursor;
        let Some(hub) = self.hub().filter(|hub| hub.asking.is_none()) else {
            return Outcome::Stay;
        };
        let next = match hub.screen() {
            Screen::Query(text) => {
                let text = text.trim();
                if text.is_empty() {
                    return Outcome::Stay;
                }
                if hf::is_repo_id(text) {
                    Next::Ask(Ask::Files(text.to_owned()))
                } else {
                    Next::Ask(Ask::Search(text.to_owned()))
                }
            }
            Screen::Repos { hits, .. } => match hits.get(cursor) {
                Some(hit) => Next::Ask(Ask::Files(hit.id.clone())),
                None => return Outcome::Stay,
            },
            Screen::Files(listing) => {
                // The license row comes first.
                let Some(index) = cursor.checked_sub(1) else {
                    return Outcome::Stay;
                };
                let Some(choice) = listing.files.choices.get(index) else {
                    return Outcome::Stay;
                };
                let need = listing.missing.get(index).copied().unwrap_or(choice.size);
                Next::Open(Screen::Confirm(Box::new(Confirm {
                    job: Job {
                        id: listing.files.repo.id.clone(),
                        commit: listing.files.repo.commit.clone(),
                        choice: choice.clone(),
                        license: listing.files.license.clone(),
                    },
                    hub: listing.hub.clone(),
                    free: listing.free,
                    short: hf::space_check(need, listing.free).err(),
                })))
            }
            Screen::Confirm(_) => return Outcome::Stay,
        };
        match next {
            Next::Ask(ask) => {
                if let Some(hub) = self.hub_mut() {
                    hub.asking = Some(ask.clone());
                }
                Outcome::Ask(ask)
            }
            Next::Open(screen) => {
                self.push(screen);
                Outcome::Stay
            }
        }
    }

    /// `y` on the Confirm screen: the download, once, and back to the
    /// llamacpp model level, whose entry row then shows its progress.
    pub fn hub_download(&mut self, draft: &AiConfig, detection: Option<&Detection>) -> Outcome {
        let Some(Screen::Confirm(confirm)) = self.hub().map(Hub::screen) else {
            return Outcome::Stay;
        };
        if confirm.short.is_some() {
            return Outcome::Stay;
        }
        let job = Box::new(confirm.job.clone());
        self.leave_hub(draft, detection);
        Outcome::Download(job)
    }

    /// Esc on a Hugging Face screen: back one screen, from the first one to
    /// the llamacpp model level. A request in flight is no longer waited
    /// for, so its reply is dropped.
    pub(super) fn hub_back(&mut self, draft: &AiConfig, detection: Option<&Detection>) -> Outcome {
        let Some(hub) = self.hub_mut() else {
            return Outcome::Stay;
        };
        hub.asking = None;
        hub.frames.pop();
        match hub.frames.last() {
            Some(frame) => {
                let (cursor, offset) = (frame.cursor, frame.offset);
                self.cursor = cursor;
                self.offset = offset;
            }
            None => self.leave_hub(draft, detection),
        }
        Outcome::Stay
    }

    /// Back to llamacpp's model level, the cursor on the entry row.
    pub(super) fn leave_hub(&mut self, draft: &AiConfig, detection: Option<&Detection>) {
        self.level = Level::Models {
            engine: "llamacpp".to_owned(),
        };
        let rows = self.rows(draft, detection);
        self.cursor = rows
            .iter()
            .position(|r| r.key == PickKey::HfDownload)
            .unwrap_or(0);
        self.offset = 0;
        self.cancel_prompt = false;
        self.follow();
    }

    /// Opens `screen` over the current one, keeping where its cursor was.
    fn push(&mut self, screen: Screen) {
        let (cursor, offset) = (self.cursor, self.offset);
        let Some(hub) = self.hub_mut() else {
            return;
        };
        if let Some(frame) = hub.frames.last_mut() {
            frame.cursor = cursor;
            frame.offset = offset;
        }
        hub.frames.push(Frame {
            screen,
            cursor: 0,
            offset: 0,
        });
        // The first row that can be chosen.
        let rows = hub.rows();
        self.cursor = rows.iter().position(|r| r.enabled).unwrap_or(0);
        self.offset = 0;
        self.follow();
    }

    /// The reply to `ask`. Kept only while the picker still waits for that
    /// very request; a reply for a screen the user left is dropped. A
    /// failure stays on the screen that asked and comes back as the message
    /// to show.
    pub fn hub_reply(&mut self, ask: &Ask, answer: Result<Answer, String>) -> Option<String> {
        let hub = self.hub_mut()?;
        if hub.asking.as_ref() != Some(ask) {
            return None;
        }
        hub.asking = None;
        match (ask, answer) {
            (_, Err(message)) => return Some(message),
            (Ask::Search(text), Ok(Answer::Repos { mut hits, ram })) => {
                hits.sort_by_key(|hit| std::cmp::Reverse(hit.downloads));
                self.push(Screen::Repos {
                    text: text.clone(),
                    hits,
                    popular: false,
                    ram,
                });
            }
            (Ask::Popular, Ok(Answer::Repos { mut hits, ram })) => {
                // Only what is known to fit; with the memory unknown, all.
                if let Some(ram) = ram {
                    hits.retain(|hit| {
                        hit.params
                            .is_some_and(|params| hf::fits(hf::model_bytes(params), ram))
                    });
                }
                hits.sort_by_key(|hit| std::cmp::Reverse(hit.downloads));
                hits.truncate(hf::SEARCH_LIMIT);
                self.push(Screen::Repos {
                    text: String::new(),
                    hits,
                    popular: true,
                    ram,
                });
            }
            (Ask::Files(_), Ok(Answer::Files(listing))) => {
                self.push(Screen::Files(listing));
            }
            // An answer of the other kind is not a reply to this request.
            _ => {}
        }
        None
    }

    /// Types one character into the Query field; ignored while asking.
    pub(super) fn hub_type(&mut self, c: char) {
        if let Some(hub) = self.hub_mut()
            && hub.asking.is_none()
            && let Screen::Query(text) = hub.screen_mut()
            && accepts(c)
            && text.chars().count() < MAX_QUERY
        {
            text.push(c);
        }
    }

    /// Edits the Query text in place; ignored while asking.
    pub(super) fn hub_edit(&mut self, change: impl FnOnce(&mut String)) {
        if let Some(hub) = self.hub_mut()
            && hub.asking.is_none()
            && let Screen::Query(text) = hub.screen_mut()
        {
            change(text);
        }
    }
}
