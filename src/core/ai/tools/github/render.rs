//! Turns the JSON of `gh` into the text a model reads. Every field is
//! attacker text: it is cleaned, stripped of HTML comments, flattened when it
//! is one line, redacted, then capped, in that order, so a cut can never split
//! a secret and a title cannot fake a header. We write every label ourselves;
//! the JSON only supplies values. Pure.

use std::fmt::Write;

use serde_json::Value;

use super::{GhCall, Slug};
use crate::core::ai::{
    memory::redact::redact,
    sanitize::clean,
    tools::label::{self, CAP_MARKER, OUTPUT_CAP},
};

const BODY_CAP: usize = 12 * 1024;
const COMMENT_CAP: usize = 3 * 1024;
const LINE_CAP: usize = 256;
const MAX_COMMENTS: usize = 30;
const MAX_FILES: usize = 100;

/// `gh` answered with JSON this build cannot read. It carries nothing: a
/// parse error can quote the text it choked on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Unreadable;

/// A finished result: the wrapped text for the model and a row summary made
/// of counts only, never of attacker text.
#[derive(PartialEq, Eq, Debug)]
pub struct Output {
    pub text: String,
    pub summary: String,
}

/// Reads a field the call did not promise: a missing or mistyped one is
/// empty, never an error, so an unknown `gh` shape still renders.
fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().to_owned()
}

fn num(v: &Value, key: &str) -> u64 {
    v[key].as_u64().unwrap_or_default()
}

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map_or(&[], Vec::as_slice)
}

/// The JSON of `gh`, an object or an array as the call expects.
fn parse(json: &str, array: bool) -> Result<Value, Unreadable> {
    let value: Value = serde_json::from_str(json).map_err(|_| Unreadable)?;
    if value.is_array() == array && (array || value.is_object()) {
        Ok(value)
    } else {
        Err(Unreadable)
    }
}

/// Collects whether any cap cut something.
#[derive(Default)]
struct Text {
    cut: bool,
}

impl Text {
    /// A multi-line field of at most `cap` bytes. Every line is prefixed, so
    /// no line of attacker text can start at column 0, where our own
    /// structure lines live.
    fn field(&mut self, raw: &str, cap: usize) -> String {
        let text = self.capped(redact(&strip_comments(&scrub(raw))).text, cap);
        text.split('\n')
            .map(|l| {
                if l.is_empty() {
                    "|".to_owned()
                } else {
                    format!("| {l}")
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// A one-line field: no newline survives.
    fn line(&mut self, raw: &str) -> String {
        let flat = strip_comments(&scrub(raw))
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        self.capped(redact(&flat).text, LINE_CAP)
    }

    fn capped(&mut self, mut text: String, cap: usize) -> String {
        if text.len() > cap {
            let end = (0..=cap)
                .rev()
                .find(|&i| text.is_char_boundary(i))
                .unwrap_or(0);
            text.truncate(end);
            text.push_str(" [cut]");
            self.cut = true;
        }
        text
    }

    /// The label names of an issue or pull request, comma separated.
    fn labels(&mut self, item: &Value) -> String {
        let names: Vec<String> = list(item, "labels")
            .iter()
            .map(|l| self.line(&text(l, "name")))
            .collect();
        names.join(", ")
    }

    /// The `login` of an author object.
    fn who(&mut self, item: &Value) -> String {
        self.line(&text(&item["author"], "login"))
    }
}

/// The shared sanitizer, then the characters it keeps but that show nothing:
/// variation selectors, the combining grapheme joiner, fillers, the braille
/// blank, the Mongolian vowel separator, interlinear annotations and musical
/// format controls. Only GitHub text loses them: the shared sanitizer keeps
/// U+FE0F so emoji elsewhere stay whole.
fn scrub(raw: &str) -> String {
    clean(raw).chars().filter(|&c| !invisible(c)).collect()
}

fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{034f}'
            | '\u{115f}'
            | '\u{1160}'
            | '\u{180e}'
            | '\u{2800}'
            | '\u{3164}'
            | '\u{fe00}'..='\u{fe0f}'
            | '\u{ffa0}'
            | '\u{fff9}'..='\u{fffb}'
            | '\u{1d173}'..='\u{1d17a}'
            | '\u{e0100}'..='\u{e01ef}'
    )
}

/// Drops `<!-- ... -->`, an unterminated one to the end. A removed comment
/// leaves nothing and the text around it is read again, so halves of a secret
/// or of an opener join and are caught. One pass, linear: after each
/// character the tail of the output is checked for an opener.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pos = 0;
    while let Some(c) = text[pos..].chars().next() {
        pos += c.len_utf8();
        out.push(c);
        if out.ends_with("<!--") {
            out.truncate(out.len() - 4);
            match text[pos..].find("-->") {
                Some(end) => pos += end + 3,
                None => break,
            }
        }
    }
    out
}

/// Renders `json` for `call` on `slug`, with `note` (ours, possibly empty) as
/// the first line, and wraps it as untrusted data under `nonce`.
pub fn render(
    nonce: &str,
    call: &GhCall,
    slug: &Slug,
    note: &str,
    json: &str,
) -> Result<Output, Unreadable> {
    let mut t = Text::default();
    let mut out = String::new();
    if !note.is_empty() {
        writeln!(out, "note: {}", t.line(note)).ok();
    }
    let summary = match *call {
        GhCall::IssueList { state, limit } => {
            let all = parse(json, true)?;
            let all = all.as_array().map_or(&[][..], Vec::as_slice);
            let shown = all.len().min(usize::from(limit));
            writeln!(
                out,
                "GitHub issues of {slug} ({}), {shown} shown:",
                state.word()
            )
            .ok();
            for item in &all[..shown] {
                let labels = t.labels(item);
                let labels = if labels.is_empty() {
                    labels
                } else {
                    format!(" ({labels})")
                };
                let (title, state) = (t.line(&text(item, "title")), t.line(&text(item, "state")));
                writeln!(
                    out,
                    "#{} [{}] {title}{labels}",
                    num(item, "number"),
                    state.to_lowercase()
                )
                .ok();
            }
            let more = all.len() > shown;
            if more {
                writeln!(out, "[more issues exist beyond the limit of {limit}]").ok();
            }
            format!("{shown} issues{}", if more { ", more exist" } else { "" })
        }
        GhCall::IssueView {
            number,
            comments: asked,
        } => {
            let item = parse(json, false)?;
            writeln!(out, "GitHub issue #{number} of {slug}").ok();
            head(&mut out, &mut t, &item);
            writeln!(out, "body:\n{}", t.field(&text(&item, "body"), BODY_CAP)).ok();
            let all = if asked { list(&item, "comments") } else { &[] };
            comments(&mut out, &mut t, all, asked);
            if all.is_empty() {
                format!("issue #{number}")
            } else {
                format!("issue #{number}, {} comments", all.len())
            }
        }
        GhCall::PrView { number } => {
            let item = parse(json, false)?;
            writeln!(out, "GitHub pull request #{number} of {slug}").ok();
            head(&mut out, &mut t, &item);
            let draft = if item["isDraft"].as_bool().unwrap_or(false) {
                "draft"
            } else {
                "ready"
            };
            let (head_ref, base_ref) = (
                t.line(&text(&item, "headRefName")),
                t.line(&text(&item, "baseRefName")),
            );
            writeln!(out, "branches: {head_ref} -> {base_ref} ({draft})").ok();
            let changed = num(&item, "changedFiles");
            writeln!(
                out,
                "changes: +{} -{} in {changed} files",
                num(&item, "additions"),
                num(&item, "deletions")
            )
            .ok();
            writeln!(
                out,
                "description:\n{}",
                t.field(&text(&item, "body"), BODY_CAP)
            )
            .ok();
            writeln!(out, "files:").ok();
            let files = list(&item, "files");
            for file in files.iter().take(MAX_FILES) {
                writeln!(
                    out,
                    "  {} +{} -{}",
                    t.line(&text(file, "path")),
                    num(file, "additions"),
                    num(file, "deletions")
                )
                .ok();
            }
            let listed = files.len().min(MAX_FILES) as u64;
            t.cut |= files.len() > MAX_FILES;
            if changed > listed {
                writeln!(out, "[{} more files are not listed]", changed - listed).ok();
            }
            writeln!(out, "checks:").ok();
            let checks = list(&item, "statusCheckRollup");
            for check in checks {
                let name = [text(check, "name"), text(check, "context")]
                    .into_iter()
                    .find(|n| !n.is_empty());
                let result = ["conclusion", "state", "status"]
                    .into_iter()
                    .map(|k| text(check, k))
                    .find(|r| !r.is_empty());
                let (name, result) = (
                    t.line(&name.unwrap_or_default()),
                    t.line(&result.unwrap_or_else(|| "unknown".into())),
                );
                writeln!(out, "  {name}: {}", result.to_lowercase()).ok();
            }
            format!("pr #{number}, {changed} files, {} checks", checks.len())
        }
    };
    // Keep room for the one marker, so the wrapper never cuts again.
    let room = OUTPUT_CAP - CAP_MARKER.len() - 1;
    if out.len() > room {
        let end = (0..=room)
            .rev()
            .find(|&i| out.is_char_boundary(i))
            .unwrap_or(0);
        out.truncate(end);
        out.push('\n');
        t.cut = true;
    }
    let cut = t.cut;
    if cut {
        out.push_str(CAP_MARKER);
    }
    Ok(Output {
        text: label::wrap(nonce, &out),
        summary: if cut {
            format!("{summary}, output cut")
        } else {
            summary
        },
    })
}

/// Title, state, author and labels of an issue or a pull request.
fn head(out: &mut String, t: &mut Text, item: &Value) {
    let (title, state, author, labels) = (
        t.line(&text(item, "title")),
        t.line(&text(item, "state")),
        t.who(item),
        t.labels(item),
    );
    writeln!(
        out,
        "title: {title}\nstate: {}\nauthor: {author}\nlabels: {labels}",
        state.to_lowercase()
    )
    .ok();
}

/// Room kept for the two lines that head the comments.
const COMMENT_HEADS: usize = 160;

/// The newest comments that fit the space left in the output, at most
/// [`MAX_COMMENTS`], oldest first. The older ones are left out, and the lines
/// say how many were shown and how many were not. Without `asked` the JSON is
/// never read for comments and a line of ours says so.
fn comments(out: &mut String, t: &mut Text, all: &[Value], asked: bool) {
    if !asked {
        writeln!(out, "comments: not requested").ok();
        return;
    }
    let budget = OUTPUT_CAP
        .saturating_sub(CAP_MARKER.len() + 1 + COMMENT_HEADS)
        .saturating_sub(out.len());
    let mut blocks = Vec::new();
    let mut used = 0;
    for comment in all.iter().rev().take(MAX_COMMENTS) {
        let (who, body) = (t.who(comment), t.field(&text(comment, "body"), COMMENT_CAP));
        let block = format!("--- {who}\n{body}\n");
        if used + block.len() > budget {
            break;
        }
        used += block.len();
        blocks.push(block);
    }
    let left_out = all.len() - blocks.len();
    if left_out > 0 {
        writeln!(out, "[older comments omitted: {left_out} of {}]", all.len()).ok();
        t.cut = true;
    }
    writeln!(out, "comments ({}):", blocks.len()).ok();
    for block in blocks.iter().rev() {
        out.push_str(block);
    }
}

#[cfg(test)]
mod tests;
