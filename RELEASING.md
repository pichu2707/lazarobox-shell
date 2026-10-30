# Releasing

Versioning follows [SemVer](https://semver.org). From 1.0 on:

- **major**: a breaking change to `config.toml` (keys or values), to the
  `chat.db` format (a build that can no longer open an older file), to the
  public CLI flags and subcommands, or to the prefix key and key bindings;
- **minor**: new features, and new config keys or values;
- **patch**: fixes only.

The internal `__*` subcommands, log text and UI layout are outside that rule
(see "Estabilidad y compatibilidad (1.0)" in the README). Below 1.0, a minor
bump marked a feature milestone and a patch bump a fix.

The changelog follows [Keep a Changelog](https://keepachangelog.com)
and is generated from Conventional Commits with
[git-cliff](https://git-cliff.org) (`cliff.toml`).

## Steps

1. Make sure `main` is up to date and green:
   `cargo test && cargo clippy --all-targets -- -D warnings && cargo fmt --check`.
2. Bump `version` in `Cargo.toml` (this also updates `Cargo.lock` on the next build).
3. Generate the changelog entry:
   `git cliff --unreleased --tag vX.Y.Z --strip header`.
   Paste the output below `## [Unreleased]`'s place in `CHANGELOG.md` (replace
   the hand-written Unreleased notes), review it by hand to keep it
   user-facing, and update the compare links at the bottom of the file.
   `v0.7.0` is the baseline tag; git-cliff lists only changes after it.
4. Commit: `git commit -am "chore(release): vX.Y.Z"`.
5. Tag: `git tag vX.Y.Z`.
6. Check the package: `cargo publish --dry-run`.
7. Publish (run by the maintainer): `cargo publish`.
8. Push the commit and the tag: `git push && git push origin vX.Y.Z`.

## Before 1.0.0

1. Publish `1.0.0-rc.1` to crates.io (same steps as above; cargo does not
   install a pre-release unless asked, so it does not reach `cargo install`
   users by default). The release workflow accepts the tag `v1.0.0-rc.1` too:
   it publishes it and marks the GitHub Release as a pre-release.
2. Use it daily on Linux for a few days and fix what turns up (each fix is a new
   `rc.N`).
3. Publish `1.0.0` from the last good release candidate.

## Notes

- A published version can only be **yanked** (`cargo yank --version X.Y.Z`),
  never deleted. Yanking stops new projects from depending on it; it does not
  remove the code. Double-check step 6 before step 7.
- Package contents are controlled by `exclude` in `Cargo.toml`. Inspect them
  with `cargo package --list`; the crates.io limit is 10 MB.
