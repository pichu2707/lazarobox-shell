//! Connect Google (openspec change `google-connect`, issue #734): the pure
//! plan of what one action sets up for Search Console and Google Workspace
//! with the user's own Desktop OAuth client. No IO: the runtime writes the
//! plan, after the user's `y`.
//!
//! A service the user already declared, in any form, is left alone. A value
//! that already holds something in `secrets.toml` is kept. Every value comes
//! from the catalog (a preset), the client file or the e-mail the user typed.

use super::{Entry, TUTORIAL, entry, link, snippet};
use crate::core::{
    ai::{
        config::McpServer,
        mcp_secrets::{Client, SecretText, edit_all, line_of},
    },
    config_append::{AppendRefusal, Connect, appended},
};

/// The services Connect Google sets up, in the order it appends them.
pub const SERVICES: [&str; 2] = ["gsc", "google"];

/// The one-time Google Cloud setup the Connect Google screen lists: one step
/// per line, each with the page that `1`-`9` opens (or copies).
pub const CHECKLIST: [(&str, &str); 9] = [
    (
        "Create a Google Cloud project",
        "https://console.cloud.google.com/projectcreate",
    ),
    (
        "Enable the Search Console API",
        "https://console.cloud.google.com/apis/library/searchconsole.googleapis.com",
    ),
    (
        "Enable the Gmail API",
        "https://console.cloud.google.com/apis/library/gmail.googleapis.com",
    ),
    (
        "Enable the Google Docs API",
        "https://console.cloud.google.com/apis/library/docs.googleapis.com",
    ),
    (
        "Enable the Google Sheets API",
        "https://console.cloud.google.com/apis/library/sheets.googleapis.com",
    ),
    (
        "Enable the Google Drive API",
        "https://console.cloud.google.com/apis/library/drive.googleapis.com",
    ),
    (
        "Audience: add your Google account as a test user (else Google answers \
         Error 403: access_denied), then publish the app so tokens do not expire \
         after 7 days",
        "https://console.cloud.google.com/auth/audience",
    ),
    (
        "Credentials: create an OAuth client ID of type Desktop app, then save its \
         id and secret here with o",
        "https://console.cloud.google.com/apis/credentials",
    ),
    ("The step-by-step guide (in Spanish)", TUTORIAL),
];

/// What the Connect Google screen asks the runtime to plan: the services
/// ticked and the e-mail typed. Nothing in it is a secret.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ask {
    pub chosen: Vec<&'static str>,
    pub email: String,
}

/// What the plan is built from.
pub struct Input<'a> {
    /// The disk copy of the declared servers.
    pub servers: &'a [McpServer],
    /// Whether `secrets.toml` holds a non-empty value for (table, name).
    pub has: &'a dyn Fn(&str, &str) -> bool,
    /// The home folder, for the Search Console paths.
    pub home: Option<&'a str>,
    pub client: &'a Client,
    /// The Google account, for Workspace.
    pub email: &'a str,
    /// The services the user ticked, ids of [`SERVICES`].
    pub chosen: &'a [&'a str],
}

/// One `secrets.toml` value to save. Its `{:?}` never shows the value.
#[derive(Debug)]
pub struct Set {
    pub table: &'static str,
    pub name: &'static str,
    pub value: SecretText,
}

/// What Connect Google will do, shown before anything is written.
pub struct Plan {
    /// The entries appended to `config.toml`, in [`SERVICES`] order.
    pub append: Vec<&'static Entry>,
    pub set: Vec<Set>,
    /// The (table, name) values left as they are, because they hold one.
    pub kept: Vec<(&'static str, &'static str)>,
    /// The pinned install lines of the appended entries.
    pub installs: Vec<&'static str>,
    /// The chosen services that are already declared, left alone.
    pub declared: Vec<&'static str>,
}

impl std::fmt::Debug for Plan {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let ids: Vec<&str> = self.append.iter().map(|e| e.id).collect();
        let names: Vec<(&str, &str)> = self.set.iter().map(|s| (s.table, s.name)).collect();
        f.debug_struct("Plan")
            .field("append", &ids)
            .field("set", &names)
            .field("kept", &self.kept)
            .field("installs", &self.installs)
            .field("declared", &self.declared)
            .finish()
    }
}

impl Plan {
    /// The text appended to `config.toml`: the catalog entries, one empty
    /// line between them.
    pub fn snippets(&self) -> String {
        let entries: Vec<String> = self.append.iter().map(|e| snippet(e)).collect();
        entries.join("\n")
    }
}

/// Why no plan is made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refused {
    NothingChosen,
    NotAService(String),
    BadEmail,
    NoHome,
    /// A catalog value with no preset and no source here: the catalog and
    /// this planner drifted apart.
    Uncovered {
        table: &'static str,
        name: &'static str,
    },
}

impl Refused {
    /// The one line the footer shows.
    pub fn notice(&self) -> String {
        match self {
            Self::NothingChosen => "choose Search Console, Google Workspace or both".to_owned(),
            Self::NotAService(id) => format!("{id} is not connected by Connect Google"),
            Self::BadEmail => "type your Google e-mail address (name@domain, no spaces)".to_owned(),
            Self::NoHome => "no absolute home folder for the Search Console token".to_owned(),
            Self::Uncovered { table, name } => {
                format!("{table}.{name} has no value Connect Google can fill; report this")
            }
        }
    }
}

/// The plan for `input`, or why there is none.
pub fn plan(input: &Input) -> Result<Plan, Refused> {
    if input.chosen.is_empty() {
        return Err(Refused::NothingChosen);
    }
    if let Some(other) = input.chosen.iter().find(|id| !SERVICES.contains(id)) {
        return Err(Refused::NotAService((*other).to_owned()));
    }
    let mut plan = Plan {
        append: Vec::new(),
        set: Vec::new(),
        kept: Vec::new(),
        installs: Vec::new(),
        declared: Vec::new(),
    };
    for id in SERVICES.iter().filter(|id| input.chosen.contains(id)) {
        let entry = entry(id).expect("every Connect Google service is a catalog entry");
        if input.servers.iter().any(|s| s.name == entry.id) {
            plan.declared.push(entry.id);
            continue;
        }
        add_values(entry, input, &mut plan)?;
        plan.append.push(entry);
        plan.installs.push(entry.install);
    }
    Ok(plan)
}

/// The values of `entry` (its first way, the OAuth one for `gsc`), each set
/// or kept.
fn add_values(entry: &'static Entry, input: &Input, plan: &mut Plan) -> Result<(), Refused> {
    let vars = entry.ways.first().map_or(entry.env, |way| way.vars);
    for var in vars.iter().filter(|v| v.secret) {
        if (input.has)(entry.id, var.name) {
            plan.kept.push((entry.id, var.name));
            continue;
        }
        let mut value = SecretText::new();
        value.push_str(&source(entry, var, input)?);
        plan.set.push(Set {
            table: entry.id,
            name: var.name,
            value,
        });
    }
    Ok(())
}

/// Where the value of `var` comes from.
fn source(entry: &Entry, var: &super::Var, input: &Input) -> Result<String, Refused> {
    if var.preset.is_some() {
        let home = input.home.unwrap_or_default();
        return var.preset_for(home).ok_or(Refused::NoHome);
    }
    match var.name {
        "GOOGLE_OAUTH_CLIENT_ID" => Ok(input.client.id.clone()),
        "GOOGLE_OAUTH_CLIENT_SECRET" => Ok(input.client.secret.expose().to_owned()),
        "USER_GOOGLE_EMAIL" if link::plain_email(input.email) => Ok(input.email.to_owned()),
        "USER_GOOGLE_EMAIL" => Err(Refused::BadEmail),
        _ => Err(Refused::Uncovered {
            table: entry.id,
            name: var.name,
        }),
    }
}

/// A plan as the screen shows it, checked against the files it will be
/// written to, with the write that `y` sends. Its `{:?}` never shows a value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub write: Connect,
    /// The (table, name) values left as they are.
    pub kept: Vec<(&'static str, &'static str)>,
    /// The pinned install lines of the appended entries.
    pub installs: Vec<&'static str>,
    /// The chosen services already declared, left alone.
    pub declared: Vec<&'static str>,
    /// Why `config.toml` cannot take the entries: `y` then saves the values
    /// only, and the entries are pasted by hand.
    pub blocked: Option<String>,
}

/// The plan of `input`, with a dry run of both writes: the values set in the
/// `secrets.toml` text `secrets` (a refusal refuses the plan, since nothing
/// would be written), and the entries appended to the `config.toml` bytes
/// `config` (`None`: no file; a refusal is [`Preview::blocked`]). So most
/// refusals show before anything is written; the write checks again.
pub fn preview(input: &Input, config: Option<&[u8]>, secrets: &str) -> Result<Preview, String> {
    let plan = plan(input).map_err(|refused| refused.notice())?;
    let values: Vec<(&str, &str, &str)> = plan
        .set
        .iter()
        .map(|s| (s.table, s.name, s.value.expose()))
        .collect();
    if !values.is_empty() {
        edit_all(secrets, &values).map_err(|refusal| format!("connect: {}", refusal.notice()))?;
    }
    let names: Vec<&str> = plan.append.iter().map(|e| e.id).collect();
    if values.is_empty() && names.is_empty() {
        return Err(format!(
            "connect: {} {} already declared; nothing to write",
            plan.declared.join(", "),
            if plan.declared.len() == 1 {
                "is"
            } else {
                "are"
            }
        ));
    }
    let blocked = if names.is_empty() {
        None
    } else {
        let text = match config {
            None => Ok(""),
            Some(bytes) => std::str::from_utf8(bytes)
                .map_err(|e| AppendRefusal::NotToml(line_of(bytes, e.valid_up_to()))),
        };
        text.and_then(|text| appended(text, &plan.snippets(), &names))
            .err()
            .map(|refusal| refusal.notice())
    };
    let (kept, installs, declared) = (
        plan.kept.clone(),
        plan.installs.clone(),
        plan.declared.clone(),
    );
    Ok(Preview {
        write: Connect::new(plan, config),
        kept,
        installs,
        declared,
        blocked,
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod preview_tests;
