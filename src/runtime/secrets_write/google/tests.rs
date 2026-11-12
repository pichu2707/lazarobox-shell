//! Connect Google's reads (S5): the client file judged on its handle as the
//! writer judges a file, and the plan checked against the files on disk. A
//! temp config folder, never the user's own.

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
};

use super::super::tests::TestFs;
use super::*;
use crate::core::ai::mcp_secrets::client_json;

const ID: &str = "123-abc.apps.googleusercontent.com";
const SECRET: &str = "GOCSPX-abc";

struct Lab {
    _root: tempfile::TempDir,
    dir: PathBuf,
}

fn lab() -> Lab {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("lazarobox");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    Lab { _root: root, dir }
}

impl Lab {
    fn secrets(&self) -> PathBuf {
        self.dir.join("secrets.toml")
    }

    fn put(&self, name: &str, text: &str, mode: u32) {
        let path = self.dir.join(name);
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
    }

    fn client(&self) {
        self.put(CLIENT_FILE, &client_json(ID, SECRET).unwrap(), 0o600);
    }

    fn read(&self) -> Result<Option<Client>, Refusal> {
        read_client_with(&RealFs, Some(&self.secrets()))
    }

    fn plan(&self, chosen: &[&'static str], email: &str) -> Result<Preview, String> {
        let ask = Ask {
            chosen: chosen.to_vec(),
            email: email.to_owned(),
        };
        plan_with(&RealFs, Some(&self.secrets()), Some("/home/ana"), &ask)
    }
}

// Spec: the screen's `client:` line reads the file back.
#[test]
fn the_form_file_and_the_file_google_downloads_read_back() {
    let lab = lab();
    lab.client();
    let client = lab.read().unwrap().unwrap();
    assert_eq!((client.id.as_str(), client.secret.expose()), (ID, SECRET));
    let downloaded = format!(
        "{{\"installed\":{{\"client_id\":\"{ID}\",\"project_id\":\"p\",\"client_secret\":\"{SECRET}\",\"redirect_uris\":[\"http://localhost\"]}}}}"
    );
    lab.put(CLIENT_FILE, &downloaded, 0o400);
    assert_eq!(lab.read().unwrap().unwrap().id, ID, "read-only is fine");
}

#[test]
fn a_missing_file_or_folder_is_none_and_creates_nothing() {
    let lab = lab();
    assert!(lab.read().unwrap().is_none());
    let gone = lab.dir.join("missing").join("secrets.toml");
    assert!(read_client_with(&RealFs, Some(&gone)).unwrap().is_none());
    assert!(!lab.dir.join("missing").exists(), "no folder is created");
    assert!(read_client_with(&RealFs, None).unwrap().is_none());
}

// Spec: the same handle checks as the secrets writer.
#[test]
fn a_file_others_can_read_is_refused_and_says_its_mode() {
    let lab = lab();
    lab.client();
    for mode in [0o640, 0o604, 0o644] {
        fs::set_permissions(lab.dir.join(CLIENT_FILE), fs::Permissions::from_mode(mode)).unwrap();
        let refusal = lab.read().unwrap_err();
        assert!(matches!(refusal, Refusal::FileMode(_)), "{mode:o}");
        let notice = refusal.notice_of(CLIENT_FILE);
        assert!(notice.starts_with("google-oauth-client.json: mode must be 0600"));
    }
}

#[test]
fn a_symlink_a_hard_link_or_a_directory_is_refused() {
    let lab = lab();
    let real = lab.dir.join("real.json");
    fs::write(&real, client_json(ID, SECRET).unwrap()).unwrap();
    fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
    symlink(&real, lab.dir.join(CLIENT_FILE)).unwrap();
    assert_eq!(lab.read().unwrap_err(), Refusal::FileSymlink);
    fs::remove_file(lab.dir.join(CLIENT_FILE)).unwrap();
    fs::hard_link(&real, lab.dir.join(CLIENT_FILE)).unwrap();
    assert_eq!(lab.read().unwrap_err(), Refusal::HardLinked);
    fs::remove_file(lab.dir.join(CLIENT_FILE)).unwrap();
    fs::create_dir(lab.dir.join(CLIENT_FILE)).unwrap();
    assert_eq!(lab.read().unwrap_err(), Refusal::NotRegular);
}

#[test]
fn a_file_of_another_owner_is_refused() {
    let lab = lab();
    lab.client();
    let fs = TestFs::owned_by(RealFs.euid() + 1);
    let refusal = read_client_with(&fs, Some(&lab.secrets())).unwrap_err();
    assert_eq!(refusal, Refusal::FileOwner);
}

#[test]
fn a_symlinked_folder_is_refused() {
    let lab = lab();
    lab.client();
    let link = lab.dir.with_file_name("linked");
    symlink(&lab.dir, &link).unwrap();
    let refusal = read_client_with(&RealFs, Some(&link.join("secrets.toml"))).unwrap_err();
    assert_eq!(refusal, Refusal::FolderSymlink);
}

#[test]
fn a_file_over_the_bound_or_not_a_client_is_refused_without_its_text() {
    let lab = lab();
    lab.put(CLIENT_FILE, &" ".repeat(CLIENT_MAX + 1), 0o600);
    assert_eq!(lab.read().unwrap_err(), Refusal::TooLarge);
    lab.put(
        CLIENT_FILE,
        "{\"web\":{\"client_secret\":\"s3cr3t\"}}",
        0o600,
    );
    let refusal = lab.read().unwrap_err();
    assert_eq!(refusal, Refusal::BadClientFile);
    assert!(!refusal.notice_of(CLIENT_FILE).contains("s3cr3t"));
    lab.put(CLIENT_FILE, "\u{ff}\u{fe}", 0o600);
    assert_eq!(lab.read().unwrap_err(), Refusal::BadClientFile);
}

// Spec: the plan is built from fresh disk state.
#[test]
fn the_plan_reads_the_client_the_config_and_the_secrets_from_disk() {
    let lab = lab();
    lab.client();
    let config = "[[ai.mcp.servers]]\nname = \"gsc\"\ncommand = \"x\"\n";
    lab.put("config.toml", config, 0o644);
    lab.put(
        "secrets.toml",
        "[google]\nUSER_GOOGLE_EMAIL = \"old@example.com\"\n",
        0o600,
    );
    let shown = lab.plan(&["gsc", "google"], "").unwrap();
    assert_eq!(shown.declared, ["gsc"]);
    assert_eq!(shown.write.names, ["google"]);
    assert_eq!(shown.kept, [("google", "USER_GOOGLE_EMAIL")]);
    let set: Vec<(&str, &str)> = shown.write.set.iter().map(|(t, n, _)| (*t, *n)).collect();
    assert_eq!(
        set,
        [
            ("google", "GOOGLE_OAUTH_CLIENT_ID"),
            ("google", "GOOGLE_OAUTH_CLIENT_SECRET"),
            ("google", "WORKSPACE_MCP_DISABLE_LOCAL_FILES"),
        ]
    );
    assert_eq!(
        shown.write.expected,
        Some(crate::core::config_append::digest(config.as_bytes()))
    );
    assert_eq!(shown.blocked, None);
    // Nothing was written or created.
    let mut names: Vec<String> = fs::read_dir(&lab.dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["config.toml", CLIENT_FILE, "secrets.toml"]);
}

#[test]
fn without_a_client_file_the_plan_asks_for_one() {
    let lab = lab();
    assert_eq!(lab.plan(&["gsc"], "").unwrap_err(), NO_CLIENT);
    lab.client();
    fs::set_permissions(lab.dir.join(CLIENT_FILE), fs::Permissions::from_mode(0o644)).unwrap();
    let notice = lab.plan(&["gsc"], "").unwrap_err();
    assert!(
        notice.starts_with("google-oauth-client.json: mode must be 0600"),
        "{notice}"
    );
}

// Refusals show before any write.
#[test]
fn a_config_or_secrets_file_the_writer_would_refuse_refuses_the_plan() {
    let lab = lab();
    lab.client();
    let real = lab.dir.join("real.toml");
    fs::write(&real, "").unwrap();
    symlink(&real, lab.dir.join("config.toml")).unwrap();
    assert_eq!(
        lab.plan(&["gsc"], "").unwrap_err(),
        "config.toml is a symlink; edit it by hand"
    );
    fs::remove_file(lab.dir.join("config.toml")).unwrap();
    lab.put("config.toml", "mouse = false\n", 0o666);
    let notice = lab.plan(&["gsc"], "").unwrap_err();
    assert!(
        notice.starts_with("config.toml is writable by group or others"),
        "{notice}"
    );
    fs::remove_file(lab.dir.join("config.toml")).unwrap();
    lab.put("secrets.toml", "", 0o644);
    let notice = lab.plan(&["gsc"], "").unwrap_err();
    assert!(
        notice.starts_with("secrets.toml: mode must be 0600"),
        "{notice}"
    );
}

#[test]
fn a_config_that_does_not_parse_refuses_the_plan_with_its_line() {
    let lab = lab();
    lab.client();
    lab.put("config.toml", "mouse = false\n[[ai.mcp.servers\n", 0o600);
    assert_eq!(
        lab.plan(&["gsc"], "").unwrap_err(),
        "config.toml: not valid TOML (line 2)"
    );
}

#[test]
fn an_inline_server_list_is_a_blocked_plan_not_a_refusal() {
    let lab = lab();
    lab.client();
    lab.put(
        "config.toml",
        "[ai.mcp]\nservers = [{ name = \"docs\", command = \"x\" }]\n",
        0o600,
    );
    let shown = lab.plan(&["gsc"], "").unwrap();
    assert!(shown.blocked.unwrap().contains("inline ai.mcp.servers"));
}

/// The plan of `chosen` with `config` on disk, then its `y` when there is a
/// plan. Returns the plan's answer.
fn plan_and_write(lab: &Lab, config: &str, chosen: &[&'static str]) -> Result<Preview, String> {
    lab.client();
    lab.put("config.toml", config, 0o600);
    let shown = lab.plan(chosen, "ana@example.com");
    if let Ok(preview) = &shown {
        crate::runtime::config_append::connect_with(&RealFs, Some(&lab.secrets()), &preview.write);
    }
    shown
}

// Spec S3: a service declared in any form, valid or not, is left alone.
#[test]
fn an_invalid_declared_gsc_is_left_alone_end_to_end() {
    let lab = lab();
    // No `command`: the parsed server list skips it.
    let config = "[[ai.mcp.servers]]\nname = \"gsc\"\nargs = [\"x\"]\n";
    let shown = plan_and_write(&lab, config, &["gsc"]);
    assert_eq!(
        shown.unwrap_err(),
        "connect: gsc is already declared; nothing to write"
    );
    assert_eq!(
        fs::read_to_string(lab.dir.join("config.toml")).unwrap(),
        config
    );
    assert!(!lab.secrets().exists());
}

#[test]
fn an_invalid_declared_google_is_left_alone_end_to_end() {
    let lab = lab();
    // A relative `project`: the parsed server list skips it.
    let config =
        "[[ai.mcp.servers]]\nname = \"google\"\ncommand = \"uvx\"\nproject = \"rel/dir\"\n";
    let shown = plan_and_write(&lab, config, &["google"]);
    assert_eq!(
        shown.unwrap_err(),
        "connect: google is already declared; nothing to write"
    );
    assert_eq!(
        fs::read_to_string(lab.dir.join("config.toml")).unwrap(),
        config
    );
    assert!(!lab.secrets().exists());
}

// The other service is still planned, and the declared one gets no values.
#[test]
fn an_invalid_declared_gsc_leaves_google_planned() {
    let lab = lab();
    lab.client();
    let config = "[[ai.mcp.servers]]\nname = \"gsc\"\nargs = [\"x\"]\n";
    lab.put("config.toml", config, 0o600);
    let shown = lab.plan(&["gsc", "google"], "ana@example.com").unwrap();
    assert_eq!(shown.declared, ["gsc"]);
    assert_eq!(shown.write.names, ["google"]);
    assert!(
        shown
            .write
            .set
            .iter()
            .all(|(table, _, _)| *table == "google")
    );
}
