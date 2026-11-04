//! The pure core of the GitHub read tools: the validated call, the repository
//! slug read from `origin`, and the fixed `gh` argument vectors. No IO. The
//! model reaches `gh` only through the values checked here.

use std::fmt;

use serde_json::Value;

use super::{
    MAX_ARG_BYTES, ToolError,
    registry::{ArgDef, ArgKind, ArgValue, Parsed},
};

#[cfg(test)]
mod tests;

/// The most issues one `gh_issue_list` returns, and its default.
pub const LIST_MAX: i64 = 50;
const LIST_DEFAULT: u8 = 20;

const fn arg(name: &'static str, kind: ArgKind, required: bool) -> ArgDef {
    ArgDef {
        name,
        kind,
        required,
        gate: None,
    }
}

const NUMBER: ArgDef = arg(
    "number",
    ArgKind::Int {
        min: 1,
        max: u32::MAX as i64,
    },
    true,
);

pub const LIST_ARGS: &[ArgDef] = &[
    arg("state", ArgKind::Str { max: MAX_ARG_BYTES }, false),
    arg(
        "limit",
        ArgKind::Int {
            min: 1,
            max: LIST_MAX,
        },
        false,
    ),
];
pub const ISSUE_ARGS: &[ArgDef] = &[NUMBER, arg("comments", ArgKind::Bool, false)];
pub const PR_ARGS: &[ArgDef] = &[NUMBER];

/// Which issues a list asks for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Open,
    Closed,
    All,
}

impl State {
    pub(super) fn word(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Closed => "closed",
            State::All => "all",
        }
    }
}

/// A validated GitHub read. It holds only numbers, booleans and a closed set:
/// nothing the model wrote as text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GhCall {
    IssueList { state: State, limit: u8 },
    IssueView { number: u32, comments: bool },
    PrView { number: u32 },
}

impl GhCall {
    pub fn name(&self) -> &'static str {
        match self {
            GhCall::IssueList { .. } => "gh_issue_list",
            GhCall::IssueView { .. } => "gh_issue_view",
            GhCall::PrView { .. } => "gh_pr_view",
        }
    }

    /// What the call row shows. `comments` is visible, so the option is too.
    pub fn argument(&self) -> String {
        match *self {
            GhCall::IssueList { state, limit } => format!("{} (max {limit})", state.word()),
            GhCall::IssueView { number, comments } => {
                format!("#{number}{}", if comments { " comments" } else { "" })
            }
            GhCall::PrView { number } => format!("#{number}"),
        }
    }
}

/// Reads the values the table accepted. The table clamps an integer, so the
/// raw `number` is checked again: `0` must never read issue #1.
pub fn parse(name: &str, args: &Value, parsed: &Parsed) -> Result<GhCall, ToolError> {
    let number = || match args["number"].as_u64() {
        Some(n @ 1..=0xFFFF_FFFF) => Ok(n as u32),
        _ => Err(ToolError::WrongType),
    };
    match name {
        "gh_issue_list" => Ok(GhCall::IssueList {
            state: match parsed.get("state") {
                None => State::Open,
                Some(ArgValue::Str(s)) => match s.as_str() {
                    "open" => State::Open,
                    "closed" => State::Closed,
                    "all" => State::All,
                    _ => return Err(ToolError::WrongType),
                },
                Some(_) => return Err(ToolError::WrongType),
            },
            limit: match parsed.get("limit") {
                Some(ArgValue::Int(n)) => u8::try_from(*n).unwrap_or(LIST_DEFAULT),
                _ => LIST_DEFAULT,
            },
        }),
        "gh_issue_view" => Ok(GhCall::IssueView {
            number: number()?,
            comments: matches!(parsed.get("comments"), Some(ArgValue::Bool(true))),
        }),
        "gh_pr_view" => Ok(GhCall::PrView { number: number()? }),
        _ => Err(ToolError::UnknownTool),
    }
}

/// `owner/name` of a GitHub repository, checked character by character.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Slug {
    owner: String,
    repo: String,
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.owner, self.repo)
    }
}

fn valid(part: &str, max: usize, dots: bool) -> bool {
    !part.is_empty()
        && part.len() <= max
        && !part.starts_with('-')
        && !matches!(part, "." | "..")
        && part
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || (dots && matches!(b, b'.' | b'_')))
}

/// The repository of an `origin` URL, only when the host is exactly
/// `github.com`: https, `ssh://`, `git://` and the `git@host:path` form. A
/// user or token in the URL is dropped, never kept.
pub fn slug_of(url: &str) -> Option<Slug> {
    let url = url.trim();
    let (authority, path) = match ["https://", "ssh://", "git://"]
        .iter()
        .find_map(|scheme| url.strip_prefix(scheme))
    {
        Some(rest) => rest.split_once('/')?,
        None if url.contains("://") => return None,
        None => url.split_once(':')?,
    };
    let host = authority.rsplit('@').next()?;
    if !host.eq_ignore_ascii_case("github.com") {
        return None;
    }
    let path = path.trim_end_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    let (owner, repo) = path.split_once('/')?;
    (valid(owner, 39, false) && valid(repo, 100, true)).then(|| Slug {
        owner: owner.to_owned(),
        repo: repo.to_owned(),
    })
}

const ISSUE_FIELDS: &str = "number,title,state,author,labels,body,createdAt,updatedAt,url";
const PR_FIELDS: &str = "number,title,state,isDraft,author,body,baseRefName,headRefName,files,changedFiles,additions,deletions,statusCheckRollup,url";

/// The exact `gh` argument vector of a call. `--repo` carries the host, so a
/// user logged in elsewhere cannot redirect it. The list asks for one more
/// than `limit`, so "more exist" is a fact.
pub fn argv(call: &GhCall, slug: &Slug) -> Vec<String> {
    let repo = format!("github.com/{slug}");
    match *call {
        GhCall::IssueList { state, limit } => vec![
            "issue".into(),
            "list".into(),
            "--repo".into(),
            repo,
            "--state".into(),
            state.word().into(),
            "--limit".into(),
            (u16::from(limit) + 1).to_string(),
            "--json".into(),
            "number,title,state,author,labels,updatedAt".into(),
        ],
        GhCall::IssueView { number, comments } => {
            let fields = if comments {
                format!("{ISSUE_FIELDS},comments")
            } else {
                ISSUE_FIELDS.to_owned()
            };
            view("issue", number, repo, fields)
        }
        GhCall::PrView { number } => view("pr", number, repo, PR_FIELDS.to_owned()),
    }
}

fn view(kind: &str, number: u32, repo: String, fields: String) -> Vec<String> {
    [
        kind,
        "view",
        &number.to_string(),
        "--repo",
        &repo,
        "--json",
        &fields,
    ]
    .map(String::from)
    .to_vec()
}

/// The fork probe: names only.
pub fn fork_argv(slug: &Slug) -> Vec<String> {
    [
        "repo",
        "view",
        &format!("github.com/{slug}"),
        "--json",
        "isFork,parent",
    ]
    .map(String::from)
    .to_vec()
}
