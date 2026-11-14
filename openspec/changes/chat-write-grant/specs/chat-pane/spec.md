# Delta for chat-pane

Change `chat-write-grant` (the concept is called run auto-apply; the folder keeps its name). Base: `openspec/specs/chat-pane/spec.md` on origin/main (99106097), Requirement `Approval mode badge and system lines`. The requirement is given as its full current text (collapsed), then its full changed text, which is the one that replaces it. Nothing else of the base changes.

## MODIFIED Requirements

What changes: the badge sentence gains ` RUN+A ` (red, while auto-apply is held), the narrow-pane rule gains its width, and one scenario is added at the end. The marker's own rule is in agent-tools, Chat approval mode.

<details>
<summary>Current text on origin/main, <code>openspec/specs/chat-pane/spec.md</code> lines 824-850</summary>

> ### Requirement: Approval mode badge and system lines
> A chat pane with tools on in `auto` mode MUST show the badge ` AUTO ` in amber, and in `run` mode the badge ` RUN ` in red, both high contrast, in every chat view, focused or not; `ask` shows none. When the pane is narrower than the badge (below 6 columns for ` AUTO `, below 5 for ` RUN `) the badge MUST shrink to `A` or `R`; the badge text is bold on the base background colour. The badge is system-drawn: never stored, yanked or sent to the model. Changing the mode (see modal-input) MUST add a system line naming the new mode and saying what it does: `ask` asks for each command; `auto` types commands without Enter; `run` types and runs a short list of navigation commands (`cd`, `pwd`, `ls`, `tree`, `mkdir`) and sends their output to the model; the `run` line reads `approval: run (cd, pwd, ls, tree, mkdir run at once and their output goes to the model; other commands ask)`. System lines are never stored or sent to the model. After a `cd` ran under pane-execution the transcript MUST show the system line `pane N: <exact command> → <cwd>`.
>
> #### Scenario: Badges [auto, TestBackend + insta]
> - GIVEN chats in `auto`, `run` and `ask` with tools on
> - WHEN rendered
> - THEN ` AUTO ` is amber, ` RUN ` is red and `ask` has no badge
>
> #### Scenario: Narrow pane [auto, TestBackend + insta]
> - GIVEN a chat in `run` rendered at 4 columns, and one in `auto` at 5 columns
> - WHEN rendered
> - THEN the badges read `R` and `A`, a `run` chat at 5 columns still reads ` RUN `, and nothing panics
>
> #### Scenario: System line on change [auto]
> - GIVEN a chat in `auto`
> - WHEN the mode changes to `run`
> - THEN the system line reads `approval: run (cd, pwd, ls, tree, mkdir run at once and their output goes to the model; other commands ask)`; it is not in the store or the next request
>
> #### Scenario: Cd line shows the exact command [auto]
> - GIVEN `cd ~/dev/curso-rust` run and settled
> - WHEN rendered
> - THEN a system line reads `pane N: cd ~/dev/curso-rust → <cwd>`
>
> #### Scenario: Tools off shows nothing [auto, TestBackend + insta]
> - GIVEN tools off and `approval = "run"`
> - WHEN rendered
> - THEN no badge appears and the snapshot equals the one without this change

</details>

### Requirement: Approval mode badge and system lines
A chat pane with tools on in `auto` mode MUST show the badge ` AUTO ` in amber, and in `run` mode the badge ` RUN ` in red, or ` RUN+A ` in red while the chat holds auto-apply (see agent-tools, Chat approval mode), all high contrast, in every chat view, focused or not; `ask` shows none. When the pane is narrower than the badge (below 6 columns for ` AUTO `, below 5 for ` RUN `, below 7 for ` RUN+A `) the badge MUST shrink to `A` or `R`; the badge text is bold on the base background colour. The badge is system-drawn: never stored, yanked or sent to the model. Changing the mode (see modal-input) MUST add a system line naming the new mode and saying what it does: `ask` asks for each command; `auto` types commands without Enter; `run` types and runs a short list of navigation commands (`cd`, `pwd`, `ls`, `tree`, `mkdir`) and sends their output to the model; the `run` line reads `approval: run (cd, pwd, ls, tree, mkdir run at once and their output goes to the model; other commands ask)`. System lines are never stored or sent to the model. After a `cd` ran under pane-execution the transcript MUST show the system line `pane N: <exact command> → <cwd>`.

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

#### Scenario: Badge while auto-apply is held [auto, TestBackend + insta]
- GIVEN a chat in `run` with tools on and auto-apply held, and a chat in `run` without it
- WHEN rendered, focused or not
- THEN the first reads ` RUN+A ` in the red high-contrast style, the second ` RUN `, and a pane of 6 columns shows `R` for the first and nothing panics

