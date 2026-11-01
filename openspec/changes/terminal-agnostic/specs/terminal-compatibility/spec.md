# terminal-compatibility Specification

## Purpose
Make every terminal dependency either work or say clearly that it did not. Support is defined by per-terminal records, not by a tested-in-one-terminal claim.

## ADDED Requirements

### Requirement: First-run prefix probe
On the first run in a terminal with no remembered probe result, the app MUST ask the user to press `Ctrl+Space`. If `Ctrl+Space` arrives within the probe window, the app MUST remember a pass and show no offer. If `Ctrl+]` arrives, the app MUST remember `alias` and show no offer. If any other key arrives, or nothing arrives within the window, the app MUST show a notice offering `Ctrl+]` as the prefix and MUST remember the result. The window MUST be a few seconds (the exact length is left to design). The probe MUST NOT block the app: the user can leave it with any key at any time, and the notice is advice only. The probe MUST NOT use environment heuristics (`WT_SESSION`, WSL, `TERM_PROGRAM`).

#### Scenario: Ctrl+Space arrives [auto]
- GIVEN a first run with no remembered result
- WHEN the user presses `Ctrl+Space` within the window
- THEN the result is remembered as pass, no offer is shown, and PREFIX behaves as in modal-input

#### Scenario: Nothing arrives [auto]
- GIVEN a first run with no remembered result
- WHEN no key arrives within the window
- THEN a notice offers `Ctrl+]` as the prefix and the result is remembered

#### Scenario: Another key arrives [auto]
- GIVEN a first run with no remembered result
- WHEN a key other than `Ctrl+Space` arrives within the window
- THEN a notice offers `Ctrl+]` as the prefix and the result is remembered

#### Scenario: Probe never traps the user [auto]
- GIVEN the probe is waiting
- WHEN the user presses `Ctrl+]` or any other key
- THEN the app reacts to that key normally (`Ctrl+]` enters PREFIX) and the probe ends
- AND `Ctrl+]` records `alias` and shows no offer (the user already found the alias); any other key records `missed` and shows the offer

#### Scenario: Probe in a real terminal [manual, terminal records]
- GIVEN a terminal where `Ctrl+Space` is swallowed
- WHEN lazarobox starts for the first time
- THEN the notice offering `Ctrl+]` appears and the result is written in the terminal's record

### Requirement: Remembered probe result
The probe result MUST be remembered so that the probe does not run again on later starts in the same terminal. The remembered result MUST NOT be stored in `config.toml`. A remembered result MUST NOT hide the alias: `Ctrl+]` works in every run whatever the result. The user MUST have a way to run the probe again; when they do, the app MUST show a notice that the probe is running. If the stored result cannot be read or written, the app MUST show a notice and MUST continue to start (it MUST NOT fail or ignore this silently).

#### Scenario: Probe does not repeat [auto]
- GIVEN a remembered probe result for this terminal
- WHEN lazarobox starts again
- THEN no probe is shown

#### Scenario: Another terminal gets its own probe [auto]
- GIVEN a remembered result made in terminal A
- WHEN lazarobox starts in a different terminal B with no result
- THEN the probe runs

#### Scenario: Rerun on request [auto]
- GIVEN a remembered result
- WHEN the user asks to run the probe again
- THEN a notice says the probe is running and the probe starts

#### Scenario: Storage unavailable [auto]
- GIVEN the place where the result is kept cannot be read or written
- WHEN lazarobox starts
- THEN a notice says the result could not be kept, and the app starts normally

#### Scenario: Alias always works [auto]
- GIVEN a remembered pass result
- WHEN `Ctrl+]` is pressed in TERMINAL
- THEN PREFIX is entered

### Requirement: Support policy by records
A terminal MUST be listed as supported in the README only if it has a record at `docs/terminals/<terminal>/<version>-<date>.md`. Terminals without a record MUST be described as "untested" and MUST NOT be blocked. This change MUST include records for Kitty, WezTerm and Windows Terminal (WSL). The README MUST NOT say "tested in Kitty" or "Windows: not supported"; it MUST describe WSL as a supported path when its record exists. Any in-app notice that names the terminal state MUST say whether the terminal has a record, and MUST NOT say "tested in Kitty".

#### Scenario: Recorded terminal is listed [manual]
- GIVEN a record exists for Kitty, WezTerm and Windows Terminal (WSL)
- WHEN the README is read
- THEN these three are listed as supported and every other terminal is listed as untested

#### Scenario: Untested terminal is not blocked [auto]
- GIVEN a terminal with no record
- WHEN lazarobox starts
- THEN it runs normally and any terminal-state notice says the terminal has no record

#### Scenario: No Kitty claim [manual]
- GIVEN the README and the app notices
- WHEN searched for "tested in Kitty" and "Windows: not supported"
- THEN neither phrase appears

### Requirement: Shared checklist and record format
`docs/terminals/` MUST hold one shared checklist and, per terminal, records named `<terminal>/<version>-<date>.md`. The checklist MUST cover: prefix (`Ctrl+Space`), alias (`Ctrl+]`), `Alt+Enter`, `Ctrl+J`, `Ctrl+O`, `Ctrl+S`, `Ctrl+Y`, `Ctrl+B`, `Ctrl+R`, `Ctrl+G`, `Ctrl+D`, `Ctrl+U`, `Ctrl+C`, `Alt+Up/Down` when present, mouse, paste, focus, truecolor and clipboard. Each record MUST state, for every checklist item, pass, fail or not applicable, and the terminal, its version and the date.

#### Scenario: Record is complete [manual]
- GIVEN a record file
- WHEN it is compared with the checklist
- THEN every checklist item has a result and the terminal, version and date are present

#### Scenario: Manual tags point to records [manual]
- GIVEN a manual scenario that depends on a real terminal
- WHEN the scenario is run
- THEN its result is written in the matching terminal record
