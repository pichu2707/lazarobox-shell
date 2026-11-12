//! The plan of Connect Google on the lazarobox route (openspec change
//! `gsc-lazarobox-client`): no own client file is read, the cache path is
//! written, and a build with no pin never plans it.

use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};

use super::*;
use crate::core::ai::mcp_catalog::connect::ClientRoute;
use crate::core::ai::mcp_secrets::cache_path;

const PIN: &str = "123456789012";

struct Lab {
    _root: tempfile::TempDir,
    secrets: PathBuf,
}

fn lab() -> Lab {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("lazarobox");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    Lab {
        secrets: dir.join("secrets.toml"),
        _root: root,
    }
}

impl Lab {
    fn plan(
        &self,
        route: ClientRoute,
        pin: Option<&str>,
        chosen: &[&'static str],
    ) -> Result<Preview, String> {
        let ask = Ask {
            chosen: chosen.to_vec(),
            email: "ana@example.com".to_owned(),
            route,
        };
        plan_with(&RealFs, Some(&self.secrets), Some("/home/ana"), &ask, pin)
    }
}

fn gsc_file(shown: &Preview) -> Option<String> {
    shown
        .write
        .set
        .iter()
        .find(|(_, name, _)| *name == "GSC_OAUTH_CLIENT_SECRETS_FILE")
        .map(|(_, _, value)| value.expose().to_owned())
}

// Spec (config-menu, Search Console only): no own client file needed.
#[test]
fn the_lazarobox_route_plans_the_cache_path_with_no_own_client_file() {
    let lab = lab();
    let shown = lab
        .plan(ClientRoute::Lazarobox, Some(PIN), &["gsc"])
        .unwrap();
    assert_eq!(gsc_file(&shown), cache_path(Some(&lab.secrets)));
    assert!(
        gsc_file(&shown)
            .unwrap()
            .ends_with("lazarobox-google-client.json")
    );
    // Nothing was created by planning.
    assert_eq!(
        fs::read_dir(lab.secrets.parent().unwrap()).unwrap().count(),
        0
    );
}

// Spec (Pin not filled): the route is never planned without a pin.
#[test]
fn without_a_pin_the_lazarobox_route_is_not_planned() {
    let lab = lab();
    for pin in [None, Some("")] {
        let notice = lab.plan(ClientRoute::Lazarobox, pin, &["gsc"]).unwrap_err();
        assert_eq!(
            notice,
            "lazarobox client not configured in this build; u uses your own client"
        );
    }
}

// Owner decision 2026-11-12: Workspace is never on the lazarobox route.
#[test]
fn workspace_on_the_lazarobox_route_is_refused() {
    let lab = lab();
    for chosen in [&["google"][..], &["gsc", "google"][..]] {
        let notice = lab
            .plan(ClientRoute::Lazarobox, Some(PIN), chosen)
            .unwrap_err();
        assert!(notice.contains("only for Search Console alone"), "{notice}");
    }
}

#[test]
fn the_own_route_still_needs_the_own_client_file() {
    let lab = lab();
    assert_eq!(
        lab.plan(ClientRoute::Own, Some(PIN), &["gsc"]).unwrap_err(),
        NO_CLIENT
    );
}
