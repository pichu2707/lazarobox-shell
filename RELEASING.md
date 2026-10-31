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

## One-click release

1. Write the user-facing notes under `## [Unreleased]` in `CHANGELOG.md` as
   part of the PRs that make the changes, and merge them to `main`.
2. Wait for CI on `main` to pass. The release refuses to start without a
   successful `ci.yml` run for the exact commit it would release, because the
   release workflow only repeats the Linux checks.
3. Rehearse (optional, recommended the first time): Actions -> Release -> Run
   workflow on `main` with **dry_run** ticked, or
   `gh workflow run release.yml -f bump=auto -f dry_run=true`. Nothing is
   committed, tagged or published; the run summary shows the new version, the
   diff and the release notes.
4. Release: the same without dry_run, or
   `gh workflow run release.yml -f bump=auto`.

The `prepare` job then picks the version, moves the `[Unreleased]` entries
under a new `## [X.Y.Z] - <date>` heading, rewrites the compare links at the
bottom of `CHANGELOG.md`, bumps `Cargo.toml` and `Cargo.lock`, commits
`chore(release): vX.Y.Z` to `main` as `github-actions[bot]` and pushes that
commit and the tag `vX.Y.Z` together. The same run then verifies, tests,
publishes to crates.io and creates the GitHub Release.

`bump` is one of:

- `auto` (default): `minor` when `[Unreleased]` has a `### Added` heading,
  otherwise `patch`. It never picks `major`.
- `patch`, `minor`, `major`: the bump you name, whatever the changelog says.
  A breaking change (see the list at the top) is always a `major` you choose.

A pre-release version in `Cargo.toml` (for example `1.1.0-rc.1`) is handled
through its base version `1.1.0`: `auto` and `patch` finalize it to `1.1.0`;
`minor` and `major` bump from the base, as if it had been released (`1.2.0`,
`2.0.0`). To start a new release candidate, use the manual path below.

The workflow stops with a clear error when it is not run on `main`, when
`[Unreleased]` has no entries, or when the tag already exists. Because the
version commit is pushed with the workflow token, CI does not run on it; the
release workflow's own Linux gate covers it, and the next release needs new
merged work anyway.

Review the entries in `[Unreleased]` by hand before releasing: the script moves
them as they are. The script is `.github/scripts/prepare-release.sh`, with
tests in `.github/scripts/tests/` that run in CI.

### If a release fails after `prepare`

Once `prepare` succeeds, the version commit and the tag are already on `main`,
so a later failure (a new clippy lint in the gate, a crates.io hiccup) leaves
the release half done. Do not dispatch again: `[Unreleased]` is now empty and
the version commit has no CI run, so `prepare` refuses.

- A flaky failure: open the run and use **Re-run failed jobs**. The jobs keep
  `prepare`'s outputs, and `publish` skips a version already on crates.io.
- A real fix is needed and the version is not on crates.io yet: delete the
  tag (`git push origin :refs/tags/vX.Y.Z`), merge the fix to `main` with its
  entry under the `## [X.Y.Z]` section, wait for CI, then tag the new `main`
  commit and push it to run the tag path
  (`git tag -f vX.Y.Z origin/main && git push origin vX.Y.Z`). `Cargo.toml`
  already says `X.Y.Z`, so the tag check passes.
- The version is already on crates.io: it can never be replaced. Merge the
  fix with an `[Unreleased]` entry and release it as the next patch.

## Manual release (fallback)

Pushing a tag still works exactly as before and runs the same verify, gate,
publish and release jobs, so use it when the one-click path is not an option
(for example to cut a release candidate).

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
5. Merge that commit to `main`, then tag it: `git tag vX.Y.Z`.
6. Check the package: `cargo publish --dry-run`.
7. Push the tag: `git push origin vX.Y.Z`. The workflow publishes to crates.io
   (Trusted Publishing); there is no manual `cargo publish`.

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
  remove the code. Run `cargo publish --dry-run` before releasing.
- Package contents are controlled by `exclude` in `Cargo.toml`. Inspect them
  with `cargo package --list`; the crates.io limit is 10 MB.
