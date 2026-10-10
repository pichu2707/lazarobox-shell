# Delta for Tabs (config-menu)

All scenarios [auto] unless tagged. Builds on the panes-tabs tabs spec.

## MODIFIED Requirements

### Requirement: Tab bar
The tab bar MUST be drawn at the configured edge and MUST appear only when there is more than one tab, EXCEPT while the config menu is open: then it MUST be shown even with one tab (a single tab label), so the user sees the effect of the tab bar position. When the menu closes (Enter or Esc) the bar returns to the normal rule, so with one tab it is hidden again. Body height rules, label rules, clipping and the active-tab distinction are unchanged; while the menu is open with one tab the body is `rows - 1 (statusline) - 1 (bar)`, at least 1.
(Previously: the bar appeared only with more than one tab, with no exception)

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
