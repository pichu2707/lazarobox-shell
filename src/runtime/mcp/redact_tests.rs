//! A value of `secrets.toml` that a server echoes in what it says (stderr, an
//! RPC error) never reaches a notice, in a probe or in a chat start.

use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::Duration,
};

use super::{
    client::Timeouts,
    external::{
        ExternalSet,
        fake::{Lab, host_env, lab, timeouts},
    },
    probe::probe,
};
use crate::core::ai::{config::McpServer, mcp_catalog::checks::Verdict, mcp_secrets::Secrets};

const LONG: Duration = Duration::from_secs(10);

/// A token short, and one in a shape no pattern would know.
const TOKENS: [&str; 2] = ["k9", "p@ss w0rd/x"];

/// Each token with a short lead (it sits inside the probe's 64-character cut)
/// and one that puts it across the 200-character cut of an RPC message.
fn cases() -> Vec<(&'static str, usize)> {
    TOKENS.iter().flat_map(|t| [(*t, 10), (*t, 195)]).collect()
}

fn secrets_text(token: &str) -> String {
    format!("[clarity]\nCLARITY_API_TOKEN = \"{token}\"\n")
}

fn token_file(lab: &Lab, token: &str) -> PathBuf {
    let path = lab.home.join("secrets.toml");
    std::fs::write(&path, secrets_text(token)).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    path
}

async fn probed(lab: &Lab, server: &McpServer, secrets: &Path) -> String {
    let report = probe(
        server,
        &lab.home,
        host_env,
        Some(secrets.to_path_buf()),
        Timeouts {
            handshake: LONG,
            call: LONG,
        },
        std::future::pending::<()>(),
    )
    .await;
    assert_eq!(report.verdict, Verdict::Failed, "{report:?}");
    format!("{report:?}")
}

#[tokio::test]
async fn the_probe_does_not_show_a_token_the_server_says_on_stderr() {
    for token in TOKENS {
        let lab = lab();
        let path = token_file(&lab, token);
        let shown = probed(&lab, &lab.noisy("clarity"), &path).await;
        assert!(!shown.contains(token), "{token}: {shown}");
        assert!(shown.contains("[redacted]"), "{shown}");
    }
}

#[tokio::test]
async fn the_probe_does_not_show_a_token_in_an_rpc_error_nor_its_cut_start() {
    for (token, pad) in cases() {
        let lab = lab();
        let path = token_file(&lab, token);
        let shown = probed(&lab, &lab.leaky("clarity", pad), &path).await;
        assert!(!shown.contains(token), "{token}: {shown}");
        if token != "k9" {
            // The cut must not leave the start of the token.
            assert!(!shown.contains("p@s"), "{shown}");
        }
    }
}

#[tokio::test]
async fn a_chat_start_notice_does_not_show_the_token_either() {
    for (token, pad) in cases() {
        let lab = lab();
        let loaded = Secrets::parse(secrets_text(token).as_bytes());
        for server in [lab.noisy("clarity"), lab.leaky("clarity", pad)] {
            let set = ExternalSet::start_with(
                &[&server],
                &lab.home,
                host_env,
                &loaded.secrets,
                timeouts(LONG),
            )
            .await;
            let notices = set.notices().join("\n");
            assert!(notices.contains("clarity unavailable"), "{notices}");
            assert!(!notices.contains(token), "{token}: {notices}");
            if token != "k9" {
                assert!(!notices.contains("p@s"), "{notices}");
            }
            set.shutdown().await;
        }
    }
}
