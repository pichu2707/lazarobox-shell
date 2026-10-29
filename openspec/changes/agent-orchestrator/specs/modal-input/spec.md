# Delta for modal-input

Slice tags as in agent-orchestrator.

## ADDED Requirements

### Requirement: Notice and scope keys [S1]
In a chat pane the provider notice MUST accept with Enter or `y` and dismiss with Esc; the run-start scope prompt MUST approve with `y` and deny with `n` or Esc. These keys apply only while the view is shown and MUST NOT reach any PTY. A key typed as part of a message MUST NOT answer either view: the notice is accepted only by `y` pressed after an arm delay (as approvals), or only while the composer is empty, so an Enter that sends a message never accepts it; S1e-1b picks one of the two and applies the same guard to the scope prompt (key audit, 2026-10-29).

#### Scenario: Notice keys [auto]
- GIVEN the notice shown
- WHEN Enter is pressed (or Esc)
- THEN it is accepted (or dismissed without accepting)

#### Scenario: Scope keys [auto]
- GIVEN the scope prompt shown
- WHEN `y` (or `n`) is pressed
- THEN the scope is approved (or denied, leaving an empty scope)

### Requirement: ORCH toggle key [S1]
In a chat pane, Ctrl+O MUST toggle the pane's ORCH mode when the global orchestrator gate is on, as a one-shot key (a held key does not toggle again, like the other debounced chat keys). Ctrl+O is not bound in the chat key table today (bound: Enter, Esc, Ctrl+C, Ctrl+Y, Ctrl+B, Ctrl+R, Ctrl+J, Alt+Enter, Ctrl+U, PageUp, PageDown, and on main Ctrl+G, the orphan batch review of coding-tools) and MUST NOT shadow any of them. It MUST be handled before the key can reach a PTY and be shown in the chat footer hint while the gate is on. With the gate off it does nothing.

#### Scenario: Ctrl+O toggles [auto]
- GIVEN the gate on and a chat pane with ORCH off
- WHEN Ctrl+O is pressed, then pressed again
- THEN the mode is on, then off

#### Scenario: Held key toggles once [auto]
- GIVEN the gate on and ORCH off
- WHEN Ctrl+O arrives as a Repeat
- THEN the mode does not change

#### Scenario: Existing keys unchanged [auto]
- GIVEN the chat key table
- WHEN Ctrl+C, Ctrl+Y, Ctrl+B, Ctrl+R, Ctrl+J and Ctrl+U are pressed
- THEN each behaves as before this change

### Requirement: Failure alert keys [S1, S2]
While a failure alert is shown: `r` MUST retry, `c` or Esc MUST cancel the run; [S2] `m` MUST open the picker at model level (change model) and `p` at engine level (change provider). Other keys MUST NOT answer the alert. The letter keys (`r`, `c`, and [S2] `m`, `p`) MUST act only while the composer is empty or after an arm delay (as approvals), so text being typed cannot pick an option (key audit, 2026-10-29). An approval prompt, if any, keeps its own keys and is answered by user key events only.

#### Scenario: Retry key [auto]
- GIVEN a failure alert
- WHEN `r` is pressed
- THEN the worker is relaunched from scratch, with the previous error appended to its prompt

#### Scenario: Cancel keys [auto]
- GIVEN a failure alert
- WHEN `c` is pressed (or Esc)
- THEN the run is cancelled

#### Scenario: Change keys [S2, auto]
- GIVEN a failure alert
- WHEN `m` is pressed, then Esc, then `p`
- THEN the model picker opens, the alert returns, then the engine picker opens

### Requirement: Cancel keys for workers [S1, S3]
Ctrl+C MUST cancel the orchestrator turn and all workers of the run (existing chat cancel). [S3] A chat key MUST cancel the selected worker, and another the whole run; each is shown in the footer hint while a run is active.

#### Scenario: Ctrl+C [auto]
- GIVEN a run with two workers
- WHEN Ctrl+C is pressed
- THEN the turn and both workers are cancelled

#### Scenario: Cancel the selected worker [S3, auto]
- GIVEN a run with two workers and worker 1 selected
- WHEN the cancel-worker key is pressed
- THEN worker 1 is cancelled and worker 2 keeps running
p='openspec/changes/agent-orchestrator/specs/modal-input/spec.md'
s=open(p).read()
old="In a chat pane the provider notice MUST accept with Enter or `y` and dismiss with Esc; the run-start scope prompt MUST approve with `y` and deny with `n` or Esc. These keys apply only while the view is shown and MUST NOT reach any PTY."
assert s.count(old)==1
s=s.replace(old,"In a chat pane the provider notice MUST accept with Enter or `y` and dismiss with Esc; the run-start scope prompt MUST approve with `y` and deny with `n` or Esc. These keys apply only while the view is shown and MUST NOT reach any PTY. A key typed as part of a message MUST NOT answer either view: the notice is accepted only by `y` pressed after an arm delay (as approvals), or only while the composer is empty, so an Enter that sends a message never accepts it; S1e-1b picks one of the two and applies the same guard to the scope prompt (key audit, 2026-10-29).")
old="Ctrl+O is not bound in the chat key table today (bound: Enter, Esc, Ctrl+C, Ctrl+Y, Ctrl+B, Ctrl+R, Ctrl+J, Alt+Enter, Ctrl+U, PageUp, PageDown)"
assert s.count(old)==1
s=s.replace(old,"Ctrl+O is not bound in the chat key table today (bound: Enter, Esc, Ctrl+C, Ctrl+Y, Ctrl+B, Ctrl+R, Ctrl+J, Alt+Enter, Ctrl+U, PageUp, PageDown, and on main Ctrl+G, the orphan batch review of coding-tools)")
old="While a failure alert is shown: `r` MUST retry, `c` or Esc MUST cancel the run; [S2] `m` MUST open the picker at model level (change model) and `p` at engine level (change provider). Other keys MUST NOT answer the alert."
assert s.count(old)==1
s=s.replace(old, old+" The letter keys (`r`, `c`, and [S2] `m`, `p`) MUST act only while the composer is empty or after an arm delay (as approvals), so text being typed cannot pick an option (key audit, 2026-10-29).")
s=s.rstrip("\n")+"\n"+open('/dev/stdin').read()
open(p,'w').write(s)

## MODIFIED Requirements

Dependency: the allowed-chords sentence in "Chat pane input" is written against the explicit list of direct chat chords that the `key-freeze` change adds to this rule on main (Ctrl+G); once it is merged, this block only adds Ctrl+O to that list and keeps the rest of its text. Base text otherwise verbatim; changes: the Ctrl+O row and that sentence, the Ctrl+O clause of "Memory keys" and the gate in its "Unbound Ctrl keys ignored" scenario.

### Requirement: Chat pane input
In TERMINAL, with a chat pane focused and the conversation view shown, keys MUST act as follows; every key not listed below that is not a printable character MUST be swallowed (no effect, no WritePty):

| Key | Effect |
|---|---|
| printable characters, Backspace, Delete, Left, Right, Home, End | edit the composer |
| Enter | send the message |
| Alt+Enter, Ctrl+J | insert a newline in the composer |
| Ctrl+U | clear the composer |
| Esc, Ctrl+C | cancel the running stream; with no stream running, do nothing |
| Ctrl+Y | yank the last assistant answer (see chat-pane) |
| Ctrl+B | yank the last code block of the last assistant answer (see chat-pane) |
| PageUp, PageDown | scroll the transcript |
| Ctrl+R | expand or collapse the most recent reasoning block (see chat-pane) |
| Ctrl+O | with the orchestrator gate on, toggle the pane's ORCH mode (see ORCH toggle key); with the gate off, swallowed like an unlisted key |
| Ctrl+Space | enter PREFIX as in any pane |

The tag editor (see chat-history) is opened by the prefix key `T` (`Ctrl+Space T`, capital), not by a direct Ctrl+letter chord: new chat keys live behind the prefix (user rule 2026-10-13), and `t` is already the TAB group. The direct chat chords allowed as exceptions to that rule are Ctrl+G (orphan batch review, coding-tools) and Ctrl+O (ORCH toggle, this change); any other new chat key still lives behind the prefix. `T` MUST be a root leaf of the prefix table, left out of the key hint like `o` and `s`, and on a shell pane it MUST only show a notice.

Ctrl+C on a chat pane MUST NEVER emit Quit or a WritePty, whether a stream is running or not. Pressing Ctrl+Space twice MUST swallow the second press (no NUL is inserted in the composer). While the conversation list, the tag editor or the delete confirmation is shown, keys act as defined in chat-history, with the same swallow rule. The key bindings are defaults for design to confirm; Ctrl+R (R for reasoning) is a proposed default chosen in the style of Ctrl+Y, Ctrl+B and Ctrl+T, and is an open question until confirmed on a real terminal (manual check M8).

#### Scenario: Typing edits the composer [auto]
- GIVEN a focused idle chat pane
- WHEN `h`, `i` and Backspace are pressed
- THEN the composer holds `h` and no WritePty is emitted

#### Scenario: Enter sends, newline bindings do not [auto]
- GIVEN the composer holding `hi`
- WHEN Alt+Enter, then Ctrl+J, then Enter are pressed
- THEN two newlines are inserted and only the Enter emits a ChatSend

#### Scenario: Ctrl+B yanks a code block [auto]
- GIVEN a chat pane whose last answer holds a fenced block
- WHEN Ctrl+B is pressed
- THEN a SetClipboard effect carries the block and no WritePty is emitted

#### Scenario: Ctrl+R toggles reasoning [auto]
- GIVEN a chat pane whose last turn has a collapsed reasoning block
- WHEN Ctrl+R is pressed twice
- THEN the block expands and then collapses, no WritePty is emitted and the composer text is unchanged

#### Scenario: Ctrl+R in the list view [auto]
- GIVEN the conversation list shown in a chat pane
- WHEN Ctrl+R is pressed
- THEN nothing changes and nothing is typed into a composer

#### Scenario: Esc and Ctrl+C cancel a stream [auto]
- GIVEN a streaming chat pane
- WHEN Esc is pressed, and in another run Ctrl+C is pressed
- THEN each time a ChatCancel effect is emitted and the partial answer is kept

#### Scenario: Esc and Ctrl+C idle do nothing [auto]
- GIVEN an idle chat pane with composer text
- WHEN Esc and Ctrl+C are pressed
- THEN the composer text is unchanged, no Quit and no WritePty is emitted

#### Scenario: Unlisted key swallowed [auto]
- GIVEN a focused chat pane
- WHEN Tab, F5 and Ctrl+X are pressed
- THEN nothing changes and no WritePty is emitted

#### Scenario: Prefix still works [auto]
- GIVEN a focused chat pane
- WHEN `Ctrl+Space` then `w` then `v` are pressed
- THEN a split is made and mode is TERMINAL
- AND when `Ctrl+Space` is pressed twice instead, nothing is inserted into the composer

#### Scenario: Keys in the list view [auto]
- GIVEN the conversation list shown in a chat pane
- WHEN `j`, `d` and `y` are pressed
- THEN the cursor moves, the delete confirmation shows and `y` confirms it, and nothing is typed into a composer

#### Scenario: Real keys in Kitty [manual, Kitty]
- GIVEN the app in Kitty with a chat pane focused
- WHEN the user types, sends with Enter, adds a line with Alt+Enter, cancels with Esc and presses Ctrl+C when idle
- THEN each key behaves as listed and the app never quits on Ctrl+C

### Requirement: Memory keys
In TERMINAL, with a chat pane focused and the conversation view shown, no key is added to the Chat pane input table, and the memory block and the summary are driven by prefix keys:

| Key | Effect |
|---|---|
| Ctrl+Space then `s` | request the end-of-conversation summary (see chat-memory). A root leaf of the prefix table, left out of the hint (the root hint is already 79 columns and a longer one would clip `q quit` at 80); on a pane that is not a chat it shows `The summary is for chat panes; open one with Ctrl+Space c` |
| Ctrl+Space then `o` | expand or collapse the memory block (see chat-memory); it never touches the reasoning block, which stays on Ctrl+R. A root leaf of the prefix table, left out of the hint; on a pane that is not a chat it shows `The memory line is for chat panes; open one with Ctrl+Space c` |

Ctrl+N and Ctrl+E are NOT bound: in a chat pane they are swallowed as any other unbound key. Ctrl+O is bound only as the ORCH toggle while the orchestrator gate is on (see ORCH toggle key); with the gate off it is swallowed as any other unbound key, as before. With memory off or not ready, `Prefix s` only shows its notices and `Prefix o` does nothing when no memory line exists. These keys MUST NEVER reach a PTY and MUST NOT type anything into the composer. In the conversation list, the tag editor and the delete confirmation they do nothing.

#### Scenario: Prefix n opens the editor [auto]
- GIVEN a conversation with an answer and a ready memory
- WHEN Ctrl+Space then `n` are pressed
- THEN the note editor opens, no WritePty is emitted and the composer text is unchanged

#### Scenario: Ctrl+N is not bound [auto]
- GIVEN a focused chat pane
- WHEN Ctrl+N is pressed, and as a Repeat event
- THEN nothing happens, no notice shows and no effect is emitted

#### Scenario: Prefix s requests a summary [auto]
- GIVEN an idle conversation with an answer and a ready memory
- WHEN Ctrl+Space then `s` are pressed
- THEN one summary request effect is emitted and no WritePty is emitted

#### Scenario: Prefix o and Ctrl+R are independent [auto]
- GIVEN a last turn with a collapsed reasoning block and a collapsed memory line
- WHEN Ctrl+Space then `o` are pressed, then Ctrl+R
- THEN the memory line expands first and the reasoning block expands second, each only on its own key

#### Scenario: Unbound Ctrl keys ignored [auto]
- GIVEN a focused chat pane and the orchestrator gate off
- WHEN Ctrl+E and Ctrl+O are pressed, and as Repeat events
- THEN nothing happens and no effect is emitted

#### Scenario: Memory off [auto]
- GIVEN memory off
- WHEN Ctrl+Space then `n`, and Ctrl+Space then `s`, are pressed
- THEN only the notice `Memory is off; enable it in the menu (Ctrl+Space m)` appears and no effect is emitted

#### Scenario: Other views ignore them [auto]
- GIVEN the conversation list shown
- WHEN Ctrl+Space then `s` and Ctrl+Space then `o` are pressed
- THEN nothing changes and nothing is typed
