//! claude's model listing (`claude -p /model --output-format json`): run like
//! the codex one, through the jail and never bare, and cached the same way.

use super::*;
use crate::core::ai::parse::claude_tests::REAL_REPLY;

/// A fake claude: `body` answers `-p /model`, anything else is the version
/// probe.
fn fake_claude(lab: &Lab, body: &str) -> PathBuf {
    write_script(
        &lab.bin,
        "claude",
        &format!(
            "REC='{}'\nif [ \"$1\" = -p ] && [ \"$2\" = /model ]; then\n{body}\nelse echo '2.1.322 (Claude Code)'; fi",
            lab.rec.display()
        ),
        0o755,
    )
}

/// A fake claude that counts its listing runs and prints the real reply.
fn counting_claude(lab: &Lab) {
    let data = lab.rec.join("reply.json");
    fs::write(&data, REAL_REPLY).unwrap();
    fake_claude(
        lab,
        &format!(
            "echo run >> '{}/runs'\ncat '{}'",
            lab.rec.display(),
            data.display()
        ),
    );
}

/// The helper's argv with its empty entries (`--tools ""`) kept.
fn raw_helper_args(lab: &Lab) -> Vec<String> {
    let text = fs::read_to_string(lab.rec.join("helper_args")).unwrap_or_default();
    let mut args: Vec<String> = text.split('\0').map(str::to_owned).collect();
    args.pop();
    args
}

const ALIASES: usize = 9;

#[tokio::test]
async fn listing_runs_through_the_claude_jail_with_text_only_flags() {
    let lab = lab();
    counting_claude(&lab);
    let env = env_of(&lab, true);
    assert_eq!(
        claude_listing(&env).await,
        Ok(REAL_REPLY.as_bytes().to_vec())
    );
    let args = raw_helper_args(&lab);
    assert_eq!(args[0], "__jail-exec");
    // After `--`: the script's interpreter, then claude and its argv.
    let program = args.iter().rposition(|a| a.ends_with("/claude")).unwrap();
    assert!(args[..program].contains(&"--".to_owned()), "{args:?}");
    let tail = &args[program..];
    assert_eq!(
        &tail[1..],
        [
            "-p",
            "/model",
            "--output-format",
            "json",
            "--tools",
            "",
            "--strict-mcp-config",
            "--safe-mode",
            "--permission-prompts",
            "none",
            "--no-session-persistence",
        ]
    );
}

#[tokio::test]
async fn listing_without_a_jail_never_starts_claude() {
    let lab = lab();
    fake_claude(&lab, &format!("echo x > '{}/ran'", lab.rec.display()));
    let env = env_of(&lab, false);
    assert_eq!(
        claude_listing(&env).await,
        Err("jail: the jail was not probed".to_owned())
    );
    assert!(!lab.rec.join("ran").exists());
    assert!(!lab.rec.join("helper_args").exists());
}

#[tokio::test]
async fn a_layout_the_jail_refuses_never_runs_claude_bare() {
    // A claude chat turn would go on bare here (the jail is its second
    // layer); the listing does not.
    let lab = lab();
    fake_claude(&lab, &format!("echo x > '{}/ran'", lab.rec.display()));
    let mut env = env_of(&lab, true);
    env.home = None;
    let got = claude_listing(&env).await;
    assert!(
        got.as_ref().is_err_and(|why| why.starts_with("jail: ")),
        "{got:?}"
    );
    assert!(!lab.rec.join("ran").exists(), "claude ran without the jail");
    assert!(!lab.rec.join("helper_args").exists());
}

#[tokio::test]
async fn a_helper_that_could_not_enforce_the_jail_stops_the_claude_listing() {
    let lab = lab();
    fake_claude(&lab, &format!("echo x > '{}/ran'", lab.rec.display()));
    let mut env = env_of(&lab, true);
    env.helper = write_script(
        &lab.rec,
        "helper",
        "echo 'lazarobox-jail: jail not enforced: ABI 2 is below the required 3' >&2\nexit 125",
        0o755,
    );
    assert_eq!(
        claude_listing(&env).await,
        Err("jail: ABI 2 is below the required 3".to_owned())
    );
    assert!(!lab.rec.join("ran").exists());
}

#[tokio::test]
async fn claude_listing_has_stdin_closed_a_private_cwd_and_a_scrubbed_env() {
    let lab = lab();
    fake_claude(
        &lab,
        "pwd\nif read line; then echo STDIN-OPEN; else echo STDIN-EOF; fi\necho \"agent=${SSH_AUTH_SOCK-unset}\"",
    );
    let mut env = env_of(&lab, true);
    env.inject = vec![("SSH_AUTH_SOCK".into(), "/tmp/agent".into())];
    let out = String::from_utf8(claude_listing(&env).await.unwrap()).unwrap();
    let mut lines = out.lines();
    let cwd = Path::new(lines.next().unwrap());
    assert!(
        cwd.starts_with(fs::canonicalize(&lab.data).unwrap()),
        "{cwd:?}"
    );
    assert_eq!(lines.next(), Some("STDIN-EOF"));
    assert_eq!(lines.next(), Some("agent=unset"));
}

#[tokio::test]
async fn a_hanging_claude_listing_times_out() {
    let lab = lab();
    fake_claude(&lab, "sleep 30");
    let started = std::time::Instant::now();
    let got = listing_of(
        &env_of(&lab, true),
        Cli::Claude,
        Duration::from_millis(300),
        CLAUDE_LISTING_CAP,
    )
    .await;
    assert_eq!(got, Err("timed out after 300 ms".to_owned()));
    assert!(started.elapsed() < Duration::from_secs(15));
}

#[tokio::test]
async fn a_failing_claude_listing_says_the_exit_code() {
    let lab = lab();
    fake_claude(&lab, "echo 'not logged in' >&2\nexit 1");
    assert_eq!(
        claude_listing(&env_of(&lab, true)).await,
        Err("claude exited 1: not logged in".to_owned())
    );
}

#[test]
fn the_claude_timeout_fits_a_cold_start() {
    // Measured cold on the user's machine: 2.7 s (claude 2.1.322).
    assert!(CLAUDE_LISTING_TIMEOUT > Duration::from_millis(2700));
    assert!(CLAUDE_LISTING_TIMEOUT <= Duration::from_secs(5));
    assert_eq!(LISTING_TIMEOUT, Duration::from_secs(4), "codex unchanged");
}

#[test]
fn claude_on_macos_is_never_listed() {
    use crate::core::ai::jail::Os;
    assert!(lists_claude(Os::Linux));
    assert!(!lists_claude(Os::Macos), "it would run bare there");
}

#[tokio::test]
async fn detect_reads_claude_and_codex_listings_but_not_gemini() {
    let lab = lab();
    counting_claude(&lab);
    let data = lab.rec.join("listing.json");
    fs::write(&data, LISTING).unwrap();
    fake_codex(&lab, &format!("cat {}", data.display()));
    write_script(
        &lab.bin,
        "gemini",
        &format!(
            "[ \"$1\" = --version ] || echo \"$@\" >> '{}/gemini'\necho 0.64.0",
            lab.rec.display()
        ),
        0o755,
    );
    let targets = cli_targets(&["claude", "codex", "gemini"]);
    let env = env_of(&lab, true);
    let cache = real_cache();
    let d = detect_in(
        &targets,
        &join(&[&lab.bin]),
        Some(lister(&env, &cache, false)),
    )
    .await;
    let get = |id| d.engines.iter().find(|e| e.id == id).unwrap().clone();
    let claude = get("claude");
    if lists_claude(crate::core::ai::jail::Os::CURRENT) {
        assert_eq!(claude.model_labels.len(), ALIASES);
        assert_eq!(claude.model_labels[0], ("sonnet".into(), "sonnet".into()));
    }
    assert_eq!(claude.version.as_deref(), Some("2.1.322 (Claude Code)"));
    assert_eq!(claude.diagnostics.models, None);
    assert_eq!(get("codex").model_labels.len(), 7);
    assert!(get("gemini").model_labels.is_empty());
    assert_eq!(get("gemini").diagnostics.models, None);
    assert!(
        !lab.rec.join("gemini").exists(),
        "gemini got more than --version"
    );
}

#[tokio::test]
async fn a_bad_claude_reply_in_detection_leaves_no_aliases_and_a_note() {
    let lab = lab();
    fake_claude(&lab, "echo 'Unknown command: /model'");
    let env = env_of(&lab, true);
    let cache = real_cache();
    let d = detect_in(
        &cli_targets(&["claude"]),
        &join(&[&lab.bin]),
        Some(lister(&env, &cache, false)),
    )
    .await;
    let claude = d.engines.iter().find(|e| e.id == "claude").unwrap();
    assert!(claude.model_labels.is_empty());
    assert_eq!(claude.models, ["default"]);
    if lists_claude(crate::core::ai::jail::Os::CURRENT) {
        assert_eq!(
            claude.diagnostics.models.as_deref(),
            Some(crate::core::ai::BAD_LISTING)
        );
    }
}

#[tokio::test]
async fn claude_and_codex_listings_are_cached_apart() {
    let lab = lab();
    counting_claude(&lab);
    fake_codex(
        &lab,
        &format!(
            "echo codex >> '{}/codex_runs'\necho '{{\"models\":[]}}'",
            lab.rec.display()
        ),
    );
    let env = env_of(&lab, true);
    let (clock, now) = fake_clock();
    let cache = ListingCache::new(clock);
    let claude = cache.listing_of(&env, Cli::Claude, false).await;
    assert_eq!(claude, Ok(REAL_REPLY.as_bytes().to_vec()));
    assert_eq!(
        cache.listing(&env, false).await,
        Ok(b"{\"models\":[]}\n".to_vec())
    );
    advance(&now, Duration::from_secs(9 * 60 + 59));
    assert_eq!(cache.listing_of(&env, Cli::Claude, false).await, claude);
    assert!(cache.listing(&env, false).await.is_ok());
    let codex_runs = fs::read_to_string(lab.rec.join("codex_runs")).unwrap();
    assert_eq!((runs(&lab), codex_runs.lines().count()), (1, 1));
    // `r` re-lists claude.
    let _ = cache.listing_of(&env, Cli::Claude, true).await;
    assert_eq!(runs(&lab), 2);
}

#[tokio::test]
async fn a_failing_claude_falls_back_to_its_cached_list_within_the_ttl() {
    let lab = lab();
    counting_claude(&lab);
    let env = env_of(&lab, true);
    let (clock, now) = fake_clock();
    let cache = ListingCache::new(clock);
    let good = cache.listing_of(&env, Cli::Claude, false).await;
    assert!(good.is_ok());
    fake_claude(&lab, "exit 2");
    advance(&now, Duration::from_secs(5 * 60));
    assert_eq!(cache.listing_of(&env, Cli::Claude, true).await, good);
    advance(&now, Duration::from_secs(5 * 60));
    assert_eq!(
        cache.listing_of(&env, Cli::Claude, false).await,
        Err("claude exited 2".to_owned())
    );
}

#[tokio::test]
async fn detection_reuses_the_claude_list_and_only_r_forces() {
    let lab = lab();
    counting_claude(&lab);
    let env = env_of(&lab, true);
    let (clock, _now) = fake_clock();
    let cache = ListingCache::new(clock);
    let targets = cli_targets(&["claude"]);
    let path = join(&[&lab.bin]);
    for force in [false, false, true] {
        let d = detect_in(&targets, &path, Some(lister(&env, &cache, force))).await;
        let claude = d.engines.iter().find(|e| e.id == "claude").unwrap();
        if lists_claude(crate::core::ai::jail::Os::CURRENT) {
            assert_eq!(claude.model_labels.len(), ALIASES, "force={force}");
        }
    }
    if lists_claude(crate::core::ai::jail::Os::CURRENT) {
        assert_eq!(
            runs(&lab),
            2,
            "startup + menu open share one run; r adds one"
        );
    }
}
