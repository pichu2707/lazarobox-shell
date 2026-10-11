# Tabs Specification

## Purpose

Tab lifecycle, navigation, tab bar and the last-tab-quits rule. Each tab owns an independent pane layout. Tags: [auto] = `cargo test`; [manual] = real Kitty.

## Requirements

### Requirement: Tab lifecycle
The app MUST start with exactly one tab. `t n` MUST create a new tab holding one pane, switch to it, and spawn its shell in the focused pane's cwd. A new tab MUST be appended at the end of the tab list, so existing tab numbers never shift. `t c` MUST ask for confirmation ("Close tab? (y/n)", or "Quit? (y/n)" when it is the only tab) and, only on lowercase `y`, close the active tab and all its panes. After closing tab number i, the tab that takes index i MUST become active, or the last tab when i was the last. Closing an inactive tab (for example its last shell exits) MUST keep the active tab.

#### Scenario: New tab [auto]
- GIVEN one tab
- WHEN `t n` is pressed
- THEN two tabs exist, the new one is active, and a SpawnPane effect carries the focused pane's cwd

#### Scenario: Close tab confirmed [auto]
- GIVEN two tabs and the second active
- WHEN `t c` then `y` are pressed
- THEN the second tab and its panes are closed and the first is active

#### Scenario: Next tab becomes active [auto]
- GIVEN three tabs with the second active
- WHEN the second tab is closed
- THEN the tab that was third is active (now second)

#### Scenario: New tab is appended [auto]
- GIVEN three tabs with the first active
- WHEN `t n` is pressed
- THEN the new tab is the fourth and `b 1`..`b 3` still select the original tabs

#### Scenario: Close tab declined [auto]
- GIVEN two tabs
- WHEN `t c` then `n` (or Esc, or `Y`) are pressed
- THEN both tabs remain and mode is TERMINAL

#### Scenario: Close the only tab asks Quit [auto]
- GIVEN one tab
- WHEN `t c` is pressed
- THEN the prompt is "Quit? (y/n)"

#### Scenario: Close the only tab quits [auto]
- GIVEN one tab
- WHEN `t c` then `y` are pressed
- THEN a Quit effect is emitted

### Requirement: Tab navigation
`g b` MUST activate the next tab and `g B` the previous tab, both wrapping around at the ends. `b 1`..`b 9` MUST activate tab N (1-based). Navigating to a tab that does not exist MUST be a no-op. With one tab, `g b` and `g B` MUST be no-ops.

#### Scenario: Next wraps [auto]
- GIVEN three tabs with the third active
- WHEN `g b` is pressed
- THEN the first tab is active

#### Scenario: Previous wraps [auto]
- GIVEN three tabs with the first active
- WHEN `g B` is pressed
- THEN the third tab is active

#### Scenario: Go to tab N [auto]
- GIVEN three tabs
- WHEN `b 2` is pressed
- THEN the second tab is active

#### Scenario: Missing tab is a no-op [auto]
- GIVEN two tabs
- WHEN `b 5` is pressed
- THEN the active tab is unchanged and mode is TERMINAL

#### Scenario: Single tab navigation [auto]
- GIVEN one tab
- WHEN `g b` is pressed
- THEN nothing changes

### Requirement: Tab state is independent
Each tab MUST keep its own layout, focus and zoom state. Switching tabs MUST preserve them. Panes of inactive tabs MUST keep running and have their output parsed. Switching tabs MUST leave COPY and RESIZE (see modal-input).

#### Scenario: State preserved [auto]
- GIVEN tab 1 with two panes and the right focused, and tab 2 created
- WHEN the user returns to tab 1
- THEN the layout is unchanged and the right pane is still focused

#### Scenario: Background output [auto]
- GIVEN tab 2 active and tab 1's shell prints output
- WHEN the user returns to tab 1
- THEN the output is present on its screen

### Requirement: Tab bar
The tab bar MUST be drawn at the configured edge (default top; see the configuration spec) and MUST appear only when there is more than one tab, EXCEPT while the config menu is open: then it MUST be shown even with one tab (a single tab label), so the user sees the effect of the tab bar position. When the menu closes (Enter or Esc) the bar returns to the normal rule, so with one tab it is hidden again. With the default positions the body (pane area) MUST start at row 0 without the bar or row 1 with it; in general it sits between the bars, and its height MUST be rows - 1 (statusline) - 1 (bar, when visible), at least 1; while the menu is open with one tab the body is `rows - 1 (statusline) - 1 (bar)`, at least 1. The active tab MUST be visually distinct. With one tab and the menu closed, the layout MUST match the pre-tabs layout (no reserved row). Each tab label MUST be `N cwd-basename`, where N is the 1-based tab number and cwd-basename is the basename of the tab's focused pane cwd (`/` for the root; just `N` when the cwd is unknown). Labels that do not fit MUST be clipped, and the active tab MUST stay visible.
(Previously: the bar appeared only with more than one tab, with no exception)

#### Scenario: Hidden with one tab [auto, TestBackend + insta]
- GIVEN one tab and the menu closed
- WHEN rendered
- THEN no tab bar row is drawn and the pane area starts at the top row (default top bar position)

#### Scenario: Shown with two tabs [auto, TestBackend + insta]
- GIVEN two tabs with the second active
- WHEN rendered
- THEN the top row (default position) shows both tabs with the second marked active

#### Scenario: Tab label [auto]
- GIVEN two tabs whose focused panes have cwd `/home/u/proj` and `/tmp`
- WHEN the tab labels are computed
- THEN they are `1 proj` and `2 tmp`

#### Scenario: Label without cwd [auto]
- GIVEN a tab whose focused pane has no known cwd
- WHEN its label is computed
- THEN the label is its number only

#### Scenario: Body math [auto]
- GIVEN a 80x25 terminal with default bar positions
- WHEN the menu is closed and there is one tab, then two tabs
- THEN the body is 80x24 at row 0, then 80x23 at row 1

#### Scenario: Body math with configured positions [auto]
- GIVEN a 80x25 terminal and two tabs
- WHEN the bars are placed (the tab bar is the outermost row of its edge)
- THEN statusline bottom + tabbar top (default): bar row 0, body from row 1, status row 24; both top: bar row 0, status row 1, body from row 2; both bottom: body from row 0, status row 23, bar row 24; statusline top + tabbar bottom: status row 0, body from row 1, bar row 24. The body is 23 rows in all four

(Position comes from `[tabbar] position`, default `"top"`; see the configuration spec.)

#### Scenario: Bar disappears on close [auto]
- GIVEN two tabs
- WHEN one is closed
- THEN the tab bar is hidden and panes are resized to the larger area

#### Scenario: Narrow width [auto]
- GIVEN many tabs on a very narrow terminal
- WHEN rendered
- THEN rendering does not panic, labels are clipped, and the active tab remains visible

#### Scenario: Bar forced with one tab [auto, TestBackend + insta]
- GIVEN one tab and the menu open
- WHEN rendered
- THEN the tab bar is drawn at its configured edge with one label (the active tab) and the body is one row shorter

#### Scenario: Bar hidden on close [auto]
- GIVEN one tab and the menu open
- WHEN the menu closes by Enter (saved) or by Esc
- THEN the tab bar is not drawn, the body returns to the pre-menu size, and ResizePty is emitted only for panes whose size changed

#### Scenario: Bar with several tabs unaffected [auto]
- GIVEN two tabs and the menu opened and closed with no changes
- WHEN the menu opens and closes
- THEN the bar is shown throughout and no ResizePty is emitted

#### Scenario: Tiny terminal with forced bar [auto]
- GIVEN any terminal size down to 0x0 and the menu open with one tab
- WHEN the screen is computed
- THEN nothing panics and the body is at least 1x1

### Requirement: Last tab quits
When the last pane of the last tab closes (shell exit or confirmed close), the app MUST quit.

#### Scenario: Last shell exits [auto]
- GIVEN one tab with one pane
- WHEN that shell exits
- THEN a Quit effect is emitted

#### Scenario: Last pane of a tab with siblings tabs [auto]
- GIVEN two tabs, the active one with a single pane
- WHEN that shell exits
- THEN the tab closes, the other tab becomes active and no Quit is emitted

### Requirement: Visible tabs milestone
#### Scenario: Tabs and tab bar [manual, Kitty]
- GIVEN the app in Kitty with one tab (no tab bar)
- WHEN `t n` is pressed, then `g b`, `g B` and `b 1` are used, then `t c` and `y`
- THEN the bar appears with the second tab, navigation works and wraps, and the bar vanishes when one tab remains

#### Scenario: New tab from a vanished cwd [auto]
- GIVEN the focused pane's cwd no longer exists
- WHEN `t n` is pressed
- THEN the new tab spawns in the launch cwd, preferred over the `$HOME` that the PTY layer would otherwise pick (the app falls back to the launch cwd, then an absolute `$HOME`, then `/`)

#### Scenario: New tab inherits cwd [manual, Kitty]
- GIVEN a shell that ran `cd /tmp`
- WHEN `t n` is pressed
- THEN the new shell starts in `/tmp`
