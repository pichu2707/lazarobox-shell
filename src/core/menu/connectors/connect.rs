//! Connect Google (openspec change `google-connect`, S5): one screen that sets
//! up Search Console and Google Workspace with the user's own Desktop OAuth
//! client. It lists the services, the client, the e-mail and the one-time
//! Google Cloud steps; `n` asks the runtime for the plan, read from the files
//! as they are now; `y` on that plan sends its write, and nothing is written
//! before. Then `i` installs the pinned servers and `g` links them one after
//! the other, through the flows of their entries. A service already declared
//! is left alone. Nothing here reads or writes a file.

use super::fields::{MASK, tail};
use super::*;
use crate::core::{
    ai::mcp_catalog::{
        connect::{Ask, CHECKLIST, Preview, Refused, SERVICES},
        entry,
        link::plain_email,
    },
    config_append::Connected,
};

pub(super) const LABEL: &str = "Connect Google";
const WHAT: &str = "Search Console + Gmail, Docs, Sheets";
const TITLE: &str = "Connect Google · Search Console and Workspace";
const TITLE_PLAN: &str = "Connect Google · plan";
const TITLE_EMAIL: &str = "Connect Google · USER_GOOGLE_EMAIL";
const HINT: &str = "j/k scroll · s/w tick · e e-mail · o client · n plan · Esc";
const HINT_PLAN: &str = "j/k scroll · y write · c copy the entries · Esc back";
const HINT_EMAIL: &str = "type your Google e-mail; Enter keeps it · Esc cancels";
const READING: &str = "reading the files…";
const WRITING: &str = "writing…";
const EMAIL: &str = "USER_GOOGLE_EMAIL";
/// The indent of a checklist step's second line, under its text.
const HANG: &str = "      ";
/// The longest e-mail address the field takes.
const MAX_EMAIL: usize = 254;
const NO_CLIENT: &str = "no client yet: press o to save your client id and secret";
const PASTE: &str =
    "the entries were not added: paste them at the end of config.toml (c copies them)";

/// The screen's state; it lives as long as the menu.
#[derive(Clone, Default, PartialEq, Eq, Debug)]
pub(super) struct Google {
    /// The services the user unticked; both are ticked at first.
    off: Vec<&'static str>,
    /// The e-mail for Workspace, as typed.
    email: String,
    /// The e-mail field is open, with this text.
    draft: Option<String>,
    /// What the runtime read of the client file: its id, `None` when there is
    /// no file, or why it is refused. `None` until the first read answers.
    client: Option<Result<Option<String>, String>>,
    phase: Phase,
    /// The services the last write appended: `i` installs them before the
    /// disk copy says they are declared.
    written: Vec<&'static str>,
    /// The last write went through: the restart line shows.
    done: bool,
    /// The last write did not add the entries: they are pasted by hand.
    paste: bool,
}

impl Google {
    pub(super) fn editing(&self) -> bool {
        self.draft.is_some()
    }
}

#[derive(Clone, Default, PartialEq, Eq, Debug)]
enum Phase {
    #[default]
    Setup,
    /// `n` was pressed: the runtime reads the files.
    Planning,
    /// The plan, until `y` or Esc.
    Plan(Box<Preview>),
    /// `y` was pressed: the write runs.
    Writing,
}

fn service(id: &str) -> &'static str {
    if id == "gsc" {
        "Search Console"
    } else {
        "Google Workspace"
    }
}

fn catalog(id: &str) -> &'static Entry {
    entry(id).expect("every Connect Google service is a catalog entry")
}

/// Whether the value `name` of `table` is drawn masked: a catalog name that
/// is confidential, or one the catalog does not know.
fn masked(table: &str, name: &str) -> bool {
    entry(table)
        .and_then(|e| e.env.iter().find(|v| v.name == name))
        .is_none_or(|v| v.confidential())
}

/// The index of `gsc` in the catalog: the client form's entry.
fn client_at() -> usize {
    mcp_catalog::entries()
        .iter()
        .position(|e| e.id == "gsc")
        .expect("gsc is a catalog entry")
}

impl Connectors {
    /// Enter on the row. A write that runs keeps its state.
    pub(super) fn open_google(&mut self) {
        self.screen = Some(Screen::Google { scroll: 0 });
        self.way = None;
        self.linking = None;
        if self.google.phase != Phase::Writing {
            self.google.phase = Phase::Setup;
        }
    }

    /// Back to the top of the screen, when it is shown.
    fn rescroll(&mut self) {
        if let Some(Screen::Google { .. }) = self.screen {
            self.screen = Some(Screen::Google { scroll: 0 });
        }
    }

    /// Declared in any form: a parsed server, or an entry of that name in
    /// `config.toml` that the parsed list skipped as invalid.
    fn declared(&self, id: &str) -> bool {
        self.server(id).is_some() || self.presence.as_ref().is_some_and(|p| p.declares(id))
    }

    /// The value of the row.
    pub(super) fn google_state(&self) -> &'static str {
        if SERVICES.iter().all(|id| self.declared(id)) {
            "both declared"
        } else {
            WHAT
        }
    }

    /// The services `n` plans and `c` copies: ticked and not declared.
    fn chosen(&self) -> Vec<&'static str> {
        SERVICES
            .iter()
            .copied()
            .filter(|id| !self.google.off.contains(id) && !self.declared(id))
            .collect()
    }

    /// The e-mail `secrets.toml` already holds for Workspace.
    fn kept_email(&self) -> Option<&str> {
        let presence = self.presence.as_ref()?;
        if !presence.has("google", EMAIL) {
            return None;
        }
        Some(presence.shown("google", EMAIL).unwrap_or("set"))
    }

    fn linked(&self, id: &str) -> Option<bool> {
        let presence = self.presence.as_ref()?;
        if id == "gsc" {
            presence.linked()
        } else {
            presence.google_linked()
        }
    }

    /// The services `i` installs: declared, or appended by the last write.
    fn installable(&self) -> Vec<&'static str> {
        SERVICES
            .iter()
            .copied()
            .filter(|id| self.declared(id) || self.google.written.contains(id))
            .collect()
    }

    fn install_line(ids: &[&'static str]) -> String {
        let lines: Vec<&str> = ids.iter().map(|id| catalog(id).install).collect();
        lines.join(" && ")
    }

    /// What the runtime read of the client file.
    pub(in crate::core::menu) fn google_client(&mut self, reply: Result<Option<String>, String>) {
        self.google.client = Some(reply);
    }

    /// The runtime's plan, or why there is none; dropped when the user left
    /// the plan meanwhile.
    pub(in crate::core::menu) fn google_plan(&mut self, reply: Result<Box<Preview>, String>) {
        if self.google.phase != Phase::Planning {
            return;
        }
        match reply {
            Ok(preview) => {
                if let Some(why) = &preview.blocked {
                    self.said = Some(why.clone());
                }
                self.google.phase = Phase::Plan(preview);
            }
            Err(notice) => {
                self.google.phase = Phase::Setup;
                self.said = Some(notice);
            }
        }
        self.rescroll();
    }

    /// What became of the write: `true` when the files are read again (some
    /// value or entry was written).
    pub(in crate::core::menu) fn google_connected(&mut self, done: &Connected) -> bool {
        if self.google.phase == Phase::Writing {
            self.google.phase = Phase::Setup;
            self.rescroll();
        }
        let written = matches!(done, Connected::Done(_));
        self.google.done = written;
        self.google.paste = !written;
        if !written {
            self.google.written.clear();
        }
        !matches!(done, Connected::Refused(_))
    }

    pub(super) fn google_hint(&self) -> Cow<'static, str> {
        if self.google.draft.is_some() {
            return Cow::Borrowed(HINT_EMAIL);
        }
        if let Some(hint) = self.field_hint(client_at()) {
            return Cow::Owned(hint);
        }
        Cow::Borrowed(match self.google.phase {
            Phase::Setup => HINT,
            Phase::Planning => READING,
            Phase::Plan(_) => HINT_PLAN,
            Phase::Writing => WRITING,
        })
    }

    pub(super) fn google_rows(&self, scroll: usize, room: usize) -> Vec<Row> {
        if let Some(draft) = &self.google.draft {
            return vec![
                title(TITLE_EMAIL),
                text(format!("  {}", tail(&clean(draft), room))),
            ];
        }
        if let Some(rows) = self.open_field_rows(LABEL, &[], room) {
            return rows;
        }
        let heading = match self.google.phase {
            Phase::Plan(_) => TITLE_PLAN,
            _ => TITLE,
        };
        let mut rows = vec![title(heading)];
        let content = self.google_content(room);
        let window = VIEW_BODY - 1;
        let top = scroll.min(content.len().saturating_sub(window));
        rows.extend(content.into_iter().skip(top).take(window));
        rows
    }

    fn google_content(&self, room: usize) -> Vec<Row> {
        match &self.google.phase {
            Phase::Plan(preview) => plan_rows(preview, room),
            _ => self.setup_rows(room),
        }
    }

    fn setup_rows(&self, room: usize) -> Vec<Row> {
        let mut rows = vec![title("Services (s and w tick them)")];
        for (id, what) in [
            ("gsc", "Search Console"),
            (
                "google",
                "Google Workspace: Gmail read + drafts, Docs, Sheets",
            ),
        ] {
            let line = if self.declared(id) {
                format!("[-] {}: already declared", service(id))
            } else if self.google.off.contains(&id) {
                format!("[ ] {what}")
            } else {
                format!("[x] {what}")
            };
            rows.extend(wrap(&line, "  ", room));
        }
        let client = match &self.google.client {
            None => "client: checking…".to_owned(),
            Some(Ok(Some(id))) => format!("client: {}", clean(id)),
            Some(Ok(None)) => "client: missing — press o".to_owned(),
            Some(Err(notice)) => format!("client: {notice}"),
        };
        rows.extend(wrap(&client, "", room));
        rows.extend(wrap(&self.email_line(), "", room));
        rows.push(text("n shows the plan · c copies the entries"));
        if self.google.paste {
            rows.extend(wrap(PASTE, "", room));
        }
        self.next_steps(&mut rows, room);
        rows.push(title("Google Cloud, once (1-9 open the links)"));
        for (n, (step, _)) in CHECKLIST.iter().enumerate() {
            let mut lines = wrap(step, HANG, room);
            if let Some(Row::Text(first)) = lines.first_mut() {
                let rest = first.trim_start().to_owned();
                *first = Cow::Owned(format!("  [{}] {rest}", n + 1));
            }
            rows.extend(lines);
        }
        rows
    }

    fn email_line(&self) -> String {
        if self.declared("google") {
            "e-mail: not needed (Google Workspace is declared)".to_owned()
        } else if self.google.off.contains(&"google") {
            "e-mail: not needed (Google Workspace is not ticked)".to_owned()
        } else if let Some(kept) = self.kept_email() {
            format!("e-mail: kept in secrets.toml ({})", clean(kept))
        } else if self.google.email.is_empty() {
            "e-mail: not set — press e".to_owned()
        } else {
            format!("e-mail: {} (e changes it)", clean(&self.google.email))
        }
    }

    /// Install, link and restart, once a service is declared or written.
    fn next_steps(&self, rows: &mut Vec<Row>, room: usize) {
        let ids = self.installable();
        if ids.is_empty() && !self.google.done {
            return;
        }
        rows.push(title("Next steps"));
        if !ids.is_empty() {
            let line = format!("i install: {}", Self::install_line(&ids));
            rows.extend(wrap(&line, "  ", room));
        }
        let links: Vec<String> = SERVICES
            .iter()
            .filter(|id| self.declared(id))
            .map(|id| {
                let state = match self.linked(id) {
                    Some(true) => "linked",
                    _ => "not linked",
                };
                format!("{} {state}", service(id))
            })
            .collect();
        if !links.is_empty() {
            rows.extend(wrap(&format!("g link: {}", links.join(" · ")), "  ", room));
        }
        if self.google.done || self.restart() {
            rows.push(text(format!("  {RESTART}")));
        }
    }

    pub(super) fn google_key(
        &mut self,
        code: KeyCode,
        scroll: usize,
        dirty: bool,
        room: usize,
    ) -> MenuCommand {
        let setup = self.google.phase == Phase::Setup;
        match code {
            KeyCode::Char('j') | KeyCode::Down => {
                let window = VIEW_BODY - 1;
                let last = self.google_content(room).len().saturating_sub(window);
                self.screen = Some(Screen::Google {
                    scroll: (scroll + 1).min(last),
                });
            }
            KeyCode::Char('k') | KeyCode::Up => {
                let scroll = scroll.saturating_sub(1);
                self.screen = Some(Screen::Google { scroll });
            }
            KeyCode::Esc => match self.google.phase {
                Phase::Plan(_) | Phase::Planning => {
                    self.google.phase = Phase::Setup;
                    self.rescroll();
                }
                Phase::Setup | Phase::Writing => self.screen = Some(Screen::List(0)),
            },
            KeyCode::Char('y') => return self.google_write(),
            KeyCode::Char('c') => return self.google_copy(),
            KeyCode::Char(d @ '1'..='9') => {
                let (_, link) = CHECKLIST[usize::from(d as u8 - b'1')];
                return MenuCommand::Open(link.to_owned());
            }
            KeyCode::Char('s' | 'w' | 'e' | 'o' | 'n' | 'i' | 'g') | KeyCode::Enter if !setup => {
                self.notice = Some(match self.google.phase {
                    Phase::Writing => WRITING,
                    Phase::Planning => READING,
                    _ => "Esc goes back to change the plan",
                });
            }
            KeyCode::Char('s') => self.tick("gsc"),
            KeyCode::Char('w') => self.tick("google"),
            KeyCode::Char('e') => self.edit_email(),
            KeyCode::Char('o') => self.open_client_form(),
            KeyCode::Char('n') | KeyCode::Enter => return self.google_ask(),
            KeyCode::Char('i') => self.google_install(dirty, room),
            KeyCode::Char('g') => return self.google_link(dirty, room),
            _ => self.notice = Some("no such key here"),
        }
        MenuCommand::None
    }

    fn tick(&mut self, id: &'static str) {
        if self.declared(id) {
            self.said = Some(format!(
                "{} is already declared; it is left alone",
                service(id)
            ));
        } else if let Some(at) = self.google.off.iter().position(|off| *off == id) {
            self.google.off.remove(at);
        } else {
            self.google.off.push(id);
        }
    }

    fn edit_email(&mut self) {
        if self.declared("google") || self.google.off.contains(&"google") {
            self.notice = Some("the e-mail is only for Google Workspace, ticked and not declared");
        } else if self.kept_email().is_some() {
            self.notice = Some(
                "USER_GOOGLE_EMAIL is already set in secrets.toml; f on the Google entry changes it",
            );
        } else {
            self.google.draft = Some(self.google.email.clone());
        }
    }

    /// `n`: the plan of the ticked services, asked of the runtime once the
    /// menu knows of nothing missing.
    fn google_ask(&mut self) -> MenuCommand {
        let chosen = self.chosen();
        if chosen.is_empty() {
            if SERVICES.iter().all(|id| self.declared(id)) {
                self.notice = Some("both services are declared: g links them");
            } else {
                self.said = Some(Refused::NothingChosen.notice());
            }
            return MenuCommand::None;
        }
        match &self.google.client {
            Some(Ok(None)) => {
                self.notice = Some(NO_CLIENT);
                return MenuCommand::None;
            }
            Some(Err(notice)) => {
                self.said = Some(notice.clone());
                return MenuCommand::None;
            }
            _ => {}
        }
        let needs_email = chosen.contains(&"google") && self.kept_email().is_none();
        if needs_email && self.google.email.is_empty() {
            self.notice = Some("type your Google e-mail first (e)");
            return MenuCommand::None;
        }
        self.google.phase = Phase::Planning;
        let email = if needs_email {
            self.google.email.clone()
        } else {
            String::new()
        };
        MenuCommand::PlanGoogle(Ask {
            chosen,
            email,
            route: Default::default(),
        })
    }

    /// `y` on the plan: its write, once.
    fn google_write(&mut self) -> MenuCommand {
        match &self.google.phase {
            Phase::Plan(preview) => {
                let entries = preview.blocked.is_none() && !preview.write.names.is_empty();
                if preview.write.set.is_empty() && !entries {
                    self.notice =
                        Some("nothing to write: paste the entries by hand (c copies them)");
                    return MenuCommand::None;
                }
                let Phase::Plan(preview) =
                    std::mem::replace(&mut self.google.phase, Phase::Writing)
                else {
                    return MenuCommand::None;
                };
                self.google.written = preview.write.names.clone();
                self.google.done = false;
                self.google.paste = false;
                self.rescroll();
                MenuCommand::ConnectGoogle(Box::new(preview.write))
            }
            Phase::Writing => {
                self.notice = Some(WRITING);
                MenuCommand::None
            }
            Phase::Setup | Phase::Planning => {
                self.notice = Some("n shows the plan first");
                MenuCommand::None
            }
        }
    }

    /// `c`: the entries to paste by hand, as the plan would append them.
    fn google_copy(&mut self) -> MenuCommand {
        if let Phase::Plan(preview) = &self.google.phase
            && !preview.write.snippets.is_empty()
        {
            return MenuCommand::Copy(preview.write.snippets.clone());
        }
        let chosen = self.chosen();
        if chosen.is_empty() {
            self.notice = Some("nothing to copy: every ticked service is declared");
            return MenuCommand::None;
        }
        let entries: Vec<String> = chosen.iter().map(|id| snippet(catalog(id))).collect();
        MenuCommand::Copy(entries.join("\n"))
    }

    /// `i`: the install lines in one new tab, after the prompt that shows them.
    fn google_install(&mut self, dirty: bool, room: usize) {
        let ids = self.installable();
        if ids.is_empty() {
            self.notice = Some("nothing to install yet: n plans the entries first");
            return;
        }
        let prompt = Prompt::Run {
            what: "install",
            id: Cow::Owned(ids.join(", ")),
            line: Cow::Owned(Self::install_line(&ids)),
            discards: dirty,
        };
        self.ask(prompt, room);
    }

    /// `g`: the link of the first declared service that is not linked,
    /// Search Console first.
    fn google_link(&mut self, dirty: bool, room: usize) -> MenuCommand {
        let declared: Vec<&str> = SERVICES
            .iter()
            .copied()
            .filter(|id| self.declared(id))
            .collect();
        if declared.is_empty() {
            self.notice = Some("nothing to link yet: n plans the entries first");
            return MenuCommand::None;
        }
        let Some(id) = declared
            .into_iter()
            .find(|id| self.linked(id) != Some(true))
        else {
            self.notice =
                Some("every declared service is linked; restart lazarobox to use it in chats");
            return MenuCommand::None;
        };
        if id == "gsc" {
            self.link_gsc(catalog(id), dirty)
        } else {
            self.link_google(catalog(id), room);
            MenuCommand::None
        }
    }

    /// A key while the e-mail field or the client form is open.
    pub(super) fn google_field_key(&mut self, key: &KeyEvent, room: usize) -> MenuCommand {
        let Some(draft) = self.google.draft.as_mut() else {
            return self.field_key(key, client_at(), room);
        };
        let plain = key.modifiers.difference(KeyModifiers::SHIFT).is_empty();
        match key.code {
            KeyCode::Esc => self.google.draft = None,
            KeyCode::Enter => {
                let typed = draft.trim().to_owned();
                if typed.is_empty() || plain_email(&typed) {
                    self.google.email = typed;
                    self.google.draft = None;
                } else {
                    self.said = Some(Refused::BadEmail.notice());
                }
            }
            KeyCode::Backspace if plain => {
                draft.pop();
            }
            KeyCode::Char(c) if plain => self.google_paste(c.encode_utf8(&mut [0; 4])),
            _ => self.notice = Some("no such key here"),
        }
        MenuCommand::None
    }

    /// Text typed or pasted into the e-mail field: one line, bounded.
    pub(super) fn google_paste(&mut self, more: &str) {
        let Some(draft) = self.google.draft.as_mut() else {
            return;
        };
        if more.chars().any(char::is_control) {
            self.notice = Some("the e-mail must be one line without control characters");
        } else if draft.len() + more.len() > MAX_EMAIL {
            self.notice = Some("the e-mail is longer than 254 bytes");
        } else {
            draft.push_str(more);
        }
    }
}

/// The plan as the screen lists it: entries, values (the confidential ones
/// masked), kept values, services left alone and install lines.
fn plan_rows(preview: &Preview, room: usize) -> Vec<Row> {
    let write = &preview.write;
    let mut rows = Vec::new();
    if !write.names.is_empty() {
        rows.push(title("Append to config.toml"));
        for id in &write.names {
            rows.push(text(format!("  {id} ({})", service(id))));
        }
        let line = if write.expected.is_some() {
            "config.toml gets these entries at its end; the old file is kept as config.toml.bak"
        } else {
            "config.toml is created with these entries"
        };
        rows.extend(wrap(line, "", room));
        if let Some(why) = &preview.blocked {
            rows.extend(wrap(why, "", room));
            rows.extend(wrap(
                "y saves the values only; paste the entries by hand (c copies them)",
                "",
                room,
            ));
        }
    }
    if !write.set.is_empty() {
        rows.push(title("Set in secrets.toml"));
        for (table, name, value) in &write.set {
            let shown = if masked(table, name) {
                MASK.to_owned()
            } else {
                clean(value.expose())
            };
            rows.extend(verbatim(&format!("[{table}] {name} = {shown}"), "  ", room));
        }
    }
    if !preview.kept.is_empty() {
        rows.push(title("Kept in secrets.toml"));
        for (table, name) in &preview.kept {
            rows.push(text(format!("  [{table}] {name}")));
        }
    }
    if !preview.declared.is_empty() {
        rows.push(title("Already declared, left alone"));
        for id in &preview.declared {
            rows.push(text(format!("  {id}")));
        }
    }
    if !preview.installs.is_empty() {
        rows.push(title("Install after the write (i)"));
        for line in &preview.installs {
            rows.extend(wrap(line, "  ", room));
        }
    }
    rows.push(text("y writes · c copies the entries · Esc back"));
    rows
}
