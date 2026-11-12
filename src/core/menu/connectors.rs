//! The Connectors area of the menu (mcp-connect): the catalog entries with
//! their state, and the screen of one entry (guide, snippet, template, files,
//! actions). It writes no configuration and draws no confidential value; the
//! one write to `secrets.toml` is a confirmed secret field (`fields`), and
//! nothing runs until the user's `y` on a prompt that shows what will run.

use std::{borrow::Cow, collections::HashMap, path::PathBuf};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use self::fields::{Field, HINT_FIELDS, Pending};
use super::{MenuCommand, Row, VIEW_BODY};
use crate::core::ai::{
    config::McpServer,
    mcp_catalog::{
        self, Entry, Group, checks::Probed, checks::Verdict, presence::Presence, secrets_template,
        snippet,
    },
    mcp_secrets::{BACKUP, FILE as SECRETS_FILE},
    sanitize::clean,
};

mod fields;

/// The narrowest text column a prompt is shown in.
const MIN_ROOM: usize = 24;
const CANCELLED: &str = "cancelled";
const WIDEN: &str = "widen the terminal to read the command";
pub(super) const RESTART: &str = "restart lazarobox to use it in chats";
const UNREAD: &str = "config.toml could not be read; showing the list loaded at startup";
const NOT_DECLARED: &str = "not declared: paste the entry into config.toml first";
const HINT_LIST: &str = "j/k move · Enter open · Esc back";
const HINT_ENTRY: &str = "j/k scroll · i install · l login · p probe · f fill · Esc";
const HINT_PROMPT: &str = "y yes · any other key cancels";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    List(usize),
    Entry {
        at: usize,
        scroll: usize,
    },
    /// The values of entry `at`, one row each.
    Fields {
        at: usize,
        cursor: usize,
    },
}

/// What waits for the user's `y`.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Prompt {
    /// A catalog line typed in a new tab (install or login).
    Run {
        what: &'static str,
        id: &'static str,
        line: &'static str,
        discards: bool,
    },
    Probe(Box<McpServer>),
    /// The declared command is not the catalog's: a save asks first.
    Command(Pending),
    /// A value is already saved under that name.
    Replace(Pending),
    /// `d`: delete `secrets.toml.bak`.
    DeleteBackup,
}

/// The area's state; it lives as long as the menu.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(super) struct Connectors {
    /// The servers loaded at startup, which the chats use.
    startup: Vec<McpServer>,
    probes: HashMap<String, Probed>,
    config_path: Option<PathBuf>,
    /// The disk copy read when the area opened (D20).
    presence: Option<Presence>,
    screen: Option<Screen>,
    prompt: Option<Prompt>,
    probing: Option<String>,
    notice: Option<&'static str>,
    /// The field that is open on a `Screen::Fields`.
    field: Option<Field>,
    /// A save went through: the list says when it takes effect.
    saved: bool,
}

/// `text` cut at `room` columns on word boundaries (a long word is split).
/// Spaces between words are collapsed: for prose, never for a command.
fn wrap(text: &str, indent: &str, room: usize) -> Vec<Row> {
    let room = room.saturating_sub(indent.chars().count()).max(1);
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let mut word = word.to_owned();
        while word.chars().count() > room {
            let head: String = word.chars().take(room).collect();
            word = word.chars().skip(room).collect();
            lines.push(head);
        }
        let line = lines.last_mut().expect("one line");
        if line.is_empty() {
            *line = word;
        } else if line.chars().count() + 1 + word.chars().count() > room {
            lines.push(word);
        } else {
            line.push(' ');
            line.push_str(&word);
        }
    }
    lines
        .into_iter()
        .filter(|l| !l.is_empty())
        .map(|l| text_row(format!("{indent}{l}")))
        .collect()
}

/// `text` in rows of at most `room` columns, breaking after a space when it
/// can and keeping every character: the rows, joined, are `text` exactly (a
/// command is shown as it is typed).
fn verbatim(text: &str, indent: &str, room: usize) -> Vec<Row> {
    let room = room.saturating_sub(indent.chars().count()).max(1);
    let chars: Vec<char> = text.chars().collect();
    let mut rows = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = (start + room).min(chars.len());
        if end < chars.len()
            && let Some(space) = chars[start..end].iter().rposition(|c| *c == ' ')
        {
            end = start + space + 1;
        }
        let piece: String = chars[start..end].iter().collect();
        rows.push(text_row(format!("{indent}{piece}")));
        start = end;
    }
    rows
}

fn text_row(line: String) -> Row {
    Row::Text(Cow::Owned(line))
}

/// The rows of a prompt, or `None` when its command does not fit whole in
/// the body at this width: it is then never asked, because the user would
/// confirm a command they could not read.
fn prompt_rows(prompt: &Prompt, room: usize) -> Option<Vec<Row>> {
    if room < MIN_ROOM {
        return None;
    }
    let mut rows = match prompt {
        Prompt::Run {
            what,
            id,
            line,
            discards,
        } => {
            let mut rows = vec![
                title(format!("{what} {id}? (y/n)")),
                text("Runs in a new tab, with Enter:"),
            ];
            rows.extend(verbatim(line, "  ", room));
            if *discards {
                rows.push(text("Unsaved menu changes are discarded."));
            }
            rows
        }
        Prompt::Probe(server) => {
            let mut rows = vec![
                title(format!("probe {}? (y/n)", server.name)),
                text("Starts, without a shell:"),
            ];
            rows.extend(verbatim(&command_of(server), "  ", room));
            rows
        }
        Prompt::Command(pending) => {
            let command = pending.command.as_deref().unwrap_or_default();
            let ask = format!(
                "save {} for a server that runs {command}? (y/n)",
                pending.name
            );
            verbatim(&ask, "", room)
        }
        Prompt::Replace(pending) => {
            vec![title(format!("replace the saved {}? (y/n)", pending.name))]
        }
        Prompt::DeleteBackup => vec![title(format!(
            "delete {BACKUP}? It holds your previous values (y/n)"
        ))],
    };
    (rows.len() <= VIEW_BODY).then(|| {
        rows.shrink_to_fit();
        rows
    })
}

/// What a probe spawns, as one line: the command and its arguments.
fn command_of(server: &McpServer) -> String {
    let mut line = clean(&server.command);
    for arg in &server.args {
        line.push(' ');
        line.push_str(&clean(arg));
    }
    line
}

fn text(line: impl Into<String>) -> Row {
    Row::Text(Cow::Owned(line.into()))
}

fn title(line: impl Into<String>) -> Row {
    Row::Title(Cow::Owned(line.into()))
}

impl Connectors {
    pub(super) fn new(
        startup: Vec<McpServer>,
        probes: Vec<Probed>,
        config_path: Option<PathBuf>,
    ) -> Self {
        let probes = probes.into_iter().map(|p| (p.server.clone(), p)).collect();
        Self {
            startup,
            probes,
            config_path,
            ..Self::default()
        }
    }

    /// The servers the states are read from: the disk copy once it is in.
    fn servers(&self) -> &[McpServer] {
        match self.presence.as_ref().and_then(|p| p.servers.as_deref()) {
            Some(disk) => disk,
            None => &self.startup,
        }
    }

    fn server(&self, id: &str) -> Option<&McpServer> {
        self.servers().iter().find(|s| s.name == id)
    }

    /// The value of the `Connectors` row.
    pub(super) fn value(&self) -> Cow<'static, str> {
        let n = mcp_catalog::entries()
            .iter()
            .filter(|e| self.server(e.id).is_some_and(|s| s.enabled))
            .count();
        match n {
            0 => Cow::Borrowed("none"),
            n => Cow::Owned(format!("{n} declared")),
        }
    }

    pub(super) fn is_open(&self) -> bool {
        self.screen.is_some()
    }

    /// Opens the list; the disk copy of the last visit is dropped.
    pub(super) fn open(&mut self) -> MenuCommand {
        self.screen = Some(Screen::List(0));
        self.presence = None;
        MenuCommand::LoadPresence
    }

    pub(super) fn presence_reply(&mut self, presence: Presence) {
        self.presence = Some(presence);
    }

    pub(super) fn probe_reply(&mut self, report: Probed) {
        if self.probing.as_deref() == Some(report.server.as_str()) {
            self.probing = None;
        }
        self.probes.insert(report.server.clone(), report);
    }

    pub(super) fn take_notice(&mut self) -> Option<&'static str> {
        self.notice.take()
    }

    /// The disk copy differs from what the chats loaded at startup. The order
    /// of the entries does not matter.
    fn restart(&self) -> bool {
        let sorted = |list: &[McpServer]| {
            let mut list = list.to_vec();
            list.sort_by(|a, b| (&a.name, &a.project).cmp(&(&b.name, &b.project)));
            list
        };
        self.presence
            .as_ref()
            .and_then(|p| p.servers.as_ref())
            .is_some_and(|disk| sorted(disk) != sorted(&self.startup))
    }

    /// The state of an entry, as the list shows it.
    fn state(&self, entry: &Entry) -> String {
        let Some(server) = self.server(entry.id) else {
            return "not declared".to_owned();
        };
        if !server.enabled {
            return "disabled".to_owned();
        }
        let missing =
            entry.missing(|name| self.presence.as_ref().is_none_or(|p| p.has(entry.id, name)));
        if !missing.is_empty() {
            return format!("values missing: {}", missing.join(", "));
        }
        match self.probes.get(entry.id).map(|p| p.verdict) {
            Some(Verdict::Started) => "declared · probe: started".to_owned(),
            Some(Verdict::Failed) => "declared · probe: failed".to_owned(),
            Some(Verdict::Stopped) => "declared · probe: stopped".to_owned(),
            None => "declared".to_owned(),
        }
    }

    pub(super) fn rows(&self, room: usize) -> Vec<Row> {
        let mut rows = match (&self.prompt, self.screen) {
            (Some(prompt), _) => prompt_rows(prompt, room).unwrap_or_else(|| vec![text(WIDEN)]),
            (None, Some(Screen::Entry { at, scroll })) => self.entry_rows(at, scroll, room),
            (None, Some(Screen::Fields { at, cursor })) => self.field_rows(at, cursor, room),
            (None, Some(Screen::List(cursor))) => self.list_rows(cursor, room),
            (None, None) => Vec::new(),
        };
        rows.truncate(VIEW_BODY);
        rows
    }

    fn list_rows(&self, cursor: usize, room: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut group = None;
        for (at, entry) in mcp_catalog::entries().iter().enumerate() {
            if group != Some(entry.group) {
                group = Some(entry.group);
                rows.push(title(match entry.group {
                    Group::Analytics => "Analytics",
                    Group::Workspace => "Workspace",
                }));
            }
            rows.push(Row::Item {
                label: entry.title,
                value: Some(Cow::Owned(self.state(entry))),
                selected: at == cursor,
                enabled: true,
            });
        }
        self.notes(&mut rows, room);
        rows
    }

    /// The lines about where the list came from.
    fn notes(&self, rows: &mut Vec<Row>, room: usize) {
        if self.presence.as_ref().is_some_and(|p| p.servers.is_none()) {
            rows.extend(wrap(UNREAD, "", room));
        } else if self.restart() {
            rows.push(text(RESTART));
        }
        // Why a value may be missing: the file is not used, or has errors.
        for problem in self.presence.iter().flat_map(|p| &p.problems).take(3) {
            rows.extend(wrap(problem, "", room));
        }
    }

    /// The entry screen: a fixed title, then the content from `scroll`.
    fn entry_rows(&self, at: usize, scroll: usize, room: usize) -> Vec<Row> {
        let entry = &mcp_catalog::entries()[at];
        let mut rows = vec![title(format!(
            "{} · {} ({})",
            entry.title, entry.project, entry.license
        ))];
        let window = VIEW_BODY - 1;
        let content = self.entry_content(entry, room);
        let top = scroll.min(content.len().saturating_sub(window));
        rows.extend(content.into_iter().skip(top).take(window));
        rows
    }

    fn entry_content(&self, entry: &Entry, room: usize) -> Vec<Row> {
        let mut rows = vec![text(format!("State: {}", self.state(entry)))];
        self.notes(&mut rows, room);
        if let Some(probe) = self.probes.get(entry.id) {
            rows.push(title("Last probe"));
            for line in probe.lines.iter().chain(&probe.problems) {
                rows.extend(wrap(line, "  ", room));
            }
        }
        rows.push(title("Guide"));
        for step in entry.guide {
            rows.extend(wrap(step, "  ", room));
        }
        for (n, link) in entry.links.iter().enumerate() {
            rows.extend(wrap(&format!("[{}] {link}", n + 1), "  ", room));
        }
        rows.push(title("Paste into config.toml"));
        rows.extend(snippet(entry).lines().map(|l| text(format!("  {l}"))));
        if let Some(template) = secrets_template(entry) {
            rows.push(title("Fill in secrets.toml"));
            rows.extend(template.lines().map(|l| text(format!("  {l}"))));
            for var in entry.env.iter().filter(|v| v.secret && v.preset.is_none()) {
                let set = self
                    .presence
                    .as_ref()
                    .is_some_and(|p| p.has(entry.id, var.name));
                rows.push(text(format!(
                    "  {}: {}",
                    var.name,
                    if set { "set" } else { "not set" }
                )));
            }
        }
        rows.push(title("Files"));
        if let Some(config) = &self.config_path {
            rows.extend(wrap(
                &format!("config.toml: {}", config.display()),
                "  ",
                room,
            ));
            let secrets = config.with_file_name(SECRETS_FILE);
            rows.extend(wrap(
                &format!("secrets.toml: {}", secrets.display()),
                "  ",
                room,
            ));
        }
        rows.push(title("Actions"));
        rows.extend(wrap(&format!("i install: {}", entry.install), "  ", room));
        if let Some(login) = entry.login {
            rows.extend(wrap(&format!("l log in: {}", login.line), "  ", room));
        }
        if self.server(entry.id).is_some() {
            rows.push(text("  p probe"));
        } else {
            rows.extend(wrap(NOT_DECLARED, "  ", room));
        }
        rows.push(text("  1-9 open a link · c copy the entry"));
        if secrets_template(entry).is_some() {
            rows.push(text("  f fill the values"));
            rows.push(text("  t copy the secrets.toml table"));
        }
        rows
    }

    pub(super) fn hint(&self) -> Cow<'static, str> {
        if let Some(name) = &self.probing {
            return Cow::Owned(format!("probing {name}… · Esc stops it"));
        }
        if let Some(Screen::Fields { at, .. }) = self.screen
            && let Some(hint) = self.field_hint(at)
        {
            return Cow::Owned(hint);
        }
        Cow::Borrowed(match (&self.prompt, self.screen) {
            (Some(_), _) => HINT_PROMPT,
            (None, Some(Screen::Entry { .. })) => HINT_ENTRY,
            (None, Some(Screen::Fields { .. })) => HINT_FIELDS,
            _ => HINT_LIST,
        })
    }

    pub(super) fn key(&mut self, key: &KeyEvent, dirty: bool, room: usize) -> MenuCommand {
        if let Some(prompt) = self.prompt.take() {
            // Only a plain `y` confirms (not Ctrl+y or Alt+y), and only while
            // the command is shown whole at this width.
            let plain_y = key.code == KeyCode::Char('y') && key.modifiers.is_empty();
            if plain_y && prompt_rows(&prompt, room).is_none() {
                self.notice = Some(WIDEN);
                return MenuCommand::None;
            }
            return match (plain_y, prompt) {
                (true, Prompt::Run { line, .. }) => MenuCommand::Connector(line.to_owned()),
                (true, Prompt::Probe(server)) => {
                    self.probing = Some(server.name.clone());
                    MenuCommand::Probe(server)
                }
                (true, Prompt::Command(pending)) => self.after_command(pending, room),
                (true, Prompt::Replace(pending)) => pending.into_command(),
                (true, Prompt::DeleteBackup) => MenuCommand::DeleteBackup,
                (false, Prompt::Command(_)) => {
                    self.notice = Some("not saved");
                    MenuCommand::None
                }
                (false, Prompt::Replace(_)) => {
                    self.notice = Some("not replaced");
                    MenuCommand::None
                }
                _ => {
                    self.notice = Some(CANCELLED);
                    MenuCommand::None
                }
            };
        }
        if let Some(Screen::Fields { at, .. }) = self.screen
            && self.field.is_some()
        {
            return self.field_key(key, at, room);
        }
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        let (Some(screen), true) = (self.screen, plain) else {
            self.notice = Some("no such key here");
            return MenuCommand::None;
        };
        if key.code == KeyCode::Esc && self.probing.is_some() {
            return MenuCommand::CancelProbe;
        }
        match screen {
            Screen::List(cursor) => self.list_key(key.code, cursor),
            Screen::Entry { at, scroll } => self.entry_key(key.code, (at, scroll), dirty, room),
            Screen::Fields { at, cursor } => self.fields_key(key.code, (at, cursor), room),
        }
    }

    fn list_key(&mut self, code: KeyCode, cursor: usize) -> MenuCommand {
        let len = mcp_catalog::entries().len();
        self.screen = Some(match code {
            KeyCode::Char('j') | KeyCode::Down => Screen::List((cursor + 1) % len),
            KeyCode::Char('k') | KeyCode::Up => Screen::List((cursor + len - 1) % len),
            KeyCode::Enter => Screen::Entry {
                at: cursor,
                scroll: 0,
            },
            KeyCode::Esc => {
                self.screen = None;
                return MenuCommand::None;
            }
            _ => {
                self.notice = Some("no such key here");
                return MenuCommand::None;
            }
        });
        MenuCommand::None
    }

    fn entry_key(
        &mut self,
        code: KeyCode,
        (at, scroll): (usize, usize),
        dirty: bool,
        room: usize,
    ) -> MenuCommand {
        let entry = &mcp_catalog::entries()[at];
        let run = |what, line| Prompt::Run {
            what,
            id: entry.id,
            line,
            discards: dirty,
        };
        let moved = |scroll| Screen::Entry { at, scroll };
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                let last = self
                    .entry_content(entry, room)
                    .len()
                    .saturating_sub(VIEW_BODY - 1);
                self.screen = Some(moved((scroll + 1).min(last)));
            }
            KeyCode::Char('k') | KeyCode::Up => self.screen = Some(moved(scroll.saturating_sub(1))),
            KeyCode::Esc => self.screen = Some(Screen::List(at)),
            KeyCode::Char('i') => self.ask(run("install", entry.install), room),
            KeyCode::Char('l') => match entry.login {
                Some(login) => self.ask(run("log in to", login.line), room),
                None => self.notice = Some("this entry has no login"),
            },
            KeyCode::Char('p') => return self.ask_probe(entry, room),
            KeyCode::Char('f') => self.open_fields(entry, at),
            KeyCode::Char('c') => return MenuCommand::Copy(snippet(entry)),
            KeyCode::Char('t') => match secrets_template(entry) {
                Some(template) => return MenuCommand::Copy(template),
                None => self.notice = Some("this entry has no secrets table"),
            },
            KeyCode::Char(d @ '1'..='9') => match entry.links.get(usize::from(d as u8 - b'1')) {
                Some(link) => return MenuCommand::Open((*link).to_owned()),
                None => self.notice = Some("no such link"),
            },
            _ => self.notice = Some("no such key here"),
        }
        MenuCommand::None
    }

    /// Asks `prompt`, unless its command cannot be shown whole at this width.
    fn ask(&mut self, prompt: Prompt, room: usize) {
        if prompt_rows(&prompt, room).is_some() {
            self.prompt = Some(prompt);
        } else {
            self.notice = Some(WIDEN);
        }
    }

    fn ask_probe(&mut self, entry: &Entry, room: usize) -> MenuCommand {
        if self.probing.is_some() {
            self.notice = Some("a probe is already running");
        } else {
            match self.server(entry.id) {
                None => self.notice = Some(NOT_DECLARED),
                Some(server) if !server.enabled => self.notice = Some("disabled in config.toml"),
                Some(server) => self.ask(Prompt::Probe(Box::new(server.clone())), room),
            }
        }
        MenuCommand::None
    }
}
