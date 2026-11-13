//! The lazarobox Google client (openspec change `gsc-lazarobox-client`): the
//! Desktop OAuth client the app fetches from www.javilazaro.es and keeps in a
//! cache file, so Search Console links without the user's own Google Cloud
//! project. Pure: the fetch and the cache are `runtime::google_client`. A
//! notice never holds the secret or the body that was fetched.

use super::{CLIENT_MAX, Client, client::ID_SUFFIX, read_client, valid_id};

/// The Google Cloud project number every id of the lazarobox client starts
/// with. `None` until the owner supplies it: the route is then off.
pub const PROJECT_NUMBER: Option<&str> = None;

/// The cache file, in the config folder.
pub const CACHE_FILE: &str = "lazarobox-google-client.json";

/// The path of the file on `https://www.javilazaro.es`.
pub const URL_PATH: &str = "/lazarobox/google-client.json";

/// The absolute path of the cache next to `secrets.toml` (so it honours
/// `XDG_CONFIG_HOME`); `None` without an absolute one. The planner and the
/// `f` preset write this path, and Link fetches when the plan holds it.
pub fn cache_path(secrets: Option<&std::path::Path>) -> Option<String> {
    let path = secrets?.parent()?.join(CACHE_FILE);
    path.is_absolute()
        .then(|| path.to_str().map(str::to_owned))
        .flatten()
}

/// The screen's client line on this route.
pub const CLIENT_LINE: &str = "client: lazarobox (fetched from www.javilazaro.es when you link)";

/// Why the client was not taken: a refusal never caches anything.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Why {
    /// The build has no pin.
    NotConfigured,
    /// The request failed (the sanitised curl line).
    Curl(String),
    Http(u16),
    Redirected(u16),
    TooLarge,
    NotDesktop,
    Foreign,
}

impl Why {
    /// The `<why>` of a notice.
    pub fn text(&self) -> String {
        match self {
            Self::NotConfigured => "not configured in this build".to_owned(),
            Self::Curl(line) => line.clone(),
            Self::Http(code) => format!("HTTP {code}"),
            Self::Redirected(code) => format!("redirected (HTTP {code})"),
            Self::TooLarge => "larger than 16 KiB".to_owned(),
            Self::NotDesktop => "not a Desktop client file".to_owned(),
            Self::Foreign => "not from the lazarobox project".to_owned(),
        }
    }

    /// The answer came but its content is refused, not a failed request.
    fn refused(&self) -> bool {
        matches!(self, Self::TooLarge | Self::NotDesktop | Self::Foreign)
    }
}

/// Whether `id` is a Desktop client id of the project `pin`: the number, a
/// `-`, and the rest of a valid id. The `-` keeps a longer number that starts
/// with the pin out.
pub fn pinned(id: &str, pin: Option<&str>) -> bool {
    let Some(pin) = pin.filter(|p| !p.is_empty()) else {
        return false;
    };
    valid_id(id)
        && id
            .strip_prefix(pin)
            .is_some_and(|rest| rest.starts_with('-') && rest.len() > 1 + ID_SUFFIX.len())
}

/// The client of an answer, or why it is refused. Checked in order: the pin
/// is set, the status is 200 (a redirect is a failure), the size, the shape
/// of a Desktop client file, the project.
pub fn accept(status: u16, body: &str, pin: Option<&str>) -> Result<Client, Why> {
    if pin.is_none_or(str::is_empty) {
        return Err(Why::NotConfigured);
    }
    match status {
        200 => {}
        300..=399 => return Err(Why::Redirected(status)),
        _ => return Err(Why::Http(status)),
    }
    if body.len() > CLIENT_MAX {
        return Err(Why::TooLarge);
    }
    let client = read_client(body).map_err(|_| Why::NotDesktop)?;
    if !pinned(&client.id, pin) {
        return Err(Why::Foreign);
    }
    Ok(client)
}

/// The screen's notice when the build has no pin: Connect Google then plans
/// with your own client.
pub const NOT_CONFIGURED_SCREEN: &str =
    "lazarobox client not configured in this build; this screen uses your own client";

/// The instruction of a Link failure, first so the footer's single clipped
/// line (58 columns) never cuts it. Link runs on a declared `gsc`, whose
/// client row is the first on its `f` screen.
const OWN_AFTER_LINK: &str = "own client: f on gsc, Enter, g";

/// The notice of a Link that has no saved copy to fall back on: the way out,
/// then the cause, which starts within the first 58 columns.
pub fn failure_notice(why: &Why) -> String {
    match why {
        Why::NotConfigured => {
            format!("{OWN_AFTER_LINK} \u{b7} lazarobox: not configured in this build")
        }
        why if why.refused() => format!(
            "{OWN_AFTER_LINK} \u{b7} lazarobox: refused ({}); nothing saved",
            why.text()
        ),
        why => format!(
            "{OWN_AFTER_LINK} \u{b7} lazarobox: not fetched ({}); link later",
            why.text()
        ),
    }
}

/// The notice of a Link that goes on with the saved copy.
pub fn fallback_notice(why: &Why) -> String {
    format!(
        "lazarobox client not refreshed ({}); linking with the saved copy",
        why.text()
    )
}

/// The notice of a cache write that was refused, from the refusal's own.
pub fn not_saved_notice(refusal: &str) -> String {
    format!("lazarobox client not saved: {refusal}")
}

#[cfg(test)]
mod tests;
