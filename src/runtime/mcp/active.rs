//! Which of the user's MCP servers a chat uses, with paths compared as real
//! paths.
//!
//! `servers::resolve` is pure and compares text. A `project` that goes through
//! a symlink would never equal the git root of the chat, which fails closed for
//! an entry that adds a server, but would fail OPEN for a disabled entry meant
//! to remove a global one. This is the one place that resolves both sides
//! before `resolve` runs (4a review).

use std::{
    fs,
    path::{Path, PathBuf},
};

use crate::{
    core::ai::{config::McpServer, tools::servers::resolve},
    runtime::workspace::git_root,
};

/// The servers of a chat opened from `origin`: the git root of that directory
/// and the `project` of every entry are canonicalized, then resolved.
pub fn active_servers(servers: &[McpServer], origin: Option<&Path>) -> Vec<McpServer> {
    let root = origin.and_then(git_root);
    active_with(servers, root.as_deref(), &|path| {
        fs::canonicalize(path).ok()
    })
}

/// [`active_servers`] with the canonicalization injected.
///
/// - An entry whose path cannot be resolved is never enabled: it neither adds
///   a server nor replaces one.
/// - A disabled entry also keeps its literal path, so it still removes the
///   global server for a chat whose root is that same text. A disable never
///   fails open.
fn active_with(
    servers: &[McpServer],
    root: Option<&Path>,
    canon: &dyn Fn(&Path) -> Option<PathBuf>,
) -> Vec<McpServer> {
    let root = root.map(|root| canon(root).unwrap_or_else(|| root.to_path_buf()));
    let mut list = Vec::new();
    for server in servers {
        let Some(project) = server.project.as_deref() else {
            list.push(server.clone());
            continue;
        };
        if !server.enabled {
            list.push(server.clone());
        }
        if let Some(real) = canon(project) {
            list.push(McpServer {
                project: Some(real),
                ..server.clone()
            });
        }
    }
    resolve(&list, root.as_deref())
        .into_iter()
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use std::{fs, os::unix::fs::symlink, path::Path};

    use super::*;

    fn server(name: &str, command: &str, project: Option<&Path>, enabled: bool) -> McpServer {
        McpServer {
            name: name.to_owned(),
            command: command.to_owned(),
            args: Vec::new(),
            env: Vec::new(),
            project: project.map(Path::to_path_buf),
            enabled,
        }
    }

    /// `<tmp>/repo` (a git root with `sub`) and `<tmp>/link` pointing at it.
    fn lab() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::create_dir_all(repo.join("sub")).unwrap();
        let link = tmp.path().join("link");
        symlink(&repo, &link).unwrap();
        (tmp, repo, link)
    }

    fn names(list: &[McpServer]) -> Vec<(&str, &str)> {
        list.iter()
            .map(|s| (s.name.as_str(), s.command.as_str()))
            .collect()
    }

    #[test]
    fn a_symlinked_project_path_matches_the_real_root_of_the_chat() {
        let (_tmp, repo, link) = lab();
        let list = [server("c", "pc", Some(&link), true)];
        assert_eq!(
            names(&active_servers(&list, Some(&repo.join("sub")))),
            [("c", "pc")]
        );
        // The chat was opened through the link: the same real root.
        assert_eq!(
            names(&active_servers(&list, Some(&link.join("sub")))),
            [("c", "pc")]
        );
    }

    #[test]
    fn a_disabled_entry_under_a_symlink_still_removes_the_global_server() {
        let (_tmp, repo, link) = lab();
        let list = [
            server("a", "ga", None, true),
            server("a", "off", Some(&link), false),
            server("b", "gb", None, true),
        ];
        assert_eq!(
            names(&active_servers(&list, Some(&repo.join("sub")))),
            [("b", "gb")]
        );
        // Another project keeps it.
        assert_eq!(
            names(&active_servers(&list, Some(Path::new("/"))))
                .first()
                .copied(),
            Some(("a", "ga"))
        );
    }

    #[test]
    fn an_enabled_entry_whose_path_cannot_be_resolved_is_never_active() {
        let (tmp, repo, _link) = lab();
        let missing = tmp.path().join("gone");
        let list = [
            server("a", "ga", None, true),
            server("a", "other", Some(&missing), true),
            server("c", "pc", Some(&missing), true),
        ];
        // Neither replaces the global `a` nor adds `c`.
        assert_eq!(
            names(&active_servers(&list, Some(&repo.join("sub")))),
            [("a", "ga")]
        );
    }

    #[test]
    fn when_nothing_can_be_resolved_a_disabled_entry_matches_the_text_and_an_enabled_one_is_off() {
        let no_canon = |_: &Path| None;
        let root = Some(Path::new("/p"));
        let off = server("a", "off", Some(Path::new("/p")), false);
        let on = server("a", "on", Some(Path::new("/p")), true);
        let global = server("a", "ga", None, true);
        // The disable rule never fails open.
        assert_eq!(
            names(&active_with(
                &[global.clone(), off.clone()],
                root,
                &no_canon
            )),
            [] as [(&str, &str); 0]
        );
        assert_eq!(
            names(&active_with(
                &[global.clone(), off],
                Some(Path::new("/q")),
                &no_canon
            )),
            [("a", "ga")]
        );
        // An enabled entry that cannot be resolved neither replaces nor adds.
        assert_eq!(
            names(&active_with(&[global, on], root, &no_canon)),
            [("a", "ga")]
        );
    }

    #[test]
    fn no_git_root_means_only_the_global_servers() {
        let tmp = tempfile::tempdir().unwrap();
        let list = [
            server("a", "ga", None, true),
            server("c", "pc", Some(tmp.path()), true),
        ];
        assert_eq!(
            names(&active_servers(&list, Some(tmp.path()))),
            [("a", "ga")]
        );
        assert_eq!(names(&active_servers(&list, None)), [("a", "ga")]);
    }
}
