# Design: Config Menu

## Technical Approach

This is exploration Approach A. It keeps the functional-core / imperative-shell split from panes-tabs.

- **Pure core.** The new `core::menu` holds a static descriptor table (`SECTIONS`) and a pure `MenuState`. `MenuState` stores the cursor, the `original` config captured at open and an error line. The **live `App.config` is the draft**, so there is one source of truth for the preview. The new pure `core::config_edit::apply_edit` rewrites only the owned keys with `toml_edit`.
- **App.** `App` gets an `InputMode::Menu` marker and an `App.menu: Option<MenuState>` field (`InputMode` is `Copy`). Each value change goes through the existing `refresh_screen` + `relayout`, which emits the ResizePty diffs. Enter emits `Effect::SaveConfig(Vec<ConfigEdit>)`.
- **Runtime.** It resolves the config path once at startup, does the read, edit and atomic write, and feeds back `AppEvent::ConfigSaved` or `ConfigSaveFailed(String)` through the existing `drive` loop.
- **UI.** `ui::render` draws the popup last as an overlay, from a view model (`MenuState::rows`). The widget knows nothing about settings.

ADR numbering restarts for this change and uses the `CM-` prefix, so it never collides with the panes-tabs ADRs. The design is over the 800-word budget on purpose: the orchestrator asked for 9 decision areas plus 8 spec open questions.

## Architecture Decisions

| # | Topic | Options | Decision / rationale |
|---|---|---|---|
| CM-1 | State placement | (a) `InputMode::Menu(MenuState)`; (b) `InputMode::Menu` marker + `App.menu: Option<MenuState>`; (c) a separate `App.overlay` stack | **(b)**. `InputMode` is `Copy` and pattern-matched everywhere, so (a) would cost `Copy` across the code. (c) is overbuilt for one overlay. **Invariant:** `menu.is_some() == (input == Menu)`. Only `open_menu()` and `close_menu(Outcome::{Saved, Reverted})` touch both, and `close_menu` is the only path out of MENU. A `debug_assert!` at the end of `update` checks it. **Audit of the code that sets the input mode:** `activate`, `drop_tab` and `set_focus_in` reset only `Copy \| Resize`, and `remove_pane` resets only `Confirm`. None of them touches `Menu`, so a focus or tab change keeps MENU (spec). The last pane of the last tab still returns `Effect::Quit`. Tests pin each of these paths. |
| CM-2 | Draft location | `MenuState.draft` copied into the App; the live `App.config` as the draft | **The live config is the draft.** `App.bars` becomes `App.config: Config` (`Config` is `Copy`). `MenuState` keeps `original: Config`. A change mutates `App.config` through the descriptor `set`; Esc does `config = original`. With no copy there is no sync bug. The edits to save are `diff(original, live)` over the descriptors. |
| CM-3 | Descriptor table | free-form trait objects; enum of fn-pointer descriptors | `Section { title, items: &[Item] }` and `Item { label, kind: ItemKind }`. `ItemKind::Choice(Choice { table, key, values: &[&str], get: fn(&Config) -> usize, set: fn(&mut Config, usize) })` or `ItemKind::Disabled`. The value string doubles as the display text and the TOML value (`"top"`/`"bottom"`). The disabled row's label is `Mouse: off (coming soon)`. **The widget renders `MenuState::rows(&Config) -> Vec<Row::{Title, Item{label, value, selected, enabled}}>`.** A new setting is a table entry, and a new kind (for example a keymap action) is one `ItemKind` arm in core, never a widget change. Not built yet: section switching, scrolling, string or number inputs. |
| CM-4 | Navigation keys | clamp vs wrap; land on vs skip the disabled row | **Wrap and skip, per the spec.** `selected: Option<usize>` indexes the selectable items in table order (`None` when there are none). `j`/Down/`k`/Up move; `l`/Right/Space go to the next value and `h`/Left to the previous, with wrap. `MenuState::on_key(&KeyEvent, repeat, &mut Config) -> MenuCommand::{None, Changed, Save, Cancel}`, the same shape as `CopyState::on_key`. |
| CM-5 | Event kinds in MENU | — | Release is ignored (existing rule). Repeat acts like Press for move and cycle keys and is **dropped for Enter and Esc**: `on_key` passes `repeat` in and `MenuState` ignores Enter and Esc when `repeat` is true. Ctrl+Space and every unlisted key are swallowed. `on_paste` already writes only in TERMINAL. A pending `error` is cleared by the next handled Press, which then acts normally. |
| CM-6 | Opening | — | `PrefixAction::OpenMenu` at the root (`m`, description `menu`, hinted). It is reachable only from the root of PREFIX, which is entered only from TERMINAL. `m` in a group, COPY, RESIZE or a confirmation is a group Cancel or is swallowed, as today. Opening refreshes the screen and relayouts (forced bar, CM-8). |
| CM-7 | Root hint order (spec Q4) | `m` first; after `[`; last | **After `[ copy`, before `q quit`**: `w window · t tab · g go · b buffer · [ copy · m menu · q quit`. Groups stay first and the destructive `q` stays last. The clipping tests in `prefix.rs` (widths 52/51/47/46) and the `root_hint` and `clipped_hint` snapshots get new values. |
| CM-8 | Forced one-tab bar (spec Q1) | (a) real row: body shrinks, ResizePty on open and close; (b) overlay row over the body, no resize | **(a)**. `refresh_screen` uses `bar = tabs > 1 \|\| menu.is_some()`. The tabs spec requires the body to be "one row shorter". The preview must show the real geometry the position will produce. (b) would hide a row of the pane, and the statusline preview resizes panes anyway. Cost: with one tab, open and close each send one SIGWINCH, so nvim redraws twice. That is the same as tmux toggling its status bar, and it never happens with two or more tabs. |
| CM-9 | Persistence edit | `apply_edit(existing, &Config)`; `apply_edit(existing, &[ConfigEdit])` | **`fn apply_edit(existing: Option<&str>, edits: &[ConfigEdit]) -> Result<String, EditError>`**, with `ConfigEdit { table, key, value: &'static str }` defined in `core::config`. Only **changed** keys are written, so untouched owned keys are never materialised as defaults. It uses `toml_edit::DocumentMut`. `None` means an empty document. A parse error gives `EditError::Unparseable`. A missing table is inserted as a standard `[table]`. `get_mut(table).as_table_like_mut()` covers `[t]`, inline `t = {…}` and dotted `t.position = …`. A table entry that is a non-table value or an array of tables gives `EditError::NotATable(table)`. An existing value is replaced **keeping its decor**, so a trailing `# comment` survives. No newline normalisation is done, so untouched bytes stay identical. `mouse` is never read or written. Property: `Config::parse(output)` yields the draft positions with no problems. |
| CM-10 | Two TOML stacks (spec Q5) | migrate `Config::parse` to `toml_edit`; keep both | **Keep both.** The lockfile shows `toml 0.8.23` already depends on `toml_edit 0.22.27`. Adding `toml_edit = "0.22"` therefore adds no crate, and **the same parser decides what is a syntax error for both**. Migrating the merged and tested S8c parser would be churn with no gain. Guard: a test feeds the same broken inputs to `Config::parse` and `apply_edit` and expects both to reject them. Bump `toml` and `toml_edit` together; `toml 0.9` drops `toml_edit`, so a bump would revisit this ADR. |
| CM-11 | Save effect and runtime | async task; synchronous in `Panes::apply` | **Synchronous** (spec Q7). The file is tiny. `drive` turns the feedback into an event before the next input event is polled, so no key can arrive while a save is pending. There is no loading state, flag or timeout. A pure-test double Enter just requests two idempotent saves. `run()` resolves `config_path` **once** and passes it to `load_config` and to `Panes::with_config_path(Option<PathBuf>)`, a builder, so the existing tests are untouched. `fn save_config(path: Option<&Path>, edits) -> Result<(), String>`: `None` gives `save failed: no config path (HOME is not set)`. Read: `NotFound` means `None`; other errors give `save failed: <e>`. Then `apply_edit` → `create_dir_all(parent)` → temp `.config.toml.<pid>.tmp` with `create_new` → write → `sync_all` → copy permissions → `rename`. On any failure after the temp exists, the temp is removed (best effort). `Unparseable` gives `config.toml has errors; fix it before saving`; `NotATable(t)` gives `config.toml: <t> is not a table; fix it before saving`. |
| CM-12 | Permissions and symlinks (spec Q6) | replace the link; write through | **Write through, keep the mode.** If `config.toml` exists, the target is `fs::canonicalize(path)`, so dotfile symlinks (stow, home-manager) stay links and the temp is created next to the real file, which keeps rename atomic. The existing file's permissions are applied to the temp before the rename. A read-only target (for example the Nix store) fails with `save failed: …`. |
| CM-13 | Feedback | — | `AppEvent::ConfigSaved` calls `close_menu(Saved)`: the live config stays and the bar rule is restored. `AppEvent::ConfigSaveFailed(msg)` sets `menu.error = Some(msg)` and keeps the draft and the preview. Both are ignored when no menu is open. **There is no success notice** (spec Q8): closing happens only on success, and a failure keeps the menu open, so closing is the confirmation. That needs no spec delta and puts no noise in the path segment. The spawn-failure notice still goes to the statusline, never to the footer. |
| CM-14 | Popup widget (spec Q2) | — | `ui/components/menu.rs`, drawn after the statusline: `Clear` then a bordered `Block` titled ` Menu `. Rows: section title (bold), items `label  value`, then a blank row and the footer. The selected row uses the reversed MENU accent and the disabled row uses `text_muted`. Wanted size: `min(frame.w, 51) × 8`, centred by a pure `popup_area(area, w, h) -> Rect`. The blank row is dropped first when height is short. **The full popup needs width ≥ 24 and height ≥ 7.** Below that the fallback is a single row `MENU · Esc cancel · Enter save` on the middle row, clipped with `…`. With 0 rows or columns nothing is drawn. Footer: the hint or the error, clipped with `…`. The real cursor is **not placed** while MENU is open, so it is hidden instead of blinking under the popup. |
| CM-15 | MENU accent (spec Q3) | reuse an existing colour; new palette entry | **New `info_blue: 0x89B4FA`** (Catppuccin Mocha blue, the same family as the rest of the palette). The spec requires a colour distinct from every other mode. `input_accent(Menu)`, `InputMode::label` = `MENU`, and the separators follow automatically. The polished visual design is a later change. |
| CM-16 | Chain base | wait for panes-tabs on main; base on the panes-tabs tracker now | **Base `feat/config-menu` on `feat/panes-tabs` now** (user wants to start). CM1 targets the tracker; CM2..CM4 each target the previous slice branch (feature-branch-chain). **Retarget plan:** when panes-tabs merges to main, run `git rebase main` on `feat/config-menu`. Panes-tabs commits are already ancestors, so only CM commits replay. Then open the tracker PR to main. Cost: if panes-tabs S9 touches `app.rs`, `ui/mod.rs`, `prefix.rs` or `theme.rs`, conflicts must be resolved during that rebase. Mitigation: keep the CM diffs additive (new modules, new match arms) and do the rebase as soon as S9 merges. Waiting would give a clean diff but block the user. |

## Data Flow

```
key ─> App::on_key ─ MENU ─> MenuState::on_key(&key, &mut config)
                                  │ Changed → refresh_screen + relayout → ResizePty (diffs)
                                  │ Cancel  → config = original → close_menu(Reverted) + relayout
                                  │ Save    → edits = diff(original, config)
                                  │            empty → close_menu(Saved)
                                  │            else  → Effect::SaveConfig(edits)
Panes::apply ── save_config(path, edits): read → apply_edit → tmp → fsync → rename
             └─> ConfigSaved | ConfigSaveFailed(msg) ─> App::update (same drive call)
render: tab bar? ─ panes ─ separators ─ statusline ─ [menu overlay from MenuState::rows]
```

## File Changes

| File | Action | Description |
|---|---|---|
| `src/core/menu.rs` | Create | `SECTIONS`, `Item`, `Choice`, `MenuState`, `MenuCommand`, `Row`, `edits()` |
| `src/core/config_edit.rs` | Create | `apply_edit`, `EditError` (pure, `toml_edit`) |
| `src/core/config.rs` | Modify | `ConfigEdit` type |
| `src/core/mod.rs` | Modify | register the two modules |
| `src/core/prefix.rs` | Modify | `OpenMenu`, root `m` (hinted), updated hint and clipping tests |
| `src/app.rs` | Modify | `InputMode::Menu`, `App.config`, `App.menu`, open/close, forced bar, `SaveConfig`, `ConfigSaved`/`ConfigSaveFailed`, `menu()` accessor |
| `src/runtime.rs` | Modify | path resolved once, `Panes::with_config_path`, `save_config`, SaveConfig arm |
| `src/ui/components/menu.rs` | Create | popup widget, `popup_area`, tiny fallback |
| `src/ui/components/mod.rs`, `src/ui/mod.rs` | Modify | overlay draw; cursor hidden in MENU |
| `src/ui/theme.rs`, `src/ui/components/statusline.rs` | Modify | `info_blue`, MENU arm, snapshot |
| `Cargo.toml` | Modify | `toml_edit = "0.22"`; dev-dependency `tempfile = "3"` (already in the lockfile) |

## Interfaces / Contracts

```rust
pub struct ConfigEdit { pub table: &'static str, pub key: &'static str, pub value: &'static str }
pub enum EditError { Unparseable, NotATable(&'static str) }
pub enum MenuCommand { None, Changed, Save, Cancel }
impl MenuState {
    pub fn open(original: Config) -> Self;
    pub fn on_key(&mut self, key: &KeyEvent, repeat: bool, live: &mut Config) -> MenuCommand;
    pub fn edits(&self, live: &Config) -> Vec<ConfigEdit>;
    pub fn rows(&self, live: &Config) -> Vec<Row>;
    pub fn error(&self) -> Option<&str>;
}
// Effect::SaveConfig(Vec<ConfigEdit>); AppEvent::ConfigSaved; AppEvent::ConfigSaveFailed(String)
```

## Testing Strategy (strict TDD; `cargo test`, clippy, fmt on every slice)

| Layer | What | Approach |
|---|---|---|
| Core unit | descriptor integrity, wrap and skip, cycling, Repeat on Enter/Esc, error clearing, `edits` diff; `apply_edit` with comments, decor, inline and dotted tables, missing tables, `None`, refusals, mouse untouched, parse-back property, rejection parity with `Config::parse` | plain `#[test]` in `menu.rs` and `config_edit.rs` |
| App | open from PREFIX only, swallow and paste, preview ResizePty diffs, Esc revert (one-tab bar), Save/unchanged, feedback, pane exit/spawn fail/resize/last-pane Quit while open, invariant | `app_tests` through `update` |
| UI | popup rows, muted row, footer and error, forced one-tab bar, MENU label and colour, 0x0..small sizes, fallback | `TestBackend` + insta |
| Runtime fs | create dirs and file, atomic replace with no temp left, failure keeps the original (read-only dir), symlink write-through, mode kept, no path | `tempfile::tempdir` |

## Migration / Rollout

No migration is needed: the `config.toml` format is unchanged. Slices (feature-branch-chain, ask-on-risk):

| # | Content | Est. lines | Depends |
|---|---|---|---|
| CM1 | `core::menu` + `ConfigEdit` + tests | ~280 | — |
| CM2 | App wiring, `m` + hint tests, preview/revert, forced bar, MENU label/accent; Enter closes with `edits` empty-check only (Save effect stubbed as a session-only close) | ~380 | CM1 |
| CM3 | Popup widget, overlay, fallback, hidden cursor, snapshots | ~260 | CM2 |
| CM4 | `apply_edit` + `toml_edit`, `SaveConfig`, runtime atomic write, feedback, error footer | ~390 | CM3 |

CM2 and CM4 are close to the 400-line budget. If either goes over, sdd-tasks may split CM4 into CM4a (`apply_edit`, pure) and CM4b (runtime + feedback).

## Open Questions

- [ ] CM-16: is the scope of panes-tabs S9 known? If it touches `app.rs` or `ui/mod.rs` heavily, rebase CM onto it early.
- [ ] CM2 temporarily makes Enter a session-only apply until CM4 lands. The tracker never ships that to main, but reviewers should know.
