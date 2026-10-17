# Delta for Chat Pane

Applies on top of the unarchived `agent-tools` change delta for chat-pane. Only relevant when tools are on; with tools off the pane renders exactly as before. Tags as in the main spec.

## ADDED Requirements

### Requirement: Approval mode badge and system lines
A chat pane with tools on in `auto` mode MUST show the badge ` AUTO ` in amber, and in `run` mode the badge ` RUN ` in red, both high contrast, in every chat view, focused or not; `ask` shows none. When the pane is narrower than the badge (below 6 columns for ` AUTO `, below 5 for ` RUN `) the badge MUST shrink to `A` or `R`; the badge text is bold on the base background colour. The badge is system-drawn: never stored, yanked or sent to the model. Changing the mode (see modal-input) MUST add a system line naming the new mode and saying what it does: `ask` asks for each command; `auto` types commands without Enter; `run` types and runs a short list of navigation commands (`cd`, `pwd`, `ls`, `tree`, `mkdir`) and sends their output to the model; the `run` line reads `approval: run (cd, pwd, ls, tree, mkdir run at once and their output goes to the model; other commands ask)`. System lines are never stored or sent to the model. After a `cd` ran under pane-execution the transcript MUST show the system line `pane N: <exact command> → <cwd>`.
(Previously: the `auto` marker was specified only inside agent-tools, in one unspecified high-contrast style, and there was no run mode)

#### Scenario: Badges [auto, TestBackend + insta]
- GIVEN chats in `auto`, `run` and `ask` with tools on
- WHEN rendered
- THEN ` AUTO ` is amber, ` RUN ` is red and `ask` has no badge

#### Scenario: Narrow pane [auto, TestBackend + insta]
- GIVEN a chat in `run` rendered at 4 columns, and one in `auto` at 5 columns
- WHEN rendered
- THEN the badges read `R` and `A`, a `run` chat at 5 columns still reads ` RUN `, and nothing panics

#### Scenario: System line on change [auto]
- GIVEN a chat in `auto`
- WHEN the mode changes to `run`
- THEN the system line reads `approval: run (cd, pwd, ls, tree, mkdir run at once and their output goes to the model; other commands ask)`; it is not in the store or the next request

#### Scenario: Cd line shows the exact command [auto]
- GIVEN `cd ~/dev/curso-rust` run and settled
- WHEN rendered
- THEN a system line reads `pane N: cd ~/dev/curso-rust → <cwd>`

#### Scenario: Tools off shows nothing [auto, TestBackend + insta]
- GIVEN tools off and `approval = "run"`
- WHEN rendered
- THEN no badge appears and the snapshot equals the one without this change

### Requirement: Run result rows
Within the Tool rows rules, the result row of a `run_in_pane` call MUST start with the exact command and then read `ran in pane N` (with the cwd or the line count), `ran in pane N — still running`, `ran — output not available`, `typed into pane N — no Enter` or `denied`, with at most a short sanitized preview of the output; the full bounded output goes to the model only and is never stored in the transcript.

#### Scenario: Ran row [auto, TestBackend + insta]
- GIVEN a settled `ls -l` with 30 output lines
- WHEN rendered
- THEN the row starts with `ls -l`, reads `ran in pane N` and shows at most a capped preview, not all 30 lines

#### Scenario: Timed out row [auto, TestBackend + insta]
- GIVEN a run that hit the timeout
- WHEN rendered
- THEN the row starts with the exact command and reads `ran in pane N — still running`
