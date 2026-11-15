//! The pure edit of one server's `approval` key (openspec change
//! `mcp-connectors`, S9, design D34). The text is edited by byte ranges the
//! parser reports, so every other byte stays, comments and line endings
//! included; the result is kept only when `Config::parse` reads the same
//! servers but for that entry's level.

use toml_edit::{ImDocument, Item};

use super::{AppendRefusal, MAX_BYTES};
use crate::core::{
    ai::{
        config::{McpApproval, McpServer},
        mcp_secrets::{Refusal, line_of},
    },
    config::Config,
};

/// `old` with the `approval` of the one global `[[ai.mcp.servers]]` entry
/// named `name` set to `level`: the value replaced in place, the line added
/// after the entry's last key, or, for `ask`, the line removed.
pub fn edit_approval(old: &str, name: &str, level: McpApproval) -> Result<String, AppendRefusal> {
    let body = old.strip_prefix('\u{feff}').unwrap_or(old);
    let doc = ImDocument::parse(body).map_err(|e| {
        AppendRefusal::NotToml(line_of(body.as_bytes(), e.span().map_or(0, |s| s.start)))
    })?;
    let tables = match doc
        .as_item()
        .get("ai")
        .and_then(|a| a.get("mcp")?.get("servers"))
    {
        None => return Err(AppendRefusal::NotDeclared(name.to_owned())),
        Some(Item::ArrayOfTables(tables)) => tables,
        Some(_) => return Err(AppendRefusal::Inline),
    };
    let mut named = tables
        .iter()
        .filter(|t| t.get("name").and_then(Item::as_str) == Some(name));
    let (Some(entry), None) = (named.next(), named.next()) else {
        return Err(
            match tables
                .iter()
                .filter(|t| t.get("name").and_then(Item::as_str) == Some(name))
                .count()
            {
                0 => AppendRefusal::NotDeclared(name.to_owned()),
                _ => AppendRefusal::Twice,
            },
        );
    };
    if entry.contains_key("project") {
        return Err(AppendRefusal::Limited(name.to_owned()));
    }
    let quoted = format!("\"{}\"", level.key());
    let at_line_end = |from: usize| body[from..].find('\n').map_or(body.len(), |i| from + i + 1);
    let edited = match entry.get_key_value("approval") {
        Some((key, item)) => {
            let (Some(k), Some(v), true) = (key.span(), item.span(), item.is_value()) else {
                return Err(AppendRefusal::NotToml(1));
            };
            if level == McpApproval::Ask {
                let start = body[..k.start].rfind('\n').map_or(0, |i| i + 1);
                format!("{}{}", &body[..start], &body[at_line_end(v.end)..])
            } else {
                format!("{}{quoted}{}", &body[..v.start], &body[v.end..])
            }
        }
        None if level == McpApproval::Ask => return Err(AppendRefusal::Unchanged),
        None => {
            let last = entry
                .iter()
                .filter(|(_, item)| item.is_value())
                .filter_map(|(_, item)| item.span())
                .map(|span| span.end)
                .max()
                .unwrap_or(body.len());
            let end = at_line_end(last);
            let eol = if body[..end].ends_with("\r\n")
                || (!body.ends_with('\n') && body.contains("\r\n"))
            {
                "\r\n"
            } else {
                "\n"
            };
            let lead = if body[..end].ends_with('\n') { "" } else { eol };
            format!(
                "{}{lead}approval = {quoted}{eol}{}",
                &body[..end],
                &body[end..]
            )
        }
    };
    if edited == body {
        return Err(AppendRefusal::Unchanged);
    }
    let new = format!("{}{edited}", &old[..old.len() - body.len()]);
    if new.len() as u64 > MAX_BYTES {
        return Err(Refusal::TooLarge.into());
    }
    let (before, after) = match (Config::parse(body), Config::parse(&edited)) {
        (Ok(before), Ok(after)) => (before, after),
        _ => return Err(AppendRefusal::NotToml(1)),
    };
    let same = |old: &McpServer, new: &McpServer| {
        let own = old.name == name && old.project.is_none();
        let want = McpServer {
            approval: if own { level } else { old.approval },
            ..old.clone()
        };
        want == *new
    };
    let (a, b) = (before.ai.mcp_servers(), after.ai.mcp_servers());
    let kept = a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| same(x, y))
        && b.iter().any(|s| s.name == name && s.approval == level)
        && after.problems.iter().all(|p| before.problems.contains(p));
    if kept {
        Ok(new)
    } else {
        Err(AppendRefusal::ServersDiffer)
    }
}

#[cfg(test)]
mod tests;
