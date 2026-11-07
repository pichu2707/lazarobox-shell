//! The check of a handoff against the worker's change ledger (W4, D28). Pure
//! and conservative: evidence, never a verdict; a miss beats a false flag.

use super::changes::ChangeLedger;
use super::ledger::{NONE, Verdict, cut};

/// Bounds: claims kept per worker, handoff bytes read, flagged paths and
/// path bytes shown on a line.
const MAX_CLAIMS: usize = 32;
const SCAN_BYTES: usize = 4 * 1024;
const SHOWN: usize = 8;
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
    /// More names were left unread or unkept than the caps allow.
    capped: bool,
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
        let (found, mut capped) = scan(done);
        capped |= prior.is_some_and(|audit| audit.capped);
        for path in found {
            if claims.iter().any(|known| known.eq_ignore_ascii_case(&path)) {
                continue;
            }
            if claims.len() < MAX_CLAIMS {
                claims.push(path);
            } else {
                capped = true;
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
            capped,
        }
        .recheck(ledger, verdict)
    }

    /// The same claims against the ledger as it is now.
    pub(super) fn recheck(&self, ledger: Option<&ChangeLedger>, verdict: Verdict) -> Self {
        let check = match ledger {
            None => Check::NotChecked(NO_LEDGER),
            Some(ledger) => self.check_against(ledger),
        };
        // Only a writer that staged nothing: staged files waiting for the
        // user's review are the normal state and say nothing.
        let idle = verdict == Verdict::OnCourse && ledger.is_some_and(|l| l.changes().is_empty());
        Self {
            check,
            idle,
            ..self.clone()
        }
    }

    fn check_against(&self, ledger: &ChangeLedger) -> Check {
        if self.claims.is_empty() {
            return Check::NotChecked(self.blank);
        }
        let held = |claim: &str| ledger.changes().iter().any(|c| same(&c.path, claim));
        // Only a path with a directory in it can be flagged: a bare name may be
        // prose, and an absolute path may lie outside the project. Both can
        // still match.
        let (flaggable, other): (Vec<_>, Vec<_>) =
            (self.claims.iter()).partition(|claim| !claim.starts_with('/') && claim.contains('/'));
        let missing: Vec<_> = flaggable
            .iter()
            .filter(|claim| !held(claim))
            .map(|claim| (*claim).clone())
            .collect();
        if !missing.is_empty() {
            Check::Flagged(missing)
        } else if flaggable.is_empty() && !other.iter().any(|claim| held(claim)) {
            Check::NotChecked("no path with a directory named")
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

    /// What the ack and a collect say: the check, unless the worker has no
    /// ledger (a read-only worker is not told), and the neutral idle line.
    pub fn lines(&self) -> Vec<String> {
        if self.check == Check::NotChecked(NO_LEDGER) {
            return Vec::new();
        }
        let mut lines = self.note_lines();
        lines.extend(self.idle.then(|| IDLE.to_owned()));
        lines
    }

    /// What the stored note says: the check alone. The idle line is true only
    /// when it is read, so a note written at save time would go stale.
    pub fn note_lines(&self) -> Vec<String> {
        let mut line = match &self.check {
            Check::Flagged(paths) => flagged_line(paths),
            Check::Matches => "claims match the ledger".to_owned(),
            Check::NotChecked(why) => format!("not checked: {why}"),
        };
        if self.capped {
            line.push_str(" (+more not checked)");
        }
        vec![line]
    }

    /// Only the line that calls for the user's eye.
    pub fn flags(&self) -> Vec<String> {
        match self.check {
            Check::Flagged(_) => self.note_lines(),
            _ => Vec::new(),
        }
    }
}

/// The neutral line of a worker judged `on_course` that staged nothing.
const IDLE: &str = "on_course, nothing staged";

/// Why a read-only worker has nothing to check.
const NO_LEDGER: &str = "no change ledger";

fn flagged_line(paths: &[String]) -> String {
    let shown: Vec<_> = (paths.iter().take(SHOWN))
        .map(|path| match cut(path, PATH_BYTES) {
            head if head.len() < path.len() => format!("{head}..."),
            head => head.to_owned(),
        })
        .collect();
    let more = paths.len().saturating_sub(SHOWN);
    let tail = if more > 0 {
        format!(" (+{more} more)")
    } else {
        String::new()
    };
    format!(
        "named in handoff, not in ledger: {}{tail}",
        shown.join(", ")
    )
}

/// The path-like names of `text`, normalised, each once, in order, and whether
/// more were left. Only the first [`SCAN_BYTES`] are read. A name is a whole
/// word (a backslash reads as a slash; `, ; :` separate words and a quote, a
/// bracket or a full stop at an end is trimmed) made only of `[A-Za-z0-9._/-]`
/// and ending in a known source extension. A word with any other character,
/// such as `Über` or `[id]`, is dropped whole: cutting it would name another
/// file.
fn scan(text: &str) -> (Vec<String>, bool) {
    let mut capped = text.len() > SCAN_BYTES;
    let mut found: Vec<String> = Vec::new();
    for word in cut(text, SCAN_BYTES).split_whitespace() {
        if word.contains("://") || word.starts_with("www.") || word.starts_with("mailto:") {
            continue;
        }
        let word = word.replace('\\', "/");
        for piece in word.split([',', ';', ':']) {
            let piece = piece.trim_start_matches(['`', '\'', '"', '(']);
            let piece = piece.trim_end_matches(['`', '\'', '"', ')', '.']);
            if !piece
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._/-".contains(c))
            {
                continue;
            }
            let Some(path) = as_path(piece) else { continue };
            if found.len() >= MAX_CLAIMS {
                capped = true;
            } else if !found.iter().any(|known| known.eq_ignore_ascii_case(&path)) {
                found.push(path);
            }
        }
    }
    (found, capped)
}

/// `piece` as a normalised path, or `None` when it is not clearly one: no
/// known extension, a directory, a `..` step, a version, a product name, a
/// domain-like first component.
fn as_path(piece: &str) -> Option<String> {
    let mut rest = piece;
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
    // `github.com/a/b.rs`: a first component with a dot, which is no hidden
    // folder and no file, is a host.
    let host = !absolute
        && parts.len() > 1
        && parts[0]
            .rsplit_once('.')
            .is_some_and(|(head, ext)| !head.is_empty() && !is_source(ext));
    if stem.is_empty() || !is_source(&ext) || product || initials || host {
        return None;
    }
    let joined = parts.join("/");
    Some(if absolute {
        format!("/{joined}")
    } else {
        joined
    })
}

fn is_source(ext: &str) -> bool {
    EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str())
}

/// Whether the ledger path and the claimed path name the same file: equal, or
/// one a suffix of the other on a component boundary, ASCII case aside.
fn same(ledger_path: &str, claim: &str) -> bool {
    let ledger_path = ledger_path.trim_start_matches("./").to_ascii_lowercase();
    let claim = claim.to_ascii_lowercase();
    ledger_path == claim
        || ends_with_component(&ledger_path, &claim)
        || ends_with_component(&claim, &ledger_path)
}

fn ends_with_component(long: &str, short: &str) -> bool {
    let Some(head) = long.strip_suffix(short) else {
        return false;
    };
    head.ends_with('/') && !short.is_empty()
}
