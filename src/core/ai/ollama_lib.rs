//! The data of the Ollama download screens (ollama-download): the library
//! page read into cards, the fit rule on size tags, the registry manifest and
//! the license title. Pure: text in, values out; the runtime fetches.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

use super::catalog::OLLAMA_CURATED;
use super::hf;
use super::sanitize::is_format;

pub mod pull;

/// The site of the library page.
pub const LIBRARY: &str = "https://ollama.com";

/// The registry of manifests and blobs.
pub const REGISTRY: &str = "https://registry.ollama.ai";

/// A reply that is not what the site or the registry sends.
pub const UNREADABLE: &str = "Ollama library request failed: unreadable reply";

/// The title of a license with no text.
pub const NOT_STATED: &str = "not stated";

/// How many models the Popular screen lists.
pub const POPULAR_LIMIT: usize = 20;

/// Most bytes of a license text read.
pub const LICENSE_CAP: usize = 64 * 1024;

/// Longest license title, in characters.
const TITLE_CAP: usize = 120;

/// Longest model name or tag.
const NAME_CAP: usize = 64;

/// One model card of the library page.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Card {
    pub name: String,
    pub description: String,
    /// `tools`, `vision`, `embedding`, `cloud`…
    pub capabilities: Vec<String>,
    /// The size tags, in page order: `1b`, `270m`, `8x7b`, `e2b`…
    pub sizes: Vec<String>,
    /// As the page writes it: `86.3M`.
    pub pulls: String,
}

/// A card of the Popular screen and its sizes that fit this PC.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pick {
    pub card: Card,
    pub fitting: Vec<String>,
}

/// What a manifest says: the bytes of all its layers and the digests of its
/// license layers.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Manifest {
    pub size: u64,
    pub licenses: Vec<String>,
}

/// A pull the user confirmed: `model:tag`, from the Ollama library.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Pull {
    pub model: String,
    pub tag: String,
    /// The bytes of its manifest's layers, when known.
    pub size: Option<u64>,
    /// What the Confirm screen showed as its license.
    pub license: String,
}

impl Pull {
    /// `model:tag`, as Ollama names it.
    pub fn name(&self) -> String {
        format!("{}:{}", self.model, self.tag)
    }

    /// The body of the pull request: `{"model":"<model>:<tag>","stream":true}`.
    pub fn body(&self) -> Vec<u8> {
        serde_json::json!({ "model": self.name(), "stream": true })
            .to_string()
            .into_bytes()
    }
}

/// A request of the Ollama screens; its reply names it.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Ask {
    /// The library page, for the Popular screen.
    Popular,
    /// The manifest of each size tag of `model`, for the Sizes screen.
    Sizes { model: String, tags: Vec<String> },
    /// The license titles of `model:tag`, for the Confirm screen: one GET
    /// per license layer its manifest named. `None` when that manifest
    /// could not be read on the Sizes screen; it is asked for again first.
    Licenses {
        model: String,
        tag: String,
        digests: Option<Vec<String>>,
    },
}

/// The reply to an [`Ask`].
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Answer {
    /// The cards of the library page; the curated catalog's (`fallback`)
    /// when the page could not be read or held none.
    Popular {
        cards: Vec<Card>,
        fallback: bool,
        /// This machine's memory, when known.
        ram: Option<u64>,
    },
    /// Each size tag and its manifest; `None` when it could not be read.
    Sizes {
        manifests: Vec<(String, Option<Manifest>)>,
        ram: Option<u64>,
    },
    Licenses(Box<Licenses>),
}

/// What the Confirm screen shows besides the choice.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Licenses {
    /// The size of the manifest read again, when it had to be.
    pub size: Option<u64>,
    /// One title per license layer, in manifest order.
    pub titles: Vec<String>,
    /// The folder Ollama stores models in, when known.
    pub folder: Option<std::path::PathBuf>,
    /// The free space there.
    pub free: Option<u64>,
}

/// The license of a pull as one label: its titles, or [`NOT_STATED`].
pub fn license_label(titles: &[String]) -> String {
    if titles.is_empty() {
        NOT_STATED.to_owned()
    } else {
        titles.join(", ")
    }
}

/// The cards of the library page, in page order. A card whose name is not
/// safe to send is skipped.
pub fn parse_library(html: &str) -> Vec<Card> {
    static CARD: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?s)<li[^>]*>\s*<a href="/library/([^"]+)"(.*?)</li>"#).expect("valid")
    });
    CARD.captures_iter(html)
        .filter_map(|c| card(&c[1], &c[2]))
        .collect()
}

/// The parameters of a size tag: `8b`, `1.7b`, `270m`, `8x7b`. An
/// "effective" size (`e2b`) and anything else give `None`.
pub fn size_params(tag: &str) -> Option<u64> {
    let (number, scale) = match tag.as_bytes().last()? {
        b'b' => (&tag[..tag.len() - 1], 1_000_000_000.0),
        b'm' => (&tag[..tag.len() - 1], 1_000_000.0),
        _ => return None,
    };
    // A mixture of experts: `8x7b` is 8 experts of 7b.
    let (count, each) = number.split_once('x').unwrap_or(("1", number));
    let plain = |n: &str| {
        !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit() || b == b'.') && !n.starts_with('.')
    };
    if !plain(count) || !plain(each) || count.contains('.') {
        return None;
    }
    let params = count.parse::<f64>().ok()? * each.parse::<f64>().ok()? * scale;
    (1.0..1e18).contains(&params).then(|| params.round() as u64)
}

/// Whether `text` may be sent as a model name or tag.
pub fn is_name(text: &str) -> bool {
    (1..=NAME_CAP).contains(&text.len())
        && !text.starts_with(['.', '-'])
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// The Popular screen: the listable cards with at least one size that fits
/// `ram`, at most [`POPULAR_LIMIT`], in page order.
pub fn popular(cards: Vec<Card>, ram: Option<u64>) -> Vec<Pick> {
    cards
        .into_iter()
        .filter(Card::listable)
        .filter_map(|card| {
            let fitting: Vec<String> = card
                .sizes
                .iter()
                .filter(|tag| match ram {
                    Some(ram) => size_params(tag)
                        .is_some_and(|params| hf::fits(hf::model_bytes(params), ram)),
                    None => true,
                })
                .cloned()
                .collect();
            (!fitting.is_empty()).then_some(Pick { card, fitting })
        })
        .take(POPULAR_LIMIT)
        .collect()
}

/// The library page, on [`LIBRARY`].
pub fn library_path() -> &'static str {
    "/library?sort=popular"
}

/// A manifest, on [`REGISTRY`].
pub fn manifest_path(model: &str, tag: &str) -> String {
    format!("/v2/library/{model}/manifests/{tag}")
}

/// A blob, on [`REGISTRY`].
pub fn blob_path(model: &str, digest: &str) -> String {
    format!("/v2/library/{model}/blobs/{digest}")
}

/// The page to read a model's license on.
pub fn page_url(model: &str, tag: &str) -> String {
    format!("{LIBRARY}/library/{model}:{tag}")
}

pub fn parse_manifest(body: &str) -> Result<Manifest, String> {
    let unreadable = || UNREADABLE.to_owned();
    let manifest: Value = serde_json::from_str(body).map_err(|_| unreadable())?;
    let layers = manifest
        .get("layers")
        .and_then(Value::as_array)
        .ok_or_else(unreadable)?;
    let mut size = 0u64;
    let mut licenses = Vec::new();
    for layer in layers {
        let bytes = layer
            .get("size")
            .and_then(Value::as_u64)
            .ok_or_else(unreadable)?;
        size = size.checked_add(bytes).ok_or_else(unreadable)?;
        let license = layer.get("mediaType").and_then(Value::as_str)
            == Some("application/vnd.ollama.image.license");
        if let Some(digest) = layer.get("digest").and_then(Value::as_str)
            && license
            && is_digest(digest)
        {
            licenses.push(digest.to_owned());
        }
    }
    Ok(Manifest { size, licenses })
}

/// The first non-empty line of a license, characters that could steer the
/// terminal drawn as `?`, at most [`TITLE_CAP`] characters.
pub fn license_title(text: &str) -> String {
    let Some(line) = text.lines().map(str::trim).find(|line| !line.is_empty()) else {
        return NOT_STATED.to_owned();
    };
    line.chars()
        .map(|c| {
            if c.is_control() || is_format(c) {
                '?'
            } else {
                c
            }
        })
        .take(TITLE_CAP)
        .collect()
}

/// The curated catalog as cards, one per model with its size tags: the
/// Popular screen's list when the library page cannot be read.
pub fn fallback_cards() -> Vec<Card> {
    let mut cards: Vec<Card> = Vec::new();
    for entry in OLLAMA_CURATED {
        let Some((name, tag)) = entry.split_once(':') else {
            continue;
        };
        match cards.iter_mut().find(|c| c.name == name) {
            Some(card) => card.sizes.push(tag.to_owned()),
            None => cards.push(Card {
                name: name.to_owned(),
                description: String::new(),
                capabilities: Vec::new(),
                sizes: vec![tag.to_owned()],
                pulls: String::new(),
            }),
        }
    }
    cards
}

impl Card {
    /// Whether a chat can use it: not an embedding model, and downloadable
    /// (a `cloud` model with no size runs only in Ollama's cloud).
    fn listable(&self) -> bool {
        let has = |c: &str| self.capabilities.iter().any(|x| x == c);
        let cloud_only = has("cloud") && self.sizes.is_empty();
        !(has("embedding") || cloud_only)
    }
}

/// A card from its name and the HTML after its link; `None` for a name that
/// is not safe to send.
fn card(name: &str, body: &str) -> Option<Card> {
    static DESCRIPTION: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?s)<p[^>]*>(.*?)</p>").expect("valid"));
    static CHIP: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"<span[^>]*>([^<]+)</span>").expect("valid"));
    static PULLS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"<span[^>]*>([^<]+)</span>\s*<span[^>]*>&nbsp;Pulls").expect("valid")
    });
    if !is_name(name) {
        return None;
    }
    let description = DESCRIPTION.captures(body)?;
    // The chips sit between the description and the line of counts.
    let rest = &body[description.get(0)?.end()..];
    let chips = rest.split("<p").next().unwrap_or_default();
    let (mut capabilities, mut sizes) = (Vec::new(), Vec::new());
    for chip in CHIP.captures_iter(chips) {
        let text = unescape(chip[1].trim());
        if text.is_empty() {
            continue;
        }
        if is_size(&text) {
            sizes.push(text);
        } else {
            capabilities.push(text);
        }
    }
    Some(Card {
        name: name.to_owned(),
        description: unescape(description[1].trim()),
        capabilities,
        sizes,
        pulls: PULLS
            .captures(rest)
            .map(|c| unescape(c[1].trim()))
            .unwrap_or_default(),
    })
}

/// Whether a chip is a size tag: `1b`, `270m`, `1.7b`, `8x7b`, `e2b`.
fn is_size(chip: &str) -> bool {
    size_params(chip.strip_prefix('e').unwrap_or(chip)).is_some() && is_name(chip)
}

/// `sha256:` and 64 lowercase hex digits.
fn is_digest(text: &str) -> bool {
    text.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// The few entities the page uses, then control and format characters
/// drawn as `?`.
fn unescape(text: &str) -> String {
    text.replace("&#39;", "'")
        .replace("&quot;", "\"")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .chars()
        .map(|c| {
            if c.is_control() || is_format(c) {
                '?'
            } else {
                c
            }
        })
        .collect()
}

#[cfg(test)]
mod tests;
