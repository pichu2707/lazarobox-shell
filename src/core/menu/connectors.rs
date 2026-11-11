//! The Connectors area of the menu (mcp-connect): the catalog entries with
//! their state, read from the declared servers, `secrets.toml` and this
//! session's probes. It writes no configuration and draws no `secrets.toml`
//! value: it only ever sees names ([`Presence`]).

use std::{borrow::Cow, collections::HashMap};

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{MenuCommand, Row, VIEW_BODY};
use crate::core::ai::{
    config::McpServer,
    mcp_catalog::{
        self, Entry, Group,
        checks::{Probed, Verdict},
        presence::Presence,
    },
};

pub(super) const RESTART: &str = "restart lazarobox to use it in chats";
const UNREAD: &str = "config.toml could not be read; showing the list loaded at startup";
const HINT_LIST: &str = "j/k move · Esc back";
/// Text is cut here; the popup is 62 wide and the renderer clips.
const WRAP: usize = 56;
const UNBOUND: &str = "no such key here";

/// `text` cut at [`WRAP`] columns on word boundaries (a long word is split).
fn wrap(text: &str, indent: &str) -> Vec<Row> {
    let room = WRAP - indent.len();
    let mut lines = vec![String::new()];
    for word in text.split_whitespace() {
        let mut word = word.to_owned();
        while word.chars().count() > room {
            let head: String = word.chars().take(room).collect();
            word = word.chars().skip(room).collect();
            lines.push(head);
        }
        let line = lines.last_mut().expect("one line");
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > room {
            lines.push(word);
        } else if line.is_empty() {
            *line = word;
        } else {
            line.push(' ');
            line.push_str(&word);
        }
    }
    lines
        .into_iter()
        .filter(|l| !l.is_empty())
        .map(|l| Row::Text(Cow::Owned(format!("{indent}{l}"))))
        .collect()
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Screen {
    List(usize),
}

/// The area's state; it lives as long as the menu.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(super) struct Connectors {
    /// The servers loaded at startup, which the chats use.
    startup: Vec<McpServer>,
    probes: HashMap<String, Probed>,
    /// The disk copy read when the area opened (D20).
    presence: Option<Presence>,
    screen: Option<Screen>,
    notice: Option<&'static str>,
}

impl Connectors {
    pub(super) fn new(startup: Vec<McpServer>, probes: Vec<Probed>) -> Self {
        let probes = probes.into_iter().map(|p| (p.server.clone(), p)).collect();
        Self {
            startup,
            probes,
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
        let missing: Vec<&str> = entry
            .env
            .iter()
            .filter(|v| v.secret && v.preset.is_none())
            .filter(|v| {
                self.presence
                    .as_ref()
                    .is_some_and(|p| !p.has(entry.id, v.name))
            })
            .map(|v| v.name)
            .collect();
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

    pub(super) fn rows(&self) -> Vec<Row> {
        let mut rows = match self.screen {
            Some(Screen::List(cursor)) => self.list_rows(cursor),
            None => Vec::new(),
        };
        rows.truncate(VIEW_BODY);
        rows
    }

    fn list_rows(&self, cursor: usize) -> Vec<Row> {
        let mut rows = Vec::new();
        let mut group = None;
        for (at, entry) in mcp_catalog::entries().iter().enumerate() {
            if group != Some(entry.group) {
                group = Some(entry.group);
                rows.push(Row::Title(Cow::Borrowed(match entry.group {
                    Group::Analytics => "Analytics",
                    Group::Workspace => "Workspace",
                })));
            }
            rows.push(Row::Item {
                label: entry.title,
                value: Some(Cow::Owned(self.state(entry))),
                selected: at == cursor,
                enabled: true,
            });
        }
        if self.presence.as_ref().is_some_and(|p| p.servers.is_none()) {
            rows.extend(wrap(UNREAD, ""));
        } else if self.restart() {
            rows.push(Row::Text(Cow::Borrowed(RESTART)));
        }
        // Why a value may be missing: the file is not used, or has errors.
        for problem in self.presence.iter().flat_map(|p| &p.problems).take(3) {
            rows.extend(wrap(problem, ""));
        }
        rows
    }

    pub(super) fn hint(&self) -> Cow<'static, str> {
        Cow::Borrowed(HINT_LIST)
    }

    pub(super) fn key(&mut self, key: &KeyEvent, _dirty: bool) -> MenuCommand {
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        let (Some(Screen::List(cursor)), true) = (self.screen, plain) else {
            self.notice = Some(UNBOUND);
            return MenuCommand::None;
        };
        let len = mcp_catalog::entries().len();
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.screen = Some(Screen::List((cursor + 1) % len));
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.screen = Some(Screen::List((cursor + len - 1) % len));
            }
            KeyCode::Esc => self.screen = None,
            _ => self.notice = Some(UNBOUND),
        }
        MenuCommand::None
    }
}
