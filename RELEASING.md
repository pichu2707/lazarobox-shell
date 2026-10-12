# Releasing

Versioning follows [SemVer](https://semver.org). While the crate is below 1.0:
bump the **minor** version for each feature milestone and the **patch** version
for fixes. The changelog follows [Keep a Changelog](https://keepachangelog.com)
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
   This assumes the retroactive tags (`v0.1.0` ... `v0.7.0`) exist; otherwise
   git-cliff will list the whole history.
4. Commit: `git commit -am "chore(release): vX.Y.Z"`.
5. Tag: `git tag vX.Y.Z`.
6. Check the package: `cargo publish --dry-run`.
7. Publish (run by the maintainer): `cargo publish`.
8. Push the commit and the tag: `git push && git push origin vX.Y.Z`.

## Notes

- A published version can only be **yanked** (`cargo yank --version X.Y.Z`),
  never deleted. Yanking stops new projects from depending on it; it does not
  remove the code. Double-check step 6 before step 7.
- Package contents are controlled by `exclude` in `Cargo.toml`. Inspect them
  with `cargo package --list`; the crates.io limit is 10 MB.
