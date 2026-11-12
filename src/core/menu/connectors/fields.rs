//! The fields form of the Connectors area (mcp-connect, Secret fields): one
//! row per value of the entry, the masked field, and the confirmations before
//! a save. The value lives in a [`SecretText`] from the first key to the
//! command that sends it; nothing here formats it.

use super::*;
use crate::core::ai::mcp_catalog::Var;
use crate::core::ai::mcp_secrets::{
    BACKUP, BAD_ID, BAD_SECRET, CLIENT_FILE, SecretText, valid_id, valid_secret,
};

/// What a confidential field draws once it holds text, whatever the length.
pub(super) const MASK: &str = "••••••••";
/// The longest value a field takes.
const MAX_FIELD: usize = 4096;
const BAD: &str = "the value must be one line without control characters";
const LONG: &str = "the value is longer than 4096 bytes";
const EMPTY: &str = "empty value not saved; edit secrets.toml by hand to remove a value";
const EMPTY_CLIENT: &str = "nothing typed; Esc cancels";
const NO_VALUES: &str = "this entry has no values to fill";
const NO_HOME: &str = "HOME is not an absolute path; edit secrets.toml by hand";
const NO_BACKUP: &str = "secrets.toml.bak: there is no backup";
pub(super) const HINT_FIELDS: &str = "j/k move · Enter fill · d delete backup · Esc back";

/// A save waiting for the user's `y` (or ready to go).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Pending {
    server: &'static str,
    pub(super) name: &'static str,
    value: SecretText,
    /// The declared command, when it is not the catalog's.
    pub(super) command: Option<String>,
    /// A value is already saved under that name.
    replace: bool,
}

impl Pending {
    pub(super) fn into_command(self) -> MenuCommand {
        MenuCommand::SaveSecret {
            server: self.server,
            name: self.name,
            value: self.value,
        }
    }
}

/// A client file write waiting for the user's `y` (or ready to go).
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct ClientPending {
    id: String,
    secret: SecretText,
    replace: bool,
}

impl ClientPending {
    pub(super) fn into_command(self) -> MenuCommand {
        MenuCommand::SaveClient {
            id: self.id,
            secret: self.secret,
            replace: self.replace,
        }
    }
}

/// What a field asks for: a value of the entry (the index of its row), or
/// a step of the OAuth client form (the secret step holds the id typed).
#[derive(Clone, PartialEq, Eq, Debug)]
enum Slot {
    Var(usize),
    ClientId,
    ClientSecret(String),
}

/// The field that is open, and what was typed.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Field {
    slot: Slot,
    text: SecretText,
}

/// The entries whose login reads the OAuth client file.
fn takes_client(entry: &Entry) -> bool {
    matches!(entry.id, "ga4" | "gsc")
}

fn secret(text: &str) -> SecretText {
    let mut value = SecretText::new();
    value.push_str(text);
    value
}

/// The values of an entry that live in `secrets.toml`, on the way chosen
/// (all of them for an entry with one way).
fn vars(entry: &Entry, way: usize) -> Vec<&'static Var> {
    let all = entry.ways.get(way).map_or(entry.env, |w| w.vars);
    all.iter().filter(|v| v.secret).collect()
}

impl Connectors {
    /// The way shown: the user's choice, else the one the table fills.
    pub(super) fn way_of(&self, entry: &Entry) -> usize {
        self.way.unwrap_or_else(|| {
            let has = |name: &str| {
                self.presence
                    .as_ref()
                    .is_some_and(|p| p.has(entry.id, name))
            };
            entry.way_for(has)
        })
    }

    /// `w`: the next way of an entry that has more than one.
    pub(super) fn choose_way(&mut self, entry: &Entry) {
        if entry.ways.len() < 2 {
            self.notice = Some("this entry has one way");
        } else {
            self.way = Some((self.way_of(entry) + 1) % entry.ways.len());
            if let Some(Screen::Fields { at, .. }) = self.screen {
                self.screen = Some(Screen::Fields { at, cursor: 0 });
            }
        }
    }

    /// What differs: the declared command, when it is not the catalog's.
    fn differing_command(&self, entry: &Entry) -> Option<String> {
        let server = self.server(entry.id)?;
        (server.command != entry.command).then(|| clean(&server.command))
    }

    /// Sends the save of `value`, or asks first: the command that differs,
    /// then the value that would be replaced.
    fn submit(&mut self, entry: &Entry, var: &Var, value: SecretText, room: usize) -> MenuCommand {
        let held = |p: &Presence| {
            p.has(entry.id, var.name)
                && (var.confidential() || p.shown(entry.id, var.name) != Some(value.expose()))
        };
        let replace = self.presence.as_ref().is_some_and(held);
        let pending = Pending {
            server: entry.id,
            name: var.name,
            command: self.differing_command(entry),
            value,
            replace,
        };
        match (&pending.command, replace) {
            (Some(_), _) => self.ask(Prompt::Command(pending), room),
            (None, true) => self.ask(Prompt::Replace(pending), room),
            (None, false) => return pending.into_command(),
        }
        MenuCommand::None
    }

    /// `y` on the command prompt: the replace prompt next, or the save.
    pub(super) fn after_command(&mut self, pending: Pending, room: usize) -> MenuCommand {
        if !pending.replace {
            return pending.into_command();
        }
        self.ask(Prompt::Replace(pending), room);
        MenuCommand::None
    }

    pub(super) fn open_fields(&mut self, entry: &Entry, at: usize) {
        if vars(entry, 0).is_empty() {
            self.notice = Some(NO_VALUES);
        } else {
            self.saved = false;
            self.screen = Some(Screen::Fields { at, cursor: 0 });
        }
    }

    /// A field takes the keys: a value, the client form, or Connect
    /// Google's e-mail.
    pub(in crate::core::menu) fn field_open(&self) -> bool {
        self.field.is_some() || self.google.editing()
    }

    /// `o`: the first step of the OAuth client form.
    pub(super) fn open_client_form(&mut self) {
        self.field = Some(Field {
            slot: Slot::ClientId,
            text: SecretText::new(),
        });
    }

    /// The result of a save: the list asks the user to restart on success.
    pub(in crate::core::menu) fn saved(&mut self, ok: bool) {
        self.saved = ok;
    }

    fn vars_at(&self, at: usize) -> Vec<&'static Var> {
        let entry = &mcp_catalog::entries()[at];
        vars(entry, self.way_of(entry))
    }

    pub(super) fn field_hint(&self, at: usize) -> Option<String> {
        let field = self.field.as_ref()?;
        let (what, enter) = match &field.slot {
            Slot::Var(i) => (self.vars_at(at)[*i].name, "saves"),
            Slot::ClientId => ("client id", "continues"),
            Slot::ClientSecret(_) => ("client secret", "saves"),
        };
        Some(format!("type or paste {what}; Enter {enter} · Esc cancels"))
    }

    pub(super) fn field_rows(&self, at: usize, cursor: usize, room: usize) -> Vec<Row> {
        let entry = &mcp_catalog::entries()[at];
        let list = self.vars_at(at);
        if let Some(rows) = self.open_field_rows(entry.title, &list, room) {
            return rows;
        }
        let mut rows = vec![title(format!("{} · values", entry.title))];
        if let Some(command) = self.differing_command(entry) {
            let line = format!("declared command differs from the catalog: {command}");
            rows.extend(wrap(&line, "", room));
        }
        for (i, var) in list.iter().enumerate() {
            rows.push(Row::Item {
                label: var.name,
                value: Some(Cow::Owned(self.value_text(entry, var, room))),
                selected: i == cursor,
                enabled: true,
            });
        }
        if takes_client(entry) {
            rows.push(text(format!("o OAuth client file ({CLIENT_FILE})")));
        }
        if self.presence.as_ref().is_some_and(Presence::backup) {
            rows.push(text(format!("d delete {BACKUP}")));
        }
        if self.saved {
            rows.push(text("restart the chat or probe again to use it"));
        }
        rows
    }

    /// The rows of the open field under `heading`: its name, and its text
    /// (masked when confidential, else its last columns). `list` names the
    /// values of a value field.
    pub(super) fn open_field_rows(
        &self,
        heading: &str,
        list: &[&'static Var],
        room: usize,
    ) -> Option<Vec<Row>> {
        let field = self.field.as_ref()?;
        let (name, confidential) = match &field.slot {
            Slot::Var(i) => (list[*i].name, list[*i].confidential()),
            Slot::ClientId => ("client id", false),
            Slot::ClientSecret(_) => ("client secret", true),
        };
        let shown = if confidential {
            if field.text.is_empty() { "" } else { MASK }.to_owned()
        } else {
            tail(&clean(field.text.expose()), room)
        };
        Some(vec![
            title(format!("{heading} · {name}")),
            text(format!("  {shown}")),
        ])
    }

    /// `set` for a confidential value; the value for the others.
    fn value_text(&self, entry: &Entry, var: &Var, room: usize) -> String {
        let presence = self.presence.as_ref();
        if var.confidential() {
            let set = presence.is_some_and(|p| p.has(entry.id, var.name));
            return if set { "set" } else { "not set" }.to_owned();
        }
        if let Some(value) = presence.and_then(|p| p.shown(entry.id, var.name)) {
            return clean(value)
                .chars()
                .take(room.saturating_sub(var.name.len() + 2))
                .collect();
        }
        match var.preset {
            Some(preset) => {
                let home = presence.map_or("", Presence::home);
                let value = var.preset_for(home).unwrap_or_else(|| preset.to_owned());
                format!("not set · Enter writes {}", clean(&value))
            }
            None => "not set".to_owned(),
        }
    }

    pub(super) fn fields_key(
        &mut self,
        code: KeyCode,
        (at, cursor): (usize, usize),
        room: usize,
    ) -> MenuCommand {
        let entry = &mcp_catalog::entries()[at];
        let list = self.vars_at(at);
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                self.screen = Some(Screen::Fields {
                    at,
                    cursor: (cursor + 1) % list.len(),
                });
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let cursor = (cursor + list.len() - 1) % list.len();
                self.screen = Some(Screen::Fields { at, cursor });
            }
            KeyCode::Esc => self.screen = Some(Screen::Entry { at, scroll: 0 }),
            KeyCode::Char('w') => self.choose_way(entry),
            KeyCode::Char('o') if takes_client(entry) => self.open_client_form(),
            KeyCode::Char('d') => match self.presence.as_ref().is_some_and(Presence::backup) {
                true => self.ask(Prompt::DeleteBackup, room),
                false => self.notice = Some(NO_BACKUP),
            },
            KeyCode::Enter => {
                let var = list[cursor];
                match var.preset {
                    None => {
                        self.field = Some(Field {
                            slot: Slot::Var(cursor),
                            text: SecretText::new(),
                        });
                    }
                    Some(_) => {
                        let home = self.presence.as_ref().map_or("", Presence::home);
                        match var.preset_for(home) {
                            Some(value) => return self.submit(entry, var, secret(&value), room),
                            None => self.notice = Some(NO_HOME),
                        }
                    }
                }
            }
            _ => self.notice = Some("no such key here"),
        }
        MenuCommand::None
    }

    /// A key while a field is open: it is text, or one of the field's keys.
    pub(super) fn field_key(&mut self, key: &KeyEvent, at: usize, room: usize) -> MenuCommand {
        let entry = &mcp_catalog::entries()[at];
        let Some(field) = self.field.as_mut() else {
            return MenuCommand::None;
        };
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        match key.code {
            KeyCode::Esc => self.field = None,
            KeyCode::Enter if field.text.is_empty() => {
                let var = matches!(field.slot, Slot::Var(_));
                self.notice = Some(if var { EMPTY } else { EMPTY_CLIENT });
            }
            KeyCode::Enter => {
                let Some(Field { slot, text }) = self.field.take() else {
                    return MenuCommand::None;
                };
                return self.enter_field(entry, slot, text, room);
            }
            KeyCode::Backspace if plain => field.text.pop(),
            KeyCode::Char(c) if plain => match accepts(&field.text, c.encode_utf8(&mut [0; 4])) {
                Ok(()) => field.text.push(c),
                Err(notice) => self.notice = Some(notice),
            },
            _ => self.notice = Some("no such key here"),
        }
        MenuCommand::None
    }

    /// Enter on a typed field: the save of a value, the next step of the
    /// client form, or its write (asked first over an existing file). A text
    /// that is refused stays in the field.
    fn enter_field(
        &mut self,
        entry: &Entry,
        slot: Slot,
        text: SecretText,
        room: usize,
    ) -> MenuCommand {
        let refuse = |this: &mut Self, slot, text, notice| {
            this.field = Some(Field { slot, text });
            this.notice = Some(notice);
            MenuCommand::None
        };
        match slot {
            Slot::Var(i) => {
                let var = vars(entry, self.way_of(entry))[i];
                return self.submit(entry, var, text, room);
            }
            Slot::ClientId if !valid_id(text.expose()) => {
                return refuse(self, Slot::ClientId, text, BAD_ID);
            }
            Slot::ClientId => {
                let slot = Slot::ClientSecret(text.expose().to_owned());
                self.field = Some(Field {
                    slot,
                    text: SecretText::new(),
                });
            }
            Slot::ClientSecret(id) if !valid_secret(text.expose()) => {
                return refuse(self, Slot::ClientSecret(id), text, BAD_SECRET);
            }
            Slot::ClientSecret(id) => {
                let replace = self.presence.as_ref().is_some_and(Presence::client_file);
                let pending = ClientPending {
                    id,
                    secret: text,
                    replace,
                };
                if !replace {
                    return pending.into_command();
                }
                self.ask(Prompt::Client(pending), room);
            }
        }
        MenuCommand::None
    }

    pub(in crate::core::menu) fn paste(&mut self, pasted: &str) {
        if self.google.editing() {
            self.google_paste(pasted);
            return;
        }
        let Some(field) = self.field.as_mut() else {
            return;
        };
        match accepts(&field.text, pasted) {
            Ok(()) => field.text.push_str(pasted),
            Err(notice) => self.notice = Some(notice),
        }
    }
}

/// The last columns of `text` that fit in a field row of `room`.
pub(super) fn tail(text: &str, room: usize) -> String {
    let skip = text.chars().count().saturating_sub(room.saturating_sub(2));
    text.chars().skip(skip).collect()
}

/// Whether `more` may follow `text` in a field: one line, no control
/// character, within [`MAX_FIELD`] bytes.
fn accepts(text: &SecretText, more: &str) -> Result<(), &'static str> {
    if more.chars().any(char::is_control) {
        Err(BAD)
    } else if text.len() + more.len() > MAX_FIELD {
        Err(LONG)
    } else {
        Ok(())
    }
}
