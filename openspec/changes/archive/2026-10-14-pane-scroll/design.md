# Design: Pane Scroll (mouse wheel + COPY for every pane)

## Technical Approach

Decisions are pure and live in new modules; `src/app.rs` and `src/runtime.rs` get only dispatch lines (ai-chat PR15 is editing them concurrently). `core::mouse` decides what a wheel tick means for one pane and encodes bytes. `app::scroll` applies that decision to the pane under the pointer. A new `InputMode::ChatCopy` drives `transcript::Scroll`. The runtime writes the capture sequences raw and always disables them on restore.

## Architecture Decisions

| Decision | Options | Tradeoff | Choice |
|---|---|---|---|
| Chat COPY state | reuse `InputMode::Copy(CopyState)` / new `InputMode::ChatCopy(ChatCopy)` | Reusing means `copy_pane`, `copy_geometry`, highlight and cursor all assume an emulator and would need chat branches. A variant keeps every `Copy` accessor returning `None` for chats | **`ChatCopy { pending_g }`**. The view position stays in `ChatState::scroll`. Selection can extend it later |
| Mouse facts | add fields to `TermModes` / separate accessor | `TermModes` has 7 struct literals in keys/pane tests | **`Pane::mouse_facts() -> MouseFacts`** with crate enums that mirror vt100 |
| Wheel in shell COPY | 3x `LineUp` / viewport scroll | `LineUp` moves only the cursor and scrolls only at the edge, which feels dead | **New `CopyMotion::WheelUp/WheelDown`**: offset ±3, cursor clamped into the viewport |
| Utf8 (1005) encoding | drop / support | Supporting it costs about 10 lines. Dropping it leaves those apps with no wheel at all | **Support** (coordinate +32 as a UTF-8 char, above 2015 drop). Default/X10: above 223 drop |
| Event shape | `AppEvent::Mouse(MouseEvent)` / custom `Wheel` | Matches `AppEvent::Key(KeyEvent)`; click support later reuses it | **`AppEvent::Mouse(MouseEvent)`**. `to_app_event` forwards only `ScrollUp/ScrollDown` |
| Capture lifecycle | `enter_modes(out, mouse)` / effect | A single path covers startup, menu toggle and revert | **`Effect::MouseCapture(bool)`**, emitted by `App::sync_mouse()` when `config.mouse` differs from the last emitted state. Called from `apply_config` and `startup_effects` |
| Badge placement | separator / pane top-right overlay | There is no per-pane border, and a top pane has no separator | **Overlay `[+N]`** on row 0, right-aligned, skipped if wider than the pane |
| Leaving live | keys only / keys and paste | Paste also writes to the PTY | **Encoded key or paste** → `set_scrollback(0)` on the focused shell |

Rejected: crossterm `EnableMouseCapture` (turns on 1003 motion, which floods events). Click-to-focus and Shift+PgUp are out of scope.

## Data Flow

    crossterm Event::Mouse(ScrollUp|Down) ─ to_app_event ─→ AppEvent::Mouse
      App::update ─→ app::scroll::on_wheel
         pane_at(tiling, col, row) ─→ (id, rect) | ignore
         facts(id): Chat{copy} | Shell{mouse_facts, modes, copy}
         core::mouse::route_wheel(facts, dir, rel, mods) ─→ WheelAction
           Write(bytes)   → Effect::WritePty(id, bytes)
           Scrollback(±3) → emu.set_scrollback (vt100 clamps)
           CopyScroll(d)  → CopyState.apply_motion(Wheel*), set_scrollback
           ChatLines(d)   → Scroll::lines_up/down(3)
    menu Changed/Cancel/close → apply_config → sync_mouse → Effect::MouseCapture
      runtime: host_out += CAPTURE_ON|OFF ; restore_to always writes CAPTURE_OFF

Wheel acts only in `Terminal`, `Copy` and `ChatCopy`, and only when `config.mouse` is on. Every other mode ignores it, and it never clears the notice or changes the focus. Chat panes with an open overlay (list, tag, note, summary) ignore the wheel and refuse `[`.

## Interfaces / Contracts

```rust
// src/core/mouse.rs (pure)
pub const WHEEL_STEP: usize = 3;
pub const CAPTURE_ON: &[u8] = b"\x1b[?1000h\x1b[?1006h";
pub const CAPTURE_OFF: &[u8] = b"\x1b[?1006l\x1b[?1000l";
pub enum WheelDir { Up, Down }
pub enum MouseMode { None, Press, PressRelease, ButtonMotion, AnyMotion }
pub enum MouseEncoding { Default, Utf8, Sgr }
pub struct MouseFacts { pub mode: MouseMode, pub encoding: MouseEncoding, pub alt_screen: bool }
pub enum PaneFacts { Shell { mouse: MouseFacts, modes: TermModes, copy: bool }, Chat }
pub enum WheelAction { Write(Vec<u8>), Scrollback(WheelDir), CopyScroll(WheelDir), ChatLines(WheelDir), Ignore }
pub fn route_wheel(f: PaneFacts, d: WheelDir, col: u16, row: u16, m: KeyModifiers) -> WheelAction;
pub fn encode_wheel(d: WheelDir, col: u16, row: u16, m: KeyModifiers, mode: MouseMode, enc: MouseEncoding) -> Option<Vec<u8>>;
```

Routing order: copy → `CopyScroll`; mode in `PressRelease`, `ButtonMotion`, `AnyMotion` → `Write(encode_wheel)`, with col/row 0-based pane-relative and 1-based in the bytes, button 64/65 plus shift 4, alt 8, ctrl 16 (X10 `Press` never receives the wheel: it falls through to the alt-screen and scrollback rules); alt screen → `Write` of 3x `encode_key(Up|Down, modes)`; otherwise `Scrollback`. Chat goes to `ChatLines`, in COPY or not.

`Scroll::lines_up(n,total,h)` / `lines_down`: same contract as `page_*` (a view that fits stays `Follow`, reaching the end gives `Follow`). `App::scrolled_by(id) -> Option<usize>` (shell offset, or chat lines below the view, `None` at 0) feeds the badge. `App::scroll_position()` gives `copy_position()` in COPY, otherwise the focused pane's `(N, total)` when scrolled, and in `ChatCopy` always (an explicit 0).

## File Changes

**Slice 1: chat COPY, line scroll, badge, statusline**

| File | Action | Description |
|---|---|---|
| `src/core/ai/chat/transcript.rs` | Modify | `lines_up/lines_down` |
| `src/core/ai/chat/copy.rs` (+ `mod`) | Create | `ChatCopy`, pure key table → `ChatCopyCommand` |
| `src/app/chat_copy.rs` | Create | `on_chat_copy_key`; exit gives `Follow` plus `hold_exit` |
| `src/app/scroll.rs` | Create | `scrolled_by`, `scroll_position` |
| `src/app.rs` | Modify | variant, label, `on_key` arm, `[` enters `ChatCopy`, drop `CHAT_COPY_REFUSED`, 3 reset matches, COPY hint in `status_path`, `mod` lines |
| `src/ui/components/scroll_badge.rs` | Create | `[+N]` widget |
| `src/ui/mod.rs` | Modify | badge per pane, `scroll_position()`, no composer cursor in `ChatCopy` |

**Slice 2: wheel routing and app handling**

| File | Action | Description |
|---|---|---|
| `src/core/mouse.rs` | Create | route and encode |
| `src/core/pane.rs` | Modify | `mouse_facts()` |
| `src/core/copy.rs` | Modify | `WheelUp/WheelDown` |
| `src/app/scroll.rs` | Modify | `on_wheel`, live reset helper |
| `src/app.rs` | Modify | `AppEvent::Mouse` arm; reset in `on_terminal_key` and `on_paste` |

**Slice 3: capture, config, menu, persistence**

| File | Action | Description |
|---|---|---|
| `src/runtime.rs` | Modify | `to_app_event` wheel, `Effect::MouseCapture` → `host_out`, `CAPTURE_OFF` in `restore_to` (so the panic hook is covered), `startup_effects` |
| `src/app.rs` | Modify | `Effect::MouseCapture`, `mouse_captured`, `sync_mouse` in `apply_config` |
| `src/core/config.rs` | Modify | manual `Default` (`mouse: true`), drop notice, root `is_flag` |
| `src/core/config_edit.rs` | Modify | `table == ""` targets the root table |
| `src/core/menu.rs` | Modify | `ItemKind::Toggle` "Mouse" off/on, `edits` |
| `openspec/specs/...` | Modify | deltas (spec phase) |

## Testing Strategy

Strict TDD, `cargo test`, tests first in each slice.

| Layer | What | Approach |
|---|---|---|
| Unit | `route_wheel` across pane kind, mode, encoding, alt, copy and app cursor | table tests |
| Unit | `encode_wheel` | golden bytes: SGR, Default, Utf8, modifiers, out of range gives `None` |
| Unit | `lines_*`, ChatCopy table, `WheelUp/Down`, root `config_edit`, Toggle edits | table / golden text |
| App | no focus change, anchored offset under output, live on key/paste, COPY wheel, chat COPY flow and resets, `MouseCapture` on toggle, revert and startup | `App` with fakes |
| Runtime | `restore_to` contains `?1000l`/`?1006l`; wheel mapped, clicks and motion dropped; `host_out` bytes | byte asserts |
| UI | badge (shell, chat, narrow), COPY hints, non-COPY position | insta snapshots |

## Migration / Rollout

No data migration. Users without the key get the mouse on; `mouse = false` turns it off. Slices revert in reverse order.

## Open Questions

- [ ] Spec must confirm: paste returns to live, overlays block chat COPY, wheel ignored in PREFIX, MENU and CONFIRM.
- [ ] toml_edit must put a new root `mouse` key before the first table (golden test).
