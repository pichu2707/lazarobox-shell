# Delta for Tabs (config-menu)

All scenarios [auto] unless tagged. Builds on the panes-tabs tabs spec.

## MODIFIED Requirements

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
