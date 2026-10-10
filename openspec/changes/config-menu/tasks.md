# Tasks: Config Menu

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1,710 total (range 1,500-2,100, tests ~1.5x code), 6 PRs of 260-330 lines |
| 400-line budget risk | Medium (CM4b is the tightest; CM1 and CM3 carry +30-60% test overrun risk) |
| Chained PRs recommended | Yes |
| Suggested split | CM1 -> CM2a -> CM2b -> CM3 -> CM4a -> CM4b |
| Delivery strategy | ask-on-risk |
| Chain strategy | feature-branch-chain |

Decision needed before apply: Yes
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: Medium

The design sized CM2 (~380) and CM4 (~390) near the budget. Given the history of slices landing 30-60% over estimate, both are pre-split up front (CM2a/CM2b, CM4a/CM4b). The decision to confirm before apply: accept the 6-PR split and the CM4b fallback below. Tests are never trimmed to fit a budget; if a slice grows past 400, it splits further.

Fallback splits (apply only if the slice exceeds 400 during apply):
- **CM4b** (~330): CM4b-i `feat/config-menu-04b1-save-feedback` (App `Effect::SaveConfig`, `ConfigSaved`/`ConfigSaveFailed`, `close_menu(Saved)`, error state; pure) then CM4b-ii `feat/config-menu-04b2-runtime-write` (`save_config`, atomic write, symlink, `with_config_path`, `run()` wiring).
- **CM1** (~290): split `ConfigEdit` + descriptor table + `rows` from `MenuState::on_key` navigation/cycling.
- **CM3** (~290): split `popup_area` + tiny fallback (pure) from the widget render + overlay wiring.

### Suggested Work Units (merge BOTTOM-UP into the tracker `feat/config-menu`)

Tracker `feat/config-menu` is based on `feat/panes-tabs` (CM-16) and rebased onto main when panes-tabs merges.

| Slice | Branch | Base | Est. lines | Spec coverage |
|---|---|---|---|---|
| CM1 | `feat/config-menu-01-core-menu` | `feat/config-menu` | ~290 | config-menu: Sections and items are data, Navigation, Value cycling, Footer (error state), Input swallowed (key level); `ConfigEdit` type |
| CM2a | `feat/config-menu-02a-app-state` | CM1 branch | ~260 | config-menu: Open and close, Live preview and revert (non-one-tab), Input swallowed (PTY/paste); modal-input: MENU mode, Repeat in MENU, Paste in MENU, Not from other modes |
| CM2b | `feat/config-menu-02b-app-bar-hint` | CM2a branch | ~270 | tabs: Tab bar (forced, close, unaffected, tiny); config-menu: Esc reverts the one-tab bar, Events while open; modal-input: `m` in table, Focus change keeps MENU; statusline: Mode block, root hint with `m menu` |
| CM3 | `feat/config-menu-03-popup` | CM2b branch | ~290 | config-menu: Settings rows, Footer hint/error render, Tiny terminals; statusline: MENU label and color snapshot; [manual] Kitty popup + live preview |
| CM4a | `feat/config-menu-04a-apply-edit` | CM3 branch | ~270 | config-persistence: Owned keys only, Refuse on errors (pure part); `toml_edit` dependency, parity test (CM-10) |
| CM4b | `feat/config-menu-04b-runtime-save` | CM4a branch | ~330 (HIGH) | config-persistence: Save on Enter, Create file and directories, Atomic write, Failure feedback; [manual] Kitty persistence |

Every PR must be green: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`. Strict TDD: RED is an assertion-level failing test written against compiling stubs (the stub exists and returns a wrong or default value, so the failure is a failed assertion, not a compile error); then GREEN, then REFACTOR. Characterization tests that pass immediately are labelled as such. [manual] = user-run in Kitty. Each PR states start/end, parent, follow-up, out of scope, and a dependency diagram with the current PR marked. Tests and code of one slice stay in the same commit set; no cross-slice commits.

## CM1: Core menu (pure)

- [x] CM1.1 Stubs (compile only): `src/core/menu.rs` with `Section`, `Item`, `ItemKind::{Choice, Disabled}`, `Choice`, `SECTIONS`, `MenuState`, `MenuCommand`, `Row`; register in `src/core/mod.rs`; add `ConfigEdit { table, key, value }` to `src/core/config.rs`. Stubs return empty/default values.
- [x] CM1.2 RED: descriptor integrity test walks `SECTIONS`: non-empty titles/labels, settings have key and >=2 values, no duplicate key paths; rows are Statusline, Tab bar, disabled `Mouse: off (coming soon)` in order [config-menu: Descriptor integrity].
- [x] CM1.3 GREEN: fill `SECTIONS` with `get`/`set` fn pointers over `Config`; confirm `Config` derives `Copy + PartialEq` (add if missing).
- [x] CM1.4 RED: `MenuState::open` selects the first row; `rows(&Config)` returns Title + Item rows with current values, selected flag, disabled row not selectable [config-menu: Settings rows (data part), Reopen starts fresh].
- [x] CM1.5 GREEN: `open`, `rows`.
- [x] CM1.6 RED: `on_key` navigation: `j`/`k`/Down/Up move, wrap both ways, disabled row skipped, Repeat moves [Move down and up, Wrap, Disabled row is skipped, Repeat moves].
- [x] CM1.7 GREEN: selection over selectable items (`None` when empty).
- [x] CM1.8 RED: cycling: `l`/Right/Space advance, `h`/Left go back, wrap with two values, mutate the live `Config`, return `Changed`; no effect without a selectable row [Cycle forward, Cycle backward and Space, Arrow keys].
- [x] CM1.9 GREEN: cycling via descriptor `set`.
- [x] CM1.10 RED: Enter -> `Save`, Esc -> `Cancel` on Press; both ignored (`None`) on Repeat; Release, Ctrl+Space and unlisted keys swallowed (`None`) [Repeat on Enter and Esc ignored, Unknown key, Release ignored].
- [x] CM1.11 GREEN: Enter/Esc/swallow handling.
- [x] CM1.12 RED: `edits(&live)` returns only changed keys vs `original` (empty when equal; one entry; both entries) with `"top"`/`"bottom"` values.
- [x] CM1.13 GREEN: `edits` as a diff over descriptors.
- [x] CM1.14 RED: `set_error`/`error()`: footer error stored; the next handled Press clears it and then acts normally; Repeat/Release do not clear it [Footer: Error replaces hint].
- [x] CM1.15 GREEN: error field and clearing.
- [x] CM1.16 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## CM2a: App state, open/close, live preview

- [x] CM2a.1 Characterization (expected to pass immediately): before refactor, pin current `screen()` for the four bar-position combinations and ResizePty diffs so the `bars` -> `config` rename is proven behaviour-neutral.
- [x] CM2a.2 REFACTOR: `src/app.rs` `App.bars` becomes `App.config: Config`; all existing tests stay green.
- [x] CM2a.3 Stubs: `InputMode::Menu`, `App.menu: Option<MenuState>`, `menu()` accessor, `PrefixAction::OpenMenu` variant in `src/core/prefix.rs` (not yet in the table), `open_menu`/`close_menu(Outcome::{Saved, Reverted})` returning no-ops.
- [x] CM2a.4 RED: injecting `OpenMenu` from PREFIX sets MENU, `menu()` is `Some`, no `WritePty`; Esc returns to TERMINAL with `menu()` `None`; reopen starts on the first row with the reverted values [Enter MENU, Open, Esc closes, Reopen starts fresh].
- [x] CM2a.5 GREEN: `open_menu`, `close_menu`, MENU branch in `on_key` delegating to `MenuState::on_key`; `debug_assert!(menu.is_some() == (input == Menu))` at the end of `update` (CM-1).
- [x] CM2a.6 RED: cycling a value updates `screen()` positions live and emits `ResizePty` only for panes whose size changed (none when equal); Esc restores `original` and relayouts; external file edit is not reloaded (no I/O) [Statusline moves live, Preview emits ResizePty only for changed panes, Esc reverts, External edit is not reloaded].
- [x] CM2a.7 GREEN: `Changed` -> `refresh_screen` + `relayout`; `Cancel` -> `config = original`.
- [x] CM2a.8 RED: in MENU `x`, Tab, Ctrl+Space and Release emit nothing and stay MENU; paste writes no PTY; Repeat `j` moves, Repeat Enter/Esc do nothing [Unknown key, Paste ignored, Release ignored, Repeat in MENU, Paste in MENU].
- [x] CM2a.9 GREEN: swallow rules; paste writes only in TERMINAL.
- [x] CM2a.10 RED: Enter with an unchanged draft closes without `SaveConfig`; Enter with a changed draft is a temporary session-only close (documented stub until CM4b) [Unchanged draft].
- [x] CM2a.11 GREEN: Enter handling (`edits` empty -> `close_menu(Saved)`; non-empty -> session-only close, marked `// replaced in CM4b`).
- [x] CM2a.12 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## CM2b: Forced bar, `m` binding, accent, events while open

- [x] CM2b.1 RED: `refresh_screen` with one tab and menu open yields a tab bar at the configured edge and body `rows - 2` (>=1); Esc/Enter-close hides it again with `ResizePty` for panes that changed back; two tabs open/close emits no `ResizePty`; sizes 0x0..tiny never panic with body >=1x1 [tabs: Bar forced, Bar hidden on close, Bar with several tabs unaffected, Tiny terminal with forced bar; config-menu: Esc reverts the one-tab bar]. Written against a stub where bar = `tabs > 1`, so assertions fail.
- [x] CM2b.2 GREEN: `bar = tabs > 1 || menu.is_some()` in `refresh_screen`; relayout on open and close (CM-8).
- [x] CM2b.3 RED: events while open: pane output still parsed; other pane exit keeps MENU and the draft; focus/active-tab change keeps MENU; spawn failure keeps MENU and does not touch `menu.error`; host Resize recomputes with preview and forced bar; last pane of last tab emits `Quit` [config-menu: Events while open (all); modal-input: Focus change keeps MENU].
- [x] CM2b.4 GREEN: audit `activate`, `drop_tab`, `set_focus_in`, `remove_pane` so none resets `Menu`; Quit path unchanged. Characterization if already passing.
- [x] CM2b.5 RED: `src/core/prefix.rs` `m` is a root leaf, hinted, `OpenMenu`, description `menu`; table integrity holds; `lookup` of `m` in a group, COPY, RESIZE or a confirmation does not open MENU [modal-input: `m` is in the table, Not from other modes].
- [x] CM2b.6 GREEN: add `m` after `[` and before `q` in `PREFIX_TREE` (CM-7).
- [x] CM2b.7 RED (update existing, CM-7): root hint exact string `w window · t tab · g go · b buffer · [ copy · m menu · q quit`; clipping tests recomputed for the new widths (previously 52/51/47/46); `root_hint` and `clipped_hint` snapshots updated with `cargo insta review` [statusline: Root hint with menu, Root hint stays table-generated, Existing snapshots].
- [x] CM2b.8 GREEN: adjust expected strings/widths and accept the new snapshots; no other snapshot changes.
- [x] CM2b.9 RED: `src/ui/theme.rs` `info_blue` (0x89B4FA); `input_accent(Menu)` differs from every other mode, other mappings unchanged; `InputMode::label` is `MENU`; statusline label snapshot; MENU path segment shows cwd/notice, never the root hint [statusline: MENU label and color, Mode color mapping includes MENU].
- [x] CM2b.10 GREEN: add `info_blue`, MENU arm in `input_accent`/`input_mode_style`/`label`.
- [x] CM2b.11 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## CM3: Popup widget and overlay

- [x] CM3.1 Stubs: `src/ui/components/menu.rs` with `popup_area`, `MenuPopup` widget (draws nothing), registered in `src/ui/components/mod.rs`.
- [x] CM3.2 RED: `popup_area(area, w, h)` table tests: centred, clamped to `min(w, 51) x 8`, zero area, odd sizes; full popup needs width >=24 and height >=7 [Tiny terminals (geometry)].
- [x] CM3.3 GREEN: pure `popup_area` and the size-threshold helper.
- [x] CM3.4 RED: TestBackend + insta: Settings title, three rows with current values, selected row reversed, Mouse row muted, footer `j/k move · h/l change · Enter save · Esc cancel`; error text replaces the hint and is clipped with `…`; blank row dropped first on short height [Settings rows, Footer hint, Error replaces hint].
- [x] CM3.5 GREEN: `Clear` + bordered ` Menu ` block rendered from `MenuState::rows`; footer from `error()` or hint.
- [x] CM3.6 RED: fallback `MENU · Esc cancel · Enter save` on the middle row below threshold, clipped with `…`; 0x0 and every size from 0..30 cols/rows never panic; key handling unchanged at tiny sizes (Esc reverts) [No panic at any size, Minimal message and Esc works].
- [x] CM3.7 GREEN: fallback path and zero-area guard.
- [ ] CM3.8 RED: `src/ui/mod.rs` draws the overlay last when `menu()` is `Some`; snapshot of one-tab forced bar with popup; real cursor not placed in MENU; cursor unchanged outside MENU (characterization) [tabs: Bar forced with one tab (render)].
- [ ] CM3.9 GREEN: overlay draw after the statusline; skip `cursor_position` while MENU.
- [ ] CM3.10 [manual] Kitty: `Ctrl+Space m` shows the popup; `j/k` move, `l` changes the statusline/tab bar position live with nvim reflowing; the one-tab bar appears and disappears on open/close (resize accepted); Esc restores; popup survives a small window (fallback line) [config-menu: Menu in Kitty (open, preview, Esc)].
- [ ] CM3.11 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## CM4a: `apply_edit` (pure) and dependencies

- [ ] CM4a.1 Dependencies: `Cargo.toml` add `toml_edit = "0.22"` and dev-dependency `tempfile = "3"`; confirm `Cargo.lock` adds no new crate for `toml_edit` (CM-10) and `cargo build` stays green.
- [ ] CM4a.2 Stubs: `src/core/config_edit.rs` with `apply_edit(existing: Option<&str>, edits: &[ConfigEdit]) -> Result<String, EditError>` and `EditError::{Unparseable, NotATable(&'static str)}` returning `Ok(String::new())`; register in `src/core/mod.rs`.
- [ ] CM4a.3 RED: comments, order, whitespace, unknown table and `mouse = false` preserved byte-for-byte except the changed value; trailing `# comment` on the value survives (decor kept) [config-persistence: Comments and order preserved].
- [ ] CM4a.4 GREEN: parse with `toml_edit::DocumentMut`, replace value keeping decor.
- [ ] CM4a.5 RED: missing `[tabbar]` table is inserted as `[tabbar]` with existing content untouched; `None` input builds a document; only listed edits are written (untouched owned keys not materialised) [Missing keys are created].
- [ ] CM4a.6 GREEN: table insertion, `None` handling.
- [ ] CM4a.7 RED: inline table `statusline = { position = "bottom" }` and dotted `statusline.position = "bottom"` are edited in place; `position = "middle"` is replaced with the valid value [Invalid value replaced].
- [ ] CM4a.8 GREEN: `as_table_like_mut` path.
- [ ] CM4a.9 RED: `mouse = true` kept as is, absent `mouse` not added [Mouse untouched]; `statusline = 3` and array-of-tables give `NotATable("statusline")` with no output [Non-table collision refused]; syntax error gives `Unparseable`.
- [ ] CM4a.10 GREEN: refusal paths.
- [ ] CM4a.11 RED: parity test (CM-10): the same broken inputs go to `Config::parse` and `apply_edit` and both reject them; property test: `Config::parse(apply_edit(..))` yields the draft positions with no problems over a matrix of inputs [Saved file parses back].
- [ ] CM4a.12 GREEN: fix any divergence (expected none; characterization if both already agree).
- [ ] CM4a.13 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## CM4b: Save effect, runtime write, feedback

- [ ] CM4b.1 Stubs: `Effect::SaveConfig(Vec<ConfigEdit>)`, `AppEvent::{ConfigSaved, ConfigSaveFailed(String)}`; `Panes::with_config_path(Option<PathBuf>)`; `save_config(path: Option<&Path>, edits) -> Result<(), String>` returning `Ok(())`.
- [ ] CM4b.2 RED: `src/app.rs`: Enter on a changed draft emits `SaveConfig(edits)` and the menu stays open; double Enter requests two idempotent saves; `ConfigSaved` closes with the draft kept and the bar rule restored; `ConfigSaveFailed(msg)` keeps MENU, draft and preview and sets the footer error; next key clears it; retry succeeds; Esc after failure reverts; both events ignored with no menu open [Save closes, Write fails, Retry and cancel after failure, No config path (app side)].
- [ ] CM4b.3 GREEN: replace the CM2a session-only stub; `SaveConfig` emission and feedback handling (CM-13).
- [ ] CM4b.4 RED: `tempfile::tempdir` runtime tests: missing dirs and file are created; atomic replace leaves no `.config.toml.<pid>.tmp`; read-only target dir keeps the original unchanged and the temp is removed; `None` path gives `save failed: no config path (HOME is not set)` and writes nothing [Missing file and dirs, Atomic replace, Failure keeps the original, No config path].
- [ ] CM4b.5 GREEN: `save_config`: read (`NotFound` -> `None`) -> `apply_edit` -> `create_dir_all` -> temp with `create_new` -> write -> `sync_all` -> permissions -> `rename`; best-effort temp cleanup (CM-11).
- [ ] CM4b.6 RED: refusals map to messages: `Unparseable` -> `config.toml has errors; fix it before saving`, file byte-identical; `NotATable("statusline")` -> `config.toml: statusline is not a table; fix it before saving`; other read errors -> `save failed: <e>` [Unparseable file refused, Non-table collision refused].
- [ ] CM4b.7 GREEN: error mapping.
- [ ] CM4b.8 RED: symlinked `config.toml` is written through (link still a link, target updated, temp next to the real file); existing mode is preserved on the replaced file [CM-12].
- [ ] CM4b.9 GREEN: `canonicalize` when the file exists; copy permissions to the temp.
- [ ] CM4b.10 RED+GREEN: `Panes::apply` handles `SaveConfig` and feeds exactly one `ConfigSaved` or `ConfigSaveFailed` through `drive` before the next input; `run()` resolves `config_path` once and shares it with `load_config` and `with_config_path`; existing `Panes` tests untouched [Failure feedback: exactly one result].
- [ ] CM4b.11 [manual] Kitty: change a position and press Enter, quit and restart, positions persist; a hand-commented `config.toml` keeps every comment and only the owned key changes; a deliberately broken file is not overwritten and the footer shows `config.toml has errors; fix it before saving`; a symlinked `config.toml` stays a symlink with the target updated; no stray temp file remains [config-persistence: Persistence in Kitty; config-menu: Menu in Kitty (restart shows saved positions)].
- [ ] CM4b.12 Final: `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.

## Deferred (out of scope)

- Mouse toggle (row stays disabled; `mouse` is never read or written by the menu).
- More sections, string/number inputs, scrolling, keymap editing (a new setting is a table entry; a new kind is one `ItemKind` arm).
- Polished visual design of the popup.
- Rebase of `feat/config-menu` onto main once panes-tabs merges (CM-16); resolve conflicts in `app.rs`, `ui/mod.rs`, `prefix.rs`, `theme.rs` if S9 touched them.
