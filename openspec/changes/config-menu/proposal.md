# Proposal: Config Menu

## Intent

Changing settings today means editing `config.toml` by hand. `Ctrl+Space m` opens a general menu as a plain popup. Its only section for now is **Settings**, where each item maps to one `config.toml` key. Each change previews live. Enter saves, keeping the user's comments, and Esc reverts.

## Scope

### In Scope
- The root prefix gets the key `m`, shown in the hint as `m menu`. It is not `Ctrl+M`, because terminals send that as Enter.
- The menu has sections. **Settings** is the only one, with these rows:
  - `statusline.position` (top/bottom)
  - `tabbar.position` (top/bottom)
  - a disabled `Mouse: off (coming soon)` row, shown muted
- Sections and items come from a static descriptor table. A new section or setting is a data change; the popup code does not change.
- Keys: `j/k`/Up/Down move; `h/l`/Left/Right/Space change the value; Enter saves and closes; Esc reverts and closes. Other keys are swallowed and paste is ignored. The PTY never receives input while the menu is open.
- Footer: `j/k move · h/l change · Enter save · Esc cancel`, or an error message.
- Live preview uses the existing relayout and its ResizePty diffs. The tab bar is forced visible while the menu is open.
- Saving changes only the menu-owned keys, written with `toml_edit`. It creates the file and its directories if missing and writes atomically. If `config.toml` has errors, it refuses to save. If the save fails, the menu stays open and shows the error.

### Out of Scope
- Sections other than Settings, such as keymaps or commands.
- Real mouse support, hot reload of external edits, and a polished visual design.

## Capabilities

### New Capabilities
- `config-menu`: opening the general menu, its sections, navigation, value cycling, the disabled row, preview and revert, the footer, and tiny terminals.
- `config-persistence`: saving owned keys while keeping comments and order, atomic writes, refusal on errors, and failure feedback.

### Modified Capabilities
- `modal-input`: the root key `m` (hinted) and a MENU mode that swallows input.
- `tabs` (delta from panes-tabs): the tab bar is shown with one tab while the menu is open.
- `statusline`: a MENU mode label. This is an assumption that design must confirm.

## Approach

This is the exploration's Approach A.
- `MenuState` in core is pure. It holds the section and item selection, the draft values, the original values and an error line.
- The `SECTIONS`/`ITEMS` descriptor table is static.
- `InputMode::Menu` is a marker variant. The state lives in `App.menu: Option<MenuState>`, because `InputMode` is `Copy`.
- The popup is drawn as an overlay above the panes.
- `Effect::SaveConfig` makes the runtime write the file. It answers with `ConfigSaved` or `ConfigSaveFailed`.
- `?` stays reserved for the command viewer.

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/menu.rs` | New |
| `src/core/config.rs` | Modified: save path |
| `src/core/prefix.rs` | Modified: root `m`. The root hint test that pins `w t g b [ q` must now include `m` |
| `src/app.rs` | Modified: Menu mode, preview, effects |
| `src/runtime.rs` | Modified: atomic write |
| `src/ui/components/menu.rs`, `src/ui/mod.rs` | New / Modified |
| `Cargo.toml` | Adds `toml_edit` |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Overwriting the user's hand edits | Med | Read the file at save time, edit only owned keys, refuse when it has errors |
| Two TOML stacks | Med | Design decides whether to parse with `toml_edit` too |
| Sections overbuilt | Med | Only data shape now; no multi-section navigation UX polish |
| Tiny terminals | Med | Clip and skip drawing below the minimum size |
| S8c not merged | High | Do not start CM1 before S8c lands |

## Rollback Plan

The tracker branch `feat/config-menu` is the only branch that merges to main. To roll back, revert that merge. The `config.toml` format does not change, so files the menu wrote stay valid.

## Dependencies

- panes-tabs S8c (PRs #30/#31): `Config`, `BarPositions`, `Config::parse`/`load`/`config_path` and `App::with_config`.
- Chain base: start the tracker from main after panes-tabs lands, because that gives a clean diff. If panes-tabs has not landed yet, start from the panes-tabs tracker and retarget later. That starts earlier but costs rebase churn.

## Delivery Slices

Feature-branch chain, with the ask-on-risk delivery strategy.

| # | Content | Lines |
|---|---|---|
| CM1 | `MenuState`, section/item descriptors, pure `apply_edit` | ~300 |
| CM2 | App wiring, `m` plus the hint test, preview and revert, forced tab bar | ~350 |
| CM3 | Popup widget, overlay, clipping | ~250 |
| CM4 | Save effect, atomic write, feedback, error footer | ~250 |

## Success Criteria

- [ ] `cargo test`, `cargo clippy --all-targets` and `cargo fmt --check` pass on every slice.
- [ ] Manual checks:
  - [ ] the root hint shows `m menu`
  - [ ] the bars move live
  - [ ] Esc restores them
  - [ ] Enter writes only the owned keys, and comments survive
  - [ ] a broken file is never overwritten
  - [ ] the PTY gets no input while the menu is open
