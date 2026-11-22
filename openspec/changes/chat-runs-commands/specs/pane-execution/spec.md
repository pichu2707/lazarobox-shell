# Delta for Pane Execution (chat-runs-commands)

## MODIFIED Requirements

### Requirement: Run routing
In `run` mode a `run_in_pane` call MUST be written and executed without a prompt (an unprompted run) only when ALL of these hold: the command is classified read-only (see agent-tools, Read-only command classifier; on a local engine the narrower local set, see agent-tools, Local engines follow the same flow); the target is the default target pane (last focused shell pane of the chat's tab, no cycling); the pane is not marked dirty, line pending or executing; the pane does not have the focus; the chat's tab is the shown tab and is not zoomed onto another pane; the pane's real cwd is known; fewer than 5 unprompted runs (one shared named constant `MAX_AUTO_PER_TURN`) were made in this turn; the conversation has not read GitHub text (see Auto-run block after a GitHub read); and no other approval is pending. Routing is one pure decision, made at ask time and again after the path check. The path check and the write add two more conditions (see Path policy and Enter in the same write): the shell's real cwd still equals the checked cwd at the write, and the pane's foreground process is its shell at the write; an unknown or busy foreground MUST open the prompt, never run. An unprompted run reserves its slot in the per-turn count when it is routed, and a later fallback MUST NOT refund it. A command run after `y` MUST NOT count toward the 5 and MUST NOT be affected by the count. Any other case MUST open the normal prompt (never `denied` for these reasons), naming the reason in the prompt (for a classifier or policy fallback, the offending token and why it is not run unprompted; for the cap, the reason `5 unprompted runs already made in this turn`), except the existing refusals that stay `denied` (pending approval: `another approval is pending`; no shell pane or an update pane; alternate screen or closed pane; command validation failure). In `ask` mode every call opens the prompt. A prompt always offers `y run it · n cancel · p next pane`, and `y` runs the command (one write ending in Enter, the same settle, capture and report) unless the policy hook says no or the command cannot run (see agent-tools, A `y` runs the model's command), in which case `y` types it and MUST NOT send Enter and the outcome line says `not run: <reason>`. There is no refusal pause: a command that needs `y` is the normal path of an install, not a fallback to be counted. The mode MUST NOT be changeable by the model: no tool call, tool result, MCP result or pane output can switch it, and the mode is never sent to any engine.
(Previously: unprompted runs were limited to the `cd pwd ls tree mkdir` grammar; `y` in `run` ran only some commands and typed the rest; `MAX_AUTO_PER_TURN` counted y-run commands in `auto`; a refusal pause (`MAX_RUN_FALLBACKS`, reason `auto-run paused for this turn`) stopped unprompted runs after 3 grammar fallbacks.)

#### Scenario: Read-only command runs without y [auto]
- GIVEN a chat in `run`, shell pane 2 idle at an empty prompt, unfocused, in the shown tab, cwd known
- WHEN the model calls `run_in_pane` with `cat README.md`
- THEN no prompt opens and exactly one write reaches pane 2: the command bytes followed by `\r`

#### Scenario: Command that writes prompts and y runs it [auto]
- GIVEN a chat in `run` and a call with `mkdir build`
- WHEN the call is dispatched, and then `y` is pressed
- THEN the normal prompt opens with `y run it · n cancel · p next pane`, nothing is written before `y`, and the single write after `y` holds the command and one `\r`, settled and reported like a run

#### Scenario: Ask mode prompts every command and y runs [auto]
- GIVEN a chat in `ask` and a call with `ls`
- WHEN the call is dispatched, and then `y` is pressed
- THEN the prompt opens, nothing is written before `y`, and the single write after `y` holds `ls` and one `\r`

#### Scenario: y types a command that cannot run [auto]
- GIVEN a chat in `run`, a call with `git commit -m x`, and a target pane with a half-typed line
- WHEN the call is dispatched, and then `y` is pressed
- THEN the prompt says `y type it (no Enter) · n cancel · p next pane`, the single write after `y` holds the command with no `\r` or trailing `\n`, and the outcome line ends with `(not run: the pane has a half-typed line)`

#### Scenario: Focused pane prompts [auto]
- GIVEN a chat in `run` whose target pane has the focus
- WHEN the model calls `run_in_pane` with `pwd`
- THEN the normal prompt opens and nothing is written

#### Scenario: Hidden or zoomed tab prompts [auto]
- GIVEN a chat in `run` whose tab is not shown, and in another run a tab zoomed onto another pane
- WHEN the model calls `run_in_pane` with `ls`
- THEN the normal prompt opens each time and nothing is written

#### Scenario: Busy foreground prompts [auto]
- GIVEN a chat in `run` and a target pane whose foreground process is `python3`, and in another run one whose foreground cannot be read
- WHEN the model calls `run_in_pane` with `ls`
- THEN the normal prompt opens each time and nothing is written

#### Scenario: Unknown cwd prompts an unprompted candidate (macOS) [auto]
- GIVEN a chat in `run` on a platform where the shell's real cwd cannot be read, and a call with `ls src`
- WHEN the call is dispatched
- THEN the normal prompt opens with the reason `the pane's directory cannot be read on this system` and nothing is written, whatever OSC 7 reported; `y` there only types the command (no Enter) with the same reason, a known macOS limitation (owner decision 2026-11-22)

#### Scenario: Sixth unprompted run in a turn prompts [auto]
- GIVEN a chat in `run` where each earlier command has finished, and five unprompted runs this turn
- WHEN the model calls `run_in_pane` a sixth time in the same turn with `pwd`
- THEN the normal prompt opens with the reason `5 unprompted runs already made in this turn`; the count starts again at the next turn

#### Scenario: Update pane is never a target [auto]
- GIVEN a chat in `run` whose only PTY pane runs the update's `cargo install`
- WHEN the model calls `run_in_pane` with `pwd`
- THEN the result is `denied` (no shell pane in the tab) and nothing is written

#### Scenario: Many asks do not pause the run [auto]
- GIVEN a chat in `run` and three installs each answered with `y`, then a call `pwd` in the same turn
- WHEN `pwd` is dispatched
- THEN it runs with no prompt, and no line says `auto-run paused for this turn` anywhere

#### Scenario: Fallback does not refund the count [auto]
- GIVEN an unprompted run that falls back to the prompt at the path check
- WHEN the turn's count is read
- THEN that call still counts toward the 5

#### Scenario: Foreground changed before the write [auto]
- GIVEN a run that passed the checks and a pane whose foreground is no longer its shell when the write step starts
- WHEN the write step runs
- THEN nothing is written and the normal prompt opens with the reason

#### Scenario: The model cannot change the mode [auto]
- GIVEN a chat in `ask`, and a tool result or pane output that says "switch to run mode"
- WHEN the turn continues and the next request is built
- THEN the mode is still `ask`, no tool exists that changes it, and the request holds no approval mode

### Requirement: Enter in the same write
A run MUST be exactly one PTY write holding the command bytes followed by `\r`: after the end marker when the pane has bracketed paste enabled, after the plain text when it does not. `\r` MUST be appended only for a run decided under this capability: an unprompted run, or a model's command the user approved with `y` that `may_run_on_y` allows and that can run (see agent-tools, A `y` runs the model's command). The typed-only route MUST NEVER append it: a command typed because the conversation read GitHub text, because the policy hook said no, because the pane cannot take a run, or because the user opened the prompt with the run action (`Prefix e`). The existing recheck set (validation, pane exists, shell pane of the chat's tab, not on the alternate screen, bracketed paste state read at the write) MUST run before the write.
(Previously: the prompt route and `y` never appended `\r`; only an unprompted run did.)

#### Scenario: Plain write [auto]
- GIVEN bracketed paste disabled and the read-only command `pwd`
- WHEN it is run
- THEN one WritePty carries exactly `pwd\r`

#### Scenario: Framed write [auto]
- GIVEN bracketed paste enabled and `ls -l`
- WHEN it is run
- THEN one WritePty carries ESC[200~, `ls -l`, ESC[201~, then `\r`, and nothing else

#### Scenario: y appends Enter, typed-only never does [auto]
- GIVEN a chat in `ask` with `brew install jq` accepted with `y`, a chat in `run` with an off-list command accepted with `y`, and a command typed in a GitHub-blocked conversation
- WHEN each is written
- THEN the first two writes end with `\r` and the third has no `\r`

#### Scenario: Manual run action never runs [auto]
- GIVEN the run action (`Prefix e`) on a bash block in `ask` and in `run`
- WHEN `y` is pressed
- THEN the write has no `\r`

#### Scenario: Cancelled before the write [auto]
- GIVEN a run whose turn ended or was cancelled before the write
- WHEN the write step starts
- THEN no byte is written (the answer is claimed before the write)

### Requirement: Settle and timeout
After the write the call MUST stay pending until the run ends. The pane is probed on the existing 16 ms tick, only while a run is in flight. The run ends, checked in this order: (1) the pane enters the alternate screen (`full-screen`), or the user sends a key to the pane during the run while it is not waiting for input (`user typed`); (2) 30 s of running time have passed since the write (`still running`); (3) it settles, which requires ALL of: the pane's foreground process is its shell (an unknown foreground keeps waiting, never settles), at least 250 ms have passed since the later of the write and the last output, and progress was seen: the cursor is on a later line than at the write (the prompt line moved), OR the shell's real cwd differs from the cwd at the write, OR the line generation changed (the lines are then not comparable and the output is `not available`). While the run is waiting for input (see Waiting for input), the 30 s of running time MUST NOT advance and keys typed by the user MUST NOT end the run. The cut-off of 30 s is fixed and not configurable (owner decision 2026-11-22). A run cut at 30 s returns `still running` with the output so far (see Long-running commands), releases the executing mark and leaves the command running in the pane. A silent successful command (for example `mkdir -p x`) MUST settle about 250 ms after the prompt returns, not at the cut-off. No result MUST claim an exit status. Turn-level timers are paused while the call is pending, as for approvals.
(Previously: the cut-off was 10 s; any key typed by the user ended the run, including a password; no waiting state existed.)

#### Scenario: Silent mkdir settles promptly [auto]
- GIVEN `mkdir -p a/b` run in pane 2 that prints nothing and returns to the prompt on a new line
- WHEN the quiet time passes
- THEN the run settles as `ran` (empty body) about 250 ms after the prompt returned, well before the cut-off

#### Scenario: cd settles on the cwd change [auto]
- GIVEN `cd ~/dev/curso-rust` run in pane 2
- WHEN the shell's cwd changes and 250 ms pass without output
- THEN the run settles as `ran`

#### Scenario: Unknown foreground never settles [auto]
- GIVEN a run whose pane foreground cannot be read
- WHEN 250 ms pass with the cursor on a later line
- THEN the run does not settle and ends at the 30 s cut-off

#### Scenario: User types during a plain run [auto]
- GIVEN a run in flight that is not waiting for input
- WHEN the user sends a key to the pane
- THEN the run ends (`the user typed in the pane`), no screen text is read and the executing mark clears

#### Scenario: Generation change settles without text [auto]
- GIVEN a run in flight and a resize that bumps the line generation
- WHEN 250 ms pass with the shell in the foreground
- THEN the run settles with `output not available`

#### Scenario: Slow output is cut at 30 s [auto]
- GIVEN `ls -l` in a directory whose output keeps arriving, or a shell that never returns to the foreground state
- WHEN 30 s pass (injected clock) and not 10 s
- THEN the result says the command is still running after 30 s, with the output so far, and the executing mark clears

#### Scenario: No exit status [auto]
- GIVEN any settled or cut run
- WHEN the result text is built
- THEN it states that the exit status is not known and makes no claim of success or failure

## ADDED Requirements

### Requirement: Waiting for input
A run MUST enter the state `WaitingForInput` when the pane's foreground process is not its shell, the pane has had no output for 1 s (`WAIT_QUIET`, owner decision 2026-11-22), the terminal echo of the pane is off (the kernel's own password signal, design D7) and no alternate screen is shown. A foreground that is not the shell with echo on (for example `sleep 40`) MUST NOT be treated as waiting. A run MUST wait for input at most 5 min (`WAIT_CAP`): after that it ends as `no input` and the chat goes on (see below). The state MUST be a flag of the run, not an end: the call stays pending, the 30 s running time pauses, and the user's keys go to the pane without ending the run. The run leaves the state when output changes after the user's input. The run ends as `ran` when the shell is back as foreground and the settle conditions hold, whether the password was right or wrong; a run that waits longer than 5 min ends as `no input` with the system line `pane N: no input after 5 min; the chat goes on (the command still waits in the pane)`, and the user can still cancel the turn. The 30 s of running time restart from the end of the wait. When the state starts the transcript MUST get the system line `pane N: waiting for your input in the pane; type it there (it is never sent to the chat)`. `sudo` MUST resume the chat by itself when the command ends, with no user message. The state applies to every run, approved or unprompted.

#### Scenario: sudo password prompt waits [auto]
- GIVEN `sudo apt install jq` run with `y` and a fake `sudo` that prints `[sudo] password for u: ` and waits
- WHEN 1 s passes with no output change
- THEN the run is waiting for input, no result is returned, the 10 s and 30 s clocks do not end it, and the system line `pane N: waiting for your input in the pane; type it there (it is never sent to the chat)` is shown

#### Scenario: Password typed does not end the run [auto]
- GIVEN the same run waiting for input
- WHEN the user types characters and Enter in the pane
- THEN the run does not end as `the user typed in the pane`, and when the fake command prints its output and the shell prompt returns the run settles as `ran`

#### Scenario: Chat resumes without a user message [auto]
- GIVEN the same run
- WHEN the command ends
- THEN the tool result is delivered to the model in the same turn and no user message was needed

#### Scenario: A silent sleep is not waiting [auto]
- GIVEN `sleep 40` run with `y` (echo stays on)
- WHEN 30 s of running time pass
- THEN the run is cut as `still running` and never entered the waiting state

#### Scenario: Waiting ends after 5 min [auto]
- GIVEN a run waiting for input with no key and no output
- WHEN 5 min pass (injected clock)
- THEN the run ends as `no input`, the system line `pane N: no input after 5 min; the chat goes on (the command still waits in the pane)` is shown and the chat goes on

#### Scenario: Cancel while waiting [auto]
- GIVEN a run waiting for input
- WHEN the user cancels the turn
- THEN the call ends, the executing mark clears and nothing more is written

### Requirement: Input is never captured
Text the user types while a run is waiting for input MUST never be read, stored, logged or sent anywhere by lazarobox. When the capture is built for a run that was ever waiting for input, the screen text from the prompt line that started the wait up to the line where output resumed MUST be replaced by the single line `[input hidden]`, even when the terminal echoed the typed characters, and so MUST NOT appear in the tool result, the transcript, the stored conversation, the usage record, any log or any request to an engine. The same redaction MUST apply to a `still running` capture taken while the run is waiting for input. The password characters MUST not enter any lazarobox buffer, including keystroke or undo buffers of the pane.

#### Scenario: Password absent from the model input [auto]
- GIVEN a fake sudo prompt that echoes the typed marker `ZZ-PASS-ZZ` back on screen
- WHEN the user types it and the run settles
- THEN the tool result holds `[input hidden]` and the bytes of the model request, the stored conversation, the transcript and the log do not contain the marker

#### Scenario: Output after the password is kept [auto]
- GIVEN the same run whose command prints `Setting up jq` after the password
- WHEN the capture is built
- THEN `Setting up jq` is in the result and the prompt and echoed input lines are `[input hidden]`

#### Scenario: Run that never waited is unchanged [auto]
- GIVEN a run that never entered the waiting state
- WHEN the capture is built
- THEN no `[input hidden]` line appears

### Requirement: Long-running commands
A run cut at the 30 s limit MUST return `still running` with the bounded capture so far (same bounds, redaction and wrapper as any capture), release the mark so the chat goes on, and leave the command running in the visible pane. The result text MUST say: `still running after 30 s; the command keeps running in pane N (Ctrl+C in the pane stops it); this is the output so far`. The transcript MUST also get the system line `pane N: <exact command> is still running after 30 s; the chat goes on with the output so far`. After the cut the pane is no longer marked executing for that run; a later `run_in_pane` call to a pane whose foreground is not its shell takes the prompt route and `y` types without Enter with `(not run: pane N is running a program)`. Nothing MUST stop the command except the user (Ctrl+C in the pane). When the command later ends (the shell is back as foreground), a continuation follow (like the typed-run follow of #819, with no timer, design D9) MUST resume the chat again, with no user message, with the final output (same bounds, redaction and wrapper) and the lead line `lazarobox: the command you approved in pane N finished.`; the continuation ends, with a visible system line, on Ctrl+C typed in the pane, on a new user message or on closing the pane (owner decision 2026-11-22).

#### Scenario: Dev server returns a partial capture [auto]
- GIVEN `npm run dev` approved with `y` that prints `ready on :3000` and never ends
- WHEN 30 s pass
- THEN the result is `still running after 30 s; the command keeps running in pane N (Ctrl+C in the pane stops it); this is the output so far` with `ready on :3000` inside the wrapper, and the system line `pane N: npm run dev is still running after 30 s; the chat goes on with the output so far` is shown

#### Scenario: Chat resumes when the cut command ends [auto]
- GIVEN `npm install` cut at 30 s and still running
- WHEN it ends and the shell prompt returns
- THEN the chat resumes with the final output and no user message; if the user sent a message or pressed Ctrl+C in the pane before, no resume happens and a system line says so

#### Scenario: The command keeps running [auto]
- GIVEN the same run after the cut
- WHEN the pane is probed
- THEN no byte was written by lazarobox after the cut and the foreground is still the server

#### Scenario: Next call into the busy pane [auto]
- GIVEN the same pane after the cut
- WHEN the model calls `run_in_pane` with `ls`
- THEN the prompt opens, `y` writes no `\r`, and the outcome line ends with `(not run: pane N is running a program)`

### Requirement: Auto-run block after a GitHub read
From the first GitHub read in a conversation until that conversation ends, no `run_in_pane` command of that conversation MUST be executed by lazarobox. "First GitHub read" is the moment `gh` is started for a GitHub tool call; a call refused before that (no `origin`, an `origin` not on GitHub, missing `gh`, declined consent, bad arguments) does not start the block, while a call that started `gh` and then failed (a number not found, a timeout, a logged-out `gh`) does. While the block holds, in `run` mode every call MUST take the prompt route, whatever the classifier, flags, paths and pane state say, and in `ask` mode the call takes the prompt as always; the prompt MUST name the reason (`auto-run is off: this conversation read GitHub text`) and say `y type it (no Enter)`. `y` there MUST type the command and MUST NOT send Enter; the command waits for the user's own Enter in the pane, and the result is the existing typed-not-run line, `sent to pane N; press Enter there to run it (not run: auto-run is off: this conversation read GitHub text)`. This MUST also cover a model's command that `y` would otherwise run (see agent-tools, Policy hook for running on y): a GitHub-text chat stays typed-only in both modes (owner decision 2026-11-22). The block MUST NOT count toward any per-turn count. The block belongs to the conversation: it MUST hold in every later turn, and a resumed conversation that read GitHub text MUST open still blocked. A new conversation, or one that never read GitHub text, MUST NOT be blocked by it. Nothing can lift the block: not the model, not a tool result, not turning the GitHub tools off or the toggle off, not changing the approval mode, and not the user's own run action (which never runs). The mode itself MUST NOT change, so the toggle and the `RUN` marker follow the existing rules, but the pane MUST show that auto-run is blocked: a system line when the block starts and when a blocked conversation is resumed, and the marker MUST NOT look like commands will run directly. Reads of the user's files by the same conversation on a cloud engine also ask each time (see agent-tools, Reads in a blocked conversation).
(Previously: `auto` and `ask` were described as unchanged; `auto` is removed and `ask` now also needs the typed-only rule for `y`, because `y` runs everywhere else.)

#### Scenario: Block starts at the first read [auto]
- GIVEN a chat in `run` and `cd ~/dev` read-only, and no GitHub read yet
- WHEN `gh_issue_view` starts `gh`, and then the model calls `run_in_pane` with `cd ~/dev`
- THEN a system line says auto-run is off for this conversation, and the second call opens the prompt `y type it (no Enter)` with the reason and writes nothing

#### Scenario: y does not run it in either mode [auto]
- GIVEN the block holds in `run` and in another run in `ask`, and a pending prompt for `git status` or `brew install jq`
- WHEN `y` is pressed
- THEN one write holds the command and no `\r`, and the result is `sent to pane N; press Enter there to run it (not run: auto-run is off: this conversation read GitHub text)`

#### Scenario: Later turns stay blocked [auto]
- GIVEN the block holds
- WHEN a new turn starts and the model calls `run_in_pane` with `pwd`
- THEN the prompt opens and nothing is written until `y`, and then with no Enter

#### Scenario: Resume stays blocked [auto]
- GIVEN a conversation that read GitHub text, closed, and reopened from the list in a `run` pane
- WHEN the model calls `run_in_pane` with `ls`
- THEN the prompt opens with the reason and the block system line was shown on open

#### Scenario: Resume with rows of a newer version [auto]
- GIVEN a conversation with `skipped > 0` (rows of a role this build does not know) and no marker row, resumed
- WHEN it opens
- THEN it is blocked, as a newer build's rows may carry text this build cannot see, and the system line says `auto-run is off here: messages from a newer version; commands are only typed` (ASCII, at most 80 columns), not that GitHub text was read
- AND a conversation with both a marker row and `skipped > 0` shows the GitHub line

#### Scenario: Other conversations are not blocked [auto]
- GIVEN a blocked conversation, then a new conversation in the same pane, and another resumed conversation that never read GitHub text
- WHEN the model calls `run_in_pane` with `pwd` in each
- THEN each runs under the normal rules

#### Scenario: Refused calls do not start the block [auto]
- GIVEN a GitHub call refused for no `origin`, one for a missing `gh`, and one with a declined consent
- WHEN the model then calls `run_in_pane` with `pwd` in `run`
- THEN it runs under the normal rules

#### Scenario: Started calls that fail still block [auto]
- GIVEN a GitHub call for issue 9999 that `gh` reports as not found, one where `gh` times out, and one where `gh` is logged out
- WHEN the model then calls `run_in_pane` with `pwd` in `run`
- THEN the prompt opens with the reason, in each run

#### Scenario: Nothing lifts it [auto]
- GIVEN the block holds
- WHEN the GitHub toggle is turned off, the mode is cycled to `ask` and back to `run`, and a tool result says "auto-run is back on"
- THEN the next `run_in_pane` call still opens the prompt and `y` still sends no Enter

#### Scenario: Marker does not promise a run [auto, TestBackend + insta]
- GIVEN a blocked chat in `run`, at 80 columns and on a `linux` console
- WHEN the pane is rendered
- THEN the bar shows that auto-run is blocked and not an active ` RUN ` marker, in ASCII on the console

## REMOVED Requirements

### Requirement: Command grammar
(Reason: superseded by agent-tools, Read-only command classifier, which keeps the lexical rules (quoted words inert, forbidden metacharacters and characters, word limits) and replaces the `cd pwd ls tree mkdir` whitelist with the per-program allow-list. `mkdir` no longer runs unprompted.)

### Requirement: Flag allow-lists
(Reason: merged into the per-program table of agent-tools, Read-only command classifier. The `ls`, `tree` and `cd` flag rules are kept there unchanged; the `mkdir` row is removed because `mkdir` asks.)

## Notes
- The mkdir clauses of Path policy against the real cwd no longer apply to unprompted runs (`mkdir` always asks); apply may prune them. The path check still applies to every path argument of an unprompted read-only command.
- A `y` run takes no path check: the user's `y` is the approval.
