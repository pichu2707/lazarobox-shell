//! The edge of the GitHub star prompt (goal #744): the mark file, the count
//! of days with a chat answer, and the user's own `gh` (`gh api
//! user/starred/<repo>`, read to check and `PUT` to star). The rules live in
//! `core::star`. Nothing runs when `[notices] star = false`, and `gh` is
//! asked only once the other conditions hold. The star itself is sent only by
//! [`star`], which the app runs on the user's `g`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use rusqlite::{Connection, OpenFlags};
use tokio::time::Instant;

use crate::core::star::{self, Due, Mark, StarReply};
use crate::runtime::desktop::which;
use crate::runtime::gh::{Run, run};
use crate::runtime::update::{data_file, read_small, write_private};

/// How long one `gh api` run may take.
pub const GH_TIMEOUT: Duration = Duration::from_secs(10);

/// The most bytes of a mark file read.
const MARK_CAP: u64 = 1024;

/// How long the day count waits for the chat store's lock.
const DB_BUSY: Duration = Duration::from_secs(1);

/// `$XDG_DATA_HOME/lazarobox/star.json`, else `~/.local/share/lazarobox/star.json`.
pub fn mark_path(xdg: Option<&OsStr>, home: Option<&OsStr>) -> Option<PathBuf> {
    data_file(xdg, home, "star.json")
}

/// The mark, or `None` when it is missing or damaged (never asked).
pub fn read_mark(path: &Path) -> Option<Mark> {
    Mark::parse(&read_small(path, MARK_CAP)?)
}

/// Writes the mark, mode 0600, through a temporary file.
pub fn write_mark(path: &Path, mark: &Mark) -> std::io::Result<()> {
    write_private(path, &mark.to_json())
}

/// The mark of an answer to the prompt: `dismissed` by lazarobox `current`
/// at `now`, else answered for good.
pub fn answer_mark(dismissed: bool, current: &str, now: u64) -> Mark {
    if dismissed {
        Mark::Dismissed {
            version: current.to_owned(),
            at: now,
        }
    } else {
        Mark::Done
    }
}

/// The distinct local days with a completed chat answer in the chat store at
/// `db`, read only. `None` when it cannot be read: no prompt then.
pub fn answer_days(db: &Path) -> Option<u64> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let conn = Connection::open_with_flags(db, flags).ok()?;
    conn.busy_timeout(DB_BUSY).ok()?;
    conn.query_row(
        "SELECT COUNT(DISTINCT date(created_at, 'unixepoch', 'localtime')) FROM messages
         WHERE role = 'assistant' AND status = 'complete'",
        [],
        |row| row.get::<_, i64>(0),
    )
    .ok()
    .and_then(|days| u64::try_from(days).ok())
}

/// What `gh` says about the star.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check {
    Starred,
    NotStarred,
    /// `gh` is missing or not logged in: nothing was asked.
    NoGh,
    /// Any other failure (network, rate limit): nothing is known.
    Unknown,
}

/// The `gh api` path of the star.
fn endpoint() -> String {
    format!("user/starred/{}", star::REPO)
}

/// Asks the `gh` found on `path` whether the user starred the repository.
pub async fn check(path: &OsStr) -> Check {
    let Some(program) = which("gh", Some(path)) else {
        return Check::NoGh;
    };
    let endpoint = endpoint();
    let deadline = Instant::now() + GH_TIMEOUT;
    match run(&program, &["api", &endpoint], None, deadline).await {
        Run::Printed(_) => Check::Starred,
        Run::NotLoggedIn => Check::NoGh,
        Run::Failed(message) if message.contains("HTTP 404") => Check::NotStarred,
        Run::Failed(_) => Check::Unknown,
    }
}

/// Stars the repository with the `gh` found on `path`. Runs only on the
/// user's `g`.
pub async fn star(path: &OsStr) -> StarReply {
    let Some(program) = which("gh", Some(path)) else {
        return StarReply::NoGh;
    };
    let endpoint = endpoint();
    let deadline = Instant::now() + GH_TIMEOUT;
    match run(&program, &["api", "-X", "PUT", &endpoint], None, deadline).await {
        Run::Printed(_) => StarReply::Starred,
        Run::NotLoggedIn => StarReply::NoGh,
        Run::Failed(message) => StarReply::Failed(first_line(&message)),
    }
}

/// The first non-empty line of `gh`'s message, for a one-line notice.
fn first_line(message: &str) -> String {
    message
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("failed")
        .to_owned()
}

/// Where the start-up check reads and writes.
pub struct Places<'a> {
    pub mark: Option<&'a Path>,
    pub db: Option<&'a Path>,
    /// The login PATH `gh` is looked up on.
    pub gh_path: &'a OsStr,
}

/// The start-up decision, run off the UI thread: the mark, then the days of
/// use, then `gh`. A star `gh` reports is marked, so it is not asked again.
pub async fn startup(enabled: bool, places: Places<'_>, now: u64, current: &str) -> Due {
    let (true, Some(mark_file), Some(db)) = (enabled, places.mark, places.db) else {
        return Due::No;
    };
    if !star::may_ask(read_mark(mark_file).as_ref(), current, now) {
        return Due::No;
    }
    let db = db.to_path_buf();
    let days = tokio::task::spawn_blocking(move || answer_days(&db))
        .await
        .ok()
        .flatten();
    if days.is_none_or(|days| days < star::MIN_DAYS) {
        return Due::No;
    }
    match check(places.gh_path).await {
        Check::Starred => {
            // A mark that cannot be written only costs a check next start.
            let _ = write_mark(mark_file, &Mark::Done);
            Due::No
        }
        Check::NotStarred => Due::Ask { gh: true },
        Check::NoGh => Due::Ask { gh: false },
        Check::Unknown => Due::No,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    const DAY: i64 = 24 * 60 * 60;
    const NOW: u64 = 1_800_000_000;

    /// A folder holding a fake `gh` running `script` (sh); `$DIR` is the folder.
    fn fake(script: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("gh");
        fs::write(
            &program,
            format!("#!/bin/sh\nDIR='{}'\n{script}", dir.path().display()),
        )
        .unwrap();
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        dir
    }

    /// Records the arguments, one per line.
    const RECORD: &str = "printf '%s\\n' \"$@\" >> \"$DIR/args\"\n";
    const NOT_FOUND: &str = "echo 'gh: Not Found (HTTP 404)' >&2\nexit 1\n";
    const NO_LOGIN: &str =
        "echo 'To get started with GitHub CLI, please run:  gh auth login' >&2\nexit 4\n";

    fn args(dir: &tempfile::TempDir) -> String {
        fs::read_to_string(dir.path().join("args")).unwrap_or_default()
    }

    /// A chat store with one completed answer on each of `days` days, plus
    /// a user line and an unfinished answer that do not count.
    fn store(dir: &Path, days: i64) -> PathBuf {
        let db = dir.join("chat.db");
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(
            "CREATE TABLE messages (conversation_id INTEGER, seq INTEGER, role TEXT,
             content TEXT, engine TEXT, model TEXT, status TEXT, created_at INTEGER)",
        )
        .unwrap();
        let noon = 1_700_000_000 - 1_700_000_000 % DAY + DAY / 2;
        for day in 0..days {
            let at = noon + day * DAY;
            for (role, status) in [
                ("assistant", "complete"),
                ("assistant", "complete"),
                ("user", "complete"),
                ("assistant", "error"),
            ] {
                conn.execute(
                    "INSERT INTO messages VALUES (1, 0, ?1, 'x', 'e', 'm', ?2, ?3)",
                    rusqlite::params![role, status, at],
                )
                .unwrap();
            }
        }
        // Unfinished answers on other days count for nothing.
        conn.execute(
            "INSERT INTO messages VALUES (1, 0, 'assistant', 'x', 'e', 'm', 'streaming', ?1)",
            [noon + 100 * DAY],
        )
        .unwrap();
        db
    }

    #[test]
    fn days_count_distinct_days_with_a_completed_answer() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(answer_days(&store(dir.path(), 3)), Some(3));
        assert_eq!(answer_days(&dir.path().join("missing.db")), None);
    }

    #[test]
    fn a_mark_survives_a_write_and_a_bad_file_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("lazarobox/star.json");
        assert_eq!(read_mark(&path), None);
        let mark = Mark::Dismissed {
            version: "1.26.0".into(),
            at: 5,
        };
        write_mark(&path, &mark).unwrap();
        assert_eq!(read_mark(&path), Some(mark));
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        fs::write(&path, "{").unwrap();
        assert_eq!(read_mark(&path), None);
    }

    #[test]
    fn an_answer_marks_a_dismissal_with_the_version_and_time_else_done() {
        assert_eq!(
            answer_mark(true, "1.27.0", NOW),
            Mark::Dismissed {
                version: "1.27.0".into(),
                at: NOW
            }
        );
        assert_eq!(answer_mark(false, "1.27.0", NOW), Mark::Done);
    }

    #[test]
    fn the_mark_lives_next_to_the_chat_store() {
        assert_eq!(
            mark_path(Some(OsStr::new("/x/data")), Some(OsStr::new("/home/u"))),
            Some(PathBuf::from("/x/data/lazarobox/star.json"))
        );
        assert_eq!(
            mark_path(None, Some(OsStr::new("/home/u"))),
            Some(PathBuf::from("/home/u/.local/share/lazarobox/star.json"))
        );
    }

    #[tokio::test]
    async fn check_reads_the_star_with_the_users_gh() {
        let starred = fake(&format!("{RECORD}exit 0\n"));
        assert_eq!(check(starred.path().as_os_str()).await, Check::Starred);
        assert_eq!(
            args(&starred),
            "api\nuser/starred/pichu2707/lazarobox-shell\n"
        );
        let not = fake(NOT_FOUND);
        assert_eq!(check(not.path().as_os_str()).await, Check::NotStarred);
        let logged_out = fake(NO_LOGIN);
        assert_eq!(check(logged_out.path().as_os_str()).await, Check::NoGh);
        let offline = fake("echo 'error connecting to api.github.com' >&2\nexit 1\n");
        assert_eq!(check(offline.path().as_os_str()).await, Check::Unknown);
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(check(empty.path().as_os_str()).await, Check::NoGh);
    }

    #[tokio::test]
    async fn star_puts_the_star_with_the_users_gh() {
        let ok = fake(&format!("{RECORD}exit 0\n"));
        assert_eq!(star(ok.path().as_os_str()).await, StarReply::Starred);
        assert_eq!(
            args(&ok),
            "api\n-X\nPUT\nuser/starred/pichu2707/lazarobox-shell\n"
        );
        let logged_out = fake(NO_LOGIN);
        assert_eq!(star(logged_out.path().as_os_str()).await, StarReply::NoGh);
        let refused = fake("echo '' >&2\necho 'gh: Forbidden (HTTP 403)' >&2\nexit 1\n");
        assert_eq!(
            star(refused.path().as_os_str()).await,
            StarReply::Failed("gh: Forbidden (HTTP 403)".into())
        );
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(star(empty.path().as_os_str()).await, StarReply::NoGh);
    }

    struct Lab {
        dir: tempfile::TempDir,
        gh: tempfile::TempDir,
    }

    impl Lab {
        fn new(days: i64, gh: &str) -> Self {
            let dir = tempfile::tempdir().unwrap();
            store(dir.path(), days);
            Self { dir, gh: fake(gh) }
        }

        fn mark(&self) -> PathBuf {
            self.dir.path().join("star.json")
        }

        async fn startup(&self, enabled: bool, current: &str) -> Due {
            let (mark, db) = (self.mark(), self.dir.path().join("chat.db"));
            let places = Places {
                mark: Some(&mark),
                db: Some(&db),
                gh_path: self.gh.path().as_os_str(),
            };
            startup(enabled, places, NOW, current).await
        }
    }

    #[tokio::test]
    async fn startup_asks_after_five_days_when_not_starred() {
        let lab = Lab::new(5, &format!("{RECORD}{NOT_FOUND}"));
        assert_eq!(lab.startup(true, "1.26.0").await, Due::Ask { gh: true });
        let lab = Lab::new(5, NO_LOGIN);
        assert_eq!(lab.startup(true, "1.26.0").await, Due::Ask { gh: false });
    }

    #[tokio::test]
    async fn startup_asks_nothing_and_runs_no_gh_before_five_days_or_when_off() {
        let lab = Lab::new(4, &format!("{RECORD}{NOT_FOUND}"));
        assert_eq!(lab.startup(true, "1.26.0").await, Due::No);
        let off = Lab::new(9, &format!("{RECORD}{NOT_FOUND}"));
        assert_eq!(off.startup(false, "1.26.0").await, Due::No);
        assert_eq!(args(&lab.gh), "");
        assert_eq!(args(&off.gh), "");
    }

    #[tokio::test]
    async fn a_star_seen_at_startup_is_marked_and_never_checked_again() {
        let lab = Lab::new(5, &format!("{RECORD}exit 0\n"));
        assert_eq!(lab.startup(true, "1.26.0").await, Due::No);
        assert_eq!(read_mark(&lab.mark()), Some(Mark::Done));
        assert_eq!(lab.startup(true, "9.0.0").await, Due::No);
        assert_eq!(args(&lab.gh).lines().count(), 2, "one check only");
    }

    #[tokio::test]
    async fn a_dismissal_waits_for_a_newer_version_and_60_days() {
        let lab = Lab::new(5, NOT_FOUND);
        let at = NOW - 60 * 24 * 60 * 60;
        write_mark(
            &lab.mark(),
            &Mark::Dismissed {
                version: "1.26.0".into(),
                at,
            },
        )
        .unwrap();
        assert_eq!(lab.startup(true, "1.26.0").await, Due::No);
        assert_eq!(lab.startup(true, "1.27.0").await, Due::Ask { gh: true });
    }

    #[tokio::test]
    async fn an_unknown_answer_from_gh_asks_nothing() {
        let lab = Lab::new(5, "echo 'error connecting to api.github.com' >&2\nexit 1\n");
        assert_eq!(lab.startup(true, "1.26.0").await, Due::No);
        assert_eq!(read_mark(&lab.mark()), None);
    }
}
