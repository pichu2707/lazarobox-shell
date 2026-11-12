//! The pure side of Link with Google for `google` (S6b2): the preconditions,
//! the check of the sign-in link the server answers, and the names the
//! server gives the account's credentials file. Nothing here opens a file.

use crate::core::ai::config::McpServer;

pub const NO_LINK: &str = "link failed: no sign-in link in the answer";

/// What the link session tells the app while it runs.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Step {
    /// The sign-in link to open: the user is to consent now.
    Waiting(String),
    /// The session ended: the notice of the result.
    Ended(Result<String, String>),
}

/// What the link session needs from the user's values.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Ask {
    pub client_id: String,
    pub email: String,
}

/// The first line of `text` that is a single link, when it is a sign-in link
/// of this client on a local callback port. workspace-mcp 2.4.1 prints the
/// link after `Authorization URL:` (checked in S6b2), so that label may come
/// first.
pub fn sign_in_url<'a>(text: &'a str, client_id: &str) -> Option<&'a str> {
    let link = text.lines().find_map(|line| {
        let line = line.trim();
        let line = line
            .strip_prefix("Authorization URL:")
            .map_or(line, str::trim);
        line.starts_with("http").then_some(line)
    })?;
    let ascii = link.bytes().all(|b| (0x21..=0x7E).contains(&b));
    let rest = link.strip_prefix("https://accounts.google.com/o/oauth2/")?;
    let query = rest.split_once('?')?.1;
    let one = |key: &str| {
        let mut found = query.split('&').filter_map(|pair| {
            let (name, value) = pair.split_once('=')?;
            (name == key).then(|| decoded(value))
        });
        let value = found.next()?;
        found.next().is_none().then_some(value).flatten()
    };
    let local = one("redirect_uri").is_some_and(|uri| local_port(&uri));
    (ascii && link.len() <= 2048 && one("client_id").as_deref() == Some(client_id) && local)
        .then_some(link)
}

/// `value` with its `%XX` escapes resolved; `None` for a broken escape.
fn decoded(value: &str) -> Option<String> {
    let mut out = Vec::new();
    let mut bytes = value.bytes();
    while let Some(b) = bytes.next() {
        if b != b'%' {
            out.push(b);
            continue;
        }
        let hex = [bytes.next()?, bytes.next()?];
        let hex = std::str::from_utf8(&hex).ok()?;
        out.push(u8::from_str_radix(hex, 16).ok()?);
    }
    String::from_utf8(out).ok()
}

/// `http://localhost:<port>` or `http://127.0.0.1:<port>`, port 8000-8004,
/// then nothing or a path.
fn local_port(uri: &str) -> bool {
    let rest = ["http://localhost:", "http://127.0.0.1:"]
        .iter()
        .find_map(|head| uri.strip_prefix(head));
    let port = rest.map(|r| r.split('/').next().unwrap_or_default());
    port.is_some_and(|p| matches!(p, "8000" | "8001" | "8002" | "8003" | "8004"))
}

/// The file names workspace-mcp may keep the account's credentials under: the
/// percent-encoded e-mail, and the older name with `_` for every other
/// character (used by the server only when it exists).
pub fn credentials_names(email: &str) -> Vec<String> {
    let plain = |c: char| c.is_ascii_alphanumeric() || "@._-".contains(c);
    let encoded: String = email
        .chars()
        .map(|c| match c {
            '~' => "~".to_owned(),
            c if plain(c) => c.to_string(),
            c => c.to_string().bytes().map(|b| format!("%{b:02X}")).collect(),
        })
        .collect();
    let legacy: String = email
        .chars()
        .map(|c| if plain(c) { c } else { '_' })
        .collect();
    let mut names = vec![format!("{encoded}.json")];
    if legacy != encoded {
        names.push(format!("{legacy}.json"));
    }
    names
}

/// The values of a declared `google`, or the notice of what is missing: the
/// entry must pass the three names, and each must hold a value (`has`) the
/// link can use (`shown` gives the two that are not confidential).
pub fn plan(
    server: &McpServer,
    has: &impl Fn(&str) -> bool,
    shown: &impl Fn(&str) -> Option<String>,
) -> Result<Ask, String> {
    const NAMES: [&str; 3] = [
        "GOOGLE_OAUTH_CLIENT_ID",
        "GOOGLE_OAUTH_CLIENT_SECRET",
        "USER_GOOGLE_EMAIL",
    ];
    if let Some(name) = NAMES.iter().find(|n| !server.env.iter().any(|e| e == *n)) {
        return Err(format!(
            "the declared google entry does not pass {name}; paste the current entry"
        ));
    }
    let missing: Vec<&str> = NAMES.iter().copied().filter(|n| !has(n)).collect();
    if !missing.is_empty() {
        return Err(format!("fill {} first (f)", missing.join(", ")));
    }
    let value = |name| shown(name).unwrap_or_default();
    let (client_id, email) = (value("GOOGLE_OAUTH_CLIENT_ID"), value("USER_GOOGLE_EMAIL"));
    let graphic = |v: &str| !v.is_empty() && v.bytes().all(|b| (0x21..=0x7E).contains(&b));
    if !graphic(&client_id) || !graphic(&email) || !email.contains('@') {
        return Err(
            "USER_GOOGLE_EMAIL is not a plain e-mail address; fix it in Connectors (f)".to_owned(),
        );
    }
    Ok(Ask { client_id, email })
}

#[cfg(test)]
mod tests;
