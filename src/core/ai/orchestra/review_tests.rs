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
    let low = raise("medium", "just a bump");
    let got = assess(&changes, Some(&low));
    assert_eq!(got.tier, Tier::High);
    assert_eq!(got.source, Source::Rules);
}

// Spec: The orchestrator cannot lower.
#[test]
fn the_orchestrator_cannot_lower_a_high_floor() {
    let changes = [change(".github/workflows/ci.yml"), change("src/lib.rs")];
    let low = raise("medium", "harmless");
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
        let low = raise("medium", "fine");
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

// 7b.1 review fixes.
#[test]
fn more_build_files_are_high() {
    for path in [
        "Rakefile",
        "Procfile",
        "Containerfile",
        "Vagrantfile",
        "Brewfile",
        "Podfile",
        "rust-toolchain",
        "ext/Podfile",
    ] {
        assert_eq!(tier(&[path]), Tier::High, "{path}");
    }
}

#[test]
fn repo_and_tool_config_is_high() {
    for path in [
        ".gitmodules",
        ".gitattributes",
        ".tool-versions",
        ".yarnrc",
        ".pypirc",
        "known_hosts",
        "sub/.gitmodules",
    ] {
        assert_eq!(tier(&[path]), Tier::High, "{path}");
    }
}

#[test]
fn more_build_extensions_are_high() {
    for path in [
        "app.gradle",
        "build.sbt",
        "App.csproj",
        "All.sln",
        "Directory.Build.props",
        "x.targets",
        "pom.xml",
        "rules.mk",
        "main.tf",
    ] {
        assert_eq!(tier(&[path]), Tier::High, "{path}");
    }
}

#[test]
fn commit_hooks_and_shell_scripts_are_high() {
    for path in [
        ".husky/pre-commit",
        ".githooks/pre-push",
        "a/.husky/_/husky.sh",
        "scripts/deploy.sh",
        "RUN.SH",
    ] {
        assert_eq!(tier(&[path]), Tier::High, "{path}");
    }
}

#[test]
fn a_non_ascii_look_alike_of_a_rule_is_high() {
    // KELVIN SIGN folds to `k`; LONG S folds to `s` under simple folding only
    // through to_lowercase of the capital, so the assessment must still be high.
    for path in [
        "Ma\u{212A}efile",
        "Ma\u{212A}efile".to_lowercase().as_str(),
        ".\u{17F}\u{17F}h/config",
        "\u{212A}nown_hosts",
    ] {
        let got = assess(&[change(path)], None);
        assert_eq!(got.tier, Tier::High, "{path}");
    }
}

#[test]
fn a_non_ascii_name_with_no_rule_stays_passive() {
    assert_eq!(
        tier(&["src/caf\u{e9}.rs", "docs/\u{4e2d}\u{6587}.md"]),
        Tier::Passive
    );
}

#[test]
fn the_reason_drops_invisible_and_line_separator_characters() {
    for hidden in [
        '\u{2028}', '\u{2029}', '\u{200B}', '\u{200C}', '\u{200D}', '\u{2060}', '\u{FEFF}',
        '\u{3164}', '\u{2800}', '\u{200E}', '\u{200F}', '\u{061C}',
    ] {
        let up = raise("high", &format!("a{hidden}b"));
        assert_eq!(up.reason(), "ab", "{hidden:?}");
    }
}

#[test]
fn a_tab_in_the_reason_becomes_a_space() {
    assert_eq!(raise("high", "a\tb").reason(), "a b");
}

#[test]
fn a_reason_with_nothing_visible_is_refused() {
    for text in ["\u{3164}\u{2800}", "\u{200B}\t", "   ", "\u{2028}"] {
        assert_eq!(
            Raise::new("high", text).unwrap_err(),
            RaiseError::Reason,
            "{text:?}"
        );
    }
    assert!(Raise::new("high", "!").is_ok());
}

#[test]
fn a_passive_raise_is_refused_because_the_tier_only_goes_up() {
    let err = Raise::new("passive", "fine").unwrap_err();
    assert_eq!(err, RaiseError::Lowering);
    assert!(err.to_string().contains("raise the tier above passive"));
}
