//! The check of a handoff against the worker's change ledger (agent-
//! orchestrator W4, D28). Pure and conservative: it reads the files the
//! handoff names and flags those the ledger does not hold. It is evidence
//! for the user and the coordinator, never a verdict, and a miss is better
//! than a false flag.

use super::changes::ChangeLedger;
use super::ledger::{NONE, Verdict, cut};

/// The most claimed paths one worker keeps.
const MAX_CLAIMS: usize = 32;

/// The most of a handoff's text that is read.
const SCAN_BYTES: usize = 4 * 1024;

/// The most flagged paths a line lists.
const SHOWN: usize = 8;

/// The longest path a line shows.
const PATH_BYTES: usize = 80;

/// What a name needs to be read as a file: one of these extensions.
const EXTENSIONS: &[&str] = &[
    "rs", "toml", "md", "json", "yaml", "yml", "js", "jsx", "ts", "tsx", "py", "go", "java", "kt",
    "c", "h", "cpp", "hpp", "cs", "rb", "php", "css", "scss", "html", "sh", "sql", "txt", "lock",
    "xml", "ini", "cfg", "lua", "swift", "zig", "vue", "svelte",
];

/// What the check of a handoff found.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Check {
    /// These claimed paths are not in the ledger, in the order claimed.
    Flagged(Vec<String>),
    /// Every path named is in the ledger.
    Matches,
    /// The check could not run, and why.
    NotChecked(&'static str),
}

/// The check of one worker's handoff. It keeps the paths ever claimed, so a
/// later handoff that drops a name cannot drop its flag.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Audit {
    claims: Vec<String>,
    /// Why there is nothing to check when no path was claimed.
    blank: &'static str,
    check: Check,
    idle: bool,
}

impl Audit {
    /// The check of `done` and the paths `prior` already claimed against
    /// `ledger`; `None` when the worker has no ledger.
    pub(super) fn new(
        prior: Option<&Audit>,
        done: &str,
        ledger: Option<&ChangeLedger>,
        verdict: Verdict,
    ) -> Self {
        let mut claims = prior.map(|audit| audit.claims.clone()).unwrap_or_default();
        for path in scan(done) {
            if claims.len() < MAX_CLAIMS && !claims.contains(&path) {
                claims.push(path);
            }
        }
        let blank = if done.trim() == NONE {
            "no handoff text"
        } else {
            "no file named in the handoff"
        };
        let blank = prior.map_or(blank, |audit| audit.blank);
        Self {
            claims,
            blank,
            check: Check::Matches,
            idle: false,
        }
        .recheck(ledger, verdict)
    }

    /// The same claims against the ledger as it is now.
    pub(super) fn recheck(&self, ledger: Option<&ChangeLedger>, verdict: Verdict) -> Self {
        let check = match ledger {
            None => Check::NotChecked("no change ledger"),
            Some(ledger) => self.check_against(ledger),
        };
        let idle = verdict == Verdict::OnCourse
            && ledger.is_some_and(|ledger| ledger.changes().iter().all(|c| !c.applied));
        Self {
            claims: self.claims.clone(),
            blank: self.blank,
            check,
            idle,
        }
    }

    fn check_against(&self, ledger: &ChangeLedger) -> Check {
        if self.claims.is_empty() {
            return Check::NotChecked(self.blank);
        }
        let held = |claim: &str| ledger.changes().iter().any(|c| same(&c.path, claim));
        // An absolute path may lie outside the root: it can match, never flag.
        let (absolute, relative): (Vec<_>, Vec<_>) =
            self.claims.iter().partition(|claim| claim.starts_with('/'));
        let missing: Vec<_> = relative
            .iter()
            .filter(|claim| !held(claim))
            .map(|claim| (*claim).clone())
            .collect();
        if !missing.is_empty() {
            Check::Flagged(missing)
        } else if relative.is_empty() && !absolute.iter().any(|claim| held(claim)) {
            Check::NotChecked("only paths outside the project named")
        } else {
            Check::Matches
        }
    }

    pub fn check(&self) -> &Check {
        &self.check
    }

    /// A write-capable worker was judged `on_course` and nothing it staged is
    /// applied (yet).
    pub fn idle(&self) -> bool {
        self.idle
    }

    /// Everything the check says, one plain ASCII line each.
    pub fn lines(&self) -> Vec<String> {
        let first = match &self.check {
            Check::Flagged(paths) => flagged_line(paths),
            Check::Matches => "claims match the ledger".to_owned(),
            Check::NotChecked(why) => format!("not checked: {why}"),
        };
        let mut lines = vec![first];
        lines.extend(self.idle.then(|| IDLE.to_owned()));
        lines
    }

    /// Only the lines that call for the user's eye.
    pub fn flags(&self) -> Vec<String> {
        let flagged = matches!(self.check, Check::Flagged(_));
        let lines = self.lines();
        let skip = usize::from(!flagged);
        lines.into_iter().skip(skip).collect()
    }
}

/// The neutral line of a worker judged `on_course` with nothing applied.
const IDLE: &str = "on_course, nothing applied yet";

fn flagged_line(paths: &[String]) -> String {
    let shown: Vec<_> = paths
        .iter()
        .take(SHOWN)
        .map(|p| cut(p, PATH_BYTES))
        .collect();
    let more = paths.len().saturating_sub(SHOWN);
    let tail = if more > 0 {
        format!(" (+{more} more)")
    } else {
        String::new()
    };
    format!("claimed, not in ledger: {}{tail}", shown.join(", "))
}

/// The path-like names of `text`, normalised, each once, in order. Only the
/// first [`SCAN_BYTES`] are read. A name is a run of `[A-Za-z0-9._/-]` whose
/// last segment ends in a known source extension.
fn scan(text: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for word in cut(text, SCAN_BYTES).split_whitespace() {
        if word.contains("://") || word.starts_with("www.") || word.starts_with("mailto:") {
            continue;
        }
        let pieces = word.split(|c: char| !(c.is_ascii_alphanumeric() || "._/-".contains(c)));
        for path in pieces.filter_map(as_path) {
            if found.len() < MAX_CLAIMS && !found.contains(&path) {
                found.push(path);
            }
        }
    }
    found
}

/// `piece` as a normalised path, or `None` when it is not clearly one: no
/// known extension, a directory, a `..` step, a version, a product name.
fn as_path(piece: &str) -> Option<String> {
    let mut rest = piece.trim_end_matches('.');
    while let Some(tail) = rest.strip_prefix("./") {
        rest = tail;
    }
    let absolute = rest.starts_with('/');
    let parts: Vec<_> = rest
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let name = parts.last()?;
    if rest.ends_with('/') || parts.contains(&"..") {
        return None;
    }
    let (stem, ext) = name.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    // `Node.js`, `Next.js`: a capitalised bare name is a product, not a file.
    let product = parts.len() == 1
        && matches!(ext.as_str(), "js" | "ts")
        && stem.starts_with(|c: char| c.is_ascii_uppercase());
    // `a.b.c` is not `a.b.c`: a one-letter extension after a dotted stem.
    let initials = ext.len() == 1 && stem.contains('.');
    if stem.is_empty() || !EXTENSIONS.contains(&ext.as_str()) || product || initials {
        return None;
    }
    let joined = parts.join("/");
    Some(if absolute {
        format!("/{joined}")
    } else {
        joined
    })
}

/// Whether the ledger path and the claimed path name the same file: equal, or
/// one a suffix of the other on a component boundary.
fn same(ledger_path: &str, claim: &str) -> bool {
    let ledger_path = ledger_path.trim_start_matches("./");
    ledger_path == claim
        || ends_with_component(ledger_path, claim)
        || ends_with_component(claim, ledger_path)
}

fn ends_with_component(long: &str, short: &str) -> bool {
    let Some(head) = long.strip_suffix(short) else {
        return false;
    };
    head.ends_with('/') && !short.is_empty()
}
