# Proposal: Links in Panes and the Chat

## Intent

Links printed in a shell pane or written by a model in the chat cannot be opened. With mouse capture on (the default), the host terminal's own link handling no longer gets plain clicks, and lazarobox handles no click outside the menu. The user copies the link by hand: a modifier-drag selection, or COPY mode. This change lets the user open a visible `https://` or `http://` link with one click or with a few keys, and keeps the wheel as it is. Refs #597.

**For:** anyone reading a URL in a pane or in the chat, such as a PR link from `git push`, a dev server address, or a link in a model's answer, at the moment they want it in the browser.

## Scope

### In Scope
- **Detection:** a URL is found in the visible text of a pane: the shell grid, or the chat transcript. Soft-wrapped rows are joined, so a URL that wraps is one link.
- **Mouse:** a left click on a detected URL opens it, in shell panes and in the chat, while `mouse` is on.
- **Keyboard:** a PREFIX action lists the visible links of the focused pane and opens the chosen one.
- **Opening:** the existing opener (`wslview`, `rundll32.exe`, `xdg-open`, `open`). When no browser opens, the link is copied instead and a notice says so, as the menu does today.
- **Only web links:** `https://` and `http://`. `file://`, `javascript:` and every other scheme are never opened.
- **Views:** the link list works at 80 columns and in `TERM=linux`.

### Out of Scope
- **Mouse text selection** (drag to select without a modifier). This is a later improvement with no priority (S2).
- OSC 8 hyperlinks whose target differs from the text shown
- Links split by a hard newline (for example a tool that breaks its own output)
- Opening links from scrollback that is not on screen without scrolling to it first
- Clicks forwarded to pane applications in mouse mode (`vim`, `less`); today they get none
- Hover underlining or cursor changes (no motion events are captured)
- Paths and `file:line` references

**Known limitation:** a URL that is not on screen must be scrolled into view (wheel or COPY) before it can be clicked or listed.

## Capabilities

### New Capabilities
- `pane-links`: URL detection over visible pane and chat text, the click action, the link list, the scheme rule, and the copy fallback.

### Modified Capabilities
- `mouse-input`: "Events handled" today ignores every left press outside MENU; a click on a link becomes handled, on the release at the cell of the press.
- `modal-input`: a new PREFIX key for the link list.
- `chat-pane`: chat lines become clickable where they hold a URL.

## Approach

One pure detector takes rows of text with their soft-wrap flags and returns link spans (row, start column, end column, URL). Shell panes feed it from the `vt100` grid through the same row reading COPY uses. The chat feeds it from the transcript `Line`s, after their drawing prefix. The click handler reuses the wheel's pane hit-test, maps the cell to a span, and emits the existing `Effect::OpenUrl`. The failure path generalizes `on_open_failed` so it works without the menu open. About three chained PRs: the detector and opener rule, the click, the keyboard list.

## Affected Areas

| Area | Impact |
|------|--------|
| `src/core/` (new `links.rs`) | Pure URL detector, scheme rule |
| `src/core/opener.rs` | Scheme rule for pane links (`https` and `http`); the menu's `https`-only rule stays |
| `src/app/scroll.rs` | `on_click` handles a click on a pane link |
| `src/runtime.rs` | `to_app_event` also passes the left release |
| `src/app.rs` | `on_open_failed` without the menu: copy and a notice |
| `src/core/pane.rs` | Read visible rows with their wrap flags for the detector |
| `src/core/ai/chat/transcript.rs` | Visible lines and their `lead` for the detector |
| `src/core/prefix.rs`, new link list view | PREFIX key and the list |
| `README.md`, `openspec/specs/mouse-input` | Docs and spec |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| A click opens a link the user did not mean to open | Med | Only on a detected URL; press and release on the same cell; the notice names what opened |
| Pane output (or a model) shows a link to a harmful site | Med | Only a human click or key opens; `http(s)` only; the URL opened is exactly the text shown, with no hidden OSC 8 target |
| Trailing punctuation or Markdown taken as part of the URL | High | Strip trailing `.,;:!?'"` and unbalanced `)` `]` `>`; tests with Markdown links and parentheses |
| A wrapped URL joined with the next unrelated row | Low | Join only rows marked soft-wrapped |
| No browser over SSH or on a bare host | Med | Copy fallback (OSC 52 and a local tool), as the menu does |
| Hit-test and drawing disagree after a resize or zoom | Low | Same last-frame layout as the wheel |

## Rollback Plan

A plain revert. No stored data or config format changes.

## Success Criteria

- [ ] A click on a URL in a shell pane, with `mouse` on, opens it in the browser on Linux, WSL and macOS.
- [ ] A click on a URL in a model's answer, including a Markdown link `[text](https://…)`, opens only the URL.
- [ ] A URL that wraps across rows opens whole.
- [ ] From the keyboard alone, the user opens any visible link of the focused pane.
- [ ] With no browser, the link is copied and a notice says so.
- [ ] A `file://` or `javascript:` text is never opened.
- [ ] The wheel, focus and every other mouse behavior stay as today.

## Evidence

Verified on `origin/main` f4900e12 (v1.10.0). The behaviors below were checked again on 6eaeb565 (v1.15.2) and still hold; line numbers have moved since.

- **Capture is wheel-only today:** `CAPTURE_ON` enables button reporting with SGR, no motion (`src/core/mouse.rs:11-12`); `mouse` defaults to on and `mouse = false` gives the wheel and selection back to the terminal (`README.md:364`, `README.md:368`).
- **Native selection needs a modifier:** Shift, or Option on macOS (`README.md:365`).
- **Only the left press and the vertical wheel reach the app;** releases, drags and motion are dropped (`src/runtime.rs:2320-2333`).
- **A left press outside MENU does nothing:** `on_click` returns early unless the input mode is MENU (`src/app/scroll.rs:145-159`); the spec says no left press outside MENU is forwarded to any PTY (`openspec/specs/mouse-input/spec.md:59-60`). So a plain click in a pane is free today: it neither focuses a pane nor reaches `vim`.
- **Pane hit-test exists:** the wheel finds the pane under the pointer from the last frame's tiling (`src/app/scroll.rs:90-98`).
- **No OSC 8 in shell panes:** `vt100` 0.16.2 cells carry text and attributes only (`vt100-0.16.2/src/cell.rs:12-176`); the app's `unhandled_osc` keeps only OSC 7 (`src/core/pane.rs:122-128`). OSC 8 is stripped from model output (`src/core/ai/sanitize/tests.rs:35`). Only plain-text URLs are visible.
- **Wrap flags exist:** a shell row reports `row_wrapped` (`src/core/pane.rs:414-431`); a chat `Line` has `text`, the drawing prefix width `lead`, and `wrap` (`src/core/ai/chat/transcript.rs:44-54`). The chat is wrapped plain text, not rendered Markdown, so `[text](url)` appears literally.
- **The opener exists:** `Effect::OpenUrl` (`src/app.rs:348-350`); the runtime runs the first candidate opener and reports a failure within 5 s as `AppEvent::OpenFailed` (`src/runtime/opener.rs:18`, `src/runtime.rs:607-620`); candidates per host (`src/core/opener.rs:8-14`); `is_web_url` accepts only one-line `https://` links (`src/core/opener.rs:27-33`).
- **Failure copies the link, but only in the menu:** `on_open_failed` copies when a menu is open, and otherwise only sets `The browser did not open` (`src/app.rs:2442-2463`).
- **Keys:** PREFIX top level binds `[ m c n q h j k o s e T W r ?` among others, and `u` is unbound (`src/core/prefix.rs:228-264`). COPY binds `g v V h l 0 $ j k G y` and Ctrl+D/U (`src/core/copy.rs:229-249`); chat COPY adds Ctrl+Y and Ctrl+B (`src/core/ai/chat/copy.rs:129-130`).

## Decisions

Reviewed by the maintainer on 2026-11-09; each one is the option the review recommended.

- **D1. Click.** A plain left click, opened on the release at the same cell as the press. A plain click is free today, and Ctrl+click is a right-click on macOS Terminal. Opening on release keeps a later drag selection (S2) possible. A future change that forwards clicks to `vim` with `mouse=a` has to decide which wins on a link.
- **D2. Detection.** Plain-text `https://` and `http://` up to whitespace, with trailing punctuation and unbalanced closing brackets removed, over soft-wrapped rows joined. No OSC 8: `vt100` has no per-cell link storage, and the user always sees the exact target.
- **D3. Keyboard.** `Ctrl+Space`, then `u`, opens a small list of the focused pane's visible links, newest last, each with a key (`1`…`9`, then letters). The key opens it, `y` on the selected row copies it, and `Esc` closes.
- **D4. Where.** Shell panes and the chat in the same change, through one detector. The chat slice can land first, since its lines are already plain text.
- **D5. No browser.** The same as the menu: copy the link (OSC 52 and a local tool) and show `The browser did not open: link copied`. With no opener at all, copy at once. `on_open_failed` must work outside the menu.
- **D6. Which links.** `https://` and `http://` only, one line, no whitespace or control characters, and no confirmation: only a human click or key opens a link, and the target is exactly the text shown. `http://` is needed for local dev servers. The menu keeps its `https`-only rule.
- **D7. Config.** No new key. The click follows `mouse`, and the keyboard list is always there.
