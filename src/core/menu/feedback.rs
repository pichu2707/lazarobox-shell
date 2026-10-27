//! The `Feedback` form of the menu (config-menu, Feedback): a bug report or
//! an idea, a title and a description, with the environment attached. Pure:
//! Ctrl+S builds the prefilled GitHub new-issue link for the app to copy.
//! Nothing is sent from here: the user opens the link in a browser.
//!
//! The title and the description go through the redaction pass before any
//! link is made; a change stays in the form, so the user sees what was
//! masked before copying again.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::core::ai::chat::composer::Composer;
use crate::core::ai::memory::{redact::redact, route::percent_encode};

/// The most characters of a title.
pub const TITLE_CAP: usize = 120;
/// The most characters of a description.
pub const DESCRIPTION_CAP: usize = 2000;
/// The longest link made: GitHub refuses much longer new-issue URLs.
pub const MAX_LINK: usize = 8000;

pub const TITLE_REQUIRED: &str = "Title is required";
pub const DESCRIPTION_REQUIRED: &str = "Description is required";
pub const REDACTED: &str = "Secrets were masked; review and press Ctrl+S again";
pub const TOO_LONG: &str = "Too long for a link: shorten the description";

/// Where new issues are opened.
const NEW_ISSUE: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/issues/new");

/// What is being sent.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Bug,
    Idea,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Bug => "Bug report",
            Kind::Idea => "Idea or goal",
        }
    }

    /// The issue form it opens, and the id of the form field the
    /// description fills.
    fn form(self) -> (&'static str, &'static str) {
        match self {
            Kind::Bug => ("bug.yml", "what-happened"),
            Kind::Idea => ("idea.yml", "idea"),
        }
    }
}

/// What lazarobox knows of where it runs. Never a path, a name or a key.
#[derive(Clone, PartialEq, Eq, Debug, Default)]
pub struct Facts {
    pub version: String,
    /// `std::env::consts::OS`.
    pub os: String,
    pub wsl: bool,
    /// The shell's program name.
    pub shell: Option<String>,
    /// The terminal, as `TERM_PROGRAM` names it.
    pub terminal: Option<String>,
    /// One `<engine> <status>` per detected engine; `None` before any
    /// detection.
    pub engines: Option<Vec<String>>,
}

impl Facts {
    /// The two lines attached to the issue.
    pub fn text(&self) -> String {
        let mut first = vec![format!("lazarobox {}", self.version)];
        first.push(if self.wsl {
            format!("{} (WSL)", self.os)
        } else {
            self.os.clone()
        });
        first.extend(self.shell.iter().chain(&self.terminal).cloned());
        let engines = match &self.engines {
            Some(engines) if !engines.is_empty() => engines.join(" · "),
            _ => "not detected".to_owned(),
        };
        format!("{}\nengines: {engines}", first.join(" · "))
    }
}

/// The longest shell or terminal name attached.
const NAME_CAP: usize = 40;

/// The facts of this host, from `var` (the environment) and `osrelease`
/// (`/proc/sys/kernel/osrelease`, when there is one). The engines are added
/// by whoever knows them.
pub fn host(
    version: &str,
    os: &str,
    var: impl Fn(&str) -> Option<String>,
    osrelease: Option<&str>,
) -> Facts {
    let wsl = var("WSL_DISTRO_NAME").is_some_and(|v| !v.is_empty())
        || osrelease.is_some_and(|r| r.to_ascii_lowercase().contains("microsoft"));
    let shell = var("SHELL").and_then(|path| name(path.rsplit('/').next().unwrap_or("")));
    let terminal = var("TERM_PROGRAM")
        .and_then(|t| name(&t))
        .or_else(|| var("WT_SESSION").map(|_| "Windows Terminal".to_owned()));
    Facts {
        version: version.to_owned(),
        os: os.to_owned(),
        wsl,
        shell,
        terminal,
        engines: None,
    }
}

/// `text` as an attached name: printable ASCII only, cut to [`NAME_CAP`];
/// `None` when it is empty or holds anything else.
fn name(text: &str) -> Option<String> {
    let text = text.trim();
    let plain = !text.is_empty() && text.bytes().all(|b| b == b' ' || b.is_ascii_graphic());
    plain.then(|| text.chars().take(NAME_CAP).collect())
}

/// The field the keys act on, in Tab order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Kind,
    Title,
    Description,
    Attach,
}

/// What a key did, for the menu to act on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// Consumed; only a redraw.
    Handled,
    /// The link to copy.
    Copy(String),
    /// Esc: back to the row.
    Close,
}

/// The form.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Feedback {
    facts: Facts,
    kind: Kind,
    title: Composer,
    description: Composer,
    attach: bool,
    field: Field,
    error: Option<&'static str>,
}

impl Feedback {
    pub fn new(facts: Facts) -> Self {
        Feedback {
            facts,
            kind: Kind::Bug,
            title: Composer::with_cap(TITLE_CAP),
            description: Composer::with_cap(DESCRIPTION_CAP),
            attach: true,
            field: Field::Kind,
            error: None,
        }
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn field(&self) -> Field {
        self.field
    }

    pub fn attach(&self) -> bool {
        self.attach
    }

    pub fn facts(&self) -> &Facts {
        &self.facts
    }

    pub fn title(&self) -> String {
        self.title.text()
    }

    pub fn description(&self) -> String {
        self.description.text()
    }

    /// The composers, to draw them with their cursor.
    pub fn title_composer(&self) -> &Composer {
        &self.title
    }

    pub fn description_composer(&self) -> &Composer {
        &self.description
    }

    /// Why the last Ctrl+S made no link; cleared by the next edit.
    pub fn error(&self) -> Option<&'static str> {
        self.error
    }

    /// One key. `repeat` marks a held key.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool) -> Outcome {
        let (code, mods) = (key.code, key.modifiers);
        if code == KeyCode::Esc && mods == KeyModifiers::NONE {
            return if repeat {
                Outcome::Handled
            } else {
                Outcome::Close
            };
        }
        let plain = mods - KeyModifiers::SHIFT == KeyModifiers::NONE;
        match code {
            KeyCode::Char('s') if mods == KeyModifiers::CONTROL => {
                if !repeat {
                    return self.link();
                }
            }
            KeyCode::Tab if plain => self.move_field(true),
            KeyCode::BackTab => self.move_field(false),
            KeyCode::Left | KeyCode::Right if plain && self.field == Field::Kind => {
                self.kind = match self.kind {
                    Kind::Bug => Kind::Idea,
                    Kind::Idea => Kind::Bug,
                };
            }
            KeyCode::Enter | KeyCode::Char(' ') if plain && self.field == Field::Attach => {
                if !repeat {
                    self.attach = !self.attach;
                }
            }
            KeyCode::Enter if mods == KeyModifiers::NONE => match self.field {
                Field::Title => self.field = Field::Description,
                Field::Description => self.edit(|c| c.newline()),
                Field::Kind | Field::Attach => {}
            },
            KeyCode::Char(ch) if plain => self.edit(|c| c.insert(ch)),
            KeyCode::Backspace if plain => self.edit(|c| c.backspace()),
            KeyCode::Delete if plain => self.edit(|c| c.delete()),
            KeyCode::Left if plain => self.edit(|c| c.left()),
            KeyCode::Right if plain => self.edit(|c| c.right()),
            KeyCode::Home if plain => self.edit(|c| c.home()),
            KeyCode::End if plain => self.edit(|c| c.end()),
            KeyCode::Up if plain && self.field == Field::Description => self.edit(|c| c.up()),
            KeyCode::Down if plain && self.field == Field::Description => self.edit(|c| c.down()),
            _ => {}
        }
        Outcome::Handled
    }

    /// A paste into the focused text field. The title is one line: a
    /// newline becomes a space.
    pub fn paste(&mut self, text: &str) {
        match self.field {
            Field::Title => {
                let one_line = text.replace(['\r', '\n'], " ");
                self.edit(|c| c.paste(&one_line));
            }
            Field::Description => self.edit(|c| c.paste(text)),
            Field::Kind | Field::Attach => {}
        }
    }

    fn move_field(&mut self, forward: bool) {
        const ORDER: [Field; 4] = [Field::Kind, Field::Title, Field::Description, Field::Attach];
        let at = ORDER.iter().position(|f| *f == self.field).unwrap_or(0);
        let next = if forward {
            at + 1
        } else {
            at + ORDER.len() - 1
        };
        self.field = ORDER[next % ORDER.len()];
    }

    /// An edit of the focused text field; it clears the error.
    fn edit(&mut self, change: impl FnOnce(&mut Composer)) {
        let composer = match self.field {
            Field::Title => &mut self.title,
            Field::Description => &mut self.description,
            Field::Kind | Field::Attach => return,
        };
        change(composer);
        self.error = None;
    }

    /// Ctrl+S: the link, or why there is none.
    fn link(&mut self) -> Outcome {
        let (title, description) = (self.title(), self.description());
        let missing = if title.trim().is_empty() {
            Some((TITLE_REQUIRED, Field::Title))
        } else if description.trim().is_empty() {
            Some((DESCRIPTION_REQUIRED, Field::Description))
        } else {
            None
        };
        if let Some((error, field)) = missing {
            self.error = Some(error);
            self.field = field;
            return Outcome::Handled;
        }
        let (safe_title, safe_description) = (redact(&title).text, redact(&description).text);
        if safe_title != title || safe_description != description {
            replace(&mut self.title, &safe_title);
            replace(&mut self.description, &safe_description);
            self.error = Some(REDACTED);
            return Outcome::Handled;
        }
        let (form, field) = self.kind.form();
        let mut link = format!(
            "{NEW_ISSUE}?template={form}&title={}&{field}={}",
            percent_encode(title.trim()),
            percent_encode(description.trim_end())
        );
        if self.attach {
            link.push_str("&environment=");
            link.push_str(&percent_encode(&self.facts.text()));
        }
        if link.len() > MAX_LINK {
            self.error = Some(TOO_LONG);
            return Outcome::Handled;
        }
        Outcome::Copy(link)
    }
}

/// Puts `text` in `composer`, the cursor at its end.
fn replace(composer: &mut Composer, text: &str) {
    composer.clear();
    composer.paste(text);
}
