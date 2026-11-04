//! Sends the Feedback issue with the user's own `gh` (open-links S4-S7):
//! `gh issue create --repo <owner/repo> --title <title> --body-file -
//! [--label <label>]`, found on the login PATH and run with no shell, the
//! body on stdin. lazarobox stores no GitHub token: `gh` uses its own
//! session (or `GH_TOKEN`/`GITHUB_TOKEN` from the environment it inherits).
//! Nothing it prints is logged.
//!
//! `gh` missing from PATH, or a failure that is `gh`'s "not logged in"
//! (exit code 4, or a message naming `gh auth login`), is
//! [`SendReply::NoGh`]: the app opens the prefilled page instead. A failure
//! naming a label or a permission is tried once more without labels.
//!
//! Before creating, a [`Request::Find`] lists the open issues like the new
//! one (`gh issue list --state open --search <words> --json number,title`):
//! when some share its key words the app shows them instead, and the user
//! may add the report to one (`gh issue comment <n> --body-file -`). A
//! search that fails or finds nothing is not an error: the issue is created.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::process::{Child, Command};
use tokio::time::Instant;

use crate::core::menu::feedback::{
    Issue, Request, SendReply, Similar, comment_body, comment_url, issue_url, search_query, similar,
};
use crate::runtime::desktop::which;

/// How long one `gh` run may take before it is killed.
pub const TIMEOUT: Duration = Duration::from_secs(30);
/// How long the search for similar issues may take: past it, the issue is
/// created as if none were found.
pub const SEARCH_TIMEOUT: Duration = Duration::from_secs(10);
/// The issues the search asks GitHub for, before keeping the similar ones.
const SEARCH_LIMIT: &str = "20";

/// `gh`'s exit code when it has no credential.
const AUTH_REQUIRED: i32 = 4;

/// The failure of a run cut short: the issue may exist all the same.
const TIMED_OUT: &str = "timed out; check GitHub before sending again";
/// The failure of a run that printed no issue link.
const NO_LINK: &str = "printed no issue link; check GitHub first";
/// The failure of a comment that printed no comment link.
const NO_COMMENT_LINK: &str = "printed no comment link; check GitHub first";

/// Does `request` with the `gh` found on `path`, the login PATH the engines
/// use. A search waits at most [`SEARCH_TIMEOUT`]; a create or a comment at
/// most [`TIMEOUT`], the retry included.
pub async fn send(path: &OsStr, request: &Request) -> SendReply {
    send_with(path, request, SEARCH_TIMEOUT, TIMEOUT).await
}

/// [`send`] with the timeouts given.
async fn send_with(
    path: &OsStr,
    request: &Request,
    search_timeout: Duration,
    timeout: Duration,
) -> SendReply {
    let Some(program) = which("gh", Some(path)) else {
        return SendReply::NoGh;
    };
    // One limit for the whole request: a slow search leaves the create
    // only what is left of it.
    let deadline = Instant::now() + timeout;
    match request {
        Request::Find(issue) => {
            let search = (Instant::now() + search_timeout).min(deadline);
            match find(&program, issue, search).await {
                Found::Some(issues) => SendReply::Similar(issues),
                Found::NotLoggedIn => SendReply::NoGh,
                Found::None => create(&program, issue, deadline).await,
            }
        }
        Request::Create(issue) => create(&program, issue, deadline).await,
        Request::Comment { issue, number } => comment(&program, issue, *number, deadline).await,
    }
}

/// What the search for similar issues found.
enum Found {
    Some(Vec<Similar>),
    /// Nothing like it, or the search failed: the issue is created.
    None,
    NotLoggedIn,
}

/// The open issues like `issue`, by the words of its title (see
/// [`search_query`] and [`similar`]).
async fn find(program: &Path, issue: &Issue, deadline: Instant) -> Found {
    let Some(query) = search_query(&issue.title) else {
        return Found::None;
    };
    let args = [
        "issue",
        "list",
        "--repo",
        &issue.repo,
        "--state",
        "open",
        "--limit",
        SEARCH_LIMIT,
        "--search",
        &query,
        "--json",
        "number,title",
    ];
    match run(program, &args, None, deadline).await {
        Run::Printed(stdout) => match similar(&issue.title, listed(&stdout)) {
            issues if issues.is_empty() => Found::None,
            issues => Found::Some(issues),
        },
        Run::NotLoggedIn => Found::NotLoggedIn,
        Run::Failed(_) => Found::None,
    }
}

/// The issues of `gh issue list --json number,title`: an entry without a
/// number or a title is skipped, and output that is not that list is none.
fn listed(stdout: &str) -> Vec<Similar> {
    let Ok(serde_json::Value::Array(entries)) = serde_json::from_str(stdout) else {
        return Vec::new();
    };
    (entries.iter())
        .filter_map(|entry| {
            Some(Similar {
                number: entry.get("number")?.as_u64()?,
                title: entry.get("title")?.as_str()?.to_owned(),
            })
        })
        .collect()
}

/// Adds `issue` to the issue `number` as a comment.
async fn comment(program: &Path, issue: &Issue, number: u64, deadline: Instant) -> SendReply {
    let shown = number.to_string();
    let args = [
        "issue",
        "comment",
        &shown,
        "--repo",
        &issue.repo,
        "--body-file",
        "-",
    ];
    match run(program, &args, Some(&comment_body(issue)), deadline).await {
        Run::Printed(stdout) => match comment_url(&stdout, &issue.repo, number) {
            Some(url) => SendReply::Commented(url),
            None => SendReply::Failed(NO_COMMENT_LINK.to_owned()),
        },
        Run::NotLoggedIn => SendReply::NoGh,
        Run::Failed(message) => SendReply::Failed(message),
    }
}

/// Creates `issue`, once more without labels when GitHub refuses them.
async fn create(program: &Path, issue: &Issue, deadline: Instant) -> SendReply {
    let mut run = run(program, &args(issue, true), Some(&issue.body), deadline).await;
    // Only once: the second answer is the answer.
    if let Run::Failed(message) = &run
        && !issue.labels.is_empty()
        && about_labels(message)
    {
        run = self::run(program, &args(issue, false), Some(&issue.body), deadline).await;
    }
    match run {
        Run::Printed(stdout) => match issue_url(&stdout, &issue.repo) {
            Some(url) => SendReply::Sent(url),
            None => SendReply::Failed(NO_LINK.to_owned()),
        },
        Run::NotLoggedIn => SendReply::NoGh,
        Run::Failed(message) => SendReply::Failed(message),
    }
}

/// How one `gh` run ended.
enum Run {
    /// Success: what it printed on stdout.
    Printed(String),
    /// It has no credential.
    NotLoggedIn,
    /// Anything else: its message.
    Failed(String),
}

/// Whether a failure names a label or a permission: GitHub keeps labels
/// only for users with write access.
fn about_labels(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("label") || message.contains("permission")
}

/// The arguments of `gh` for `issue`, with its labels or without them.
fn args(issue: &Issue, labels: bool) -> Vec<&str> {
    let mut args = vec![
        "issue",
        "create",
        "--repo",
        &issue.repo,
        "--title",
        &issue.title,
        "--body-file",
        "-",
    ];
    if labels {
        for label in &issue.labels {
            args.extend(["--label", label]);
        }
    }
    args
}

/// Starts `program` with `args`. A binary that is being written (an
/// upgrade, or a concurrent fork holding its write handle) fails with ETXTBSY
/// for a moment, so that one error is retried briefly.
async fn spawn(program: &Path, args: &[&str]) -> std::io::Result<Child> {
    let mut attempt = 0;
    loop {
        match Command::new(program)
            .args(args)
            .env("GH_PROMPT_DISABLED", "1")
            .env("GH_NO_UPDATE_NOTIFIER", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Err(error) if error.raw_os_error() == Some(libc::ETXTBSY) && attempt < 4 => {
                attempt += 1;
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            result => return result,
        }
    }
}

/// One run of `program` with `args` (no shell): `input` on stdin, stdout
/// and stderr captured, the environment inherited (`gh` reads its own config
/// and tokens there). Killed when `deadline` passes.
async fn run(program: &Path, args: &[&str], input: Option<&str>, deadline: Instant) -> Run {
    let mut child = match spawn(program, args).await {
        Ok(child) => child,
        Err(error) => return Run::Failed(error.to_string()),
    };
    let stdin = child.stdin.take();
    let work = async move {
        if let (Some(mut stdin), Some(input)) = (stdin, input) {
            // A `gh` that exits before reading says why on its own.
            let _ = stdin.write_all(input.as_bytes()).await;
        }
        child.wait_with_output().await
    };
    // On expiry the dropped child is killed.
    let output = match tokio::time::timeout_at(deadline, work).await {
        Err(_) => return Run::Failed(TIMED_OUT.to_owned()),
        Ok(Err(error)) => return Run::Failed(error.to_string()),
        Ok(Ok(output)) => output,
    };
    // A success first: the issue exists, whatever stderr says.
    if output.status.success() {
        return Run::Printed(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if output.status.code() == Some(AUTH_REQUIRED) || stderr.contains("gh auth login") {
        return Run::NotLoggedIn;
    }
    Run::Failed(if stderr.trim().is_empty() {
        output.status.to_string()
    } else {
        stderr
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::time::Instant;

    use super::*;

    const URL: &str = "https://github.com/pichu2707/lazarobox-shell/issues/9";

    fn issue(title: &str, body: &str) -> Issue {
        Issue {
            repo: "pichu2707/lazarobox-shell".to_owned(),
            title: title.to_owned(),
            body: body.to_owned(),
            labels: vec!["bug".to_owned()],
        }
    }

    /// A folder holding a fake `gh` running `script` (sh), which can write
    /// next to itself through `$DIR`.
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

    fn read(dir: &tempfile::TempDir, name: &str) -> String {
        fs::read_to_string(dir.path().join(name)).unwrap_or_default()
    }

    /// Creates `issue` with the fake `gh` of `dir`, as Ctrl+S on the similar
    /// issues does (no search first).
    async fn run(dir: &tempfile::TempDir, issue: &Issue) -> SendReply {
        let request = Request::Create(issue.clone());
        send_with(dir.path().as_os_str(), &request, TIMEOUT, TIMEOUT).await
    }

    /// Records every run's arguments, one per line, then a `--` line.
    const RECORD: &str = "printf '%s\\n' \"$@\" -- >> \"$DIR/args\"\n";

    // Spec (open-links S4): the argv is exactly the issue, with no shell,
    // and the body goes on stdin, both verbatim.
    #[tokio::test]
    async fn gh_gets_the_issue_as_arguments_and_the_body_on_stdin() {
        let dir = fake(&format!(
            "{RECORD}cat > \"$DIR/body\"\necho 'Creating issue' >&2\necho '{URL}'\n"
        ));
        let title = "$(touch pwned) `id`; echo x | y && 'q' \"w\"";
        let body = "### What happened\n\n$(rm -rf ~) `x` ; 'a' \"b\" > /dev/null\n";
        assert_eq!(
            run(&dir, &issue(title, body)).await,
            SendReply::Sent(URL.to_owned())
        );
        assert_eq!(
            read(&dir, "args"),
            format!(
                "issue\ncreate\n--repo\npichu2707/lazarobox-shell\n--title\n{title}\n\
                 --body-file\n-\n--label\nbug\n--\n"
            )
        );
        assert_eq!(read(&dir, "body"), body);
        assert!(!dir.path().join("pwned").exists());
    }

    // Spec (open-links S7): a success that prints no issue link is a
    // failure.
    #[tokio::test]
    async fn output_that_is_not_an_issue_link_is_a_failure() {
        for out in ["done", "https://github.com/someone/else/issues/9"] {
            let dir = fake(&format!("cat > /dev/null\necho '{out}'\n"));
            let reply = run(&dir, &issue("t", "b")).await;
            assert!(matches!(reply, SendReply::Failed(_)), "{out}: {reply:?}");
        }
    }

    // Spec (open-links S6): a label or permission error is tried exactly
    // once more, without labels.
    #[tokio::test]
    async fn a_label_error_is_retried_once_without_labels() {
        let dir = fake(&format!(
            "{RECORD}cat > /dev/null\n\
             case \"$*\" in *--label*) echo \"could not add label: 'bug' not found\" >&2; exit 1;; esac\n\
             echo '{URL}'\n"
        ));
        assert_eq!(
            run(&dir, &issue("t", "b")).await,
            SendReply::Sent(URL.to_owned())
        );
        let args = read(&dir, "args");
        let runs: Vec<&str> = args.split("--\n").collect();
        assert_eq!(runs.len(), 3, "{runs:?}");
        assert!(runs[0].ends_with("--label\nbug\n"), "{runs:?}");
        assert_eq!(
            runs[1],
            "issue\ncreate\n--repo\npichu2707/lazarobox-shell\n--title\nt\n--body-file\n-\n"
        );
        // A second refusal is the answer: never a loop.
        let dir = fake(&format!(
            "{RECORD}cat > /dev/null\necho 'GraphQL: permission denied' >&2\nexit 1\n"
        ));
        assert_eq!(
            run(&dir, &issue("t", "b")).await,
            SendReply::Failed("GraphQL: permission denied\n".to_owned())
        );
        assert_eq!(read(&dir, "args").matches("--\n").count(), 2);
        // Any other error is not retried.
        let dir = fake(&format!(
            "{RECORD}cat > /dev/null\necho 'HTTP 502: Bad gateway' >&2\nexit 1\n"
        ));
        assert_eq!(
            run(&dir, &issue("t", "b")).await,
            SendReply::Failed("HTTP 502: Bad gateway\n".to_owned())
        );
        assert_eq!(read(&dir, "args").matches("--\n").count(), 1);
    }

    // Spec (open-links S7): a `gh` that hangs is killed at the timeout.
    #[tokio::test]
    async fn a_hung_gh_is_killed_at_the_timeout() {
        let dir = fake("echo $$ > \"$DIR/pid\"\nexec sleep 30\n");
        let start = Instant::now();
        let reply = send_with(
            dir.path().as_os_str(),
            &Request::Create(issue("t", "b")),
            TIMEOUT,
            Duration::from_millis(300),
        )
        .await;
        assert!(start.elapsed() < Duration::from_secs(10));
        let SendReply::Failed(message) = reply else {
            panic!("{reply:?}");
        };
        assert!(message.contains("timed out"), "{message}");
        let pid = read(&dir, "pid").trim().to_owned();
        assert!(!pid.is_empty());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let stat = fs::read_to_string(format!("/proc/{pid}/stat")).unwrap_or_default();
            // Gone, or a zombie awaiting its reaper.
            if stat.is_empty() || stat.contains(") Z ") {
                break;
            }
            assert!(Instant::now() < deadline, "still running: {stat}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    // Spec (open-links S5): no `gh` on PATH, or one not logged in, is
    // `NoGh`, and the latter is not retried.
    #[tokio::test]
    async fn no_gh_or_not_logged_in_is_no_gh() {
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(run(&empty, &issue("t", "b")).await, SendReply::NoGh);
        let dir = fake(&format!(
            "{RECORD}cat > /dev/null\n\
             echo 'To get started with GitHub CLI, please run:  gh auth login' >&2\nexit 4\n"
        ));
        assert_eq!(run(&dir, &issue("t", "b")).await, SendReply::NoGh);
        assert_eq!(read(&dir, "args").matches("--\n").count(), 1);
        // The exit code alone is enough.
        let dir = fake("cat > /dev/null\nexit 4\n");
        assert_eq!(run(&dir, &issue("t", "b")).await, SendReply::NoGh);
    }

    // Spec (open-links S7): a run that succeeds is a sent issue, whatever
    // its stderr says: reading it as "not logged in" would send it twice.
    #[tokio::test]
    async fn a_success_is_sent_even_when_stderr_names_gh_auth_login() {
        let dir = fake(&format!(
            "cat > /dev/null\necho 'hint: gh auth login --with-token' >&2\necho '{URL}'\n"
        ));
        assert_eq!(
            run(&dir, &issue("t", "b")).await,
            SendReply::Sent(URL.to_owned())
        );
    }

    // Spec (open-links S7): the timeout bounds the whole send, the retry
    // without labels included.
    #[tokio::test]
    async fn the_retry_shares_the_timeout_of_the_first_run() {
        let dir = fake(
            "cat > /dev/null\n\
             case \"$*\" in *--label*) sleep 0.6; echo 'label not found' >&2; exit 1;; esac\n\
             exec sleep 30\n",
        );
        let start = Instant::now();
        let reply = send_with(
            dir.path().as_os_str(),
            &Request::Create(issue("t", "b")),
            TIMEOUT,
            Duration::from_secs(1),
        )
        .await;
        let elapsed = start.elapsed();
        let SendReply::Failed(message) = reply else {
            panic!("{reply:?}");
        };
        assert!(message.contains("timed out"), "{message}");
        // One timeout per run would take 1.6 s.
        assert!(elapsed < Duration::from_millis(1400), "{elapsed:?}");
    }

    // Issue duplicates.

    /// Looks for issues like `issue` with the fake `gh` of `dir`, as Ctrl+S
    /// in the form does, the search cut after `search`.
    async fn find(dir: &tempfile::TempDir, issue: &Issue, search: Duration) -> SendReply {
        let request = Request::Find(issue.clone());
        send_with(dir.path().as_os_str(), &request, search, TIMEOUT).await
    }

    /// A fake `gh` whose `issue list` runs `list` and whose `issue create`
    /// prints [`URL`]; every run is recorded.
    fn fake_list(list: &str) -> tempfile::TempDir {
        fake(&format!(
            "{RECORD}case \"$2\" in\n\
             list) {list};;\n\
             create) cat > /dev/null; echo '{URL}';;\n\
             esac\n"
        ))
    }

    const SEARCH_ARGS: &str = "issue\nlist\n--repo\npichu2707/lazarobox-shell\n--state\nopen\n\
         --limit\n20\n--search\nscroll OR jumps in:title,body\n--json\nnumber,title\n--\n";

    // S1-S3: the search finds open issues that share the title's key words:
    // they come back to the form and nothing is created.
    #[tokio::test]
    async fn similar_open_issues_come_back_and_nothing_is_created() {
        let dir = fake_list(
            "echo '[{\"number\":583,\"title\":\"Scroll jumps to the top\"},\
             {\"number\":1,\"title\":\"Slow start\"}]'",
        );
        assert_eq!(
            find(&dir, &issue("Scroll jumps", "b"), TIMEOUT).await,
            SendReply::Similar(vec![Similar {
                number: 583,
                title: "Scroll jumps to the top".to_owned(),
            }])
        );
        assert_eq!(read(&dir, "args"), SEARCH_ARGS);
    }

    // S3: with no similar issue, or a search that fails or hangs, the issue
    // is created as before.
    #[tokio::test]
    async fn without_similar_issues_the_issue_is_created() {
        for list in [
            "echo '[]'",
            "echo '[{\"number\":1,\"title\":\"Slow start\"}]'",
            "echo 'HTTP 502' >&2; exit 1",
            "echo 'not json'",
        ] {
            let dir = fake_list(list);
            let reply = find(&dir, &issue("Scroll jumps", "b"), TIMEOUT).await;
            assert_eq!(reply, SendReply::Sent(URL.to_owned()), "{list}");
            let args = read(&dir, "args");
            assert!(args.starts_with(SEARCH_ARGS), "{list}: {args}");
            assert!(args.contains("issue\ncreate\n"), "{list}: {args}");
        }
        let dir = fake_list("exec sleep 30");
        let start = Instant::now();
        let reply = find(
            &dir,
            &issue("Scroll jumps", "b"),
            Duration::from_millis(300),
        )
        .await;
        assert_eq!(reply, SendReply::Sent(URL.to_owned()));
        assert!(start.elapsed() < Duration::from_secs(10));
    }

    // S3: a title with no key word is not searched; a search refused for
    // the login is `NoGh`, like a create.
    #[tokio::test]
    async fn a_title_with_no_key_word_is_not_searched_and_no_login_is_no_gh() {
        let dir = fake_list("echo '[]'");
        assert_eq!(
            find(&dir, &issue("a: b", "body"), TIMEOUT).await,
            SendReply::Sent(URL.to_owned())
        );
        assert!(!read(&dir, "args").contains("list"));
        let dir = fake_list("echo 'run gh auth login' >&2; exit 4");
        assert_eq!(
            find(&dir, &issue("Scroll jumps", "b"), TIMEOUT).await,
            SendReply::NoGh
        );
        assert!(!read(&dir, "args").contains("create"));
    }

    // S2: Enter on a similar issue adds the report to it as a comment: the
    // number as an argument, the text on stdin, and the comment's link back.
    #[tokio::test]
    async fn the_report_is_added_to_the_issue_as_a_comment() {
        let link = "https://github.com/pichu2707/lazarobox-shell/issues/583#issuecomment-9";
        let dir = fake(&format!("{RECORD}cat > \"$DIR/body\"\necho '{link}'\n"));
        let issue = issue("Scroll jumps", "### What happened\n\n$(id) `x`\n");
        let request = Request::Comment {
            issue: issue.clone(),
            number: 583,
        };
        let reply = send_with(dir.path().as_os_str(), &request, TIMEOUT, TIMEOUT).await;
        assert_eq!(reply, SendReply::Commented(link.to_owned()));
        assert_eq!(
            read(&dir, "args"),
            "issue\ncomment\n583\n--repo\npichu2707/lazarobox-shell\n--body-file\n-\n--\n"
        );
        assert_eq!(read(&dir, "body"), comment_body(&issue));
        // A success that prints no comment link is a failure.
        let dir = fake(&format!("cat > /dev/null\necho '{URL}'\n"));
        let reply = send_with(dir.path().as_os_str(), &request, TIMEOUT, TIMEOUT).await;
        assert!(matches!(reply, SendReply::Failed(_)), "{reply:?}");
    }

    // Spec (Sending with gh): gh is killed 30 seconds after Ctrl+S, the
    // search included: a slow search leaves the create only the rest.
    #[tokio::test]
    async fn the_search_and_the_create_share_one_time_limit() {
        // The search hangs past its own limit, then the create hangs too.
        let dir = fake("cat > /dev/null\nexec sleep 30\n");
        let start = Instant::now();
        let reply = send_with(
            dir.path().as_os_str(),
            &Request::Find(issue("Scroll jumps", "b")),
            Duration::from_millis(500),
            Duration::from_millis(800),
        )
        .await;
        let elapsed = start.elapsed();
        assert!(matches!(reply, SendReply::Failed(_)), "{reply:?}");
        assert!(elapsed < Duration::from_millis(1100), "{elapsed:?}");
    }
}
