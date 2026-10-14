# Tasks: Pane Scroll (mouse wheel + COPY for every pane)

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~1,700 total (tests included): PR1 ~600, PR2 ~550, PR3 ~550 |
| 400-line budget risk | High (each PR above 400; the user accepts `size:exception`) |
| Chained PRs recommended | Yes |
| Suggested split | PR1 (chat COPY) -> PR2 (wheel routing) -> PR3 (capture, config, menu) |
| Delivery strategy | auto-chain |
| Chain strategy | feature-branch-chain (tracker `feat/pane-scroll`) |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: feature-branch-chain
400-line budget risk: High

Rules for every PR:
- Strict TDD: RED tests first (compiling stubs, exact assertions), then GREEN, then refactor. Tests are never trimmed.
- Gate = `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `ENGRAM_PORT=8123 cargo test`. Every commit passes fmt and clippy.
- One work-unit commit per task group (conventional commits, no AI attribution).
- Logic lives in new modules; `src/app.rs` and `src/runtime.rs` get dispatch lines only (ai-chat PR15 edits them concurrently): rebase on the tracker before each PR.
- PR1 base = `feat/pane-scroll` (tracker); PR2 base = PR1 branch; PR3 base = PR2 branch. Only the tracker merges to main.
- Do not split below the three design slices; if a PR exceeds ~700 lines, report before opening.
- Archive order: ai-chat, then engram-memory, then pane-scroll (R8).

### Suggested Work Units

| Unit | Goal | PR | Est. | Depends on | Base |
|------|------|----|------|-----------|------|
| S1 | Chat COPY, `lines_*`, `[+N]` badge, COPY hint, scroll position | PR1 | ~600 | ai-chat PR1-PR14 on main | `feat/pane-scroll` |
| S2 | `core::mouse` route/encode, `mouse_facts`, `WheelUp/Down`, `on_wheel`, live reset | PR2 | ~550 | S1 | PR1 branch |
| S3 | Capture lifecycle, config default, root `config_edit`, menu Toggle, runtime wiring, manual checks | PR3 | ~550 | S2 | PR2 branch |

## Reconciliation applied

Orchestrator rulings R1-R8 were applied in place to the spec deltas during this phase.

Files changed:
- `specs/mouse-input/spec.md`: R1 (offset reset on focus/tab switch for the pane left; an unfocused wheel-scrolled pane keeps its offset until wheeled to 0, focused-and-left, or its tab left; the "Persists across focus" scenario replaced by 4 scenarios; chat exception stated), R2 (paste returns to live, scenario added), R3 (overlay list: list, tag editor, note editor, summary draft, guidance), R7 (modifier bits encoded except in Press, Utf8 encoding with drop above 2015, 3 scenarios; "Modifier ignored" sentence narrowed to the step).
- `specs/copy-mode-scrollback/spec.md`: R3 (note editor and summary draft refuse COPY with `nothing to scroll here`).
- `specs/statusline/spec.md`: R5 (scenario: chat COPY always shows a position, `↑0` included).

| Ruling | Result |
|--------|--------|
| R1, R2, R3, R5, R7 | Spec edited (see above) |
| R4 | Spec already matched (mouse-input "Events handled", modal-input "Mouse events by mode"); no edit |
| R5 hint text | Spec already matched (statusline COPY key hint) |
| R6 | Spec matched (`Mouse` row cycles on/off, Esc reverts, root boolean before first table); `ItemKind::Toggle` is design only |
| R8 | Already stated in every delta header; repeated in the rules above |

## PR1 / S1: Chat COPY, badge, statusline (`src/core/ai/chat/{transcript,copy}.rs`, `src/app/{chat_copy,scroll}.rs`, `src/app.rs`, `src/ui/`)

- [x] 1.1 RED: `Scroll::lines_up/lines_down(n,total,h)` tests in `transcript.rs`: 1 and 3 line steps, wrapped lines, clamp at both ends, short transcript unchanged, reaching the end gives `Follow`.
- [x] 1.2 GREEN: implement `lines_up/lines_down` (same contract as `page_*`). Commit.
- [x] 1.3 RED: `src/core/ai/chat/copy.rs` key table to `ChatCopyCommand`: `j k Ctrl+d Ctrl+u gg G Esc q Ctrl+Space`, lone `g` discarded, `v V y i h l 0 $ a Enter Backspace Tab` swallowed, Repeat rules (j/k/Ctrl+d/u act; Esc/q ignored).
- [x] 1.4 GREEN: create `copy.rs` (`ChatCopy { pending_g }`) plus `mod` line. Commit.
- [x] 1.5 RED: app tests for `InputMode::ChatCopy`: `[` on a conversation enters it with no notice and no `WritePty`; `[` on list, tag editor, note editor, summary draft, guidance with no transcript stays TERMINAL with notice `nothing to scroll here`, cleared by the next key (R3); `CHAT_COPY_REFUSED` gone; composer, `ChatSend`, clipboard untouched by swallowed keys.
- [x] 1.6 RED: app tests: Esc/`q` leave to TERMINAL with `Follow`; held exit key debounced (`hold_exit`); stream pinned while in COPY; focus change, tab change and zoom-out of the chat reset COPY (3 reset matches); chat COPY on another pane's COPY does not touch shell offsets; `Ctrl+Space` gives PREFIX.
- [x] 1.7 GREEN: `src/app/chat_copy.rs` (`on_chat_copy_key`), `InputMode::ChatCopy` variant, label `COPY`, `on_key` arm, `[` entry, drop `CHAT_COPY_REFUSED`, reset matches in `src/app.rs`. Commit.
- [x] 1.8 RED: `App::scrolled_by(id)` and `App::scroll_position()` tests: shell offset, chat lines below view, `None` at 0; `copy_position()` in shell COPY; chat COPY always `Some` (explicit 0, R5); focused-only; none on the alternate screen.
- [x] 1.9 GREEN: `src/app/scroll.rs` (`scrolled_by`, `scroll_position`). Commit.
- [x] 1.10 RED: insta snapshots (TestBackend): `[+N]` badge on shell (wheel-scrolled and COPY), chat, unfocused, `[+0]` never, tiny pane (3 cols) and alternate screen show none, no content shift; statusline `COPY` hint for shell `j/k scroll · v select · y copy · Esc exit` and chat `j/k scroll · Esc exit`, VISUAL keeps the shell hint, notice replaces hint, whole-entry clipping with `…`, widths 0-40 do not panic; `TERMINAL ↑12/340`, `CHAT ↑9/120`, `COPY ↑0/N`, zoom marker order, unfocused scrolled pane has no position. Existing snapshots unchanged except COPY ones.
- [x] 1.11 GREEN: `src/ui/components/scroll_badge.rs`; `src/ui/mod.rs` (badge per pane, `scroll_position()`, no composer cursor in `ChatCopy`); COPY hint in `status_path` (`src/app.rs`). Review and accept new snapshots. Commit.
- [x] 1.12 Gate (fmt, clippy, tests) passed; every commit passes fmt+clippy. The PR is NOT opened yet (left to the orchestrator): open it against `feat/pane-scroll`.

### Apply notes (PR1)

- 1.6 / 1.7 (revised after review): `Ctrl+Space` inside chat COPY gives PREFIX, which ends COPY without resetting the view, so a focus or tab change after it leaves the chat pinned (R1). Spec scenarios amended (copy-mode-scrollback "Focus or tab change after Ctrl+Space keeps the view", chat-pane "State survives modes"). The unreachable `ChatCopy` reset arms in `activate`, `set_focus_in` and `drop_tab` were removed; tests drive real keys (ChatCopy, `kkk`, Ctrl+Space, `h` / `gb`). `debug_assert_copy_on_shell` also asserts ChatCopy implies a focused chat. A held `g`/`G` is ignored in `ChatCopy::on_key`.
- 1.5: entering chat COPY is refused for the list, tag editor, note editor, summary draft, and for guidance with an empty transcript; an empty transcript with no guidance enters COPY (nothing scrolls). The old test `copy_on_a_chat_is_refused_with_the_notice` was replaced by the new-notice test.
- 1.8: the chat total of `scroll_position()` is the scrollable lines (`lines - viewport rows`), the analogue of the shell's history rows; `COPY ↑0/0` on a short transcript.
- 1.10 / 1.11: the existing test `the_highlight_lands_on_the_selected_rows_of_a_scrolled_viewport` now strips the `[+8]` badge from row 0 (the badge is the new overlay in COPY scrolled). No existing snapshot changed. Badge style: bold `bg_base` text on `warning_orange`. `prefix::clip_hint` was extracted from `prefix::hint` to clip the COPY hint with the same whole-entry rule.
- Test plumbing: helpers of `chat_live.rs` and `with_draft` of `chat_summary.rs` became `pub(super)` so `chat_copy.rs` and `scroll_position.rs` test modules reuse them.

## PR2 / S2: Wheel routing and app handling (`src/core/{mouse,pane,copy}.rs`, `src/app/scroll.rs`, `src/app.rs`)

- [ ] 2.1 RED: `route_wheel` table tests in `src/core/mouse.rs` (stubs compile): pane kind x mouse mode x alt screen x copy x app cursor. Order: copy gives `CopyScroll`; mode != None gives `Write(encode_wheel)`; alt screen gives 3x `encode_key(Up|Down)` honoring DECCKM (`\x1b[A` vs `\x1bOA`); else `Scrollback`; chat gives `ChatLines` in COPY or not.
- [ ] 2.2 RED: `encode_wheel` golden bytes: SGR up/down at pane-local coords (origin 10,5 pointer 14,8 gives `\x1b[<64;5;4M`), Default `\x1b[M` + `\x60 \x21 \x21` / `a # \x22`, drop above 223, Utf8 +32 as char and drop above 2015, modifiers Shift 4 / Alt 8 / Ctrl 16 (`\x1b[<80;1;1M`), none in Press (R7), all four modes receive the wheel, one sequence per event.
- [ ] 2.3 GREEN: create `src/core/mouse.rs` (`WHEEL_STEP`, `CAPTURE_ON/OFF`, enums, `PaneFacts`, `route_wheel`, `encode_wheel`) plus `mod` line. Commit.
- [ ] 2.4 RED: `Pane::mouse_facts()` tests in `src/core/pane.rs` mirroring vt100 mode/encoding/alt screen for each enum value.
- [ ] 2.5 GREEN: implement `mouse_facts()`. Commit.
- [ ] 2.6 RED: `CopyMotion::WheelUp/WheelDown` tests in `src/core/copy.rs`: offset +/-3, cursor clamped into the viewport, selection preserved and extended like `j/k`, clamp at top and bottom, alternate screen no panic.
- [ ] 2.7 GREEN: add the motions. Commit.
- [ ] 2.8 RED: `on_wheel` app tests: hit-test via `tiling()` rects (pane under pointer, unfocused pane, separator, statusline, tab bar, beyond body ignored, zoomed pane is the only target, after split/resize); no focus/tab/mode change; non-wheel and horizontal events ignored (R4: ignored in PREFIX, groups, RESIZE, MENU, confirmations, no state change and notice kept); no wheel when `config.mouse` is false.
- [ ] 2.9 RED: `on_wheel` behaviour: SGR write to the right pane's PTY only; mouse mode wins over scrollback; less/alt-screen arrows; main-screen offset +/-3 clamped with no bytes and no COPY; anchored under new output (offset grows); wheel over another pane while COPY is elsewhere leaves COPY untouched; wheel over the COPY pane moves the viewport; chat transcript 3 lines, unfocused chat, short transcript unchanged, wheel-down to bottom gives `Follow`.
- [ ] 2.10 RED: chat overlays (list, tag editor, note editor, summary draft, guidance no transcript) ignore the wheel (R3).
- [ ] 2.11 RED: live reset: an encoded key and a paste on the focused scrolled shell set offset 0 (R2); swallowed keys (prefix then Esc) do not; focus change and tab switch reset the shell pane left (R1); an unfocused wheel-scrolled pane keeps its offset while focus moves between other panes, resets after being focused-and-left or when its tab is left; chat scroll survives focus and tab (R1).
- [ ] 2.12 GREEN: `src/app/scroll.rs` (`on_wheel`, live-reset helper); `src/app.rs` (`AppEvent::Mouse` arm, reset calls in `on_terminal_key` and `on_paste`, focus/tab reset keeps current behavior). Commit.
- [ ] 2.13 Gate. Open PR2 against the PR1 branch.

## PR3 / S3: Capture, config, menu, persistence (`src/runtime.rs`, `src/app.rs`, `src/core/{config,config_edit,menu}.rs`)

- [ ] 3.1 RED: config tests: empty file gives `mouse` true; `mouse = false` parses with no notice; `mouse = "yes"` gives true and `config: mouse: expected true or false`; 3 invalid keys give `config (3 problems): ...`; text `mouse is not supported yet` appears nowhere.
- [ ] 3.2 GREEN: `src/core/config.rs`: manual `Default` (`mouse: true`), drop the notice, root `is_flag` handling. Commit.
- [ ] 3.3 RED: `config_edit` root-table tests: new `mouse` inserted before the first table header, appended when no table, empty file gives `mouse = false\n`, existing line updated in place keeping trailing comment, non-boolean replaced by boolean, unchanged draft writes nothing, toggled back writes nothing, `mouse = true` written explicitly when changed to default, no other keys touched, round trip with `Config::parse`.
- [ ] 3.4 GREEN: `src/core/config_edit.rs`: `table == ""` targets the root table. Commit.
- [ ] 3.5 RED: menu tests: `ItemKind::Toggle` `Mouse` (key `mouse`, values `on`/`off`) third Settings row; descriptor integrity; cursor skips no row and wraps (`Tab bar position` -> `Mouse` -> `Set all tasks`); row reflects config; `edits` emit `mouse` only when the draft differs from open; insta snapshot of the Settings rows (R6).
- [ ] 3.6 GREEN: `src/core/menu.rs`: Toggle kind, Mouse row, `edits`; remove the disabled row and cursor skip. Commit.
- [ ] 3.7 RED: app tests: `Effect::MouseCapture(bool)` emitted by `sync_mouse()` only when the effective value changes; cycling the Mouse row emits it live with no `WritePty` and no `ResizePty`; Esc reverts capture and does not write the file; save keeps the previewed state with no extra sequence; failed save keeps the preview; `startup_effects` emits it by default and never when `mouse = false`.
- [ ] 3.8 GREEN: `src/app.rs`: `Effect::MouseCapture`, `mouse_captured`, `sync_mouse` called from `apply_config` and `startup_effects`. Commit.
- [ ] 3.9 RED: runtime byte tests: `to_app_event` forwards only `ScrollUp/ScrollDown` as `AppEvent::Mouse` (clicks, drags, motion, horizontal dropped); `MouseCapture` writes exactly `\x1b[?1000h\x1b[?1006h` / `\x1b[?1006l\x1b[?1000l` to `host_out`, no `?1002`/`?1003`/`?1015`; `restore_to` always contains `?1006l`/`?1000l` before child teardown; panic-hook path covered through `restore_to`; capture-off run never writes mouse bytes.
- [ ] 3.10 GREEN: `src/runtime.rs`: `to_app_event` wheel mapping, `Effect::MouseCapture` into `host_out`, `CAPTURE_OFF` in `restore_to`, startup capture after raw mode/alt screen and before the first frame. Commit.
- [ ] 3.11 Refactor: dedupe helpers, update doc comments and README mouse note (Shift-drag selects natively; `mouse = false` disables). Commit.
- [ ] 3.12 Gate. Open PR3 against the PR2 branch.
- [ ] 3.13 [manual] Kitty: wheel in a shell pane (scrollback, `[+N]` badge, key returns to live) and in less/man (3 lines per notch), vim (`:set mouse=a`) and htop.
- [ ] 3.14 [manual] Kitty: wheel over a chat answer; COPY on the chat with `Ctrl+Space [`, j/k, Ctrl+u/d, gg, G, Esc; stream continues while scrolled.
- [ ] 3.15 [manual] Kitty: Shift-drag native selection works; the terminal is clean (no escape junk, own scrollback works) after quit, after `exit` in the last pane, and after a forced panic if a debug path exists (otherwise record as not testable).
- [ ] 3.16 [manual] Kitty: menu `Mouse` toggle on/off previews live, Esc reverts, `s` persists `mouse = false` before the first table; the new run honors it.
- [ ] 3.17 After all three PRs merge into the tracker, run the final gate on `feat/pane-scroll` and open the tracker PR to main.
