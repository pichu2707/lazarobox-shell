//! What the read tools show of a staged batch: a read-only snapshot of its
//! text, so the model sees its own pending changes before they are applied.
//! The snapshot holds paths and text only and has no way to write. It is
//! consulted AFTER the path rules: every staged path is judged again against
//! the secret roots of the call, and its text goes through the same clean and
//! redaction as a file, because a staged overwrite carries text from the disk.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    path::{Path, PathBuf},
};

use crate::core::ai::tools::{
    paths::{SecretRoots, display_target, is_git_config, real_target},
    stage::{Batch, Entry, State},
};

/// Said after a path, or before a text, that comes from a staged batch.
pub const MARKER: &str = "(pending, not on disk)";
/// The words of [`MARKER`] without its parentheses.
const WORDS: &str = "pending, not on disk";
/// What disk-sourced text shows instead of [`WORDS`], so a file or a name
/// cannot pass for staged state.
const DEFANGED: &str = "pending; not on disk";

/// `text` with the marker words defanged. Always, view or not, so a missing
/// view can never make the marker forgeable.
pub fn plain(text: String) -> String {
    if text.contains(WORDS) {
        text.replace(WORDS, DEFANGED)
    } else {
        text
    }
}

/// The same for a borrowed text.
pub fn plain_str(text: &str) -> Cow<'_, str> {
    if text.contains(WORDS) {
        Cow::Owned(text.replace(WORDS, DEFANGED))
    } else {
        Cow::Borrowed(text)
    }
}

/// An immutable snapshot of the files and folders a batch holds.
#[derive(Clone, Default, Debug)]
pub struct StagedView {
    files: BTreeMap<PathBuf, String>,
    dirs: BTreeSet<PathBuf>,
}

impl StagedView {
    /// The snapshot of `batch`. A batch that is closed shows nothing.
    pub fn from_batch(batch: &Batch) -> StagedView {
        let mut view = StagedView::default();
        if !matches!(batch.state(), State::Staging | State::Proposed) {
            return view;
        }
        for staged in batch.entries() {
            let real = staged.target.real.clone();
            match &staged.entry {
                Entry::Dir => view.dirs.insert(real),
                Entry::File { text, .. } => view.files.insert(real, text.clone()).is_some(),
            };
        }
        view
    }

    /// The real path `input` names (the file may not exist), when the read
    /// rules allow it: under HOME, no secret name or root. Staged keys are
    /// real paths, so the lookup resolves links the way a disk read does.
    fn locate(roots: &SecretRoots, origin: &Path, input: &str) -> Option<PathBuf> {
        // The same text gate as a disk ask: nothing the review could not show.
        display_target(roots.home(), origin, input)?;
        let path = real_target(roots, origin, input)?;
        roots.judge_real(&path).then_some(path)
    }

    /// The staged text of the file `input` names. A git setting is never one.
    pub(super) fn file(&self, roots: &SecretRoots, origin: &Path, input: &str) -> Option<&str> {
        let path = Self::locate(roots, origin, input)?;
        (!is_git_config(&path))
            .then(|| self.files.get(&path).map(String::as_str))
            .flatten()
    }

    /// The folder `input` names when the batch makes it or something in it.
    pub(super) fn dir(&self, roots: &SecretRoots, origin: &Path, input: &str) -> Option<PathBuf> {
        let path = Self::locate(roots, origin, input)?;
        let below = |p: &PathBuf| p.starts_with(&path) && (*p != path || self.dirs.contains(p));
        self.dirs
            .iter()
            .chain(self.files.keys())
            .any(below)
            .then_some(path)
    }

    /// The names the batch puts directly in `dir`, and whether each is a folder.
    pub(super) fn children(&self, dir: &Path) -> BTreeMap<OsString, bool> {
        let mut out = BTreeMap::new();
        for (path, is_dir) in self
            .dirs
            .iter()
            .map(|p| (p, true))
            .chain(self.files.keys().map(|p| (p, false)))
        {
            let Ok(rest) = path.strip_prefix(dir) else {
                continue;
            };
            let mut parts = rest.components();
            if let Some(first) = parts.next() {
                let folder = is_dir || parts.next().is_some();
                *out.entry(first.as_os_str().to_owned()).or_insert(false) |= folder;
            }
        }
        out
    }

    /// The staged files below `dir` with their text.
    pub(super) fn files_below<'a>(
        &'a self,
        dir: &'a Path,
    ) -> impl Iterator<Item = (&'a Path, &'a str)> {
        self.files
            .iter()
            .filter(move |(path, _)| path.starts_with(dir))
            .map(|(path, text)| (path.as_path(), text.as_str()))
    }

    /// The staged text of the file at the exact path `real`.
    pub(super) fn text(&self, real: &Path) -> Option<&str> {
        self.files.get(real).map(String::as_str)
    }
}

// Needs the file tools: they open files with `openat2` and `/proc/self/fd`, Linux only.
// `cfg(test)` stays on its own line: the audit drops exactly that attribute.
#[cfg(test)]
#[cfg(target_os = "linux")]
mod tests {
    use super::*;
    use crate::core::ai::tools::{
        Call, Lines,
        paths::WriteTarget,
        stage::{Batch, BatchId, Existing, StageKey},
    };
    use crate::core::layout::PaneId;
    use crate::runtime::tools::fs::{Failure, Limits, ToolEnv, execute_over};
    use std::{fs, path::PathBuf, sync::atomic::AtomicBool};

    const MARK: &str = "(pending, not on disk)";

    struct Fixture {
        _dir: tempfile::TempDir,
        root: PathBuf,
        env: ToolEnv,
        batch: Batch,
    }

    /// `home/proj` is the root of the chat. `src/old.rs` is on disk.
    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().canonicalize().unwrap().join("home");
        let root = home.join("proj");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("src/old.rs"), "fn old() {}\n").unwrap();
        fs::write(root.join("notes.txt"), "plain notes\n").unwrap();
        let env = ToolEnv {
            home: Some(home.into_os_string()),
            xdg_config_home: None,
            xdg_data_home: None,
            origin: root.clone(),
        };
        let id = BatchId {
            key: StageKey::Pane(PaneId::for_test(1)),
            generation: 0,
        };
        Fixture {
            _dir: dir,
            root,
            env,
            batch: Batch::new(id),
        }
    }

    impl Fixture {
        fn target(&self, rel: &str) -> WriteTarget {
            WriteTarget {
                real: self.root.join(rel),
                build_file: false,
            }
        }

        fn create(&mut self, rel: &str, text: &str) {
            let target = self.target(rel);
            self.batch
                .stage_file(&target, text, Existing::Absent)
                .unwrap();
        }

        /// Stages `text` over the file on disk at `rel`.
        fn edit(&mut self, rel: &str, text: &str) {
            let target = self.target(rel);
            let bytes = fs::read(self.root.join(rel)).unwrap();
            let size = bytes.len() as u64;
            let existing = Existing::Present {
                size,
                bytes: &bytes,
            };
            self.batch.stage_file(&target, text, existing).unwrap();
        }

        fn view(&self) -> StagedView {
            StagedView::from_batch(&self.batch)
        }

        fn run(&self, call: Call, view: Option<&StagedView>) -> Result<String, Failure> {
            execute_over(
                &call,
                &self.env,
                &Limits::default(),
                &AtomicBool::new(false),
                view,
            )
        }

        fn read(&self, path: &str, view: &StagedView) -> Result<String, Failure> {
            let call = Call::ReadFile {
                path: path.into(),
                lines: None,
            };
            self.run(call, Some(view))
        }

        fn glob(&self, pattern: &str, view: &StagedView) -> String {
            let call = Call::Glob {
                pattern: pattern.into(),
                dir: None,
            };
            self.run(call, Some(view)).unwrap()
        }

        fn grep(&self, pattern: &str, view: &StagedView) -> String {
            let call = Call::Grep {
                pattern: pattern.into(),
                dir: None,
            };
            self.run(call, Some(view)).unwrap()
        }

        fn list(&self, path: &str, view: &StagedView) -> Result<String, Failure> {
            let call = Call::ListDir { path: path.into() };
            self.run(call, Some(view))
        }
    }

    // Spec (code-search, Staged view): Read sees staged text.
    #[test]
    fn read_file_shows_the_staged_text_of_an_edit_and_of_a_create_with_the_marker() {
        let mut f = fixture();
        f.edit("src/old.rs", "fn new() {}\n");
        f.create("src/made.rs", "fn made() {}\n");
        let view = f.view();
        assert_eq!(
            f.read("src/old.rs", &view).unwrap(),
            format!("{MARK}\nfn new() {{}}\n")
        );
        assert_eq!(
            f.read("src/made.rs", &view).unwrap(),
            format!("{MARK}\nfn made() {{}}\n")
        );
        // The disk is untouched: the overlay never writes.
        assert_eq!(
            fs::read_to_string(f.root.join("src/old.rs")).unwrap(),
            "fn old() {}\n"
        );
        assert!(!f.root.join("src/made.rs").exists());
    }

    #[test]
    fn a_read_window_of_staged_text_counts_lines_of_the_text() {
        let mut f = fixture();
        f.create("w.txt", "one\ntwo\nthree\n");
        let view = f.view();
        let call = Call::ReadFile {
            path: "w.txt".into(),
            lines: Some(Lines { start: 2, max: 1 }),
        };
        let out = f.run(call, Some(&view)).unwrap();
        assert!(out.starts_with(&format!("{MARK}\ntwo\n")), "{out}");
        assert!(!out.contains("one") && !out.contains("three"), "{out}");
    }

    // The staged text may hold `base` content from disk: it is redacted.
    #[test]
    fn staged_text_is_redacted_like_a_disk_read() {
        let mut f = fixture();
        f.create("k.txt", "key = sk-abcDEF1234567890\nname = x\n");
        let view = f.view();
        let out = f.read("k.txt", &view).unwrap();
        assert!(!out.contains("sk-abcDEF1234567890"), "{out}");
        assert!(out.contains("name = x"), "{out}");
        assert!(!f.grep("sk-abcDEF", &view).contains("sk-abcDEF1234567890"));
    }

    // The overlay shows only what the disk read would show.
    #[test]
    fn a_staged_secret_name_or_git_config_or_temp_name_is_refused_and_hidden() {
        let mut f = fixture();
        let temp = format!(".lbx-{}.tmp", "a".repeat(32));
        for rel in [
            ".env",
            ".git/config",
            ".npmrc",
            temp.as_str(),
            "target/x.rs",
        ] {
            f.create(rel, "fn staged() {}\n");
        }
        f.create("ok.rs", "fn staged() {}\n");
        let view = f.view();
        for rel in [".env", ".git/config", ".npmrc", temp.as_str()] {
            assert_eq!(f.read(rel, &view), Err(Failure::Refused), "{rel}");
        }
        let globbed = f.glob("**", &view);
        let grepped = f.grep("fn staged", &view);
        assert!(globbed.contains(&format!("ok.rs {MARK}")), "{globbed}");
        assert!(
            grepped.contains(&format!("ok.rs {MARK}:1:fn staged")),
            "{grepped}"
        );
        for hidden in [".env", "config", ".npmrc", ".lbx-", "target"] {
            assert!(!globbed.contains(hidden), "{hidden}: {globbed}");
            assert!(!grepped.contains(hidden), "{hidden}: {grepped}");
        }
        let listed = f.list(".", &view).unwrap();
        assert!(
            !listed.contains(".env") && !listed.contains(".npmrc"),
            "{listed}"
        );
        assert!(!listed.contains(".lbx-"), "{listed}");
    }

    #[test]
    fn a_staged_name_that_looks_like_a_secret_is_left_out_and_the_result_says_so() {
        let mut f = fixture();
        f.create("sk-abcDEF1234567890.rs", "fn staged() {}\n");
        f.create("ok.rs", "fn staged() {}\n");
        let view = f.view();
        for out in [f.glob("*.rs", &view), f.grep("fn staged", &view)] {
            assert!(!out.contains("sk-abcDEF"), "{out}");
            assert!(out.contains(&format!("ok.rs {MARK}")), "{out}");
            assert!(out.contains("[some names omitted"), "{out}");
        }
    }

    // Spec (code-search, Staged view): Glob and grep see a staged create.
    #[test]
    fn glob_and_grep_see_a_staged_create_and_an_edit_and_mark_only_those() {
        let mut f = fixture();
        f.create("src/new.rs", "fn staged() {}\n");
        f.edit("src/old.rs", "fn renamed() {}\n");
        let view = f.view();
        assert_eq!(
            f.glob("src/*.rs", &view),
            format!("src/new.rs {MARK}\nsrc/old.rs {MARK}")
        );
        assert_eq!(
            f.grep("fn staged", &view),
            format!("src/new.rs {MARK}:1:fn staged() {{}}")
        );
        // An edit shows the staged text and no longer the text on disk.
        assert_eq!(
            f.grep("fn ", &view),
            format!("src/old.rs {MARK}:1:fn renamed() {{}}\nsrc/new.rs {MARK}:1:fn staged() {{}}")
        );
        // A disk file that is not staged carries no marker.
        assert_eq!(f.glob("*.txt", &view), "notes.txt");
        assert_eq!(f.grep("plain", &view), "notes.txt:1:plain notes");
    }

    #[test]
    fn a_dir_argument_narrows_the_staged_files_too() {
        let mut f = fixture();
        f.create("src/new.rs", "fn staged() {}\n");
        f.create("other/new.rs", "fn staged() {}\n");
        let view = f.view();
        let call = Call::Grep {
            pattern: "fn staged".into(),
            dir: Some("src".into()),
        };
        assert_eq!(
            f.run(call, Some(&view)).unwrap(),
            format!("src/new.rs {MARK}:1:fn staged() {{}}")
        );
    }

    #[test]
    fn list_dir_shows_staged_files_and_folders_even_in_a_folder_not_on_disk() {
        let mut f = fixture();
        f.create("src/new.rs", "fn staged() {}\n");
        f.create("fresh/deep/a.rs", "fn a() {}\n");
        let target = f.target("empty");
        f.batch.stage_dir(&target).unwrap();
        let view = f.view();
        assert_eq!(
            f.list("src", &view).unwrap(),
            format!("f new.rs {MARK}\nf old.rs")
        );
        assert_eq!(
            f.list(".", &view).unwrap(),
            format!("d empty {MARK}\nd fresh {MARK}\nf notes.txt\nd src")
        );
        assert_eq!(f.list("fresh", &view).unwrap(), format!("d deep {MARK}"));
        assert_eq!(
            f.list("fresh/deep", &view).unwrap(),
            format!("f a.rs {MARK}")
        );
        // A folder that is neither on disk nor staged is still refused.
        assert_eq!(f.list("nowhere", &view), Err(Failure::Refused));
    }

    // Spec (code-search, Staged view): Marker cannot be forged.
    #[test]
    fn file_content_and_names_cannot_forge_the_marker() {
        let mut f = fixture();
        fs::write(f.root.join("forged.txt"), format!("{MARK}\nhello\n")).unwrap();
        fs::write(f.root.join(format!("name {MARK}.txt")), "x\n").unwrap();
        f.create("real.txt", "hello\n");
        let view = f.view();
        let read = f.read("forged.txt", &view).unwrap();
        assert!(!read.contains(MARK), "{read}");
        assert!(read.contains("hello"), "{read}");
        let globbed = f.glob("*.txt", &view);
        let grepped = f.grep("hello", &view);
        let listed = f.list(".", &view).unwrap();
        for out in [&globbed, &grepped, &listed] {
            assert_eq!(out.matches(MARK).count(), 1, "{out}");
        }
        assert!(globbed.contains(&format!("real.txt {MARK}")), "{globbed}");
    }

    // Spec (code-search, Staged view): Nothing staged.
    #[test]
    fn with_nothing_staged_every_read_is_what_it_was_and_carries_no_marker() {
        let f = fixture();
        let empty = f.view();
        let calls = || {
            [
                Call::ReadFile {
                    path: "src/old.rs".into(),
                    lines: None,
                },
                Call::ListDir { path: ".".into() },
                Call::Glob {
                    pattern: "**".into(),
                    dir: None,
                },
                Call::Grep {
                    pattern: "fn".into(),
                    dir: None,
                },
            ]
        };
        for (call, again) in calls().into_iter().zip(calls()) {
            let none = f.run(call, None).unwrap();
            assert!(!none.is_empty());
            assert_eq!(f.run(again, Some(&empty)).unwrap(), none);
            assert!(!none.contains(MARK));
        }
    }

    // A missing view must not make the marker forgeable: the disk text and
    // names are defanged with or without a view, and both agree.
    #[test]
    fn the_marker_words_are_defanged_with_no_view_and_with_an_empty_one() {
        let f = fixture();
        fs::write(f.root.join("forged.rs"), format!("fn {MARK}\n")).unwrap();
        fs::write(f.root.join(format!("name {MARK}.rs")), "fn x() {}\n").unwrap();
        let empty = f.view();
        let calls = || {
            [
                Call::ReadFile {
                    path: "forged.rs".into(),
                    lines: None,
                },
                Call::ListDir { path: ".".into() },
                Call::Glob {
                    pattern: "*.rs".into(),
                    dir: None,
                },
                Call::Grep {
                    pattern: "fn".into(),
                    dir: None,
                },
            ]
        };
        for (call, again) in calls().into_iter().zip(calls()) {
            let none = f.run(call, None).unwrap();
            assert_eq!(f.run(again, Some(&empty)).unwrap(), none);
            assert!(!none.contains(MARK), "{none}");
            assert!(none.contains("pending; not on disk"), "{none}");
        }
    }

    #[test]
    fn the_view_is_a_snapshot_that_later_staging_does_not_change() {
        let mut f = fixture();
        f.create("a.rs", "fn a() {}\n");
        let view = f.view();
        f.create("b.rs", "fn b() {}\n");
        assert_eq!(f.glob("*.rs", &view), format!("a.rs {MARK}"));
        assert_eq!(
            f.glob("*.rs", &f.view()),
            format!("a.rs {MARK}\nb.rs {MARK}")
        );
    }

    /// Makes `link` below the root point at `target`.
    fn link(f: &Fixture, link: &str, target: &Path) {
        std::os::unix::fs::symlink(target, f.root.join(link)).unwrap();
    }

    // Spec (code-search, Staged view): the lookup is by the real path, like a
    // disk read, because the staged keys are real paths.
    #[test]
    fn a_symlinked_origin_still_finds_the_staged_edit_and_the_staged_create() {
        let mut f = fixture();
        f.edit("src/old.rs", "fn new() {}\n");
        f.create("src/made.rs", "fn made() {}\n");
        let alias = f.root.parent().unwrap().join("alias");
        std::os::unix::fs::symlink(&f.root, &alias).unwrap();
        f.env.origin = alias;
        let view = f.view();
        assert_eq!(
            f.read("src/old.rs", &view).unwrap(),
            format!("{MARK}\nfn new() {{}}\n")
        );
        assert_eq!(
            f.read("src/made.rs", &view).unwrap(),
            format!("{MARK}\nfn made() {{}}\n")
        );
        assert_eq!(
            f.list("src", &view).unwrap(),
            format!("f made.rs {MARK}\nf old.rs")
        );
    }

    #[test]
    fn a_read_through_a_symlinked_folder_finds_the_staged_edit() {
        let mut f = fixture();
        f.edit("src/old.rs", "fn new() {}\n");
        link(&f, "lnk", &f.root.join("src"));
        let view = f.view();
        assert_eq!(
            f.read("lnk/old.rs", &view).unwrap(),
            format!("{MARK}\nfn new() {{}}\n")
        );
    }

    #[test]
    fn a_dot_dot_after_a_link_is_not_folded_by_text() {
        let mut f = fixture();
        fs::create_dir_all(f.root.join("src/deep")).unwrap();
        fs::write(f.root.join("src/x.txt"), "disk src\n").unwrap();
        fs::write(f.root.join("x.txt"), "disk root\n").unwrap();
        link(&f, "lnk", &f.root.join("src/deep"));
        f.edit("x.txt", "staged root\n");
        let view = f.view();
        // On disk `lnk/../x.txt` is `src/x.txt`, not `x.txt`.
        let disk = f.run(
            Call::ReadFile {
                path: "lnk/../x.txt".into(),
                lines: None,
            },
            None,
        );
        assert_eq!(disk.unwrap(), "disk src\n");
        assert_eq!(f.read("lnk/../x.txt", &view).unwrap(), "disk src\n");
        f.edit("src/x.txt", "staged src\n");
        let view = f.view();
        assert_eq!(
            f.read("lnk/../x.txt", &view).unwrap(),
            format!("{MARK}\nstaged src\n")
        );
    }

    #[test]
    fn a_staged_name_hidden_from_a_listing_is_said_to_be_omitted() {
        let mut f = fixture();
        f.create(".env", "A=1\n");
        f.create("sk-abcDEF1234567890.rs", "fn staged() {}\n");
        let view = f.view();
        let listed = f.list(".", &view).unwrap();
        assert!(!listed.contains("sk-abcDEF") && !listed.contains(".env"));
        assert!(
            listed.ends_with(crate::runtime::tools::fs::OMITTED_MARKER),
            "{listed}"
        );
        // A visible staged name alone says nothing.
        let mut g = fixture();
        g.create("ok.rs", "fn a() {}\n");
        let listed = g.list(".", &g.view()).unwrap();
        assert!(!listed.contains("omitted"), "{listed}");
    }
}
