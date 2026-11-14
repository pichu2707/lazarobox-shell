# Delta for agent-tools

Change `chat-write-grant` (the concept is called run auto-apply; the folder keeps its name). Base: `openspec/specs/agent-tools/spec.md` on origin/main (99106097), Requirements `Approval model` and `Chat approval mode`. Each requirement below is given as its full current text (collapsed), then its full changed text, which is the one that replaces it. Nothing else of the base changes.

## MODIFIED Requirements

What changes in `Approval model`: the sentence "The only exception is a `run_in_pane` call in a chat whose approval mode is `auto`" lists exactly the exceptions that exist (the `auto` and `run` typing of `run_in_pane`, which Chat approval mode already defines, and this change's batch exception) so "only" and "two" no longer clash; the sentences on "nothing reaches the disk", "no per-session or per-tool trust" and the single pending approval follow; two scenarios are added.

<details>
<summary>Current text on origin/main, <code>openspec/specs/agent-tools/spec.md</code> lines 231-278</summary>

> ### Requirement: Approval model
> `gh_issue_draft` needs its own `y` on every call, like `run_command`, `git_add` and `git_commit`, and no approval mode answers it (see issue-draft).
> `find_dir` and `list_dir` MUST run without approval, and `read_file` and `grep` MUST run without approval only on a local engine (see Read tools and their approval, code-search). `glob` MUST run without approval. Every other tool call MUST need an explicit user `y`, every time, and the approval of the file write tools is the batch review (see code-write): `create_dir`, `write_file` and `edit_file` stage and ask nothing by themselves, and nothing reaches the disk until the user approves the proposed batch as a whole. `run_command`, `git_add` and `git_commit` MUST each need their own `y` (see command-runner, git-tools). The only exception is a `run_in_pane` call in a chat whose approval mode is `auto` (see Chat approval mode): it is typed without a prompt and never executed. The fixed program allow-list of `run_command` bounds the program and is not an approval. There MUST be no "always allow", no per-session or per-tool trust, no configuration that approves automatically, and nothing MUST ever execute a command for the user without a `y`. Tools from a user MCP server MUST always be write tools. Waiting for an answer MUST NEVER end in an approval: a timeout, a cancel, a closed chat, a closed target pane, a lost connection and an app quit all end as `denied`. A chat MUST have at most one pending approval; another write request while one is pending MUST be denied at once with the reason `another approval is pending`.
>
> #### Scenario: No auto approval [auto]
> - GIVEN a write tool call from any engine in a chat in `ask` mode, or an external tool call, a `run_command` or a cloud `read_file` in either mode
> - WHEN no key is pressed for any length of time (injected clock)
> - THEN the call is never executed
>
> #### Scenario: Timeout is denied [auto]
> - GIVEN a pending approval and a caller whose wait expires
> - WHEN the expiry fires
> - THEN the call result is `denied`, the prompt is withdrawn with a system line, and a later `y` writes nothing
>
> #### Scenario: Each call asks again [auto]
> - GIVEN a chat in `ask` mode and two `run_in_pane` calls in one turn, the first approved
> - WHEN the second is dispatched
> - THEN a new approval is required
>
> #### Scenario: Second request while pending [auto]
> - GIVEN a pending approval
> - WHEN another write tool call arrives
> - THEN it returns `denied` (`another approval is pending`) and the first stays pending
>
> #### Scenario: Denied paths [auto]
> - GIVEN a pending approval ended in turn by cancel, pane close, target-pane exit and quit
> - WHEN each ends
> - THEN the result is `denied` each time and nothing is written
>
> #### Scenario: Free read tools around a pending approval [auto]
> - GIVEN a pending approval
> - WHEN `find_dir`, `list_dir` or `glob` is called (or `read_file` on a local engine)
> - THEN it runs and returns without waiting for the approval
>
> #### Scenario: Second read_file while pending [auto]
> - GIVEN a pending approval on a cloud engine
> - WHEN a `read_file` call arrives
> - THEN it returns `denied` (`another approval is pending`)
>
> #### Scenario: Staging asks nothing, apply needs y [auto]
> - GIVEN `write_file` and `edit_file` calls in a chat
> - WHEN they are dispatched
> - THEN no approval opens and the disk is untouched, and only the review of the proposed batch can write
>
> #### Scenario: Allow-list is not an approval [auto]
> - GIVEN `run_command` with an allow-listed program in any mode
> - WHEN it is dispatched
> - THEN a prompt opens and nothing starts before `y`

</details>

### Requirement: Approval model
`gh_issue_draft` needs its own `y` on every call, like `run_command`, `git_add` and `git_commit`, and no approval mode answers it (see issue-draft).
`find_dir` and `list_dir` MUST run without approval, and `read_file` and `grep` MUST run without approval only on a local engine (see Read tools and their approval, code-search). `glob` MUST run without approval. Every other tool call MUST need an explicit user `y`, every time, and the approval of the file write tools is the batch review (see code-write): `create_dir`, `write_file` and `edit_file` stage and ask nothing by themselves, and nothing reaches the disk until the user approves the proposed batch as a whole, or the chat holds auto-apply and the batch is one it applies (see Chat approval mode). `run_command`, `git_add` and `git_commit` MUST each need their own `y` (see command-runner, git-tools). The exceptions are these two, and no other. (1) A `run_in_pane` call in a chat whose approval mode is `auto` or `run` (see Chat approval mode): in `auto` it is typed without a prompt and never executed; in `run` a command that passes pane-execution is typed and run without a prompt. (2) A `propose_batch` call of a chat in `run` mode that holds auto-apply, for a batch that auto-apply covers (see Chat approval mode and code-write): it applies without the batch review, because the user gave auto-apply with a key in an earlier review of that chat pane. The fixed program allow-list of `run_command` bounds the program and is not an approval. There MUST be no "always allow", no per-session or per-tool trust other than exception (2), no configuration that approves automatically, and nothing MUST ever execute a command for the user without a `y` other than what exception (1) lets run. Tools from a user MCP server MUST always be write tools. Waiting for an answer MUST NEVER end in an approval: a timeout, a cancel, a closed chat, a closed target pane, a lost connection and an app quit all end as `denied`. A chat MUST have at most one pending approval; another write request while one is pending MUST be denied at once with the reason `another approval is pending`. A batch that auto-apply would cover is no exception: while an approval is pending it MUST be denied with that reason, never queued and never applied.

#### Scenario: No auto approval [auto]
- GIVEN a write tool call from any engine in a chat in `ask` mode, or an external tool call, a `run_command` or a cloud `read_file` in either mode
- WHEN no key is pressed for any length of time (injected clock)
- THEN the call is never executed

#### Scenario: Timeout is denied [auto]
- GIVEN a pending approval and a caller whose wait expires
- WHEN the expiry fires
- THEN the call result is `denied`, the prompt is withdrawn with a system line, and a later `y` writes nothing

#### Scenario: Each call asks again [auto]
- GIVEN a chat in `ask` mode and two `run_in_pane` calls in one turn, the first approved
- WHEN the second is dispatched
- THEN a new approval is required

#### Scenario: Second request while pending [auto]
- GIVEN a pending approval
- WHEN another write tool call arrives
- THEN it returns `denied` (`another approval is pending`) and the first stays pending

#### Scenario: Denied paths [auto]
- GIVEN a pending approval ended in turn by cancel, pane close, target-pane exit and quit
- WHEN each ends
- THEN the result is `denied` each time and nothing is written

#### Scenario: Free read tools around a pending approval [auto]
- GIVEN a pending approval
- WHEN `find_dir`, `list_dir` or `glob` is called (or `read_file` on a local engine)
- THEN it runs and returns without waiting for the approval

#### Scenario: Second read_file while pending [auto]
- GIVEN a pending approval on a cloud engine
- WHEN a `read_file` call arrives
- THEN it returns `denied` (`another approval is pending`)

#### Scenario: Staging asks nothing, apply needs y [auto]
- GIVEN `write_file` and `edit_file` calls in a chat
- WHEN they are dispatched
- THEN no approval opens and the disk is untouched, and only the review of the proposed batch can write

#### Scenario: Allow-list is not an approval [auto]
- GIVEN `run_command` with an allow-listed program in any mode
- WHEN it is dispatched
- THEN a prompt opens and nothing starts before `y`

#### Scenario: Auto-apply covers no other tool [auto]
- GIVEN a chat in `run` mode holding auto-apply
- WHEN the model calls `run_command`, `git_add`, `git_commit`, `gh_issue_draft` and a user MCP tool
- THEN each opens its own approval and nothing runs until `y`

#### Scenario: An auto-apply batch while an approval is pending [auto]
- GIVEN a chat in `run` mode holding auto-apply and an approval pending (for example a `run_command` waiting for `y`)
- WHEN the model proposes a batch that auto-apply would cover
- THEN the propose call returns `denied` (`another approval is pending`), nothing is written, nothing is queued, and the pending approval stays pending

What changes in `Chat approval mode`: the marker sentence, the "MUST NOT cover any other ask" sentence, a new "Auto-apply" paragraph after the base paragraph, and new scenarios at the end. The base text is otherwise as it is.

<details>
<summary>Current text on origin/main, <code>openspec/specs/agent-tools/spec.md</code> lines 313-449</summary>

> ### Requirement: Chat approval mode
> Every chat pane MUST have an approval mode, `ask`, `auto` or `run`, for tool calls made by the model. `ask` is the default and is the behaviour of every other requirement here. A chat pane MUST take the mode of `[ai.tools] approval` (see configuration), as it is at that moment, when the pane opens and whenever the user switches to or starts another conversation in that pane; the toggle (see modal-input) MUST cycle the mode `ask`, `auto`, `run`, `ask` of the focused chat pane only, for its current conversation, and the mode MUST NOT be stored or sent to any engine. Only the user can change the mode: no tool, tool result, MCP result, pane output or model text can switch it. Entering `run` needs no extra confirmation. In `auto` a `run_in_pane` call MUST be typed into the default target pane (the last focused shell pane of the chat's tab, no cycling) without a prompt, and MUST NEVER be followed by Enter, `\r` or a newline outside a bracketed paste. In `run` a call whose command is runnable under pane-execution MUST be typed and followed by Enter in one write; every other command MUST open the normal prompt, where `y` runs the model's command like a pane-execution run (one write ending in Enter, the same settle, capture and report, not counted toward the 5-command limit) unless it is multi-line, the pane's directory cannot be read, the pane is executing, line pending or dirty, or a non-ASCII command meets a non-UTF-8 locale, or the write step refuses it; then `y` types it without Enter and the chat's outcome line says why (`not run: <reason>`). `auto` and `run` MUST NOT cover any other ask: `read_file` on a cloud engine and every user MCP tool still need `y`, `find_dir` and `list_dir` stay free, and the run action of pane-commands still opens its confirm. Before typing, the same checks as accepting with `y` MUST run (validation, the pane exists, it is a shell pane of the chat's tab, it is not on the alternate screen, a multi-line or tab command needs bracketed paste), and the target pane's foreground process MUST be its shell; when that cannot be known the call is refused in `auto` and takes the prompt in `run`. A failed check MUST return `denied` with a short reason and draw the result row; nothing is written. After an `auto` command is typed, and after a command is typed with `y`, its target pane MUST be marked line pending (only the `auto` and `run` routing reads the mark). The mark MUST clear only when the user sends Enter, Ctrl+C or Ctrl+U to that pane through lazarobox's input, or pastes text holding `\r` or `\n` into it while that pane has bracketed paste off (inside a bracketed paste a newline executes nothing and clears nothing), when that pane closes, or when the typed call ends `denied`; the screen is never read, and the mark belongs to one pane only. The call MUST instead open the normal prompt, as in `ask` (never denied for these reasons), when its target pane is marked line pending, dirty or executing (see pane-execution), when 5 `auto` or `run` commands (one shared named constant) were already typed or run in this turn, when the target pane has the focus, or when the chat's tab is not the shown tab or is zoomed onto another pane. The pane of an update (`cargo install`) MUST NOT be a target. A call that arrives while an approval is pending MUST be denied (`another approval is pending`), and switching mode MUST NOT answer a pending approval. The call row MUST show the exact command, and the result row MUST say what happened: `typed into pane N — no Enter`, or, for a run, the exact command that ran and its status (see pane-execution). A call whose turn ended or was cancelled before the write MUST get no bytes written (the answer is claimed before the write). The registry, tool descriptions and requests MUST be the same in all three modes. While `auto` or `run` is on and tools are on, the chat pane MUST show a marker in high contrast in every chat view, focused or not: ` AUTO ` in amber and ` RUN ` in red (see chat-pane); `ask` shows no marker. Only `run` may execute a command: one that passes pane-execution, or a model's command the user approved with `y`; never by any key or result of the model.
>
> #### Scenario: Default is ask [auto]
> - GIVEN a file with no `approval` under `[ai.tools]`
> - WHEN a chat pane opens and the model calls `run_in_pane`
> - THEN the chat is in `ask` mode, a confirm opens and nothing is written until `y`
>
> #### Scenario: Auto types without y and without Enter [auto]
> - GIVEN a chat in `auto` mode, shell pane 2 idle at its prompt, not marked line pending, focused elsewhere, in the shown tab
> - WHEN the model calls `run_in_pane` with `cd ~/dev/curso-rust`
> - THEN no confirm opens, exactly one write reaches pane 2 with the command bytes and no `\r` or trailing `\n`, and the result is `typed into pane N — no Enter`
>
> #### Scenario: Run executes a safe command [auto]
> - GIVEN a chat in `run` mode and the same pane state
> - WHEN the model calls `run_in_pane` with `cd ~/dev/curso-rust`
> - THEN no confirm opens and exactly one write reaches pane 2: the command bytes then `\r`
>
> #### Scenario: Run prompts for an off-list command [auto]
> - GIVEN a chat in `run` mode
> - WHEN the model calls `run_in_pane` with `git status`, and `y` is pressed
> - THEN a confirm opens first, and the write after `y` has no `\r`
>
> #### Scenario: Auto never covers reads or external tools [auto]
> - GIVEN a chat in `auto` mode, and in another run `run` mode, on a cloud engine with a user MCP server
> - WHEN the model calls `read_file` and then an external tool
> - THEN each opens an approval and nothing runs until `y`; `find_dir` and `list_dir` still run free
>
> #### Scenario: User run action unchanged [auto]
> - GIVEN a chat in `auto` mode, and in another run `run` mode
> - WHEN the run action of pane-commands is used on a bash block
> - THEN the confirm opens as in `ask` and `y` writes no Enter
>
> #### Scenario: Toggle cycles only the current chat [auto]
> - GIVEN two chat panes A and B, both in `ask`
> - WHEN the toggle is pressed on A three times
> - THEN A reads `auto`, then `run`, then `ask` again, B stays `ask`, and A's transcript gets a system line naming each new mode
>
> #### Scenario: New chat uses the config value [auto]
> - GIVEN `approval = "run"` in the config and an open chat toggled to `ask`
> - WHEN a new chat pane opens
> - THEN the new chat is `run` and the open chat stays `ask`
>
> #### Scenario: Conversation switch resets the mode [auto]
> - GIVEN `approval = "ask"` in the config and a chat pane toggled to `run`
> - WHEN the user opens another conversation from the list in that pane, and in another run starts a new conversation in it
> - THEN the pane is `ask` each time and the marker is gone
>
> #### Scenario: Bar shows AUTO and RUN [auto, TestBackend + insta]
> - GIVEN chats in `auto`, `run` and `ask`, unfocused
> - WHEN rendered
> - THEN the first shows ` AUTO ` in the amber high-contrast style, the second ` RUN ` in the red one, and the third no marker
>
> #### Scenario: Recheck failure in auto is refused [auto]
> - GIVEN a chat in `auto` mode and a target pane on the alternate screen, or a closed target pane, or a multi-line command with bracketed paste off, or a command with an escape sequence
> - WHEN the model calls `run_in_pane`
> - THEN the result is `denied` with the reason, a result row shows it and nothing is written
>
> #### Scenario: Not at a shell prompt [auto]
> - GIVEN a chat in `auto` mode and a target pane whose foreground process is `python3`, or whose foreground cannot be read
> - WHEN the model calls `run_in_pane`
> - THEN the result is `denied` (`pane N is running a program`) and nothing is written; in `run` mode the normal prompt opens instead
>
> #### Scenario: Second call prompts while the line is pending [auto]
> - GIVEN a chat in `auto` mode that has typed `cd ~/dev` into pane 2, and the user has sent nothing to pane 2 since
> - WHEN the model calls `run_in_pane` with `ls` for pane 2, in the same turn, in a later turn, or in the same batch
> - THEN the normal confirm opens and nothing more is written until `y`
>
> #### Scenario: Second call is typed after Enter [auto]
> - GIVEN the same chat, and the user then pressed Enter in pane 2
> - WHEN the model calls `run_in_pane` with `ls`
> - THEN `ls` is typed into pane 2 without a prompt and pane 2 is marked line pending again
>
> #### Scenario: Ctrl+C or Ctrl+U clears the mark [auto]
> - GIVEN pane 2 marked line pending
> - WHEN the user sends Ctrl+C to pane 2, and in another run Ctrl+U, and in another run pastes text holding `\r` or `\n` with bracketed paste off
> - THEN the mark is cleared each time and the next `auto` call to pane 2 is typed without a prompt; other keys such as Backspace leave the mark set
>
> #### Scenario: A bracketed paste leaves the mark [auto]
> - GIVEN pane 2 marked line pending and bracketed paste on in pane 2
> - WHEN the user pastes text holding `\r` or `\n` into pane 2
> - THEN the mark stays set and the next `auto` call to pane 2 opens the prompt
>
> #### Scenario: A line typed with y is marked [auto]
> - GIVEN a chat in `ask` mode whose `run_in_pane` was accepted with `y` (typed, no Enter), and then switched to `auto` or `run`
> - WHEN the model calls `run_in_pane` for the same pane before the user sent anything to it
> - THEN the normal confirm opens
>
> #### Scenario: A zoomed tab or an update pane is not typed into [auto]
> - GIVEN a chat in `auto` or `run` mode in a tab zoomed onto the chat, or whose only PTY pane runs the update's `cargo install`
> - WHEN the model calls `run_in_pane`
> - THEN in the first case the normal confirm opens, in the second the result is `denied` (no shell pane in the tab), and nothing is written
>
> #### Scenario: The mark is per pane [auto]
> - GIVEN pane 2 marked line pending and shell pane 3 not marked
> - WHEN the target is pane 3, and in another run pane 2 is closed
> - THEN the call to pane 3 is typed without a prompt, and closing pane 2 removes its mark
>
> #### Scenario: Sixth call in a turn prompts [auto]
> - GIVEN a chat in `auto` or `run` mode where each earlier command finished and the user pressed Enter after each typed one
> - WHEN the model calls `run_in_pane` six times in one turn, typed and run commands counted together
> - THEN the first five go without a prompt and the sixth opens the normal confirm; the count starts again at the next turn
>
> #### Scenario: Focused target or hidden tab asks [auto]
> - GIVEN a chat in `auto` or `run` mode whose target pane has the focus, and in another run a chat whose tab is not shown
> - WHEN the model calls `run_in_pane`
> - THEN the normal confirm opens and nothing is written until `y`
>
> #### Scenario: Toggle does not answer a pending approval [auto]
> - GIVEN a chat in `ask` mode with a pending `run_in_pane` confirm
> - WHEN the toggle switches it to `auto` or `run`
> - THEN the confirm is still pending and nothing is written
>
> #### Scenario: Pending approval blocks auto and run [auto]
> - GIVEN a chat in `auto` or `run` mode with a pending cloud `read_file` approval
> - WHEN the model calls `run_in_pane`
> - THEN the result is `denied` (`another approval is pending`) and nothing is written
>
> #### Scenario: Bad config value falls back to ask [auto]
> - GIVEN `approval = "yes"` under `[ai.tools]`
> - WHEN the app starts and a chat opens
> - THEN the chat is in `ask` mode and the startup notice names `ai.tools.approval`
>
> #### Scenario: Tools off is unaffected [auto]
> - GIVEN tools off and `approval = "auto"`, and in another run `approval = "run"`
> - WHEN requests are built for every adapter and the chat is rendered
> - THEN each request is byte-identical to the one without tools, no marker is shown and the toggle only shows the tools-off notice
>
> #### Scenario: Same request in all modes [auto]
> - GIVEN tools on
> - WHEN the request is built for a chat in `ask`, one in `auto` and one in `run`
> - THEN the three are byte-identical
>
> #### Scenario: Auto in a real shell [manual, Kitty]
> - GIVEN `auto` on and bash, zsh and fish panes
> - WHEN the model asks to go to `~/dev/curso-rust`
> - THEN `cd ~/dev/curso-rust` appears on the input line unexecuted, the ` AUTO ` marker is visible, and it runs only after Enter

</details>

### Requirement: Chat approval mode
Every chat pane MUST have an approval mode, `ask`, `auto` or `run`, for tool calls made by the model. `ask` is the default and is the behaviour of every other requirement here. A chat pane MUST take the mode of `[ai.tools] approval` (see configuration), as it is at that moment, when the pane opens and whenever the user switches to or starts another conversation in that pane; the toggle (see modal-input) MUST cycle the mode `ask`, `auto`, `run`, `ask` of the focused chat pane only, for its current conversation, and the mode MUST NOT be stored or sent to any engine. Only the user can change the mode: no tool, tool result, MCP result, pane output or model text can switch it. Entering `run` needs no extra confirmation. In `auto` a `run_in_pane` call MUST be typed into the default target pane (the last focused shell pane of the chat's tab, no cycling) without a prompt, and MUST NEVER be followed by Enter, `\r` or a newline outside a bracketed paste. In `run` a call whose command is runnable under pane-execution MUST be typed and followed by Enter in one write; every other command MUST open the normal prompt, where `y` runs the model's command like a pane-execution run (one write ending in Enter, the same settle, capture and report, not counted toward the 5-command limit) unless it is multi-line, the pane's directory cannot be read, the pane is executing, line pending or dirty, or a non-ASCII command meets a non-UTF-8 locale, or the write step refuses it; then `y` types it without Enter and the chat's outcome line says why (`not run: <reason>`). `auto` and `run` MUST NOT cover any other ask, except the batch of a chat in `run` that holds auto-apply (see Auto-apply below): `read_file` on a cloud engine and every user MCP tool still need `y`, `find_dir` and `list_dir` stay free, and the run action of pane-commands still opens its confirm. Before typing, the same checks as accepting with `y` MUST run (validation, the pane exists, it is a shell pane of the chat's tab, it is not on the alternate screen, a multi-line or tab command needs bracketed paste), and the target pane's foreground process MUST be its shell; when that cannot be known the call is refused in `auto` and takes the prompt in `run`. A failed check MUST return `denied` with a short reason and draw the result row; nothing is written. After an `auto` command is typed, and after a command is typed with `y`, its target pane MUST be marked line pending (only the `auto` and `run` routing reads the mark). The mark MUST clear only when the user sends Enter, Ctrl+C or Ctrl+U to that pane through lazarobox's input, or pastes text holding `\r` or `\n` into it while that pane has bracketed paste off (inside a bracketed paste a newline executes nothing and clears nothing), when that pane closes, or when the typed call ends `denied`; the screen is never read, and the mark belongs to one pane only. The call MUST instead open the normal prompt, as in `ask` (never denied for these reasons), when its target pane is marked line pending, dirty or executing (see pane-execution), when 5 `auto` or `run` commands (one shared named constant) were already typed or run in this turn, when the target pane has the focus, or when the chat's tab is not the shown tab or is zoomed onto another pane. The pane of an update (`cargo install`) MUST NOT be a target. A call that arrives while an approval is pending MUST be denied (`another approval is pending`), and switching mode MUST NOT answer a pending approval. The call row MUST show the exact command, and the result row MUST say what happened: `typed into pane N — no Enter`, or, for a run, the exact command that ran and its status (see pane-execution). A call whose turn ended or was cancelled before the write MUST get no bytes written (the answer is claimed before the write). The registry, tool descriptions and requests MUST be the same in all three modes. While `auto` or `run` is on and tools are on, the chat pane MUST show a marker in high contrast in every chat view, focused or not: ` AUTO ` in amber and ` RUN ` in red, or ` RUN+A ` in red while the chat holds auto-apply (see chat-pane); `ask` shows no marker. Only `run` may execute a command: one that passes pane-execution, or a model's command the user approved with `y`; never by any key or result of the model.

**Auto-apply.** Only in `run`, with tools on, in a plain chat (not a worker) whose folder is below HOME and is not a granted Windows drive folder, a chat pane MUST be able to hold auto-apply, given by the user with `a` in the review of one of its own batches (see code-write, Batch review and coverage gate). While it is held, the later batches of that pane apply without a review where code-write says so. Auto-apply is not the `/write` or `/exec` grant: it turns no tool family on, and `/write` does not give it. It MUST be set only once the host reports that first batch applied in full (the call's tool result has status `Ok`; an answer sent proves nothing), and only if the pane is still in the state in which `a` was pressed. It MUST be held in memory in that pane's own chat state, never stored, never sent to any engine, and never set or read by runtime code (the runtime MUST NOT be able to name it, as for the mode, design D19): no tool, tool result, MCP result, pane output or model text can give it. It MUST end, with the system line `Auto-apply is off because <why>`, when the user starts or switches to another conversation in that pane, resumes a saved conversation, or changes the pane's approval mode (leaving `run` included), changes the chat's folder (`/cd`), turns tools off, when the Write family stops being offered to the chat (the saved `write` toggle changes, or the `/write` grant ends), and when a batch applied under it does not apply in full (drift, refusal, failure, partial); it also ends when the pane closes and on quit. It has its own end hook: ending a `/write` or `/exec` grant does not clear it by itself, and it is not cleared by the grants' own end function. A pending review is not answered by its end. While it is held and tools are on, the marker MUST read ` RUN+A ` in red instead of ` RUN `. It belongs to the pane that gave it: another chat pane MUST NOT inherit it, and a batch of another pane MUST be reviewed as without it.

#### Scenario: Default is ask [auto]
- GIVEN a file with no `approval` under `[ai.tools]`
- WHEN a chat pane opens and the model calls `run_in_pane`
- THEN the chat is in `ask` mode, a confirm opens and nothing is written until `y`

#### Scenario: Auto types without y and without Enter [auto]
- GIVEN a chat in `auto` mode, shell pane 2 idle at its prompt, not marked line pending, focused elsewhere, in the shown tab
- WHEN the model calls `run_in_pane` with `cd ~/dev/curso-rust`
- THEN no confirm opens, exactly one write reaches pane 2 with the command bytes and no `\r` or trailing `\n`, and the result is `typed into pane N — no Enter`

#### Scenario: Run executes a safe command [auto]
- GIVEN a chat in `run` mode and the same pane state
- WHEN the model calls `run_in_pane` with `cd ~/dev/curso-rust`
- THEN no confirm opens and exactly one write reaches pane 2: the command bytes then `\r`

#### Scenario: Run prompts for an off-list command [auto]
- GIVEN a chat in `run` mode
- WHEN the model calls `run_in_pane` with `git status`, and `y` is pressed
- THEN a confirm opens first, and the write after `y` has no `\r`

#### Scenario: Auto never covers reads or external tools [auto]
- GIVEN a chat in `auto` mode, and in another run `run` mode, on a cloud engine with a user MCP server
- WHEN the model calls `read_file` and then an external tool
- THEN each opens an approval and nothing runs until `y`; `find_dir` and `list_dir` still run free

#### Scenario: User run action unchanged [auto]
- GIVEN a chat in `auto` mode, and in another run `run` mode
- WHEN the run action of pane-commands is used on a bash block
- THEN the confirm opens as in `ask` and `y` writes no Enter

#### Scenario: Toggle cycles only the current chat [auto]
- GIVEN two chat panes A and B, both in `ask`
- WHEN the toggle is pressed on A three times
- THEN A reads `auto`, then `run`, then `ask` again, B stays `ask`, and A's transcript gets a system line naming each new mode

#### Scenario: New chat uses the config value [auto]
- GIVEN `approval = "run"` in the config and an open chat toggled to `ask`
- WHEN a new chat pane opens
- THEN the new chat is `run` and the open chat stays `ask`

#### Scenario: Conversation switch resets the mode [auto]
- GIVEN `approval = "ask"` in the config and a chat pane toggled to `run`
- WHEN the user opens another conversation from the list in that pane, and in another run starts a new conversation in it
- THEN the pane is `ask` each time and the marker is gone

#### Scenario: Bar shows AUTO and RUN [auto, TestBackend + insta]
- GIVEN chats in `auto`, `run` and `ask`, unfocused
- WHEN rendered
- THEN the first shows ` AUTO ` in the amber high-contrast style, the second ` RUN ` in the red one, and the third no marker

#### Scenario: Recheck failure in auto is refused [auto]
- GIVEN a chat in `auto` mode and a target pane on the alternate screen, or a closed target pane, or a multi-line command with bracketed paste off, or a command with an escape sequence
- WHEN the model calls `run_in_pane`
- THEN the result is `denied` with the reason, a result row shows it and nothing is written

#### Scenario: Not at a shell prompt [auto]
- GIVEN a chat in `auto` mode and a target pane whose foreground process is `python3`, or whose foreground cannot be read
- WHEN the model calls `run_in_pane`
- THEN the result is `denied` (`pane N is running a program`) and nothing is written; in `run` mode the normal prompt opens instead

#### Scenario: Second call prompts while the line is pending [auto]
- GIVEN a chat in `auto` mode that has typed `cd ~/dev` into pane 2, and the user has sent nothing to pane 2 since
- WHEN the model calls `run_in_pane` with `ls` for pane 2, in the same turn, in a later turn, or in the same batch
- THEN the normal confirm opens and nothing more is written until `y`

#### Scenario: Second call is typed after Enter [auto]
- GIVEN the same chat, and the user then pressed Enter in pane 2
- WHEN the model calls `run_in_pane` with `ls`
- THEN `ls` is typed into pane 2 without a prompt and pane 2 is marked line pending again

#### Scenario: Ctrl+C or Ctrl+U clears the mark [auto]
- GIVEN pane 2 marked line pending
- WHEN the user sends Ctrl+C to pane 2, and in another run Ctrl+U, and in another run pastes text holding `\r` or `\n` with bracketed paste off
- THEN the mark is cleared each time and the next `auto` call to pane 2 is typed without a prompt; other keys such as Backspace leave the mark set

#### Scenario: A bracketed paste leaves the mark [auto]
- GIVEN pane 2 marked line pending and bracketed paste on in pane 2
- WHEN the user pastes text holding `\r` or `\n` into pane 2
- THEN the mark stays set and the next `auto` call to pane 2 opens the prompt

#### Scenario: A line typed with y is marked [auto]
- GIVEN a chat in `ask` mode whose `run_in_pane` was accepted with `y` (typed, no Enter), and then switched to `auto` or `run`
- WHEN the model calls `run_in_pane` for the same pane before the user sent anything to it
- THEN the normal confirm opens

#### Scenario: A zoomed tab or an update pane is not typed into [auto]
- GIVEN a chat in `auto` or `run` mode in a tab zoomed onto the chat, or whose only PTY pane runs the update's `cargo install`
- WHEN the model calls `run_in_pane`
- THEN in the first case the normal confirm opens, in the second the result is `denied` (no shell pane in the tab), and nothing is written

#### Scenario: The mark is per pane [auto]
- GIVEN pane 2 marked line pending and shell pane 3 not marked
- WHEN the target is pane 3, and in another run pane 2 is closed
- THEN the call to pane 3 is typed without a prompt, and closing pane 2 removes its mark

#### Scenario: Sixth call in a turn prompts [auto]
- GIVEN a chat in `auto` or `run` mode where each earlier command finished and the user pressed Enter after each typed one
- WHEN the model calls `run_in_pane` six times in one turn, typed and run commands counted together
- THEN the first five go without a prompt and the sixth opens the normal confirm; the count starts again at the next turn

#### Scenario: Focused target or hidden tab asks [auto]
- GIVEN a chat in `auto` or `run` mode whose target pane has the focus, and in another run a chat whose tab is not shown
- WHEN the model calls `run_in_pane`
- THEN the normal confirm opens and nothing is written until `y`

#### Scenario: Toggle does not answer a pending approval [auto]
- GIVEN a chat in `ask` mode with a pending `run_in_pane` confirm
- WHEN the toggle switches it to `auto` or `run`
- THEN the confirm is still pending and nothing is written

#### Scenario: Pending approval blocks auto and run [auto]
- GIVEN a chat in `auto` or `run` mode with a pending cloud `read_file` approval
- WHEN the model calls `run_in_pane`
- THEN the result is `denied` (`another approval is pending`) and nothing is written

#### Scenario: Bad config value falls back to ask [auto]
- GIVEN `approval = "yes"` under `[ai.tools]`
- WHEN the app starts and a chat opens
- THEN the chat is in `ask` mode and the startup notice names `ai.tools.approval`

#### Scenario: Tools off is unaffected [auto]
- GIVEN tools off and `approval = "auto"`, and in another run `approval = "run"`
- WHEN requests are built for every adapter and the chat is rendered
- THEN each request is byte-identical to the one without tools, no marker is shown and the toggle only shows the tools-off notice

#### Scenario: Same request in all modes [auto]
- GIVEN tools on
- WHEN the request is built for a chat in `ask`, one in `auto` and one in `run`
- THEN the three are byte-identical

#### Scenario: Auto in a real shell [manual, Kitty]
- GIVEN `auto` on and bash, zsh and fish panes
- WHEN the model asks to go to `~/dev/curso-rust`
- THEN `cd ~/dev/curso-rust` appears on the input line unexecuted, the ` AUTO ` marker is visible, and it runs only after Enter

#### Scenario: A new conversation asks again [auto]
- GIVEN a chat in `run` mode holding auto-apply
- WHEN the user starts a new conversation in the pane, and in another run resumes a saved one, and the model proposes a batch
- THEN the review opens and waits for `y` or `a`

#### Scenario: Leaving run ends auto-apply [auto]
- GIVEN a chat in `run` mode holding auto-apply
- WHEN the user toggles the mode to `ask` and back to `run`, and the model proposes a batch
- THEN the review opens, the marker reads ` RUN `, and the chat said `Auto-apply is off because` the mode changed

#### Scenario: A folder change ends auto-apply [auto]
- GIVEN a chat in `run` mode holding auto-apply, and no `/write` or `/exec` grant held
- WHEN the user changes the chat's folder with `/cd`
- THEN auto-apply is off with its own line, and the next batch opens the review

#### Scenario: Tools off ends auto-apply [auto]
- GIVEN a chat in `run` mode holding auto-apply, and no `/write` or `/exec` grant held
- WHEN tools are turned off and on again
- THEN auto-apply is off, and the next batch opens the review

#### Scenario: A changed Write toggle ends auto-apply [auto]
- GIVEN a chat in `run` mode holding auto-apply, with the saved `write` toggle on
- WHEN the user saves the settings with the `write` toggle off
- THEN auto-apply is off with its own line

#### Scenario: The /write grant does not give it [auto]
- GIVEN a chat in `run` mode that has the Write family by a `/write` grant and no auto-apply
- WHEN the model proposes a batch
- THEN the review opens and waits for `y` or `a`, and ending the `/write` grant clears nothing it did not hold

#### Scenario: Another pane does not inherit auto-apply [auto]
- GIVEN two chat panes in `run` mode and auto-apply held by the first
- WHEN the model of the second pane proposes a batch
- THEN the review opens in the second pane, its marker reads ` RUN `, and nothing is written until the user's key

#### Scenario: The model cannot give auto-apply [auto]
- GIVEN a chat in `run` mode without auto-apply
- WHEN a tool result or model text asks for it, and the model proposes a batch
- THEN the review opens and nothing is written until the user's key

#### Scenario: The runtime cannot name the flag [auto]
- GIVEN the non-test code of `src/runtime.rs` and `src/runtime/`
- WHEN the audit scans it for the guarded names (the approval mode names and the auto-apply names)
- THEN no file names any of them, and a sample that names the auto-apply flag is reported
