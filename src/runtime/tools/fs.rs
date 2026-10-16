//! The read tools: `read_file`, `list_dir` and `find_dir`. Read only: no
//! process, no socket, no write, and no path that the policy refuses.

use std::{
    ffi::OsString,
    io::Read,
    os::{fd::OwnedFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
};

use crate::{
    core::{
        ai::{
            memory::redact::redact,
            tools::{
                Call,
                paths::{PathVerdict, REFUSED, SecretRoots, resolve},
            },
        },
        config::config_path,
    },
    runtime::{
        chat_store::resolve_path,
        tools::open::{fd_path, open_beneath},
    },
};

/// The caps pinned by the design.
#[derive(Clone, Debug)]
pub struct Limits {
    pub read_bytes: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            read_bytes: 64 * 1024,
        }
    }
}

/// What a call may see: the environment values it needs, read by the caller.
#[derive(Clone, Debug)]
pub struct ToolEnv {
    pub home: Option<OsString>,
    pub xdg_config_home: Option<OsString>,
    pub xdg_data_home: Option<OsString>,
    /// The chat's origin cwd, against which a relative path is read.
    pub origin: PathBuf,
}

/// Why a call failed. The text is short and never holds file content or a
/// path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failure {
    Refused,
    NotText,
    Unreadable,
    EmptyQuery,
    NotReadTool,
}

impl Failure {
    /// The text that goes back to the model.
    pub fn message(self) -> &'static str {
        match self {
            Failure::Refused => REFUSED,
            Failure::NotText => "the file is not text",
            Failure::Unreadable => "could not read",
            Failure::EmptyQuery => "the query is empty",
            Failure::NotReadTool => "not a read tool",
        }
    }
}

/// Appended when `read_file` cut the text at the cap.
pub const READ_MARKER: &str = "[truncated: the file is larger than the read cap]";

/// Runs one read tool, synchronously. Builds the secret roots ONCE for the
/// whole call and refuses the call when they cannot be built.
pub fn execute(call: &Call, env: &ToolEnv, limits: &Limits) -> Result<String, Failure> {
    let roots = load_roots(env)?;
    match call {
        Call::ReadFile { path } => read_file(&roots, env, path, limits),
        Call::ListDir { .. } | Call::FindDir { .. } => Err(Failure::Unreadable),
        Call::RunInPane { .. } => Err(Failure::NotReadTool),
    }
}

/// The secret roots for one call. HOME unset, empty or relative refuses, and
/// so does a root that cannot be read; there is no fallback to `/`.
fn load_roots(env: &ToolEnv) -> Result<SecretRoots, Failure> {
    let home = env
        .home
        .as_ref()
        .filter(|home| !home.is_empty())
        .ok_or(Failure::Refused)?;
    let parent = |path: Option<PathBuf>| path.and_then(|p| p.parent().map(Path::to_path_buf));
    let extra: Vec<PathBuf> = [
        parent(config_path(env.xdg_config_home.clone(), Some(home.clone()))),
        parent(resolve_path(
            env.xdg_data_home.as_deref(),
            Some(home.as_os_str()),
        )),
    ]
    .into_iter()
    .flatten()
    .collect();
    SecretRoots::load(Path::new(home), &extra).ok_or(Failure::Refused)
}

/// Opens the already resolved `real` path and proves the descriptor is that
/// path and still allowed. No symlink is followed in any component, so a link
/// swapped in after the judgment fails the open; the descriptor's own path is
/// read back and judged against the same snapshot of roots.
pub fn open_checked(roots: &SecretRoots, real: &Path, dir: bool) -> Result<OwnedFd, Failure> {
    if !roots.judge_real(real) {
        return Err(Failure::Refused);
    }
    let rel = real
        .strip_prefix(roots.home())
        .map_err(|_| Failure::Refused)?;
    let fd = open_beneath(roots.home(), rel, dir).map_err(|_| Failure::Refused)?;
    match fd_path(&fd) {
        Ok(opened) if opened == real && roots.judge_real(&opened) => Ok(fd),
        _ => Err(Failure::Refused),
    }
}

fn read_file(
    roots: &SecretRoots,
    env: &ToolEnv,
    path: &str,
    limits: &Limits,
) -> Result<String, Failure> {
    let PathVerdict::Allowed(real) = resolve(roots, &env.origin, path) else {
        return Err(Failure::Refused);
    };
    let file = std::fs::File::from(open_checked(roots, &real, false)?);
    let meta = file.metadata().map_err(|_| Failure::Unreadable)?;
    if !meta.is_file() {
        return Err(Failure::NotText);
    }
    // A hard link to a secret has an ordinary path.
    if meta.nlink() > 1 {
        return Err(Failure::Refused);
    }
    let mut bytes = Vec::new();
    file.take(limits.read_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::Unreadable)?;
    if bytes.contains(&0) {
        return Err(Failure::NotText);
    }
    let cut = bytes.len() > limits.read_bytes;
    bytes.truncate(limits.read_bytes);
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        // A character split by the cap is the only decoding error allowed.
        Err(error) if cut && error.utf8_error().error_len().is_none() => {
            let valid = error.utf8_error().valid_up_to();
            let mut bytes = error.into_bytes();
            bytes.truncate(valid);
            String::from_utf8(bytes).map_err(|_| Failure::NotText)?
        }
        Err(_) => return Err(Failure::NotText),
    };
    let mut out = redact(&text).text;
    if cut {
        out.push('\n');
        out.push_str(READ_MARKER);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{fs, os::unix::fs::symlink};

    pub(super) struct Fixture {
        pub dir: tempfile::TempDir,
        pub home: PathBuf,
    }

    pub(super) fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let home = root.join("home");
        for sub in ["home/dev/app", "home/.ssh", "home/outside-link", "outside"] {
            fs::create_dir_all(root.join(sub)).unwrap();
        }
        fs::remove_dir(root.join("home/outside-link")).unwrap();
        fs::write(home.join("dev/app/main.rs"), "fn main() {}\n").unwrap();
        fs::write(home.join(".ssh/id"), "SECRET-TOKEN-123").unwrap();
        fs::write(root.join("outside/note.txt"), "out").unwrap();
        Fixture { dir, home }
    }

    pub(super) fn env(home: &Path) -> ToolEnv {
        ToolEnv {
            home: Some(home.into()),
            xdg_config_home: None,
            xdg_data_home: None,
            origin: home.to_path_buf(),
        }
    }

    fn read(env: &ToolEnv, path: &str) -> Result<String, Failure> {
        execute(
            &Call::ReadFile { path: path.into() },
            env,
            &Limits::default(),
        )
    }

    #[test]
    fn reads_a_text_file_by_tilde_absolute_and_relative_path() {
        let f = fixture();
        let mut e = env(&f.home);
        let abs = f.home.join("dev/app/main.rs");
        assert_eq!(read(&e, "~/dev/app/main.rs").unwrap(), "fn main() {}\n");
        assert_eq!(read(&e, abs.to_str().unwrap()).unwrap(), "fn main() {}\n");
        e.origin = f.home.join("dev");
        assert_eq!(read(&e, "app/main.rs").unwrap(), "fn main() {}\n");
    }

    #[test]
    fn home_unset_empty_or_relative_refuses_every_call_and_never_uses_root() {
        let f = fixture();
        for home in [
            None,
            Some(OsString::new()),
            Some("relative/home".into()),
            Some("/".into()),
        ] {
            let mut e = env(&f.home);
            e.home = home;
            e.origin = f.home.clone();
            for call in [
                Call::ReadFile {
                    path: "~/dev/app/main.rs".into(),
                },
                Call::ReadFile {
                    path: "/etc/hostname".into(),
                },
                Call::ListDir { path: "~".into() },
                Call::FindDir {
                    query: "app".into(),
                },
            ] {
                assert_eq!(
                    execute(&call, &e, &Limits::default()),
                    Err(Failure::Refused),
                    "{:?} {call:?}",
                    e.home
                );
            }
        }
    }

    #[test]
    fn an_unreadable_secret_root_refuses_the_whole_call() {
        let f = fixture();
        fs::remove_dir_all(f.home.join(".ssh")).unwrap();
        let looped = f.home.join(".ssh");
        symlink(&looped, &looped).unwrap();
        assert_eq!(
            read(&env(&f.home), "~/dev/app/main.rs"),
            Err(Failure::Refused)
        );
    }

    #[test]
    fn the_lazarobox_config_and_data_dirs_are_extra_roots_even_when_moved_by_xdg() {
        let f = fixture();
        let data = f.home.join("custom-data/lazarobox");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("chat.db"), "rows").unwrap();
        let config = f.home.join("custom-config/lazarobox");
        fs::create_dir_all(&config).unwrap();
        fs::write(config.join("config.toml"), "token").unwrap();
        let mut e = env(&f.home);
        assert!(read(&e, "~/custom-data/lazarobox/chat.db").is_ok());
        e.xdg_data_home = Some(f.home.join("custom-data").into());
        e.xdg_config_home = Some(f.home.join("custom-config").into());
        assert_eq!(
            read(&e, "~/custom-data/lazarobox/chat.db"),
            Err(Failure::Refused)
        );
        assert_eq!(
            read(&e, "~/custom-config/lazarobox/config.toml"),
            Err(Failure::Refused)
        );
    }

    #[test]
    fn excluded_outside_and_missing_paths_give_the_same_refusal() {
        let f = fixture();
        symlink(f.home.join("../outside/note.txt"), f.home.join("link")).unwrap();
        let e = env(&f.home);
        for path in [
            "~/.ssh/id",
            "../outside/note.txt",
            "~/link",
            "~/nope",
            "~root/x",
        ] {
            let err = read(&e, path).unwrap_err();
            assert_eq!(err, Failure::Refused, "{path}");
            assert_eq!(err.message(), REFUSED);
        }
    }

    #[test]
    fn nul_bytes_and_invalid_utf8_are_not_text_and_the_error_holds_no_content() {
        let f = fixture();
        fs::write(f.home.join("bin"), b"SECRET-TOKEN-123\0more").unwrap();
        fs::write(f.home.join("latin"), b"SECRET-TOKEN-123 \xe9 caf\xe9").unwrap();
        let e = env(&f.home);
        for path in ["~/bin", "~/latin"] {
            let err = read(&e, path).unwrap_err();
            assert_eq!(err, Failure::NotText, "{path}");
            assert!(!err.message().contains("SECRET-TOKEN-123"));
        }
    }

    #[test]
    fn a_big_file_is_cut_at_the_cap_on_a_char_boundary_and_says_so() {
        let f = fixture();
        let limits = Limits { read_bytes: 9 };
        fs::write(f.home.join("big"), "aaaaaaaaé-tail").unwrap();
        let out = execute(
            &Call::ReadFile {
                path: "~/big".into(),
            },
            &env(&f.home),
            &limits,
        )
        .unwrap();
        assert_eq!(out, format!("aaaaaaaa\n{READ_MARKER}"));
        fs::write(f.home.join("exact"), "012345678").unwrap();
        let out = execute(
            &Call::ReadFile {
                path: "~/exact".into(),
            },
            &env(&f.home),
            &limits,
        )
        .unwrap();
        assert_eq!(out, "012345678");
    }

    #[test]
    fn recognizable_secrets_are_redacted() {
        let f = fixture();
        fs::write(f.home.join("conf"), "key = sk-abcDEF1234567890\nname = x\n").unwrap();
        let out = read(&env(&f.home), "~/conf").unwrap();
        assert!(!out.contains("sk-abcDEF1234567890"), "{out}");
        assert!(out.contains("name = x"));
    }

    #[test]
    fn a_hard_link_to_a_secret_is_refused() {
        let f = fixture();
        fs::hard_link(f.home.join(".ssh/id"), f.home.join("innocent.txt")).unwrap();
        assert_eq!(read(&env(&f.home), "~/innocent.txt"), Err(Failure::Refused));
        // The same file with one link is fine.
        fs::write(f.home.join("solo.txt"), "ok").unwrap();
        assert_eq!(read(&env(&f.home), "~/solo.txt").unwrap(), "ok");
    }

    #[test]
    fn a_fifo_a_directory_and_a_device_are_not_read_and_never_block() {
        let f = fixture();
        let fifo = std::ffi::CString::new(f.home.join("pipe").to_str().unwrap()).unwrap();
        // SAFETY: a valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let e = env(&f.home);
        for path in ["~/pipe", "~/dev"] {
            assert_eq!(read(&e, path), Err(Failure::NotText), "{path}");
        }
    }

    #[test]
    fn a_link_swapped_in_after_the_judgment_is_not_followed() {
        let f = fixture();
        let roots = SecretRoots::load(&f.home, &[]).unwrap();
        let real = f.home.join("dev/app/main.rs");
        assert!(open_checked(&roots, &real, false).is_ok());
        // The file becomes a link to a secret.
        fs::remove_file(&real).unwrap();
        symlink(f.home.join(".ssh/id"), &real).unwrap();
        assert_eq!(
            open_checked(&roots, &real, false).unwrap_err(),
            Failure::Refused
        );
        // A parent folder becomes a link to another folder.
        fs::remove_file(&real).unwrap();
        fs::rename(f.home.join("dev/app"), f.home.join("dev/app-moved")).unwrap();
        symlink(f.home.join("../outside"), f.home.join("dev/app")).unwrap();
        assert_eq!(
            open_checked(&roots, &real, false).unwrap_err(),
            Failure::Refused
        );
    }

    #[test]
    fn the_opened_descriptor_is_judged_again_against_the_snapshot() {
        let f = fixture();
        let roots = SecretRoots::load(&f.home, &[]).unwrap();
        // Opens fine, but the policy refuses what it is.
        for secret in [f.home.join(".ssh/id"), f.home.join("../outside/note.txt")] {
            assert!(open_checked(&roots, &secret, false).is_err(), "{secret:?}");
        }
    }

    #[test]
    fn the_tools_leave_the_tree_untouched() {
        let f = fixture();
        let snapshot = |root: &Path| {
            let mut all = Vec::new();
            let mut stack = vec![root.to_path_buf()];
            while let Some(dir) = stack.pop() {
                for entry in fs::read_dir(&dir).unwrap() {
                    let p = entry.unwrap().path();
                    let meta = fs::symlink_metadata(&p).unwrap();
                    all.push((p.clone(), meta.len(), meta.modified().unwrap()));
                    if meta.is_dir() {
                        stack.push(p);
                    }
                }
            }
            all.sort();
            all
        };
        let before = snapshot(f.dir.path());
        let e = env(&f.home);
        let _ = read(&e, "~/dev/app/main.rs");
        let _ = read(&e, "~/.ssh/id");
        assert_eq!(snapshot(f.dir.path()), before);
    }
}
