//! The Ollama screens of the ollama picker (config-menu, Ollama screens):
//! Popular, Sizes and Confirm, in turn. Pure: a request leaves as
//! [`Outcome::OllamaAsk`] and its reply comes back through
//! [`Picker::ollama_reply`]; the pull leaves as [`Outcome::Pull`] on `y`,
//! and nothing else here starts one or edits the config.

use super::super::Detection;
use super::super::config::AiConfig;
use super::super::hf;
use super::super::marks::{Tools, tool_mark};
use super::super::ollama_lib::{
    self, Answer, Ask, Card, Licenses, Manifest, Pick, Pull, size_params,
};
use super::hub::{HINT_CONFIRM, HINT_LIST, Source, TOO_BIG, info, item, shown};
use super::{Level, Outcome, PickKey, PickRow, Picker, ToolMark, wrap_note};

/// The entry row on ollama's model level.
pub const OLLAMA_ROW: &str = "download from Ollama…";

/// The footer while a request runs.
pub const HINT_ASKING_OLLAMA: &str = "asking ollama.com…";

/// What Enter on an entry row says while the other source downloads.
pub const BUSY: &str = "a download is already running";

/// The info row of the curated fallback.
pub const FALLBACK: &str = "could not read the Ollama library; showing the built-in list";

/// The info row when nothing fits.
pub const NONE_FITS: &str = "no popular model fits this PC";

/// The Confirm screen's folder line when the folder is not known.
const SERVER_FOLDER: &str = "the Ollama server's models folder";

/// One Ollama screen.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Screen {
    /// The popular models that fit this PC, after the fallback's info row
    /// when the library could not be read.
    Popular { picks: Vec<Pick>, fallback: bool },
    /// The size tags of `model`, each with its manifest when it was read.
    Sizes {
        model: String,
        manifests: Vec<(String, Option<Manifest>)>,
        ram: Option<u64>,
        /// Whether the Popular card of `model` declares `tools`.
        tools: bool,
    },
    /// Everything the user agrees to with `y`.
    Confirm(Box<Confirm>),
}

/// The confirmation of one pull.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Confirm {
    pub pull: Pull,
    pub licenses: Licenses,
}

/// A screen and where its cursor was when a later one opened.
#[derive(Clone, PartialEq, Eq, Debug)]
struct Frame {
    screen: Screen,
    cursor: usize,
    offset: usize,
}

/// The Ollama screens opened so far, the current one last. None is open
/// while the Popular list is asked for.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ollama {
    frames: Vec<Frame>,
    /// The request in flight: keys other than Esc wait for its reply.
    asking: Option<Ask>,
}

impl Ollama {
    pub fn screen(&self) -> Option<&Screen> {
        self.frames.last().map(|frame| &frame.screen)
    }

    /// The request in flight, if any.
    pub fn asking(&self) -> Option<&Ask> {
        self.asking.as_ref()
    }

    /// The title of the current screen.
    pub fn title(&self) -> String {
        match self.screen() {
            None | Some(Screen::Popular { .. }) => "Ollama: most popular for this PC".to_owned(),
            Some(Screen::Sizes { model, .. }) => format!("Ollama: {}", shown(model)),
            Some(Screen::Confirm(_)) => "Ollama: download".to_owned(),
        }
    }

    /// The footer of the current screen.
    pub fn hint(&self) -> &'static str {
        if self.asking.is_some() {
            return HINT_ASKING_OLLAMA;
        }
        match self.screen() {
            Some(Screen::Confirm(_)) => HINT_CONFIRM,
            _ => HINT_LIST,
        }
    }

    /// The rows of the current screen.
    pub fn rows(&self) -> Vec<PickRow> {
        match self.screen() {
            None => Vec::new(),
            Some(Screen::Popular { picks, fallback }) => {
                let mut rows = Vec::new();
                if *fallback {
                    rows.push(info(FALLBACK.to_owned()));
                }
                if picks.is_empty() {
                    rows.push(info(NONE_FITS.to_owned()));
                }
                rows.extend(picks.iter().enumerate().map(|(index, pick)| PickRow {
                    tools: pick_mark(pick),
                    ..item(index, pick_label(pick))
                }));
                rows
            }
            Some(Screen::Sizes {
                model,
                manifests,
                ram,
                tools,
            }) => manifests
                .iter()
                .enumerate()
                .map(|(index, (tag, manifest))| PickRow {
                    tools: tag_mark(*tools, tag),
                    ..item(index, size_label(model, tag, manifest, *ram))
                })
                .collect(),
            Some(Screen::Confirm(confirm)) => confirm_lines(confirm)
                .iter()
                // A long link or folder is wrapped, not cut.
                .flat_map(|line| wrap_note(&shown(line)))
                .map(info)
                .collect(),
        }
    }
}

/// Whether the library card lists `tools` among its capabilities.
pub(super) fn declares_tools(card: &Card) -> bool {
    card.capabilities
        .iter()
        .any(|c| c.eq_ignore_ascii_case("tools"))
}

/// The mark of a Popular row: its card declares `tools`, judged by the
/// largest size that fits this PC.
fn pick_mark(pick: &Pick) -> Option<ToolMark> {
    let declared = declares_tools(&pick.card);
    let largest = pick.fitting.iter().filter_map(|t| size_params(t)).max();
    tool_mark(if declared { Tools::Yes } else { Tools::No }, largest)
}

/// The mark of a Sizes row: the model declares `tools`, judged by this tag.
pub(super) fn tag_mark(declared: bool, tag: &str) -> Option<ToolMark> {
    tool_mark(
        if declared { Tools::Yes } else { Tools::No },
        size_params(tag),
    )
}

/// A Popular row: `llama3.2  86.3M pulls  1b 3b`.
fn pick_label(pick: &Pick) -> String {
    let card = &pick.card;
    let sizes = pick.fitting.join(" ");
    if card.pulls.is_empty() {
        format!("{}  {sizes}", card.name)
    } else {
        format!("{}  {} pulls  {sizes}", card.name, card.pulls)
    }
}

/// A Sizes row: `llama3.2:3b  2.0 GB`, marked when it does not fit `ram`.
fn size_label(model: &str, tag: &str, manifest: &Option<Manifest>, ram: Option<u64>) -> String {
    match manifest {
        Some(manifest) => {
            let label = format!("{model}:{tag}  {}", hf::size_label(manifest.size));
            if ram.is_some_and(|ram| !hf::fits(manifest.size, ram)) {
                format!("{label} · {TOO_BIG}")
            } else {
                label
            }
        }
        None => format!("{model}:{tag}  size unknown"),
    }
}

/// The lines of the Confirm screen.
fn confirm_lines(confirm: &Confirm) -> Vec<String> {
    let pull = &confirm.pull;
    let licenses = &confirm.licenses;
    let size = pull
        .size
        .map_or_else(|| "size unknown".to_owned(), hf::size_label);
    let mut lines = vec![format!("download {} ({size})", pull.name())];
    if licenses.titles.is_empty() {
        lines.push(format!("license: {}", ollama_lib::NOT_STATED));
    }
    lines.extend(licenses.titles.iter().map(|t| format!("license: {t}")));
    lines.push(format!(
        "read it: {}",
        ollama_lib::page_url(&pull.model, &pull.tag)
    ));
    lines.push(format!(
        "to: {}",
        licenses
            .folder
            .as_ref()
            .map_or_else(|| SERVER_FOLDER.to_owned(), |f| f.display().to_string())
    ));
    lines.push(format!(
        "free: {}",
        licenses
            .free
            .map_or_else(|| "unknown".to_owned(), hf::size_label)
    ));
    lines
}

impl Picker {
    /// The Ollama screens, when one is open.
    pub fn ollama(&self) -> Option<&Ollama> {
        match &self.level {
            Level::Ollama(ollama) => Some(ollama),
            _ => None,
        }
    }

    fn ollama_mut(&mut self) -> Option<&mut Ollama> {
        match &mut self.level {
            Level::Ollama(ollama) => Some(ollama),
            _ => None,
        }
    }

    /// Enter on the ollama entry row: the cancel prompt while its pull runs,
    /// [`Outcome::Busy`] while a Hugging Face download runs, else the
    /// Popular list asked for.
    pub(super) fn open_ollama(&mut self) -> Outcome {
        match self.download.as_ref().map(|d| d.source) {
            Some(Source::Ollama) => {
                self.cancel_prompt = true;
                return Outcome::Stay;
            }
            Some(Source::HuggingFace) => return Outcome::Busy,
            None => {}
        }
        self.level = Level::Ollama(Ollama {
            frames: Vec::new(),
            asking: Some(Ask::Popular),
        });
        self.cursor = 0;
        self.offset = 0;
        Outcome::OllamaAsk(Ask::Popular)
    }

    /// Enter on an Ollama screen: the sizes of a model, or the licenses of
    /// a size. Nothing is pulled here.
    pub(super) fn ollama_enter(&mut self) -> Outcome {
        let cursor = self.cursor;
        let Some(ollama) = self.ollama().filter(|o| o.asking.is_none()) else {
            return Outcome::Stay;
        };
        let ask = match ollama.screen() {
            Some(Screen::Popular { picks, fallback }) => {
                // The info rows come first.
                let index = cursor.checked_sub(usize::from(*fallback));
                let Some(pick) = index.and_then(|index| picks.get(index)) else {
                    return Outcome::Stay;
                };
                Ask::Sizes {
                    model: pick.card.name.clone(),
                    tags: pick.card.sizes.clone(),
                }
            }
            Some(Screen::Sizes {
                model, manifests, ..
            }) => {
                let Some((tag, manifest)) = manifests.get(cursor) else {
                    return Outcome::Stay;
                };
                Ask::Licenses {
                    model: model.clone(),
                    tag: tag.clone(),
                    digests: manifest.as_ref().map(|m| m.licenses.clone()),
                }
            }
            None | Some(Screen::Confirm(_)) => return Outcome::Stay,
        };
        if let Some(ollama) = self.ollama_mut() {
            ollama.asking = Some(ask.clone());
        }
        Outcome::OllamaAsk(ask)
    }

    /// `y` on the Confirm screen: the pull, once, and back to the ollama
    /// model level, whose entry row then shows its progress.
    pub fn ollama_pull(&mut self, draft: &AiConfig, detection: Option<&Detection>) -> Outcome {
        let Some(ollama) = self.ollama().filter(|o| o.asking.is_none()) else {
            return Outcome::Stay;
        };
        let Some(Screen::Confirm(confirm)) = ollama.screen() else {
            return Outcome::Stay;
        };
        let pull = Box::new(confirm.pull.clone());
        self.leave_ollama(draft, detection);
        Outcome::Pull(pull)
    }

    /// Esc on an Ollama screen: a request in flight is no longer waited
    /// for (its reply is dropped) and the screen stays; else back one
    /// screen, from the first one to the ollama model level.
    pub(super) fn ollama_back(
        &mut self,
        draft: &AiConfig,
        detection: Option<&Detection>,
    ) -> Outcome {
        let Some(ollama) = self.ollama_mut() else {
            return Outcome::Stay;
        };
        if ollama.asking.take().is_none() || ollama.frames.is_empty() {
            ollama.frames.pop();
        }
        match ollama.frames.last() {
            Some(frame) => {
                let (cursor, offset) = (frame.cursor, frame.offset);
                self.cursor = cursor;
                self.offset = offset;
            }
            None => self.leave_ollama(draft, detection),
        }
        Outcome::Stay
    }

    /// Back to ollama's model level, the cursor on the entry row.
    fn leave_ollama(&mut self, draft: &AiConfig, detection: Option<&Detection>) {
        self.level = Level::Models {
            engine: "ollama".to_owned(),
        };
        let rows = self.rows(draft, detection);
        self.cursor = rows
            .iter()
            .position(|r| r.key == PickKey::OllamaDownload)
            .unwrap_or(0);
        self.offset = 0;
        self.cancel_prompt = false;
        self.follow();
    }

    /// Opens `screen` over the current one, keeping where its cursor was.
    fn push_ollama(&mut self, screen: Screen) {
        let (cursor, offset) = (self.cursor, self.offset);
        let Some(ollama) = self.ollama_mut() else {
            return;
        };
        if let Some(frame) = ollama.frames.last_mut() {
            frame.cursor = cursor;
            frame.offset = offset;
        }
        ollama.frames.push(Frame {
            screen,
            cursor: 0,
            offset: 0,
        });
        let rows = ollama.rows();
        self.cursor = rows.iter().position(|r| r.enabled).unwrap_or(0);
        self.offset = 0;
        self.follow();
    }

    /// The reply to `ask`. Kept only while the picker still waits for that
    /// very request; a reply for a screen the user left is dropped. A
    /// failure stays on the screen that asked and comes back as the message
    /// to show.
    pub fn ollama_reply(&mut self, ask: &Ask, answer: Result<Answer, String>) -> Option<String> {
        let ollama = self.ollama_mut()?;
        if ollama.asking.as_ref() != Some(ask) {
            return None;
        }
        ollama.asking = None;
        match (ask, answer) {
            (_, Err(message)) => {
                // With no screen to stay on, back to the model level.
                if ollama.frames.is_empty() {
                    self.level = Level::Models {
                        engine: "ollama".to_owned(),
                    };
                }
                return Some(message);
            }
            (
                Ask::Popular,
                Ok(Answer::Popular {
                    cards,
                    fallback,
                    ram,
                }),
            ) => self.push_ollama(Screen::Popular {
                picks: ollama_lib::popular(cards, ram),
                fallback,
            }),
            (Ask::Sizes { model, .. }, Ok(Answer::Sizes { manifests, ram })) => {
                // Whether the Popular card of this model declares tools.
                let tools = match ollama.screen() {
                    Some(Screen::Popular { picks, .. }) => picks
                        .iter()
                        .find(|p| p.card.name == *model)
                        .is_some_and(|p| declares_tools(&p.card)),
                    _ => false,
                };
                self.push_ollama(Screen::Sizes {
                    model: model.clone(),
                    manifests,
                    ram,
                    tools,
                });
            }
            (Ask::Licenses { model, tag, .. }, Ok(Answer::Licenses(licenses))) => {
                let known = match ollama.screen() {
                    Some(Screen::Sizes { manifests, .. }) => manifests
                        .iter()
                        .find(|(t, _)| t == tag)
                        .and_then(|(_, m)| m.as_ref().map(|m| m.size)),
                    _ => None,
                };
                let pull = Pull {
                    model: model.clone(),
                    tag: tag.clone(),
                    size: known.or(licenses.size),
                    license: ollama_lib::license_label(&licenses.titles),
                };
                self.push_ollama(Screen::Confirm(Box::new(Confirm {
                    pull,
                    licenses: *licenses,
                })));
            }
            // An answer of the other kind is not a reply to this request.
            _ => {}
        }
        None
    }
}
