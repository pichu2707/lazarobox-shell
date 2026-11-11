use std::{os::unix::fs::PermissionsExt, time::Duration};

use super::*;

const CONFIG: &str = "# mine\n[[ai.mcp.servers]]\nname = \"clarity\"\ncommand = \"x\"\n";

fn files(secrets_mode: u32) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let secrets = dir.path().join("secrets.toml");
    std::fs::write(&config, CONFIG).unwrap();
    std::fs::write(&secrets, "[clarity]\nCLARITY_API_TOKEN = \"sk-123\"\n").unwrap();
    std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(secrets_mode)).unwrap();
    (dir, config, secrets)
}

async fn asked(config: PathBuf, secrets: PathBuf) -> Presence {
    let (tx, mut rx) = mpsc::channel(4);
    Reader::new(Some(config), Some(secrets), tx).load();
    match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
        Ok(Some(AppEvent::McpPresence(presence))) => presence,
        other => panic!("no presence: {other:?}"),
    }
}

#[tokio::test]
async fn the_servers_and_the_names_with_a_value_are_read() {
    let (_dir, config, secrets) = files(0o600);
    let presence = asked(config, secrets).await;
    assert_eq!(presence.servers.as_ref().map(Vec::len), Some(1));
    assert!(presence.has("clarity", "CLARITY_API_TOKEN"));
    assert!(!format!("{presence:?}").contains("sk-123"));
}

#[tokio::test]
async fn a_secrets_file_that_is_not_used_has_no_values() {
    let (_dir, config, secrets) = files(0o644);
    let presence = asked(config, secrets).await;
    assert!(!presence.has("clarity", "CLARITY_API_TOKEN"));
}

#[tokio::test]
async fn a_missing_config_declares_nothing_and_an_unreadable_one_says_so() {
    let dir = tempfile::tempdir().unwrap();
    let missing = asked(dir.path().join("config.toml"), dir.path().join("s.toml")).await;
    assert_eq!(missing.servers, Some(Vec::new()));
    // A directory cannot be read as a file.
    let unreadable = asked(dir.path().to_path_buf(), dir.path().join("s.toml")).await;
    assert_eq!(unreadable.servers, None);
}

#[tokio::test]
async fn reading_changes_no_byte_and_no_mode() {
    let (_dir, config, secrets) = files(0o600);
    asked(config.clone(), secrets.clone()).await;
    assert_eq!(std::fs::read_to_string(&config).unwrap(), CONFIG);
    assert_eq!(
        std::fs::read_to_string(&secrets).unwrap(),
        "[clarity]\nCLARITY_API_TOKEN = \"sk-123\"\n"
    );
    let mode = std::fs::metadata(&secrets).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
}
