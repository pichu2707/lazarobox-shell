//! Config menu: a static descriptor table plus the pure state machine that
//! drives it. The popup only renders `MenuState::rows`, so a new setting is a
//! table entry and never a widget change.

use std::borrow::Cow;

mod account;
pub mod feedback;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use super::{
    ai::{
        Detection,
        config::{AiConfig, NotifyMode, Task, ToolToggle},
        hf::{AccountAsk, AccountReply, Answer, Ask, Job, TokenFacts},
        menu::AiMenu,
        picker::{
            HINT_ASKING, HINT_CANCEL, InstallAsk, InstallReply, Level, Mark, NOT_IN_LIST, Outcome,
            PickKey, PickRow, Picker, QUERY_HINT, Running, Screen, Target,
        },
    },
    config::{BarPosition, Config, ConfigEdit, LLAMACPP_TABLE, MEMORY_TABLE},
    layout::Rect,
};

/// A setting that cycles through an ordered list of values. The value string
/// is both the display text and the TOML value.
pub struct Choice {
    pub table: &'static str,
    pub key: &'static str,
    pub values: &'static [&'static str],
    pub get: fn(&Config) -> usize,
    pub set: fn(&mut Config, usize),
}

pub enum ItemKind {
    Choice(Choice),
    /// An `off`/`on` setting persisted as a TOML boolean (`on` is written
    /// `true`), with an empty table for a root key.
    Toggle(Choice),
    /// An AI assignment row; `None` is `Set all tasks`.
    Assign(Option<Task>),
    /// The AI profile: a static choice (`ai.profile`) that lives in the AI
    /// draft, not in the live [`Config`].
    Profile,
    /// The workspace context flag (`ai.context`): an on/off row that lives in
    /// the AI draft, like [`ItemKind::Profile`].
    Context,
    /// The memory opt-in row: a static `off`/`on` item over the AI draft.
    /// Turning it on without the acknowledgement opens the privacy warning.
    Memory(MemoryRow),
    /// The chat notify mode (`ai.notify`): a Settings row that cycles
    /// auto, desktop, bell and off.
    Notify,
    /// The agent tools flag (`ai.tools.enabled`): an on/off row that lives in
    /// the AI draft, like [`ItemKind::Notify`].
    Tools,
    /// One coding-tools family switch (`ai.tools.<key>`): an on/off row over
    /// the AI draft, found by its toggle and never by position.
    Family(ToolToggle),
    /// The approval mode of the model's commands (`ai.tools.approval`): an
    /// `ask`/`auto`/`run` row over the AI draft. It only reaches later chat opens
    /// and conversation switches, never an open chat or a pending approval.
    Approval,
    /// The notification sound flag (`ai.notify_sound`): an on/off row over
    /// the AI draft.
    NotifySound,
    /// The idle stop of the managed llama-server
    /// (`ai.servers.llamacpp.idle_minutes`): 5, 15, 30 min or never over the
    /// AI draft; a value of the config between them is shown as it is.
    ManagedIdle,
    /// The running version, and the newer one when the update check found it.
    /// Not a setting: `[updates] check` stays config-only. `h`/`l` do nothing
    /// here; Enter acts only when an update is available.
    Updates,
    /// The Hugging Face account: who the saved token belongs to. Enter
    /// connects one (a masked token field), checks it or disconnects it. It
    /// never writes a config edit.
    HfAccount,
    /// The `Feedback` row of the Settings tab: Enter opens the form.
    Feedback,
}

/// The version shown on the Updates row. Tests see a fixed one, so a release
/// bump does not rewrite every snapshot.
#[cfg(not(test))]
const RUNNING_VERSION: &str = env!("CARGO_PKG_VERSION");
#[cfg(test)]
const RUNNING_VERSION: &str = "0.15.3";

/// The descriptor of the `Memory` row; the value is the `enabled` key.
pub struct MemoryRow {
    pub table: &'static str,
    pub key: &'static str,
    pub values: &'static [&'static str],
}

impl ItemKind {
    /// Whether the row edits the AI draft, so it needs the AI section.
    const fn needs_ai(&self) -> bool {
        matches!(self, Self::Notify | Self::NotifySound | Self::ManagedIdle)
    }
}

pub struct Item {
    pub label: &'static str,
    pub kind: ItemKind,
}

pub struct Section {
    pub title: &'static str,
    pub items: &'static [Item],
}

fn position_index(position: BarPosition) -> usize {
    match position {
        BarPosition::Top => 0,
        BarPosition::Bottom => 1,
    }
}

fn position_from(index: usize) -> BarPosition {
    if index == 0 {
        BarPosition::Top
    } else {
        BarPosition::Bottom
    }
}

pub static SECTIONS: &[Section] = &[Section {
    title: "Settings",
    items: &[
        Item {
            label: "Statusline position",
            kind: ItemKind::Choice(Choice {
                table: "statusline",
                key: "position",
                values: &["top", "bottom"],
                get: |c| position_index(c.bars.statusline),
                set: |c, i| c.bars.statusline = position_from(i),
            }),
        },
        Item {
            label: "Tab bar position",
            kind: ItemKind::Choice(Choice {
                table: "tabbar",
                key: "position",
                values: &["top", "bottom"],
                get: |c| position_index(c.bars.tabbar),
                set: |c, i| c.bars.tabbar = position_from(i),
            }),
        },
        Item {
            label: "Mouse",
            kind: ItemKind::Toggle(Choice {
                table: "",
                key: "mouse",
                values: &["off", "on"],
                get: |c| usize::from(c.mouse),
                set: |c, i| c.mouse = i == 1,
            }),
        },
        Item {
            label: "Notify",
            kind: ItemKind::Notify,
        },
        Item {
            label: "Sound",
            kind: ItemKind::NotifySound,
        },
        Item {
            label: "llama-server idle",
            kind: ItemKind::ManagedIdle,
        },
        Item {
            label: "Updates",
            kind: ItemKind::Updates,
        },
        Item {
            label: "Feedback",
            kind: ItemKind::Feedback,
        },
    ],
}];

/// The AI section, attached only by [`MenuState::with_ai`]. The labels of
/// the task rows are the task ids; their key path is `Task::table`. The
/// static rows follow the tasks: `Profile`, `Context`, `Memory`, `Tools`, the five
/// family switches, `Approval` and `Hugging Face`, in that order. `Notify`, `Sound` and
/// `llama-server idle` live in the Settings section.
pub static AI_SECTION: Section = Section {
    title: "AI",
    items: &[
        Item {
            label: "Set all tasks",
            kind: ItemKind::Assign(None),
        },
        Item {
            label: "general",
            kind: ItemKind::Assign(Some(Task::General)),
        },
        Item {
            label: "code",
            kind: ItemKind::Assign(Some(Task::Code)),
        },
        Item {
            label: "chat",
            kind: ItemKind::Assign(Some(Task::Chat)),
        },
        Item {
            label: "review",
            kind: ItemKind::Assign(Some(Task::Review)),
        },
        Item {
            label: "summarize",
            kind: ItemKind::Assign(Some(Task::Summarize)),
        },
        Item {
            label: "Profile",
            kind: ItemKind::Profile,
        },
        Item {
            label: "Context",
            kind: ItemKind::Context,
        },
        Item {
            label: "Memory",
            kind: ItemKind::Memory(MemoryRow {
                table: MEMORY_TABLE,
                key: "enabled",
                values: &["off", "on"],
            }),
        },
        Item {
            label: "Tools",
            kind: ItemKind::Tools,
        },
        Item {
            label: "Search",
            kind: ItemKind::Family(ToolToggle::Search),
        },
        Item {
            label: "Write",
            kind: ItemKind::Family(ToolToggle::Write),
        },
        Item {
            label: "Exec",
            kind: ItemKind::Family(ToolToggle::Exec),
        },
        Item {
            label: "Git",
            kind: ItemKind::Family(ToolToggle::Git),
        },
        Item {
            label: "Git write",
            kind: ItemKind::Family(ToolToggle::GitWrite),
        },
        Item {
            label: "Approval",
            kind: ItemKind::Approval,
        },
        Item {
            label: "Hugging Face",
            kind: ItemKind::HfAccount,
        },
    ],
};

/// A group of sections shown under one label of the tab bar. Sections are
/// not titled when the menu is tabbed: the bar names them.
pub struct Tab {
    pub label: &'static str,
    pub sections: &'static [Section],
    /// The tab only exists when the menu has the AI section.
    pub needs_ai: bool,
    /// The update note and the `*` of the update live on this tab.
    pub note: bool,
    /// The tab reserves a row for the caution of the selected row.
    pub caution: bool,
}

/// Number of entries of [`TABS`].
pub const TAB_COUNT: usize = 2;

/// The tabs of the menu, in bar order: Settings first, then AI. A new tab is
/// one entry here plus a bump of [`TAB_COUNT`]; the compiler enforces the
/// pair.
pub static TABS: [Tab; TAB_COUNT] = [
    Tab {
        label: "Settings",
        sections: SECTIONS,
        needs_ai: false,
        note: true,
        caution: false,
    },
    Tab {
        label: "AI",
        sections: std::slice::from_ref(&AI_SECTION),
        needs_ai: true,
        note: false,
        caution: true,
    },
];

/// The selectable settings, in table order.
fn choices() -> impl Iterator<Item = (&'static Item, &'static Choice)> {
    SECTIONS
        .iter()
        .flat_map(|section| section.items.iter())
        .filter_map(|item| match &item.kind {
            ItemKind::Choice(choice) | ItemKind::Toggle(choice) => Some((item, choice)),
            ItemKind::Assign(_)
            | ItemKind::Profile
            | ItemKind::Context
            | ItemKind::Memory(_)
            | ItemKind::Notify
            | ItemKind::Tools
            | ItemKind::Family(_)
            | ItemKind::Approval
            | ItemKind::NotifySound
            | ItemKind::ManagedIdle
            | ItemKind::Updates
            | ItemKind::HfAccount
            | ItemKind::Feedback => None,
        })
}

/// Rows around the popup body: two borders, the blank row and the footer.
pub const CHROME: u16 = 4;
/// The tab bar takes one row at the top of the body when the menu is tabbed.
pub const TAB_BAR: usize = 1;
/// The caution under the AI tab for a family that can change files or run code.
const CAUTION: &str = "Can change files or run code";
/// Lines of the caution, reserved at the foot of the AI tab so the popup
/// does not change size when it appears.
pub const CAUTION_ROWS: usize = 1;
/// Lines of the update note (see [`MenuState::update_note`]).
pub const NOTE_ROWS: usize = 2;
/// The privacy warning: a title, the facts, a blank row and the prompt.
const WARNING_ROWS: usize = WARNING_TEXT.len() + 3;
/// The body under the tab bar is fixed: the popup never jumps between the
/// tabs, the picker (a title and `VIEW` rows), the custom field and the
/// privacy warning. It is the maximum over every view, so adding a row grows
/// the popup instead of overflowing it. The picker is the binding term (11
/// rows); a test keeps this equal to the tallest view.
pub const VIEW_BODY: usize = view_body();
/// The whole body of the tabbed popup: the bar and the tallest view.
pub const MENU_BODY: usize = TAB_BAR + VIEW_BODY;

/// The tallest view, from the descriptor tables.
const fn view_body() -> usize {
    // The picker (a title and its window), the warning, the custom field (a
    // title and the field).
    let mut rows = 1 + crate::core::ai::picker::VIEW;
    if WARNING_ROWS > rows {
        rows = WARNING_ROWS;
    }
    if 2 > rows {
        rows = 2;
    }
    // A tab: its items, and the update note after a gap when it has one.
    let mut tab = 0;
    while tab < TABS.len() {
        let mut items = 0;
        let mut section = 0;
        while section < TABS[tab].sections.len() {
            items += TABS[tab].sections[section].items.len();
            section += 1;
        }
        if TABS[tab].note {
            items += 1 + NOTE_ROWS;
        }
        if TABS[tab].caution {
            items += CAUTION_ROWS;
        }
        if items > rows {
            rows = items;
        }
        tab += 1;
    }
    rows
}
/// Smallest width that still shows the full popup.
pub const MIN_WIDTH: u16 = 24;

/// Whether a `width` x `height` screen is big enough for the full popup of
/// `rows` body rows (the blank row is the first thing dropped, so the height
/// may lose one). Below it the popup is a one-line fallback.
pub fn fits_size(width: u16, height: u16, rows: usize) -> bool {
    width >= MIN_WIDTH && usize::from(height) >= rows + usize::from(CHROME) - 1
}

/// Widest popup wanted without an AI section.
pub const WANT_WIDTH: u16 = 51;
/// With the AI section the `r refresh` hint needs eleven more columns.
pub const WANT_WIDTH_AI: u16 = 62;

/// `want_w` x `want_h` centred in `area`, clamped to the area.
pub fn popup_area(area: Rect, want_w: u16, want_h: u16) -> Rect {
    let width = want_w.min(area.width);
    let height = want_h.min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// Where the full popup is drawn. The renderer and the click hit-test both
/// read it, so a click finds the row drawn under it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PopupLayout {
    /// The box, borders included.
    pub popup: Rect,
    /// Inside the borders: the tab bar, the body, the blank row, the footer.
    pub inner: Rect,
    /// Body rows: the bar and the tallest view when tabbed, else the rows.
    pub body: usize,
    /// Rows of the update note added under a single-tab body.
    pub extra: usize,
    /// The screen row of body row 0 (under the tab bar when tabbed).
    pub first_row: u16,
}

/// The layout of a popup of `rows` rows on `screen`, or `None` when only
/// the one-line fallback is drawn. `note` is the update note's line count.
pub fn popup_layout(
    screen: Rect,
    rows: usize,
    tabbed: bool,
    ai: bool,
    note: usize,
) -> Option<PopupLayout> {
    // Tabbed, the body is fixed (the bar and the tallest view); with a
    // single tab it is just the rows, as before tabs existed.
    let body = if tabbed {
        MENU_BODY.max(TAB_BAR + rows)
    } else {
        rows
    };
    if !fits_size(screen.width, screen.height, body) {
        return None;
    }
    let want_width = if ai { WANT_WIDTH_AI } else { WANT_WIDTH };
    // Tabbed, the update note lives inside the fixed body. Single tab, it
    // is extra rows: it never makes the popup fall back.
    let extra = if tabbed { 0 } else { note };
    let popup = popup_area(screen, want_width, (body + extra) as u16 + CHROME);
    let inner = Rect {
        x: popup.x + 1,
        y: popup.y + 1,
        width: popup.width.saturating_sub(2),
        height: popup.height.saturating_sub(2),
    };
    let bar = if tabbed { TAB_BAR as u16 } else { 0 };
    Some(PopupLayout {
        popup,
        inner,
        body,
        extra,
        first_row: inner.y + bar,
    })
}

/// What a key did, for the app to act on.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MenuCommand {
    None,
    /// The live config changed: relayout.
    Changed,
    Save,
    Cancel,
    /// Run the engine detection again.
    Refresh,
    /// Put this text on the clipboard (the full reason behind a degraded
    /// engine or a failed model listing).
    Copy(String),
    /// Enter on the Updates row with a newer version known: the app decides
    /// between installing it and showing the command.
    Update,
    /// `r` on the Updates row: ask crates.io now, whatever the cache says.
    /// The answer comes back as a late result of the check.
    CheckUpdate,
    /// Fire one notification now, with the mode and sound flag shown in the
    /// menu (saved or not), ignoring the focus rule.
    TestNotify {
        mode: NotifyMode,
        sound: bool,
    },
    /// Send this request to the Hub; the reply comes back through
    /// [`MenuState::hub_reply`].
    HubAsk(Ask),
    /// Run this llama-server install step; the reply comes back through
    /// [`MenuState::install_reply`].
    Install(InstallAsk),
    /// Start this Hugging Face download (`y` on the Confirm screen).
    Download(Box<Job>),
    /// Stop the Hugging Face download that runs.
    CancelDownload,
    /// Connect, check or disconnect the Hugging Face account; the answer
    /// comes back through [`MenuState::account_reply`].
    HfAccount(AccountAsk),
    /// Open this link in the browser. Only made when the host has an
    /// opener (see [`MenuState::with_opener`]); without one the link is a
    /// [`MenuCommand::Copy`].
    Open(String),
    /// Send this Feedback issue with `gh`; the answer comes back through
    /// [`MenuState::feedback_reply`].
    SendIssue(Box<feedback::Issue>),
}

const HINT_SETTINGS: &str = "j/k move · h/l change · s save · Esc cancel";
const HINT_NOTIFY: &str = "j/k move · h/l change · t test · s save · Esc cancel";
const HINT_TASK: &str = "j/k move · Enter pick · r refresh · s save · Esc cancel";
const HINT_PICKER: &str = "j/k move · Enter choose · r refresh · Esc back · s save";
const HINT_NOTE: &str = "Enter or y copies the full reason · Esc back";
const HINT_ENGINE_REASON: &str = "j/k move · Enter choose · y copies the reason · Esc back";
const HINT_WARNING: &str = "y accept (then s to save) · any other key declines";
/// The privacy warning: the three facts, each line short enough for the AI
/// popup width.
const WARNING_TEXT: [&str; 7] = [
    "Saved notes leave lazarobox's private store (0600).",
    "They become readable by every local process and",
    "every agent that uses engram, and by engram sync",
    "if it is on. lazarobox cannot restrict that.",
    "Notes stay in engram until you delete them, and",
    "secret redaction is best effort.",
    "Once saved, this note is not shown again.",
];
const WARNING_PROMPT: &str = "Turn memory on? (y/n)";
const HINT_FEEDBACK_ROW: &str = "Enter open · j/k move · s save · Esc cancel";
/// The value of the `Feedback` row.
const FEEDBACK_VALUE: &str = "bug report or idea";
const HINT_CUSTOM: &str = "type a model · Enter ok · Ctrl+U clear · Esc back";

/// Why a picker row cannot be chosen: its mark, or the text of an
/// explanation row. A saved model of an engine that is gone has no mark.
fn disabled_reason(row: &PickRow) -> &str {
    match row.mark {
        Mark::DownloadRequired => "download required",
        Mark::Checking => "checking…",
        _ if row.key == PickKey::Info => &row.label,
        _ if matches!(row.key, PickKey::Note { .. }) => &row.label,
        // An engine row is disabled only when chat has no adapter for it.
        Mark::Status(ref status) if matches!(row.key, PickKey::Engine(_)) => status,
        _ => "engine not detected",
    }
}

/// The footer of the popup: the widget only styles it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Footer<'a> {
    Error(&'a str),
    Hint(Cow<'static, str>),
}

/// One line of the popup body.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Row {
    Title(Cow<'static, str>),
    Item {
        label: &'static str,
        value: Option<Cow<'static, str>>,
        selected: bool,
        enabled: bool,
    },
    /// A picker row: the cursor row is `selected`, the current assignment of
    /// the target is `current`, a row that cannot be chosen is not `enabled`.
    Pick {
        label: String,
        mark: Mark,
        selected: bool,
        enabled: bool,
        current: bool,
    },
    /// The custom model field and its buffer.
    Field(String),
    /// A plain line of text (the privacy warning).
    Text(Cow<'static, str>),
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct MenuState {
    selected: Option<usize>,
    tab: usize,
    tab_cursor: [usize; TAB_COUNT],
    original: Config,
    error: Option<String>,
    /// What the last copy did, shown in the footer until the next press.
    copied: Option<String>,
    /// What the last notification test did; shown on the Notify and Sound
    /// rows until the menu closes.
    test_report: Option<String>,
    ai: Option<AiMenu>,
    /// A detection run is in flight.
    checking: bool,
    /// The terminal size at the last key, `None` before the app gave one.
    term: Option<(u16, u16)>,
    /// The newer version the update check found. Only ever shown as text.
    update: Option<String>,
    /// A check asked with `r` is in flight (the `checking…` cue).
    checking_update: bool,
    /// The version this binary is, for the Updates row.
    current: String,
    /// The install command is on the footer, ready to copy.
    command_shown: Option<crate::core::update::Fallback>,
    /// The version an install in this run put on disk; the running binary is
    /// still the old one, so the row says so until the restart.
    installed: Option<String>,
    /// The `Hugging Face` row of the AI tab.
    account: account::Account,
    /// What the Feedback form attaches.
    facts: feedback::Facts,
    /// The Feedback form, while it is open.
    feedback: Option<feedback::Feedback>,
    /// The form is done: the view says what became of it until Esc.
    feedback_done: Option<Done>,
    /// The host has a program that opens links in the browser.
    opener: bool,
}

/// What became of the Feedback form.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Done {
    /// Ctrl+O, with no opener: the link was copied.
    Copied,
    /// Ctrl+O: the link was opened.
    Opened,
    /// Ctrl+S: the issue was created, at this link.
    Sent(String),
    /// Ctrl+S found no `gh`: the link was opened instead.
    NoGhOpened,
    /// Ctrl+S found no `gh` and the host has no opener: the link was
    /// copied instead.
    NoGhCopied,
}

impl MenuState {
    pub fn open(original: &Config) -> Self {
        Self {
            selected: (SECTIONS.iter().any(|s| !s.items.is_empty())).then_some(0),
            tab: 0,
            tab_cursor: [0; TAB_COUNT],
            original: *original,
            error: None,
            test_report: None,
            copied: None,
            ai: None,
            checking: false,
            term: None,
            update: None,
            checking_update: false,
            current: RUNNING_VERSION.to_owned(),
            command_shown: None,
            installed: None,
            account: account::Account::default(),
            facts: feedback::Facts {
                version: RUNNING_VERSION.to_owned(),
                ..feedback::Facts::default()
            },
            feedback: None,
            feedback_done: None,
            opener: false,
        }
    }

    /// Whether the host has a program that opens links in the browser:
    /// Ctrl+O then opens a link, else it copies it.
    pub fn with_opener(mut self, opener: bool) -> Self {
        self.opener = opener;
        self
    }

    /// The browser did not open the last link: the Feedback view says it
    /// was copied instead (the app copies it), and later links are copied.
    pub fn open_failed(&mut self) {
        self.opener = false;
        self.feedback_done = match self.feedback_done.take() {
            Some(Done::Opened) => Some(Done::Copied),
            Some(Done::NoGhOpened) => Some(Done::NoGhCopied),
            done => done,
        };
    }

    /// `url` opened when the host can, else copied.
    fn open_or_copy(&self, url: String) -> MenuCommand {
        if self.opener {
            MenuCommand::Open(url)
        } else {
            MenuCommand::Copy(url)
        }
    }

    /// What the Feedback form attaches: the host and the detected engines.
    pub fn with_feedback(mut self, facts: feedback::Facts) -> Self {
        self.facts = facts;
        self
    }

    /// Whether the selection is on the `Feedback` row.
    fn on_feedback_row(&self) -> bool {
        matches!(self.selected_kind(), Some(ItemKind::Feedback))
    }

    /// Whether the Feedback form, or what became of it, is shown.
    fn feedback_open(&self) -> bool {
        self.feedback.is_some() || self.feedback_done.is_some()
    }

    /// The Feedback view that replaces the rows while it is open.
    fn feedback_rows(&self) -> Option<Vec<Row>> {
        let owned = |lines: &[&str]| lines.iter().map(|l| (*l).to_owned()).collect();
        let lines = match &self.feedback_done {
            Some(Done::Copied) => owned(&feedback::COPIED),
            Some(Done::Opened) => owned(&feedback::OPENED),
            Some(Done::NoGhOpened) => owned(&feedback::NO_GH_OPENED),
            Some(Done::NoGhCopied) => owned(&feedback::NO_GH_COPIED),
            Some(Done::Sent(url)) => vec![feedback::SENT.to_owned(), url.clone()],
            None => self.feedback.as_ref()?.lines(),
        };
        let mut rows = vec![Row::Title(Cow::Borrowed("Feedback"))];
        rows.extend(lines.into_iter().map(|line| Row::Text(Cow::Owned(line))));
        Some(rows)
    }

    /// Keys while the Feedback view is open: every one goes to the form;
    /// once it is done, only Esc (back to the row) does anything, and
    /// Ctrl+O on a sent issue opens its link.
    fn on_feedback_key(&mut self, key: &KeyEvent, repeat: bool) -> MenuCommand {
        if let Some(done) = &self.feedback_done {
            if repeat {
                return MenuCommand::None;
            }
            if key.code == KeyCode::Esc {
                self.feedback_done = None;
            } else if let Done::Sent(url) = done
                && key.code == KeyCode::Char('o')
                && key.modifiers == KeyModifiers::CONTROL
            {
                return self.open_or_copy(url.clone());
            }
            return MenuCommand::None;
        }
        let Some(form) = &mut self.feedback else {
            return MenuCommand::None;
        };
        match form.on_key(key, repeat) {
            feedback::Outcome::Handled => MenuCommand::None,
            feedback::Outcome::Close => {
                self.feedback = None;
                MenuCommand::None
            }
            feedback::Outcome::Send(issue) => MenuCommand::SendIssue(Box::new(issue)),
            feedback::Outcome::Open(link) => {
                self.feedback = None;
                let command = self.open_or_copy(link);
                self.feedback_done = Some(match command {
                    MenuCommand::Open(_) => Done::Opened,
                    _ => Done::Copied,
                });
                command
            }
        }
    }

    /// What became of the Feedback issue being sent. A sent issue shows its
    /// link; a failure keeps the form with the message; without `gh` the
    /// prefilled page opens instead (copied with no opener). An answer no
    /// form waits for changes nothing.
    pub fn feedback_reply(&mut self, reply: feedback::SendReply) -> MenuCommand {
        let Some(form) = self.feedback.as_mut().filter(|form| form.sending()) else {
            return MenuCommand::None;
        };
        match reply {
            feedback::SendReply::Sent(url) => {
                self.feedback = None;
                self.feedback_done = Some(Done::Sent(url));
                MenuCommand::None
            }
            feedback::SendReply::Failed(message) => {
                form.failed(&message);
                MenuCommand::None
            }
            feedback::SendReply::NoGh => {
                let Some(link) = form.fallback() else {
                    return MenuCommand::None;
                };
                self.feedback = None;
                let command = self.open_or_copy(link);
                self.feedback_done = Some(match command {
                    MenuCommand::Open(_) => Done::NoGhOpened,
                    _ => Done::NoGhCopied,
                });
                command
            }
        }
    }

    /// What the runtime found of the Hugging Face tokens when the menu
    /// opened, and the account known from earlier in this session.
    pub fn with_hf(mut self, facts: TokenFacts, name: Option<String>) -> Self {
        self.account = account::Account::new(facts, name);
        self
    }

    /// What the runtime found for an account request of the `Hugging Face`
    /// row; a failure it waited for becomes the footer error.
    pub fn account_reply(&mut self, reply: AccountReply) {
        if let Some(message) = self.account.reply(reply) {
            self.error = Some(message);
        }
    }

    /// Whether the selection is on the `Hugging Face` row.
    fn on_account_row(&self) -> bool {
        matches!(self.selected_kind(), Some(ItemKind::HfAccount))
    }

    /// The token field that replaces the rows while it is open: the token
    /// drawn as one `•` per character, never as typed, and the link to
    /// create one.
    fn account_rows(&self) -> Option<Vec<Row>> {
        let field = self.account.field()?;
        Some(vec![
            Row::Title(Cow::Borrowed(account::TOKEN_TITLE)),
            Row::Field(field),
            Row::Text(Cow::Borrowed(account::TOKEN_HINT)),
            Row::Text(Cow::Borrowed(account::TOKEN_URL_HINT)),
            Row::Text(Cow::Borrowed(account::TOKEN_URL)),
        ])
    }

    /// Keys while the token field is open: every letter is typed, Ctrl+U
    /// clears, Ctrl+O opens the link to create a token, Enter checks the
    /// token and Esc goes back to the row (also while the check runs, which
    /// it stops).
    fn on_account_key(&mut self, key: &KeyEvent, repeat: bool) -> MenuCommand {
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        match key.code {
            KeyCode::Esc if !repeat => {
                if let Some(ask) = self.account.close() {
                    return MenuCommand::HfAccount(ask);
                }
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => self.account.clear(),
            KeyCode::Char('o') if key.modifiers == KeyModifiers::CONTROL => {
                if !repeat {
                    return self.open_or_copy(account::TOKEN_URL.to_owned());
                }
            }
            _ if !plain => {}
            KeyCode::Char(c) => self.account.type_char(c),
            KeyCode::Backspace => self.account.backspace(),
            KeyCode::Enter if !repeat => {
                if let Some(ask) = self.account.submit() {
                    return MenuCommand::HfAccount(ask);
                }
            }
            _ => {}
        }
        MenuCommand::None
    }

    /// Keys while `disconnect <name>? (y/n)` waits: only a plain `y`
    /// disconnects, any other key keeps the token.
    fn on_account_prompt_key(&mut self, key: &KeyEvent, repeat: bool) -> MenuCommand {
        if repeat {
            return MenuCommand::None;
        }
        let yes = key.code == KeyCode::Char('y') && key.modifiers.is_empty();
        self.account
            .answer(yes)
            .map_or(MenuCommand::None, MenuCommand::HfAccount)
    }

    /// Tells the menu a version was installed in this run, if any.
    pub fn with_installed(mut self, installed: Option<String>) -> Self {
        self.installed = installed;
        self
    }

    /// An install finished while the menu is open.
    pub fn set_installed(&mut self, installed: Option<String>) {
        self.installed = installed;
    }

    /// Puts the install command on the footer: Enter or `y` copies it. `why`
    /// is the reason the install did not run.
    pub fn show_update_command(&mut self, why: crate::core::update::Fallback) {
        self.command_shown = Some(why);
    }

    /// Whether the selection is on the Updates row.
    fn on_updates_row(&self) -> bool {
        matches!(self.selected_kind(), Some(ItemKind::Updates))
    }

    /// Tells the menu which version is running (tests; the binary's own
    /// version is the default).
    #[cfg(test)]
    pub fn with_current(mut self, current: &str) -> Self {
        self.current = current.to_owned();
        self
    }

    /// Tells the menu which newer version is known, if any.
    pub fn with_update(mut self, update: Option<String>) -> Self {
        self.update = update;
        self
    }

    /// A late result of the update check, while the menu is open. It also
    /// ends a check asked with `r`.
    pub fn set_update(&mut self, update: Option<String>) {
        self.update = update;
        self.checking_update = false;
    }

    /// Whether `r` may ask for a check: update checks are on and nothing was
    /// installed in this run (the running binary is already the old one).
    fn can_check_update(&self) -> bool {
        self.original.update_check && self.installed.is_none()
    }

    /// `r` on the Updates row: one check at a time.
    fn check_update(&mut self, repeat: bool) -> MenuCommand {
        if repeat || self.checking_update || !self.can_check_update() {
            return MenuCommand::None;
        }
        self.checking_update = true;
        MenuCommand::CheckUpdate
    }

    /// The lines of the update note, shown on the tab that has the note
    /// (Settings). Plain text: it is not a row, takes no key and runs
    /// nothing. Hidden on the other tabs and over the warning and the
    /// pickers, which have their own full-height views.
    pub fn update_note(&self) -> Option<[String; NOTE_ROWS]> {
        let latest = self.update.as_deref()?;
        let on_note_tab = self.tabs().nth(self.tab).is_some_and(|tab| tab.note);
        (on_note_tab && !self.in_sub_level()).then(|| match self.command_shown {
            Some(why) => crate::core::update::fallback_lines(why),
            None => crate::core::update::menu_lines(latest),
        })
    }

    /// The one-line caution of the selected row, for the family switches that
    /// can change files or run code. Plain text: it takes no key.
    pub fn caution(&self) -> Option<&'static str> {
        if self.in_sub_level() {
            return None;
        }
        match self.selected_kind()? {
            ItemKind::Family(ToolToggle::Write | ToolToggle::Exec | ToolToggle::GitWrite) => {
                Some(CAUTION)
            }
            _ => None,
        }
    }

    /// The terminal size, given by the app before each key so the menu knows
    /// whether the popup (or only its one-line fallback) is on screen.
    pub fn set_term(&mut self, cols: u16, rows: u16) {
        self.term = Some((cols, rows));
    }

    /// Where the popup of the current view is drawn on `screen`, or `None`
    /// on the one-line fallback.
    pub fn popup_layout(&self, config: &Config, screen: Rect) -> Option<PopupLayout> {
        let tabbed = self.tab_labels().len() > 1;
        let note = self.update_note().map_or(0, |lines| lines.len());
        popup_layout(screen, self.rows(config).len(), tabbed, self.has_ai(), note)
    }

    /// A left click on the screen cell `(column, row)` of the size given by
    /// [`Self::set_term`]: on the link line of the token field, or of a
    /// sent Feedback issue, it does what Ctrl+O does; anywhere else, or on
    /// the one-line fallback, nothing.
    pub fn on_click(&self, config: &Config, column: u16, row: u16) -> MenuCommand {
        let Some((width, height)) = self.term else {
            return MenuCommand::None;
        };
        let screen = Rect {
            x: 0,
            y: 0,
            width,
            height,
        };
        let Some(layout) = self.popup_layout(config, screen) else {
            return MenuCommand::None;
        };
        let inner = layout.inner;
        let inside = (inner.x..inner.x + inner.width).contains(&column)
            && (layout.first_row..inner.y + inner.height).contains(&row);
        if !inside {
            return MenuCommand::None;
        }
        let index = usize::from(row - layout.first_row);
        match self.link_at(config, index) {
            Some(url) => self.open_or_copy(url),
            None => MenuCommand::None,
        }
    }

    /// The link drawn on body row `index`, if that row is one: the token
    /// field's link to create a token, or a sent Feedback issue's link.
    fn link_at(&self, config: &Config, index: usize) -> Option<String> {
        let link = if self.account.field().is_some() {
            account::TOKEN_URL
        } else if let Some(Done::Sent(url)) = &self.feedback_done {
            url
        } else {
            return None;
        };
        let on_link = matches!(
            self.rows(config).get(index),
            Some(Row::Text(text)) if text == link
        );
        on_link.then(|| link.to_owned())
    }

    /// Whether the privacy warning is readable in full: the popup fits the
    /// terminal. An unknown size counts as fitting.
    pub fn fits_warning(&self) -> bool {
        self.term
            .is_none_or(|(cols, rows)| fits_size(cols, rows, MENU_BODY))
    }

    /// The Hugging Face download that runs when the menu opens.
    pub fn with_download(mut self, download: Option<Running>) -> Self {
        self.set_download(download);
        self
    }

    /// The Hugging Face download that runs, `None` once it ended.
    pub fn set_download(&mut self, download: Option<Running>) {
        if let Some(ai) = &mut self.ai {
            ai.set_download(download);
        }
    }

    /// A reply of the Hub. The picker keeps it only while it waits for that
    /// request; a failure it waited for becomes the footer error.
    pub fn hub_reply(&mut self, ask: Ask, answer: Result<Answer, String>) {
        let error = self.ai.as_mut().and_then(|ai| ai.hub_reply(&ask, answer));
        if let Some(message) = error {
            self.error = Some(message);
        }
    }

    /// The result of install request `id`. The wizard keeps it only while it
    /// waits for that request.
    pub fn install_reply(&mut self, id: u64, answer: Result<InstallReply, String>) {
        if let Some(ai) = &mut self.ai {
            ai.install_reply(id, answer);
        }
    }

    /// The bytes on disk of the install download of request `id`.
    pub fn install_progress(&mut self, id: u64, done: u64) {
        if let Some(ai) = &mut self.ai {
            ai.install_progress(id, done);
        }
    }

    /// Whether the open install wizard waits on a download, which closing
    /// the menu must stop.
    pub fn install_downloading(&self) -> bool {
        self.ai.as_ref().is_some_and(AiMenu::install_downloading)
    }

    /// The request the open install wizard waits on. Tests only.
    #[cfg(test)]
    pub fn install_asking(&self) -> Option<crate::core::ai::picker::InstallAsk> {
        self.ai
            .as_ref()
            .and_then(AiMenu::picker)
            .and_then(|p| p.install())
            .and_then(|w| w.asking())
    }

    /// A frame drew the wizard: its prompts now accept `y`/`n`.
    pub fn install_arm(&mut self) {
        if let Some(ai) = &mut self.ai {
            ai.install_arm();
        }
    }

    /// Opens the install wizard over the open picker. Tests only.
    #[cfg(test)]
    pub fn open_install_for_test(&mut self, variants: Vec<crate::core::ai::install::Variant>) {
        if let Some(ai) = &mut self.ai {
            ai.open_install(variants);
        }
    }

    /// Whether the install wizard is open. Tests only.
    #[cfg(test)]
    pub fn install_open(&self) -> bool {
        self.ai
            .as_ref()
            .and_then(AiMenu::picker)
            .is_some_and(|p| p.install().is_some())
    }

    /// Adds the AI section, editing a draft of `ai` against `engines`.
    pub fn with_ai(mut self, ai: AiConfig, engines: Option<Detection>) -> Self {
        self.ai = Some(AiMenu::new(ai, engines));
        self
    }

    /// Marks a detection run as in flight (the `refreshing…` cue).
    pub fn set_checking(&mut self, on: bool) {
        self.checking = on;
    }

    /// Replaces the detection in place; selection and draft stay.
    pub fn set_engines(&mut self, engines: Option<Detection>) {
        self.checking = false;
        if let Some(ai) = &mut self.ai {
            ai.set_engines(engines);
        }
    }

    /// The AI choice being edited (unset without an AI section).
    pub fn ai_draft(&self) -> AiConfig {
        self.ai
            .as_ref()
            .map(|ai| ai.draft().clone())
            .unwrap_or_default()
    }

    /// The tabs this menu shows, in table order. The AI tab needs the AI
    /// section.
    pub fn tabs(&self) -> impl Iterator<Item = &'static Tab> + use<> {
        let has_ai = self.ai.is_some();
        TABS.iter().filter(move |tab| has_ai || !tab.needs_ai)
    }

    /// The labels of the tab bar. The tab with the update note wears a `*`
    /// while an update waits, like the `* Updates` row.
    pub fn tab_labels(&self) -> Vec<String> {
        let waiting = self.update.is_some() && self.installed.is_none();
        self.tabs()
            .map(|tab| {
                if tab.note && waiting {
                    format!("{}*", tab.label)
                } else {
                    tab.label.to_owned()
                }
            })
            .collect()
    }

    /// The index of the active tab among [`MenuState::tabs`].
    pub fn active_tab(&self) -> usize {
        self.tab
    }

    /// Whether the menu is in a view that replaces the rows: the privacy
    /// warning, a picker level or the custom field. The tab keys are inert
    /// there.
    pub fn in_sub_level(&self) -> bool {
        self.in_warning()
            || self.ai.as_ref().is_some_and(|ai| ai.picker().is_some())
            || self.account.field().is_some()
            || self.feedback_open()
    }

    /// Whether the menu has an AI section.
    pub fn has_ai(&self) -> bool {
        self.ai.is_some()
    }

    /// The rows of the active tab, in table order. The chat bell rows edit
    /// the AI draft, so a menu without an AI section has none of them.
    fn tab_items(&self) -> impl Iterator<Item = &'static Item> + use<> {
        let has_ai = self.ai.is_some();
        self.tabs()
            .nth(self.tab)
            .into_iter()
            .flat_map(|tab| tab.sections.iter())
            .flat_map(|section| section.items.iter())
            .filter(move |item| has_ai || !item.kind.needs_ai())
    }

    /// The descriptor kind under the selection, in the active tab. Rows are
    /// found by kind, never by a position in a table, so a row can be added
    /// there without touching the key handling.
    fn selected_kind(&self) -> Option<&'static ItemKind> {
        self.tab_items().nth(self.selected?).map(|item| &item.kind)
    }

    /// Whether the selection is on an AI task row (the ones with a picker).
    pub fn on_ai_row(&self) -> bool {
        matches!(self.selected_kind(), Some(ItemKind::Assign(_)))
    }

    /// Whether the selection is on a row of the AI draft that is not a
    /// task: `Profile`, `Context`, `Memory`, `Notify`, `Tools`, a family switch
    /// or `Approval`.
    fn on_draft_row(&self) -> bool {
        matches!(
            self.selected_kind(),
            Some(
                ItemKind::Profile
                    | ItemKind::Context
                    | ItemKind::Memory(_)
                    | ItemKind::Notify
                    | ItemKind::Tools
                    | ItemKind::Family(_)
                    | ItemKind::Approval
                    | ItemKind::NotifySound
                    | ItemKind::ManagedIdle
            )
        )
    }

    /// Whether the selection is on the Notify or the Sound row.
    fn on_notify_row(&self) -> bool {
        matches!(
            self.selected_kind(),
            Some(ItemKind::Notify | ItemKind::NotifySound)
        )
    }

    fn test_notify(&self) -> MenuCommand {
        match &self.ai {
            Some(ai) => MenuCommand::TestNotify {
                mode: ai.draft().notify(),
                sound: ai.draft().notify_sound(),
            },
            None => MenuCommand::None,
        }
    }

    /// Whether the privacy warning is shown over the menu.
    pub fn in_warning(&self) -> bool {
        self.ai.as_ref().is_some_and(AiMenu::warning)
    }

    /// The privacy warning that replaces the rows while it is open.
    fn warning_rows() -> Vec<Row> {
        let mut rows = vec![Row::Title(Cow::Borrowed("Memory: save notes to engram"))];
        rows.extend(WARNING_TEXT.iter().map(|l| Row::Text(Cow::Borrowed(*l))));
        rows.push(Row::Text(Cow::Borrowed("")));
        rows.push(Row::Title(Cow::Borrowed(WARNING_PROMPT)));
        rows
    }

    /// Keys while the warning is open: only an unmodified lowercase `y`
    /// accepts, everything else declines, and Esc never leaves the menu.
    /// When the popup does not fit the facts are hidden behind the one-line
    /// fallback, so nothing accepts then (informed consent). Repeat events
    /// are ignored.
    fn on_warning_key(&mut self, key: &KeyEvent, repeat: bool) -> MenuCommand {
        if repeat {
            return MenuCommand::None;
        }
        let accept =
            self.fits_warning() && key.code == KeyCode::Char('y') && key.modifiers.is_empty();
        if let Some(ai) = &mut self.ai {
            ai.answer_warning(accept);
        }
        MenuCommand::None
    }

    /// Selectable rows: those of the active tab.
    fn selectable(&self) -> usize {
        self.tab_items().count()
    }

    /// The picker view: a title and the window of rows around the cursor.
    fn picker_rows(&self) -> Option<Vec<Row>> {
        let ai = self.ai.as_ref()?;
        let picker = ai.picker()?;
        let all = ai.pick_rows();
        let row = match picker.target {
            Target::All => "Set all tasks",
            Target::Task(task) => task.id(),
        };
        let mut title = match &picker.level {
            Level::Engines => format!("Engine for {row}"),
            Level::Models { engine } | Level::Custom { engine, .. } => {
                format!("Model for {row} ({engine})")
            }
            Level::Hub(hub) => hub.title(),
            Level::Install(wizard) => wizard.title(),
        };
        if let Some(counter) = picker.counter(all.len()) {
            title = format!("{title} {counter}");
        }
        let mut rows = vec![Row::Title(Cow::Owned(title))];
        if let Some(text) = picker.text() {
            rows.push(Row::Field(text.to_owned()));
            return Some(rows);
        }
        if let Some(text) = picker.hub().and_then(|hub| hub.text()) {
            rows.push(Row::Field(text.to_owned()));
            rows.push(Row::Text(Cow::Borrowed(QUERY_HINT)));
            return Some(rows);
        }
        for index in picker.visible(all.len()) {
            let pick = &all[index];
            rows.push(Row::Pick {
                label: pick.label.clone(),
                mark: pick.mark.clone(),
                selected: index == picker.cursor(),
                enabled: pick.enabled,
                current: pick.current,
            });
        }
        Some(rows)
    }

    /// The target of the selected AI row.
    fn selected_target(&self) -> Option<Target> {
        match self.selected_kind()? {
            ItemKind::Assign(None) => Some(Target::All),
            ItemKind::Assign(Some(task)) => Some(Target::Task(*task)),
            ItemKind::Choice(_)
            | ItemKind::Toggle(_)
            | ItemKind::Profile
            | ItemKind::Context
            | ItemKind::Memory(_)
            | ItemKind::Notify
            | ItemKind::Tools
            | ItemKind::Family(_)
            | ItemKind::Approval
            | ItemKind::NotifySound
            | ItemKind::ManagedIdle
            | ItemKind::Updates
            | ItemKind::HfAccount
            | ItemKind::Feedback => None,
        }
    }

    /// The Updates row: the running version, and the newer one when known.
    /// The name carries a `*` while an update is waiting.
    fn updates_row(&self, selected: bool) -> Row {
        let (label, value) = match (&self.installed, &self.update) {
            (Some(done), _) => ("Updates", crate::core::update::installed_value(done)),
            (None, Some(_)) if self.checking_update => ("* Updates", "checking…".to_owned()),
            (None, Some(latest)) => ("* Updates", format!("{latest} available")),
            (None, None) if self.checking_update => ("Updates", "checking…".to_owned()),
            (None, None) => ("Updates", self.current.clone()),
        };
        Row::Item {
            label,
            value: Some(Cow::Owned(value)),
            selected,
            enabled: true,
        }
    }

    pub fn rows(&self, config: &Config) -> Vec<Row> {
        if self.in_warning() {
            return Self::warning_rows();
        }
        if let Some(rows) = self.picker_rows() {
            return rows;
        }
        if let Some(rows) = self.account_rows() {
            return rows;
        }
        if let Some(rows) = self.feedback_rows() {
            return rows;
        }
        let mut rows = Vec::new();
        let mut index = 0;
        // With tabs the bar names the section; a title only stays when the
        // menu has a single tab (no bar is drawn then).
        let titled = self.tabs().count() == 1;
        let sections = self
            .tabs()
            .nth(self.tab)
            .map_or(&[][..], |tab| tab.sections);
        for section in sections {
            if titled {
                rows.push(Row::Title(Cow::Borrowed(section.title)));
            }
            for item in section
                .items
                .iter()
                .filter(|item| self.ai.is_some() || !item.kind.needs_ai())
            {
                rows.push(match &item.kind {
                    ItemKind::Assign(task) => {
                        let row = Row::Item {
                            label: item.label,
                            value: self.ai.as_ref().map(|ai| Cow::Owned(ai.value(*task))),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Profile => {
                        let row = Row::Item {
                            label: item.label,
                            value: self
                                .ai
                                .as_ref()
                                .map(|ai| Cow::Borrowed(ai.draft().profile().id())),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Context => {
                        let row = Row::Item {
                            label: item.label,
                            value: self.ai.as_ref().map(|ai| {
                                Cow::Borrowed(if ai.draft().context() { "on" } else { "off" })
                            }),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Memory(memory) => {
                        let on = self
                            .ai
                            .as_ref()
                            .is_some_and(|ai| ai.draft().memory.active());
                        let row = Row::Item {
                            label: item.label,
                            value: Some(Cow::Borrowed(memory.values[usize::from(on)])),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::NotifySound => {
                        let row = Row::Item {
                            label: item.label,
                            value: self.ai.as_ref().map(|ai| {
                                Cow::Borrowed(if ai.draft().notify_sound() {
                                    "on"
                                } else {
                                    "off"
                                })
                            }),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Tools => {
                        let row = Row::Item {
                            label: item.label,
                            value: self.ai.as_ref().map(|ai| {
                                Cow::Borrowed(if ai.draft().tools_enabled() {
                                    "on"
                                } else {
                                    "off"
                                })
                            }),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Family(toggle) => {
                        let row = Row::Item {
                            label: item.label,
                            value: self.ai.as_ref().map(|ai| {
                                Cow::Borrowed(if ai.draft().toggle(*toggle) {
                                    "on"
                                } else {
                                    "off"
                                })
                            }),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::ManagedIdle => {
                        let row = Row::Item {
                            label: item.label,
                            value: self.ai.as_ref().map(|ai| {
                                match ai.draft().managed_options().idle_minutes {
                                    0 => Cow::Borrowed("never"),
                                    minutes => Cow::Owned(format!("{minutes} min")),
                                }
                            }),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Approval => {
                        let row = Row::Item {
                            label: item.label,
                            value: self
                                .ai
                                .as_ref()
                                .map(|ai| Cow::Borrowed(ai.draft().approval().id())),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Notify => {
                        let row = Row::Item {
                            label: item.label,
                            value: self
                                .ai
                                .as_ref()
                                .map(|ai| Cow::Borrowed(ai.draft().notify().id())),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Updates => {
                        let row = self.updates_row(self.selected == Some(index));
                        index += 1;
                        row
                    }
                    ItemKind::HfAccount => {
                        let row = Row::Item {
                            label: item.label,
                            value: Some(Cow::Owned(self.account.value())),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Feedback => {
                        let row = Row::Item {
                            label: item.label,
                            value: Some(Cow::Borrowed(FEEDBACK_VALUE)),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                    ItemKind::Choice(choice) | ItemKind::Toggle(choice) => {
                        let row = Row::Item {
                            label: item.label,
                            value: Some(Cow::Borrowed(choice.values[(choice.get)(config)])),
                            selected: self.selected == Some(index),
                            enabled: true,
                        };
                        index += 1;
                        row
                    }
                });
            }
        }
        rows
    }

    /// Interprets a key. Release is ignored; a repeat moves and cycles but
    /// never saves or cancels; a pending error is cleared by the next press.
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool, config: &mut Config) -> MenuCommand {
        if key.kind == KeyEventKind::Release {
            return MenuCommand::None;
        }
        if !repeat {
            self.error = None;
            self.copied = None;
            self.test_report = None;
            // Enter and `y` on the row copy the shown command; any other
            // press hides it.
            let copies = self.command_shown.is_some()
                && self.on_updates_row()
                && matches!(key.code, KeyCode::Enter | KeyCode::Char('y'));
            if !copies {
                self.command_shown = None;
            }
        }
        if self.in_warning() {
            return self.on_warning_key(key, repeat);
        }
        if self.account.prompt().is_some() {
            return self.on_account_prompt_key(key, repeat);
        }
        if self.account.field().is_some() {
            return self.on_account_key(key, repeat);
        }
        if self.feedback_open() {
            return self.on_feedback_key(key, repeat);
        }
        if self.ai.as_ref().is_some_and(|ai| ai.picker().is_some()) {
            return self.on_picker_key(key, repeat);
        }
        if !key.modifiers.difference(KeyModifiers::SHIFT).is_empty() {
            return MenuCommand::None;
        }
        match key.code {
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => self.step_tab(false),
            KeyCode::Tab => self.step_tab(true),
            KeyCode::BackTab => self.step_tab(false),
            KeyCode::Char('j') | KeyCode::Down => self.step_selection(true),
            KeyCode::Char('k') | KeyCode::Up => self.step_selection(false),
            KeyCode::Char('l' | ' ') | KeyCode::Right => self.cycle(true, config),
            KeyCode::Char('h') | KeyCode::Left => self.cycle(false, config),
            KeyCode::Char('s') if !repeat => MenuCommand::Save,
            KeyCode::Char('r') if self.on_updates_row() => self.check_update(repeat),
            KeyCode::Char('r') if !repeat && self.on_ai_row() => MenuCommand::Refresh,
            KeyCode::Char('t') if !repeat && self.on_notify_row() => self.test_notify(),
            KeyCode::Enter | KeyCode::Char('y') if !repeat && self.command_shown.is_some() => {
                MenuCommand::Copy(crate::core::update::INSTALL_COMMAND.to_owned())
            }
            KeyCode::Enter if self.on_updates_row() => {
                if !repeat && self.update.is_some() && self.installed.is_none() {
                    MenuCommand::Update
                } else {
                    MenuCommand::None
                }
            }
            KeyCode::Enter if !repeat && self.on_ai_row() => self.open_picker(),
            KeyCode::Char('d') if !repeat && self.on_account_row() => {
                self.account.forget();
                MenuCommand::None
            }
            KeyCode::Enter if self.on_feedback_row() => {
                if !repeat {
                    self.feedback = Some(feedback::Feedback::new(self.facts.clone()));
                }
                MenuCommand::None
            }
            KeyCode::Enter if self.on_account_row() => match self.account.enter() {
                Some(ask) if !repeat => MenuCommand::HfAccount(ask),
                _ => MenuCommand::None,
            },
            KeyCode::Enter if repeat && (self.on_ai_row() || self.on_draft_row()) => {
                MenuCommand::None
            }
            KeyCode::Enter => self.cycle(true, config),
            KeyCode::Esc if !repeat => MenuCommand::Cancel,
            _ => MenuCommand::None,
        }
    }

    fn open_picker(&mut self) -> MenuCommand {
        if let (Some(target), Some(ai)) = (self.selected_target(), &mut self.ai) {
            ai.open_picker(target);
        }
        MenuCommand::None
    }

    /// Keys while the picker is open: nothing reaches the task list, and Esc
    /// never cancels the menu.
    fn on_picker_key(&mut self, key: &KeyEvent, repeat: bool) -> MenuCommand {
        let Some(ai) = &mut self.ai else {
            return MenuCommand::None;
        };
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        if ai.picker().is_some_and(Picker::asks_cancel) {
            // Only a plain `y` cancels; any other key keeps the download.
            if repeat {
                return MenuCommand::None;
            }
            let yes = plain && key.code == KeyCode::Char('y');
            return match ai.drive(|p, _, _| p.answer_cancel(yes)) {
                Outcome::CancelDownload => MenuCommand::CancelDownload,
                _ => MenuCommand::None,
            };
        }
        if ai.picker().is_some_and(|p| p.hub().is_some()) {
            return Self::on_hub_key(ai, key, repeat);
        }
        if ai.picker().is_some_and(|p| p.install().is_some()) {
            return Self::on_install_key(ai, key, repeat);
        }
        // The custom field is checked before the modifier guard: Ctrl+U
        // clears it, and every letter, shortcuts included, is typed.
        if ai.picker().is_some_and(|p| p.text().is_some()) {
            match key.code {
                KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                    ai.edit(Picker::clear_text);
                }
                _ if !plain => {}
                KeyCode::Char(c) => ai.edit(|p| p.type_char(c)),
                KeyCode::Backspace => ai.edit(Picker::backspace),
                KeyCode::Enter if !repeat => {
                    ai.drive(Picker::enter);
                }
                KeyCode::Esc if !repeat => {
                    ai.drive(Picker::back);
                }
                _ => {}
            }
            return MenuCommand::None;
        }
        if !plain {
            return MenuCommand::None;
        }
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => ai.edit_with(|p, d, e| p.step(true, d, e)),
            KeyCode::Char('k') | KeyCode::Up => ai.edit_with(|p, d, e| p.step(false, d, e)),
            KeyCode::Enter if !repeat => match ai.drive(Picker::enter) {
                Outcome::Copy(text) => return MenuCommand::Copy(text),
                // The Hugging Face entry row asks for the popular models.
                Outcome::Ask(ask) => return MenuCommand::HubAsk(ask),
                _ => {}
            },
            KeyCode::Char('y') if !repeat => {
                if let Outcome::Copy(text) = ai.drive(|p, d, e| p.copy(d, e)) {
                    return MenuCommand::Copy(text);
                }
            }
            KeyCode::Esc if !repeat => {
                ai.drive(Picker::back);
            }
            KeyCode::Char('s') if !repeat => return MenuCommand::Save,
            KeyCode::Char('r')
                if !repeat && ai.drive(|p, _, _| p.refresh()) == Outcome::Refresh =>
            {
                return MenuCommand::Refresh;
            }
            _ => {}
        }
        MenuCommand::None
    }

    /// Keys on the install wizard. Esc goes back one screen (a download asks
    /// first); `y` and `n` answer the step a screen proposes; Enter chooses
    /// a build. No key here saves or edits the config.
    fn on_install_key(ai: &mut AiMenu, key: &KeyEvent, repeat: bool) -> MenuCommand {
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        let outcome = match key.code {
            KeyCode::Esc if !repeat => ai.drive(Picker::back),
            _ if !plain => Outcome::Stay,
            KeyCode::Char('j') | KeyCode::Down => {
                ai.edit_with(|p, d, e| p.step(true, d, e));
                Outcome::Stay
            }
            KeyCode::Char('k') | KeyCode::Up => {
                ai.edit_with(|p, d, e| p.step(false, d, e));
                Outcome::Stay
            }
            _ if repeat => Outcome::Stay,
            KeyCode::Enter => ai.drive(Picker::enter),
            KeyCode::Char('y') => ai.drive(|p, d, e| p.install_answer(true, d, e)),
            KeyCode::Char('n') => ai.drive(|p, d, e| p.install_answer(false, d, e)),
            _ => Outcome::Stay,
        };
        match outcome {
            Outcome::Install(ask) => MenuCommand::Install(ask),
            Outcome::Copy(text) => MenuCommand::Copy(text),
            Outcome::Ask(ask) => MenuCommand::HubAsk(ask),
            _ => MenuCommand::None,
        }
    }

    /// Keys on the Hugging Face screens. Esc goes back one screen, also
    /// while a request runs; every other key waits for its reply. Only `y`
    /// on the Confirm screen downloads; no key here saves or edits the
    /// config.
    fn on_hub_key(ai: &mut AiMenu, key: &KeyEvent, repeat: bool) -> MenuCommand {
        let Some(hub) = ai.picker().and_then(Picker::hub) else {
            return MenuCommand::None;
        };
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        if key.code == KeyCode::Esc {
            if !repeat {
                ai.drive(Picker::back);
            }
            return MenuCommand::None;
        }
        if hub.asking().is_some() {
            return MenuCommand::None;
        }
        let outcome = match (hub.screen(), key.code) {
            (Screen::Query(_), KeyCode::Char('u')) if key.modifiers == KeyModifiers::CONTROL => {
                ai.edit(Picker::clear_text);
                Outcome::Stay
            }
            _ if !plain => Outcome::Stay,
            (Screen::Query(_), KeyCode::Char(c)) => {
                ai.edit(|p| p.type_char(c));
                Outcome::Stay
            }
            (Screen::Query(_), KeyCode::Backspace) => {
                ai.edit(Picker::backspace);
                Outcome::Stay
            }
            (Screen::Query(_), _) if key.code != KeyCode::Enter => Outcome::Stay,
            (_, KeyCode::Char('j') | KeyCode::Down) => {
                ai.edit_with(|p, d, e| p.step(true, d, e));
                Outcome::Stay
            }
            (_, KeyCode::Char('k') | KeyCode::Up) => {
                ai.edit_with(|p, d, e| p.step(false, d, e));
                Outcome::Stay
            }
            _ if repeat => Outcome::Stay,
            (_, KeyCode::Enter) => ai.drive(Picker::enter),
            (Screen::Confirm(_), KeyCode::Char('y')) => ai.drive(Picker::hub_download),
            (Screen::Confirm(_), KeyCode::Char('n')) => ai.drive(Picker::back),
            _ => Outcome::Stay,
        };
        match outcome {
            Outcome::Ask(ask) => MenuCommand::HubAsk(ask),
            Outcome::Download(job) => MenuCommand::Download(job),
            _ => MenuCommand::None,
        }
    }

    /// Moves to the next (or previous) tab, wrapping. Each tab remembers its
    /// own cursor; the active tab's lives in `selected`, so it is saved on
    /// the way out and loaded on the way in. A single tab does nothing.
    fn step_tab(&mut self, forward: bool) -> MenuCommand {
        let count = self.tabs().count();
        if count < 2 {
            return MenuCommand::None;
        }
        if let Some(selected) = self.selected {
            self.tab_cursor[self.tab] = selected;
        }
        self.tab = if forward {
            (self.tab + 1) % count
        } else {
            (self.tab + count - 1) % count
        };
        let rows = self.selectable();
        self.selected = (rows > 0).then(|| self.tab_cursor[self.tab].min(rows - 1));
        MenuCommand::None
    }

    fn step_selection(&mut self, forward: bool) -> MenuCommand {
        let count = self.selectable();
        debug_assert!(
            count > 0 || self.selected.is_none(),
            "selection without rows"
        );
        self.selected = self.selected.map(|index| {
            if forward {
                (index + 1) % count
            } else {
                (index + count - 1) % count
            }
        });
        MenuCommand::None
    }

    fn cycle(&mut self, forward: bool, config: &mut Config) -> MenuCommand {
        if self.selected.is_none() {
            return MenuCommand::None;
        }
        if self.on_draft_row() {
            // The draft rows only change prompts or the memory opt-in: no
            // relayout, no other effect.
            let kind = self.selected_kind();
            if let Some(ai) = &mut self.ai {
                match kind {
                    Some(ItemKind::Profile) => ai.cycle_profile(forward),
                    Some(ItemKind::Memory(_)) => ai.toggle_memory(),
                    Some(ItemKind::Tools) => ai.toggle_tools(),
                    Some(ItemKind::Family(toggle)) => ai.toggle_family(*toggle),
                    Some(ItemKind::Approval) => ai.cycle_approval(forward),
                    Some(ItemKind::Notify) => ai.cycle_notify(forward),
                    Some(ItemKind::NotifySound) => ai.toggle_notify_sound(),
                    Some(ItemKind::ManagedIdle) => ai.cycle_idle(forward),
                    _ => ai.toggle_context(),
                }
            }
            return MenuCommand::None;
        }
        let Some(ItemKind::Choice(choice) | ItemKind::Toggle(choice)) = self.selected_kind() else {
            return MenuCommand::None;
        };
        let len = choice.values.len();
        debug_assert!(len > 0, "a choice needs values to cycle");
        let current = (choice.get)(config);
        let next = if forward {
            (current + 1) % len
        } else {
            (current + len - 1) % len
        };
        (choice.set)(config, next);
        MenuCommand::Changed
    }

    /// The keys that differ from the config captured at open.
    pub fn edits(&self, live: &Config) -> Vec<ConfigEdit> {
        let settings = choices()
            .filter(|(_, c)| (c.get)(live) != (c.get)(&self.original))
            .map(|(item, c)| ConfigEdit {
                table: c.table,
                key: c.key,
                value: Cow::Borrowed(match item.kind {
                    // A toggle is written as a TOML boolean.
                    ItemKind::Toggle(_) if (c.get)(live) == 1 => "true",
                    ItemKind::Toggle(_) => "false",
                    _ => c.values[(c.get)(live)],
                }),
            });
        let tasks = self
            .ai
            .iter()
            .flat_map(AiMenu::changed_tasks)
            .flat_map(|(task, a)| {
                let value = |v: &Option<String>| Cow::Owned(v.clone().unwrap_or_default());
                [
                    ConfigEdit {
                        table: task.table(),
                        key: "engine",
                        value: value(&a.engine),
                    },
                    ConfigEdit {
                        table: task.table(),
                        key: "model",
                        value: value(&a.model),
                    },
                ]
            });
        let profile = self
            .ai
            .as_ref()
            .and_then(AiMenu::changed_profile)
            .map(|profile| ConfigEdit {
                table: "ai",
                key: "profile",
                value: Cow::Borrowed(profile.id()),
            });
        let context = self
            .ai
            .as_ref()
            .and_then(AiMenu::changed_context)
            .map(|on| ConfigEdit {
                table: "ai",
                key: "context",
                value: Cow::Borrowed(if on { "true" } else { "false" }),
            });
        let memory = self.ai.iter().flat_map(|ai| {
            ai.changed_memory()
                .into_iter()
                .map(|(key, on)| ConfigEdit::memory_flag(key, on))
        });
        let notify = self
            .ai
            .as_ref()
            .and_then(AiMenu::changed_notify)
            .map(|mode| ConfigEdit {
                table: "ai",
                key: "notify",
                value: Cow::Borrowed(mode.id()),
            });
        let sound = self
            .ai
            .as_ref()
            .and_then(AiMenu::changed_notify_sound)
            .map(|on| ConfigEdit {
                table: "ai",
                key: "notify_sound",
                value: Cow::Borrowed(if on { "true" } else { "false" }),
            });
        let tools = self
            .ai
            .as_ref()
            .and_then(AiMenu::changed_tools)
            .map(ConfigEdit::tools_flag);
        let families = self
            .ai
            .iter()
            .flat_map(AiMenu::changed_toggles)
            .map(|(toggle, on)| ConfigEdit::toggle(toggle, on));
        let approval = self
            .ai
            .as_ref()
            .and_then(AiMenu::changed_approval)
            .map(ConfigEdit::approval);
        let idle = self
            .ai
            .as_ref()
            .and_then(AiMenu::changed_idle)
            .map(|minutes| ConfigEdit {
                table: LLAMACPP_TABLE,
                key: "idle_minutes",
                value: Cow::Owned(minutes.to_string()),
            });
        let managed = self
            .ai
            .as_ref()
            .and_then(AiMenu::changed_managed)
            .into_iter()
            .flat_map(|server| {
                [
                    ConfigEdit {
                        table: LLAMACPP_TABLE,
                        key: "managed",
                        value: Cow::Borrowed("true"),
                    },
                    ConfigEdit {
                        table: LLAMACPP_TABLE,
                        key: "model",
                        value: Cow::Owned(server.model.to_string_lossy().into_owned()),
                    },
                ]
            });
        settings
            .chain(tasks)
            .chain(profile)
            .chain(context)
            .chain(memory)
            .chain(notify)
            .chain(sound)
            .chain(tools)
            .chain(families)
            .chain(approval)
            .chain(idle)
            .chain(managed)
            .collect()
    }

    /// A paste: goes to the custom field or the Hugging Face search when
    /// one is focused, else ignored.
    pub fn on_paste(&mut self, text: &str) {
        if let Some(form) = &mut self.feedback {
            form.paste(text);
            return;
        }
        if self.account.field().is_some() {
            self.account.paste(text);
            return;
        }
        if let Some(ai) = &mut self.ai {
            ai.edit(|p| p.paste(text));
        }
    }

    /// The config captured when the menu opened.
    pub fn original(&self) -> Config {
        self.original
    }

    /// The footer line: a pending error, else the hint of the current view.
    pub fn footer(&self) -> Footer<'_> {
        if let Some(error) = self.error() {
            return Footer::Error(error);
        }
        if self.in_warning() {
            return Footer::Hint(Cow::Borrowed(HINT_WARNING));
        }
        if let Some(prompt) = self.account.prompt() {
            return Footer::Hint(Cow::Owned(prompt));
        }
        if self.account.asking() && (self.account.field().is_some() || self.on_account_row()) {
            return Footer::Hint(Cow::Borrowed(HINT_ASKING));
        }
        if self.account.field().is_some() {
            return Footer::Hint(match (&self.copied, self.opener) {
                (Some(copied), _) => Cow::Owned(copied.clone()),
                (None, true) => Cow::Borrowed(account::HINT_TOKEN),
                (None, false) => Cow::Borrowed(account::HINT_TOKEN_COPY),
            });
        }
        if let Some(done) = &self.feedback_done {
            return Footer::Hint(match (&self.copied, done, self.opener) {
                (Some(copied), _, _) => Cow::Owned(copied.clone()),
                (None, Done::Sent(_), true) => Cow::Borrowed(feedback::HINT_SENT),
                (None, Done::Sent(_), false) => Cow::Borrowed(feedback::HINT_SENT_COPY),
                (None, _, _) => Cow::Borrowed(feedback::HINT_DONE),
            });
        }
        if let Some(form) = &self.feedback {
            return match form.error() {
                Some(error) => Footer::Error(error),
                None => Footer::Hint(Cow::Borrowed(form.hint())),
            };
        }
        let ai = self.ai.as_ref();
        let picker = ai.and_then(AiMenu::picker);
        if picker.is_some_and(Picker::asks_cancel) {
            return Footer::Hint(Cow::Borrowed(HINT_CANCEL));
        }
        if let Some(wizard) = picker.and_then(Picker::install) {
            // Copying the manual steps says so until the next key.
            return Footer::Hint(match &self.copied {
                Some(copied) => Cow::Owned(copied.clone()),
                None => Cow::Borrowed(wizard.hint()),
            });
        }
        if let Some(hub) = picker.and_then(Picker::hub) {
            return match hub.screen() {
                Screen::Confirm(confirm) if hub.asking().is_none() => {
                    match confirm.short.as_deref() {
                        Some(short) => Footer::Error(short),
                        None => Footer::Hint(Cow::Borrowed(hub.hint())),
                    }
                }
                _ => Footer::Hint(Cow::Borrowed(hub.hint())),
            };
        }
        if picker.is_some_and(|p| p.text().is_some()) {
            return Footer::Hint(Cow::Borrowed(HINT_CUSTOM));
        }
        if let Some(copied) = &self.copied {
            return Footer::Hint(Cow::Owned(copied.clone()));
        }
        let cursor = ai.and_then(AiMenu::cursor_row);
        if cursor
            .as_ref()
            .is_some_and(|r| matches!(r.key, PickKey::Note { .. }))
        {
            return Footer::Hint(Cow::Borrowed(HINT_NOTE));
        }
        if let (Some(row), Some(picker)) = (&cursor, picker)
            && matches!(picker.level, Level::Engines)
            && matches!(row.mark, Mark::Status(ref t) if t.contains(" · no jail: "))
        {
            return Footer::Hint(Cow::Borrowed(HINT_ENGINE_REASON));
        }
        if let Some(row) = cursor.as_ref().filter(|r| !r.enabled) {
            // The saved models of a stopped server are disabled only because
            // it is not running; its note says how to start it.
            let stopped = match picker.map(|p| &p.level) {
                Some(Level::Models { engine }) => ai.is_some_and(|ai| ai.is_not_running(engine)),
                _ => false,
            };
            let reason = match row.key {
                PickKey::Model(_) if stopped => "not running",
                _ => disabled_reason(row),
            };
            return Footer::Hint(Cow::Owned(format!("{reason} · not selectable")));
        }
        // A warning, not a block: the account list may be stale.
        if picker.is_some() && cursor.is_some_and(|r| r.mark == Mark::NotInList) {
            return Footer::Hint(Cow::Owned(format!(
                "{NOT_IN_LIST} · kept as typed, the list may be stale"
            )));
        }
        let in_ai_view = picker.is_some() || self.on_ai_row();
        let cue = ai.and_then(|ai| match (ai.has_result(), self.checking) {
            (false, _) => Some("checking engines…"),
            (true, true) => Some("refreshing…"),
            (true, false) => None,
        });
        // Enter never saves and Esc at the top level discards the draft, so
        // say it while the draft differs from the saved config.
        if ai.is_some_and(AiMenu::changed) && cue.is_none() {
            let leave = if picker.is_some() {
                "Esc back"
            } else {
                "Esc discards"
            };
            return Footer::Hint(Cow::Owned(format!("unsaved changes · s to save · {leave}")));
        }
        Footer::Hint(match cue {
            Some(cue) if in_ai_view => {
                let leave = if picker.is_some() { "back" } else { "cancel" };
                Cow::Owned(format!("{cue} · s save · Esc {leave}"))
            }
            _ if picker.is_some() => Cow::Borrowed(HINT_PICKER),
            _ if in_ai_view => Cow::Borrowed(HINT_TASK),
            _ if self.on_updates_row() => Cow::Owned(self.updates_hint()),
            _ if self.on_account_row() => Cow::Borrowed(self.account.hint()),
            _ if self.on_feedback_row() => Cow::Borrowed(HINT_FEEDBACK_ROW),
            _ if self.on_notify_row() => match &self.test_report {
                Some(report) => Cow::Owned(report.clone()),
                None => Cow::Borrowed(HINT_NOTIFY),
            },
            _ => Cow::Borrowed(HINT_SETTINGS),
        })
    }

    /// The footer on the Updates row.
    fn updates_hint(&self) -> String {
        if let Some(why) = self.command_shown {
            return why.footer().to_owned();
        }
        if self.checking_update {
            return "checking crates.io… · j/k move · s save · Esc cancel".to_owned();
        }
        let check = if self.can_check_update() {
            " · r check"
        } else {
            ""
        };
        match (&self.installed, &self.update) {
            (Some(done), _) => format!("Updated to {done} · restart lazarobox to use it"),
            (None, Some(_)) => format!("Enter updates{check} · j/k move · s save · Esc cancel"),
            (None, None) => format!("up to date{check} · j/k move · s save · Esc cancel"),
        }
    }

    /// Says what a copy did, in the footer, until the next press.
    pub fn set_copied(&mut self, message: String) {
        self.copied = Some(message);
    }

    /// Says what the notification test did, in the footer of the Notify rows.
    pub fn set_test_report(&mut self, message: String) {
        self.test_report = Some(message);
    }

    pub fn test_report(&self) -> Option<&str> {
        self.test_report.as_deref()
    }

    pub fn set_error(&mut self, message: String) {
        self.error = Some(message);
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Test helper: moves the cursor onto the row labelled `label` with the
    /// keys a user would press, and panics with `no row {label}` when no
    /// row has it. Bounded: it never presses more keys than there are
    /// rows of a tab times the number of tabs, so a missing label fails
    /// instead of hanging. It presses `j` through the current tab, then `Tab`,
    /// across every visible tab.
    #[cfg(test)]
    pub fn goto(&mut self, label: &str) {
        let mut config = Config::default();
        let down = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        let next_tab = KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE);
        for _ in 0..self.tabs().count() {
            for _ in 0..=self.selectable() {
                if self.selected_label(&config) == Some(label) {
                    return;
                }
                self.on_key(&down, false, &mut config);
            }
            self.on_key(&next_tab, false, &mut config);
        }
        panic!("no row {label}");
    }

    /// Test helper: the rows of every visible tab, in table order. A lookup
    /// by label does not need the tab to be the active one; each tab shows
    /// its own remembered cursor.
    #[cfg(test)]
    pub fn all_rows(&self, config: &Config) -> Vec<Row> {
        let mut menu = self.clone();
        while menu.tab != 0 {
            menu.step_tab(false);
        }
        let mut rows = Vec::new();
        for _ in 0..menu.tabs().count() {
            rows.extend(menu.rows(config));
            menu.step_tab(true);
        }
        rows
    }

    /// Test helper: the label of the selected row, without the `* ` the
    /// Updates row wears while an update waits.
    #[cfg(test)]
    fn selected_label(&self, config: &Config) -> Option<&'static str> {
        self.rows(config).into_iter().find_map(|row| match row {
            Row::Item {
                label,
                selected: true,
                ..
            } => Some(label.strip_prefix("* ").unwrap_or(label)),
            _ => None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> impl Iterator<Item = &'static Item> {
        SECTIONS.iter().flat_map(|s| s.items.iter())
    }

    #[test]
    fn descriptor_table_is_well_formed() {
        assert!(!SECTIONS.is_empty());
        let mut paths = Vec::new();
        for section in SECTIONS {
            assert!(!section.title.is_empty());
        }
        for item in items() {
            assert!(!item.label.is_empty());
            if let ItemKind::Choice(c) = &item.kind {
                assert!(!c.table.is_empty() && !c.key.is_empty());
                assert!(c.values.len() >= 2);
                paths.push((c.table, c.key));
            }
            // A toggle may sit in the root table (empty table name).
            if let ItemKind::Toggle(c) = &item.kind {
                assert!(!c.key.is_empty());
                assert_eq!(c.values, ["off", "on"]);
                paths.push((c.table, c.key));
            }
        }
        let mut unique = paths.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), paths.len(), "duplicate key path");
    }

    fn row(label: &'static str, value: Option<&'static str>, selected: bool) -> Row {
        Row::Item {
            label,
            value: value.map(Cow::Borrowed),
            selected,
            enabled: value.is_some(),
        }
    }

    #[test]
    fn rows_list_title_then_the_settings_without_the_ai_ones() {
        let menu = MenuState::open(&Config::default());
        assert_eq!(
            menu.rows(&Config::default()),
            vec![
                Row::Title("Settings".into()),
                row("Statusline position", Some("bottom"), true),
                row("Tab bar position", Some("top"), false),
                row("Mouse", Some("on"), false),
                row("Updates", Some("0.15.3"), false),
                row("Feedback", Some("bug report or idea"), false),
            ]
        );
    }

    #[test]
    fn rows_show_the_live_values() {
        let mut live = Config::default();
        live.bars.statusline = BarPosition::Top;
        let rows = MenuState::open(&Config::default()).rows(&live);
        assert_eq!(rows[1], row("Statusline position", Some("top"), true));
    }

    #[test]
    fn open_selects_the_first_row_with_no_error() {
        let menu = MenuState::open(&Config::default());
        assert_eq!(menu.selected, Some(0));
        assert_eq!(menu.error(), None);
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ch(c: char) -> KeyEvent {
        key(KeyCode::Char(c))
    }

    fn selected(menu: &MenuState) -> Option<usize> {
        menu.selected
    }

    #[test]
    fn move_keys_wrap_in_both_directions() {
        for (down, up) in [(ch('j'), ch('k')), (key(KeyCode::Down), key(KeyCode::Up))] {
            let mut menu = MenuState::open(&Config::default());
            let mut config = Config::default();
            assert_eq!(menu.on_key(&down, false, &mut config), MenuCommand::None);
            assert_eq!(selected(&menu), Some(1));
            menu.on_key(&down, false, &mut config);
            assert_eq!(selected(&menu), Some(2), "the Mouse row is selectable");
            menu.on_key(&down, false, &mut config);
            assert_eq!(selected(&menu), Some(3), "so is the Updates row");
            menu.on_key(&down, false, &mut config);
            assert_eq!(selected(&menu), Some(4), "and the Feedback row");
            menu.on_key(&down, false, &mut config);
            assert_eq!(selected(&menu), Some(0), "wraps past the last row");
            menu.on_key(&up, false, &mut config);
            assert_eq!(selected(&menu), Some(4), "wraps before the first row");
            menu.on_key(&up, false, &mut config);
            assert_eq!(selected(&menu), Some(3));
            menu.on_key(&up, false, &mut config);
            assert_eq!(selected(&menu), Some(2));
            menu.on_key(&up, false, &mut config);
            assert_eq!(selected(&menu), Some(1));
            menu.on_key(&up, false, &mut config);
            assert_eq!(selected(&menu), Some(0));
        }
    }

    #[test]
    fn repeat_moves_like_a_press() {
        let mut menu = MenuState::open(&Config::default());
        menu.on_key(&ch('j'), true, &mut Config::default());
        assert_eq!(selected(&menu), Some(1));
    }

    #[test]
    fn cycle_keys_change_the_selected_value() {
        let forward = [ch('l'), key(KeyCode::Right), ch(' ')];
        let backward = [ch('h'), key(KeyCode::Left)];
        for k in forward.iter().chain(&backward) {
            let mut menu = MenuState::open(&Config::default());
            let mut config = Config::default();
            assert_eq!(menu.on_key(k, false, &mut config), MenuCommand::Changed);
            assert_eq!(config.bars.statusline, BarPosition::Top, "{k:?}");
            menu.on_key(k, true, &mut config);
            assert_eq!(config.bars.statusline, BarPosition::Bottom, "wraps");
        }
    }

    #[test]
    fn cycling_acts_on_the_selected_row() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        menu.on_key(&ch('j'), false, &mut config);
        menu.on_key(&ch('h'), false, &mut config);
        assert_eq!(config.bars.tabbar, BarPosition::Bottom);
        assert_eq!(config.bars.statusline, BarPosition::Bottom);
    }

    #[test]
    fn s_saves_and_esc_cancels_on_press_only() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        assert_eq!(menu.on_key(&ch('s'), false, &mut config), MenuCommand::Save);
        assert_eq!(
            menu.on_key(&key(KeyCode::Esc), false, &mut config),
            MenuCommand::Cancel
        );
        assert_eq!(menu.on_key(&ch('s'), true, &mut config), MenuCommand::None);
        assert_eq!(
            menu.on_key(&key(KeyCode::Esc), true, &mut config),
            MenuCommand::None
        );
    }

    #[test]
    fn release_ctrl_space_and_unknown_keys_are_swallowed() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        let mut release = ch('l');
        release.kind = KeyEventKind::Release;
        let ctrl_space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL);
        for k in [release, ctrl_space, ch('x'), key(KeyCode::Tab)] {
            assert_eq!(
                menu.on_key(&k, false, &mut config),
                MenuCommand::None,
                "{k:?}"
            );
        }
        assert_eq!(config, Config::default());
    }

    #[test]
    fn edits_list_only_the_changed_keys() {
        let menu = MenuState::open(&Config::default());
        assert!(menu.edits(&Config::default()).is_empty());
        let mut live = Config::default();
        live.bars.statusline = BarPosition::Top;
        let edit = |table, value| ConfigEdit {
            table,
            key: "position",
            value: Cow::Borrowed(value),
        };
        assert_eq!(menu.edits(&live), vec![edit("statusline", "top")]);
        live.bars.tabbar = BarPosition::Bottom;
        assert_eq!(
            menu.edits(&live),
            vec![edit("statusline", "top"), edit("tabbar", "bottom")]
        );
    }

    #[test]
    fn the_error_clears_on_the_next_press_which_then_acts() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        menu.set_error("boom".to_owned());
        assert_eq!(menu.error(), Some("boom"));
        menu.on_key(&ch('j'), true, &mut config);
        let mut release = ch('j');
        release.kind = KeyEventKind::Release;
        menu.on_key(&release, false, &mut config);
        assert_eq!(menu.error(), Some("boom"), "repeat and release keep it");
        assert_eq!(
            menu.on_key(&ch('l'), false, &mut config),
            MenuCommand::Changed
        );
        assert_eq!(menu.error(), None);
    }

    #[test]
    fn an_unlisted_key_clears_the_error_with_no_other_effect() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        menu.set_error("boom".to_owned());
        assert_eq!(menu.on_key(&ch('x'), false, &mut config), MenuCommand::None);
        assert_eq!(menu.error(), None);
        assert_eq!(selected(&menu), Some(0));
        assert_eq!(config, Config::default());
    }

    // Characterization: pins behaviour that already held, so they pass at once.
    #[test]
    fn shift_is_ignored_for_listed_keys_and_uppercase_is_swallowed() {
        let mut menu = MenuState::open(&Config::default());
        let mut config = Config::default();
        let shift_space = KeyEvent::new(KeyCode::Char(' '), KeyModifiers::SHIFT);
        assert_eq!(
            menu.on_key(&shift_space, false, &mut config),
            MenuCommand::Changed
        );
        assert_eq!(config.bars.statusline, BarPosition::Top);
        let shift_enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
        assert_eq!(
            menu.on_key(&shift_enter, false, &mut config),
            MenuCommand::Changed
        );
        assert_eq!(config.bars.statusline, BarPosition::Bottom, "cycled back");
        let upper_j = KeyEvent::new(KeyCode::Char('J'), KeyModifiers::SHIFT);
        assert_eq!(menu.on_key(&upper_j, false, &mut config), MenuCommand::None);
        assert_eq!(selected(&menu), Some(0), "J is not j");
    }

    // Characterization: the descriptors already round-trip.
    #[test]
    fn every_choice_get_returns_what_set_stored() {
        for (item, choice) in choices() {
            for i in 0..choice.values.len() {
                let mut config = Config::default();
                (choice.set)(&mut config, i);
                assert_eq!((choice.get)(&config), i, "{}", item.label);
            }
        }
    }
}

#[cfg(test)]
mod ai_tests;

#[cfg(test)]
mod goto_tests;

#[cfg(test)]
mod tabs_tests;

#[cfg(test)]
mod tools_tests;

#[cfg(test)]
mod family_tests;

#[cfg(test)]
mod approval_tests;
#[cfg(test)]
mod idle_tests;

#[cfg(test)]
mod copy_tests;
#[cfg(test)]
mod picker_tests;

#[cfg(test)]
mod disabled_tests;

#[cfg(test)]
mod custom_flow_tests;

#[cfg(test)]
mod reanchor_tests;

#[cfg(test)]
mod llamacpp_tests;

#[cfg(test)]
mod llamacpp_probe_tests;

#[cfg(test)]
mod save_tests;

#[cfg(test)]
mod footer_tests;

#[cfg(test)]
mod guard_tests;

#[cfg(test)]
mod edits_tests;

#[cfg(test)]
mod memory_tests;

#[cfg(test)]
mod notify_tests;

#[cfg(test)]
mod profile_tests;

#[cfg(test)]
mod context_tests;

#[cfg(test)]
mod mouse_tests;
#[cfg(test)]
mod update_tests;

#[cfg(test)]
mod hub_tests;

#[cfg(test)]
mod account_tests;

#[cfg(test)]
mod feedback_tests;

#[cfg(test)]
mod feedback_menu_tests;
