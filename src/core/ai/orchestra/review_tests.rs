//! The risk assessment of a staged change set (agent-orchestrator S7b, D16).

use super::changes::Change;
use super::review::{Raise, RaiseError, Source, Tier, assess};

fn change(path: &str) -> Change {
    Change {
        path: path.to_owned(),
        size: Some(1),
        hash: "h".to_owned(),
        applied: false,
    }
}

fn tier(paths: &[&str]) -> Tier {
    let changes: Vec<Change> = paths.iter().map(|p| change(p)).collect();
    assess(&changes, None).tier
}

fn raise(tier: &str, reason: &str) -> Raise {
    Raise::new(tier, reason).expect("a valid raise")
}

// Spec: A lockfile is high.
#[test]
fn a_lockfile_is_high_whatever_the_orchestrator_said() {
    let changes = [change("Cargo.lock")];
    let low = raise("passive", "just a bump");
    let got = assess(&changes, Some(&low));
    assert_eq!(got.tier, Tier::High);
    assert_eq!(got.source, Source::Rules);
}

// Spec: The orchestrator cannot lower.
#[test]
fn the_orchestrator_cannot_lower_a_high_floor() {
    let changes = [change(".github/workflows/ci.yml"), change("src/lib.rs")];
    let low = raise("passive", "harmless");
    assert_eq!(assess(&changes, Some(&low)).tier, Tier::High);
}

#[test]
fn a_plain_source_file_is_passive_on_the_rules() {
    let got = assess(&[change("src/lib.rs"), change("docs/notes.md")], None);
    assert_eq!(got.tier, Tier::Passive);
    assert_eq!(got.source, Source::Rules);
}

#[test]
fn the_orchestrator_raises_and_its_reason_is_kept() {
    let changes = [change("src/auth.rs")];
    let up = raise("medium", "touches login");
    let got = assess(&changes, Some(&up));
    assert_eq!(got.tier, Tier::Medium);
    assert_eq!(got.source, Source::Raised("touches login".to_owned()));
    let high = raise("HIGH", "rewrites the session handling");
    assert_eq!(assess(&changes, Some(&high)).tier, Tier::High);
}

#[test]
fn a_raise_that_does_not_exceed_the_floor_leaves_the_rules_as_source() {
    let up = raise("high", "same");
    let got = assess(&[change("Cargo.toml")], Some(&up));
    assert_eq!(got.tier, Tier::High);
    assert_eq!(got.source, Source::Rules);
}

#[test]
fn the_highest_path_decides() {
    assert_eq!(tier(&["src/a.rs", "Makefile"]), Tier::High);
}

#[test]
fn build_ci_dependency_and_config_paths_are_high() {
    for path in [
        "build.rs",
        "Cargo.toml",
        "package.json",
        "package-lock.json",
        "yarn.lock",
        "go.sum",
        "requirements-dev.txt",
        "Makefile",
        "Dockerfile",
        "docker/app.Dockerfile",
        ".github/workflows/x.yml",
        ".gitlab-ci.yml",
        ".circleci/config.yml",
        "Jenkinsfile",
        "config/app.yaml",
        "settings.json",
        "tool.ini",
        "rust-toolchain.toml",
        ".cargo/config",
        ".git/hooks/pre-commit",
    ] {
        assert_eq!(tier(&[path]), Tier::High, "{path}");
    }
}

#[test]
fn secret_guarded_paths_are_high() {
    for path in [
        ".env",
        "app/.env.local",
        ".npmrc",
        ".netrc",
        "deploy/.ssh/config",
        "keys/server.pem",
        "id_ed25519",
    ] {
        assert_eq!(tier(&[path]), Tier::High, "{path}");
    }
}

#[test]
fn path_spellings_do_not_hide_a_rule() {
    for path in [
        "CARGO.LOCK",
        "./Cargo.lock",
        "src/../Cargo.lock",
        "src//sub/../../cargo.LOCK",
        "sub\\Cargo.lock",
        ".\\.github\\workflows\\ci.yml",
        ".GITHUB/workflows/ci.yml",
    ] {
        assert_eq!(tier(&[path]), Tier::High, "{path}");
    }
}

#[test]
fn a_look_alike_name_is_not_a_rule() {
    assert_eq!(
        tier(&["src/cargo_lock_notes.rs", "docs/package.md"]),
        Tier::Passive
    );
}

#[test]
fn a_failed_assessment_is_high() {
    for paths in [
        vec![],
        vec![""],
        vec!["   "],
        vec!["../outside.rs"],
        vec!["a/../../b.rs"],
        vec!["/etc/passwd"],
        vec!["C:\\x\\y.rs"],
        vec!["bad\0name.rs"],
    ] {
        let changes: Vec<Change> = paths.iter().map(|p| change(p)).collect();
        let low = raise("passive", "fine");
        let got = assess(&changes, Some(&low));
        assert_eq!(got.tier, Tier::High, "{paths:?}");
        assert!(matches!(got.source, Source::Failed(_)), "{paths:?}");
    }
}

#[test]
fn a_raise_reason_is_one_clean_capped_line() {
    let dirty = format!(
        "first\u{1b}[31m line\u{202E}x\u{2066}\nsecond line {}",
        "z".repeat(400)
    );
    let up = raise("medium", &dirty);
    assert_eq!(up.reason(), "first[31m linex");
    let long = raise("medium", &"é".repeat(300));
    assert!(long.reason().len() <= 200);
    assert!(long.reason().chars().all(|c| c == 'é'));
}

#[test]
fn a_bad_raise_is_refused_with_a_reason() {
    assert_eq!(Raise::new("urgent", "why").unwrap_err(), RaiseError::Tier);
    assert_eq!(Raise::new("high", " \n ").unwrap_err(), RaiseError::Reason);
    assert_eq!(
        Raise::new("high", "\u{202E}\u{1b}").unwrap_err(),
        RaiseError::Reason
    );
    assert!(!RaiseError::Tier.to_string().is_empty());
    assert!(!RaiseError::Reason.to_string().is_empty());
}

#[test]
fn tiers_order_and_name_themselves() {
    assert!(Tier::Passive < Tier::Medium && Tier::Medium < Tier::High);
    assert_eq!(Tier::High.as_str(), "high");
    assert_eq!(Tier::Passive.as_str(), "passive");
}
