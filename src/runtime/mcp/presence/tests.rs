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
    assert_eq!(presence.problems.len(), 1, "{presence:?}");
    assert!(presence.problems[0].contains("mode must be 0600"));
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

#[tokio::test]
async fn the_backup_and_the_home_are_read_for_the_fields() {
    let (dir, config, secrets) = files(0o600);
    let presence = asked(config.clone(), secrets.clone()).await;
    assert!(!presence.backup());
    std::fs::write(dir.path().join("secrets.toml.bak"), "x").unwrap();
    let presence = asked(config, secrets).await;
    assert!(presence.backup());
    assert_eq!(presence.home(), std::env::var("HOME").unwrap_or_default());
}

#[tokio::test]
async fn the_client_file_is_noticed_when_it_exists() {
    let (dir, config, secrets) = files(0o600);
    let presence = asked(config.clone(), secrets.clone()).await;
    assert!(!presence.client_file());
    std::fs::write(dir.path().join("google-oauth-client.json"), "{}").unwrap();
    let presence = asked(config, secrets).await;
    assert!(presence.client_file());
}

// Spec mcp-connect, Token files never opened: the state is a `stat`.
#[tokio::test]
async fn the_gsc_token_is_noticed_by_stat_and_never_opened() {
    let dir = tempfile::tempdir().unwrap();
    let tokens = dir.path().join("mcp-gsc");
    let config = dir.path().join("config.toml");
    let secrets = dir.path().join("secrets.toml");
    std::fs::write(
        &config,
        "[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"x\"\n\
         env = [\"GSC_OAUTH_CLIENT_SECRETS_FILE\", \"GSC_CONFIG_DIR\", \"GSC_SKIP_OAUTH\"]\n",
    )
    .unwrap();
    std::fs::write(
        &secrets,
        format!(
            "[gsc]\nGSC_OAUTH_CLIENT_SECRETS_FILE = \"/c.json\"\nGSC_CONFIG_DIR = \"{}\"\nGSC_SKIP_OAUTH = \"false\"\n",
            tokens.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o600)).unwrap();
    let presence = asked(config.clone(), secrets.clone()).await;
    assert_eq!(presence.linked(), Some(false));
    std::fs::create_dir_all(&tokens).unwrap();
    let token = tokens.join("token.json");
    std::fs::write(&token, "{\"refresh_token\":\"r\"}").unwrap();
    // Mode 000: an open would fail, a stat does not.
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o000)).unwrap();
    let presence = asked(config.clone(), secrets.clone()).await;
    assert_eq!(presence.linked(), Some(true));
    assert!(!format!("{presence:?}").contains("refresh_token"));
    std::fs::set_permissions(&token, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        std::fs::read_to_string(&token).unwrap(),
        "{\"refresh_token\":\"r\"}"
    );
    // W4: an empty file is not a link.
    std::fs::write(&token, "").unwrap();
    let presence = asked(config, secrets).await;
    assert_eq!(presence.linked(), Some(false));
}

// Connect Google (S5.4): after the presence, the client id for the screen's
// `client:` line, read with the writer's checks; never the secret.
#[tokio::test]
async fn the_client_id_follows_the_presence_and_never_the_secret() {
    let (dir, config, secrets) = files(0o600);
    let id = "123-abc.apps.googleusercontent.com";
    let client = dir.path().join(CLIENT_FILE);
    let json = crate::core::ai::mcp_secrets::client_json(id, "GOCSPX-abc").unwrap();
    std::fs::write(&client, json).unwrap();
    std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o600)).unwrap();
    let read = |config: PathBuf, secrets: PathBuf| async move {
        let (tx, mut rx) = mpsc::channel(4);
        Reader::new(Some(config), Some(secrets), tx).load();
        let mut events = Vec::new();
        for _ in 0..2 {
            let event = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
            events.push(event.unwrap().unwrap());
        }
        events
    };
    let events = read(config.clone(), secrets.clone()).await;
    assert!(matches!(events[0], AppEvent::McpPresence(_)));
    match &events[1] {
        AppEvent::GoogleClient(Ok(Some(read))) => assert_eq!(read, id),
        other => panic!("{other:?}"),
    }
    assert!(!format!("{events:?}").contains("GOCSPX"));
    std::fs::set_permissions(&client, std::fs::Permissions::from_mode(0o644)).unwrap();
    let events = read(config.clone(), secrets.clone()).await;
    match &events[1] {
        AppEvent::GoogleClient(Err(notice)) => {
            assert!(notice.starts_with("google-oauth-client.json: mode must be 0600"));
        }
        other => panic!("{other:?}"),
    }
    std::fs::remove_file(&client).unwrap();
    let events = read(config, secrets).await;
    assert!(matches!(events[1], AppEvent::GoogleClient(Ok(None))));
}
