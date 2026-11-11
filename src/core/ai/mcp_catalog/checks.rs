//! The pure checks over a tool-name list that the probe (S3) reports. Tool
//! names come from a server, so every line sanitizes and caps them.

use super::Entry;
use crate::core::ai::{
    sanitize::clean,
    tools::servers::{MAX_TOOLS, qualified_name},
};

/// The longest tool name shown in a line, in characters.
const SHOWN_MAX: usize = 64;

fn shown(raw: &str) -> String {
    clean(raw).chars().take(SHOWN_MAX).collect()
}

/// The notice lines for a server that listed `admitted` tools, of which
/// `names` were kept. `entry` is the server's catalog entry, if it has one.
pub fn report(
    server: &str,
    entry: Option<&Entry>,
    admitted: usize,
    names: &[String],
) -> Vec<String> {
    let mut lines: Vec<String> = too_many(server, admitted).into_iter().collect();
    lines.extend(collisions(server, names));
    if let Some(line) = entry.and_then(|e| unknown(server, e, names)) {
        lines.push(line);
    }
    lines
}

/// `<server>: lists <N> tools; only the first 64 are kept`.
pub fn too_many(server: &str, admitted: usize) -> Option<String> {
    (admitted > MAX_TOOLS).then(|| {
        format!(
            "{}: lists {admitted} tools; only the first {MAX_TOOLS} are kept",
            shown(server)
        )
    })
}

/// One line per tool whose qualified name equals an earlier one's: the
/// registry keeps the first, so the later one is the one dropped.
pub fn collisions(server: &str, names: &[String]) -> Vec<String> {
    let mut seen: Vec<(String, &str)> = Vec::new();
    let mut lines = Vec::new();
    for name in names {
        let Some(qualified) = qualified_name(server, name) else {
            continue;
        };
        match seen.iter().find(|(q, _)| *q == qualified) {
            Some((_, first)) => lines.push(format!(
                "{}: {} and {} collide after the 24-character cut; {} is dropped",
                shown(server),
                shown(first),
                shown(name),
                shown(name)
            )),
            None => seen.push((qualified, name)),
        }
    }
    lines
}

/// `<server>: <N> tools not in the catalog (class write)`.
pub fn unknown(server: &str, entry: &Entry, names: &[String]) -> Option<String> {
    let count = names
        .iter()
        .filter(|n| !entry.tools.iter().any(|(known, _)| known == n))
        .count();
    (count > 0).then(|| {
        format!(
            "{}: {count} tools not in the catalog (class write)",
            shown(server)
        )
    })
}
