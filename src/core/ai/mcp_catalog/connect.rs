//! Connect Google (openspec change `google-connect`, issue #734): the pure
//! plan of what one action sets up for Search Console and Google Workspace
//! with the user's own Desktop OAuth client. No IO: the runtime writes the
//! plan, after the user's `y`.
//!
//! A service the user already declared, in any form, is left alone. A value
//! that already holds something in `secrets.toml` is kept. Every value comes
//! from the catalog (a preset), the client file or the e-mail the user typed.

use super::{Entry, entry, link, snippet};
use crate::core::ai::{config::McpServer, mcp_secrets::Client, mcp_secrets::SecretText};

/// The services Connect Google sets up, in the order it appends them.
pub const SERVICES: [&str; 2] = ["gsc", "google"];

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

#[cfg(test)]
mod tests;
