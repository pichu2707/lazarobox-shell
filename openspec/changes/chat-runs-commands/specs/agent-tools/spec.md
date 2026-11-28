# Delta for Agent Tools (chat-runs-commands)

Scope notes (apply to every delta file of this change):
- Two approval modes remain: `ask` (default) and `run`. `auto` is removed.
- "Unprompted run": a `run_in_pane` command that runs with no `y` (only in `run`, only when classified read-only).
- "Approved run": a `run_in_pane` command that runs after the user's `y`, in either mode.
- Every user-visible line below is an exact string; `<N>`, `<cmd>`, `<cwd>`, `<reason>` are placeholders.

## MODIFIED Requirements

### Requirement: Approval model
`gh_issue_draft` needs its own `y` on every call, like `run_command`, `git_add` and `git_commit`, and no approval mode answers it (see issue-draft).
`find_dir` and `list_dir` MUST run without approval, and `read_file` and `grep` MUST run without approval only on a local engine (see Read tools and their approval, code-search). `glob` MUST run without approval. Every other tool call MUST need an explicit user `y`, every time, and the approval of the file write tools is the batch review (see code-write): `create_dir`, `write_file` and `edit_file` stage and ask nothing by themselves, and nothing reaches the disk until the user approves the proposed batch as a whole. `run_command`, `git_add` and `git_commit` MUST each need their own `y` (see command-runner, git-tools). The only `run_in_pane` call that may run without a prompt is a command classified read-only (see Read-only command classifier) in a chat whose approval mode is `run`, in a conversation that has not read GitHub text (see Chat approval mode). Every other `run_in_pane` call needs its own `y`, and in either mode that `y` runs the command (see A `y` runs the model's command). The fixed program allow-list of `run_command` bounds the program and is not an approval. There MUST be no "always allow", no per-session or per-tool trust, no configuration that approves automatically, and nothing MUST ever execute a command for the user without a `y`, with TWO scoped exceptions: (1) the read-only unprompted run of `run` mode above (owner decision 2026-11-22); and (2) owner decision 2026-11-14, C13: the approval level the user sets for a user-configured MCP server (`approval = "ask" | "reads" | "all"` in its `[[ai.mcp.servers]]` entry, default `ask`; see mcp-servers, Approval level per server). That level is the only configuration that approves a call automatically; it covers only the tools of that server, never a built-in tool, never a command, never a worker, and it does not change anything else in this requirement; and it is not honored in a conversation that read GitHub text (blocked, owner decision 2026-11-15): there every user MCP tool asks `y` whatever the level, and the prompt shows the block reason. A tool from a user MCP server is a write tool unless the catalog classes it `read` for a server declared with the catalog entry's own command (see mcp-catalog, Read/write class), and a server's own read-only hint is never a reason to treat it otherwise. Waiting for an answer MUST NEVER end in an approval: a timeout, a cancel, a closed chat, a closed target pane, a lost connection and an app quit all end as `denied`. A chat MUST have at most one pending approval; another write request while one is pending MUST be denied at once with the reason `another approval is pending`.
(Previously: the only exception was a `run_in_pane` call typed without a prompt in `auto` mode; `auto` is removed and `y` only typed in `ask`.)

#### Scenario: No auto approval [auto]
- GIVEN a write tool call from any engine in a chat in `ask` mode, or an external tool call of a server at level `ask`, a `run_command` or a cloud `read_file` in either mode
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

#### Scenario: A server's level is the only automatic approval besides read-only runs [S9, auto]
- GIVEN a user server at level `all`, a chat in `ask` mode, and the same chat's calls to `run_command`, `git_commit`, `write_file` staging and a cloud `read_file`
- WHEN the model calls an `all` tool and then each of those
- THEN the `all` tool runs with no prompt, and each of the others behaves exactly as without any level (prompt or batch review, nothing executed without `y`)

#### Scenario: Default level asks [S9, auto]
- GIVEN a user server with no `approval` key
- WHEN the model calls any of its tools
- THEN an approval opens and the server is not contacted before `y`

#### Scenario: A blocked conversation asks whatever the level [S9, auto]
- GIVEN a conversation that read GitHub text and user servers at level `all` and at level `reads` (with a `read`-class tool)
- WHEN the model calls a tool of each
- THEN each opens an approval showing the block reason and nothing runs before `y`

#### Scenario: Unknown tool is a write tool [S9, auto]
- GIVEN a user server tool the catalog does not list, annotated `readOnlyHint: true`, at level `reads`
- WHEN the model calls it
- THEN it is treated as a write tool and asks `y`

#### Scenario: Only read-only runs go unasked [auto]
- GIVEN a chat in `run` with `ls` and `npm install` called in turn
- WHEN each is dispatched
- THEN `ls` runs with no prompt and `npm install` opens a prompt and nothing starts before `y`

### Requirement: run_in_pane tool
`run_in_pane` takes a command string and MUST use the pane-commands path: validation, the pending confirm naming the target pane, the accept-time rechecks and the write. In both modes a command the user approved with `y` is written with Enter in one write and the call stays pending until the run ends (see pane-execution, Settle and cut-off); the write is typed without Enter only where `y` cannot run it (see A `y` runs the model's command) or where the conversation read GitHub text. In `run` mode a command classified read-only (see Read-only command classifier) and routable under pane-execution is written with Enter with no prompt; any other command takes the normal confirm. The result MUST say what actually happened (the exact texts are pinned in pane-execution, Result contract), and MUST be exactly one of: `sent` / `typed into pane N — no Enter` (typed, not executed), `ran` (with the bounded, wrapped output and, for `cd`, the new cwd), `still running` (with the output so far), `output not available`, or `denied` (with a short reason). Output, pane content and a cwd MAY be returned only for `ran` and `still running`, only for a command that ran (unprompted or approved), and only as pane-execution defines; a typed command's result MUST carry no output of the command or of the pane and no cwd. The tool description MUST be identical in every mode and MUST be the exact text pinned in Model guidance. No result carries an exit status.
(Previously: output was returned only in `run` mode; in `ask` and `auto` `y` only typed; the description was neutral and told the model nothing about running.)

#### Scenario: Sent when it cannot run [auto]
- GIVEN a `run_in_pane` call with `cd ~/dev/curso-rust` approved with `y` while the target pane has a half-typed line
- WHEN the write completes
- THEN the tool result is `sent` (typed, no Enter) with no pane content, and the outcome line ends with `(not run: the pane has a half-typed line)`

#### Scenario: Denied [auto]
- GIVEN the same call declined with `n`
- WHEN the confirm closes
- THEN the tool result is `denied` and nothing is written

#### Scenario: Refused command [auto]
- GIVEN a call whose command holds an escape sequence
- WHEN it is dispatched
- THEN the result is `denied` (`command refused`), no confirm opens and nothing is written

#### Scenario: Unprompted read in run mode [auto]
- GIVEN a chat in `run` and an idle unfocused shell pane
- WHEN the model calls `run_in_pane` with `ls -l` and it settles
- THEN the result is `ran` with the bounded wrapped output, and the row shows the exact command

#### Scenario: Approved run in ask mode returns output [auto]
- GIVEN a chat in `ask` and `brew install jq` approved with `y`
- WHEN the run settles
- THEN the result is `ran` with the bounded wrapped output and no message from the user is needed for the chat to continue

#### Scenario: Typed result carries no output [auto]
- GIVEN a command typed (not run) because the conversation read GitHub text, and the pane then prints output after the user's own Enter
- WHEN the result is built
- THEN it holds no pane output and no cwd

#### Scenario: Find then go there [manual, Kitty]
- GIVEN an engine with tools and a directory `~/dev/curso-rust`
- WHEN the user asks to find `curso-rust` and take them there, and accepts the prompt
- THEN a `find_dir` row appears, then a confirm `run in pane N (<cwd>)` with a `cd` command, and after `y` the pane is in that directory with no further key from the user

### Requirement: Chat approval mode
Every chat pane MUST have an approval mode, `ask` or `run`, for tool calls made by the model. `ask` is the default and is the behaviour of every other requirement here. A chat pane MUST take the mode of `[ai.tools] approval` (see configuration), as it is at that moment, when the pane opens and whenever the user switches to or starts another conversation in that pane; the toggle (see modal-input) MUST cycle the mode `ask`, `run`, `ask` of the focused chat pane only, for its current conversation, and the mode MUST NOT be stored or sent to any engine. Only the user can change the mode: no tool, tool result, MCP result, pane output or model text can switch it. Entering `run` needs no extra confirmation. In `ask` every `run_in_pane` call MUST open the prompt, and `y` runs it. In `run` a call whose command is classified read-only (see Read-only command classifier) and runnable under pane-execution MUST be typed and followed by Enter in one write with no prompt (an unprompted run); every other command MUST open the normal prompt, where `y` runs it like in `ask`. Before writing, the same checks as accepting with `y` MUST run (validation, the pane exists, it is a shell pane of the chat's tab, it is not on the alternate screen, a multi-line or tab command needs bracketed paste), and the target pane's foreground process MUST be its shell; when that cannot be known the call takes the prompt, and `y` there types without Enter with the outcome line `not run: <reason>`. A failed check that denies MUST return `denied` with a short reason and draw the result row; nothing is written. The mode never answers any other ask: `read_file` on a cloud engine and every user MCP tool still need `y` (only the approval level of the tool's own server can let a call run unasked, see mcp-servers, Approval level per server, and it does so in every mode alike, except in a conversation that read GitHub text, where every user tool asks), `find_dir` and `list_dir` stay free, and the run action of pane-commands still opens its confirm and `y` types without Enter (owner decision 2026-11-22: `Prefix e` keeps today's behaviour for manual sends). An unprompted run MUST instead open the normal prompt, as in `ask` (never denied for these reasons), when the target pane is marked line pending, dirty or executing (see pane-execution), when 5 unprompted runs (one shared named constant `MAX_AUTO_PER_TURN`) were already made in this turn, when the target pane has the focus, or when the chat's tab is not the shown tab or is zoomed onto another pane. The per-turn count MUST include only unprompted runs: a run approved with `y` MUST NOT count, in either mode, and a call routed to the prompt MUST NOT count. The pane of an update (`cargo install`) MUST NOT be a target. A call that arrives while an approval is pending MUST be denied (`another approval is pending`), and switching mode MUST NOT answer a pending approval. The call row MUST show the exact command, and the result row MUST say what happened: `typed into pane N — no Enter`, or, for a run, the exact command that ran and its status (see pane-execution). A call whose turn ended or was cancelled before the write MUST get no bytes written (the answer is claimed before the write). The registry, tool descriptions and requests MUST be the same in both modes. While `run` is on and tools are on, the chat pane MUST show the marker ` RUN ` in red in every chat view, focused or not (see chat-pane); `ask` shows no marker. A command executes only by one of: a read-only unprompted run in `run`, or the user's `y` on the model's command; never by any key or result of the model.
(Previously: three modes `ask`, `auto`, `run`; `auto` typed without a prompt and never pressed Enter; `y` in `ask` only typed; `y` in `run` ran only some commands; the 5-per-turn count included y-approved typed lines in `auto`.)

#### Scenario: Default is ask [auto]
- GIVEN a file with no `approval` under `[ai.tools]`
- WHEN a chat pane opens and the model calls `run_in_pane`
- THEN the chat is in `ask` mode, a confirm opens and nothing is written until `y`

#### Scenario: Ask runs on y [auto]
- GIVEN a chat in `ask`, shell pane 2 idle at its prompt and unfocused in the shown tab
- WHEN the model calls `run_in_pane` with `brew install jq` and `y` is pressed
- THEN the prompt keys read `y run it · n cancel · p next pane`, nothing is written before `y`, and exactly one write follows: the command bytes then `\r`

#### Scenario: Run executes a read-only command unprompted [auto]
- GIVEN a chat in `run` and the same pane state
- WHEN the model calls `run_in_pane` with `git status`
- THEN no confirm opens and exactly one write reaches pane 2: the command bytes then `\r`

#### Scenario: Run prompts for a command that writes [auto]
- GIVEN a chat in `run`
- WHEN the model calls `run_in_pane` with `mkdir build`, and `y` is pressed
- THEN a confirm opens first, nothing is written before `y`, and the write after `y` is the command then `\r`

#### Scenario: Mode never covers reads or external tools [auto]
- GIVEN a chat in `ask`, and in another run `run` mode, on a cloud engine with a user MCP server at the default level
- WHEN the model calls `read_file` and then an external tool
- THEN each opens an approval and nothing runs until `y`; `find_dir` and `list_dir` still run free

#### Scenario: User run action unchanged [auto]
- GIVEN a chat in `ask`, and in another run `run` mode
- WHEN the run action of pane-commands (`Prefix e`) is used on a bash block
- THEN the confirm opens, its keys read `y type it (no Enter) · n cancel · p next pane`, and `y` writes no Enter

#### Scenario: Toggle cycles only the current chat [auto]
- GIVEN two chat panes A and B, both in `ask`
- WHEN the toggle is pressed on A twice
- THEN A reads `run`, then `ask` again, B stays `ask`, and A's transcript gets a system line naming each new mode

#### Scenario: New chat uses the config value [auto]
- GIVEN `approval = "run"` in the config and an open chat toggled to `ask`
- WHEN a new chat pane opens
- THEN the new chat is `run` and the open chat stays `ask`

#### Scenario: Conversation switch resets the mode [auto]
- GIVEN `approval = "ask"` in the config and a chat pane toggled to `run`
- WHEN the user opens another conversation from the list in that pane, and in another run starts a new conversation in it
- THEN the pane is `ask` each time and the marker is gone

#### Scenario: Bar shows RUN only [auto, TestBackend + insta]
- GIVEN chats in `run` and `ask`, unfocused
- WHEN rendered
- THEN the first shows ` RUN ` in the red high-contrast style and the second no marker, and no ` AUTO ` marker exists

#### Scenario: Recheck failure is refused [auto]
- GIVEN a chat in `run` and a target pane on the alternate screen, or a closed target pane, or a command with an escape sequence
- WHEN the model calls `run_in_pane`
- THEN the result is `denied` with the reason, a result row shows it and nothing is written

#### Scenario: Not at a shell prompt [auto]
- GIVEN a chat in `ask` or `run` and a target pane whose foreground process is `python3`, or whose foreground cannot be read
- WHEN the model calls `run_in_pane` and `y` is pressed
- THEN the prompt opens, the write after `y` has no `\r`, and the outcome line ends with `(not run: pane N is running a program)`

#### Scenario: Busy pane types, never runs over a running program [auto]
- GIVEN a chat in `run` and a target pane marked executing
- WHEN the model calls `run_in_pane` with `ls`
- THEN the normal prompt opens and nothing is written until `y`

#### Scenario: A line typed with y is marked [auto]
- GIVEN a chat whose `run_in_pane` was typed with `y` without Enter (for example in a GitHub-blocked conversation)
- WHEN the model calls `run_in_pane` for the same pane before the user sent anything to it
- THEN the normal confirm opens and `y` types it without Enter with `(not run: the pane has a half-typed line)`

#### Scenario: A zoomed tab or an update pane is not a target [auto]
- GIVEN a chat in `run` in a tab zoomed onto the chat, or whose only PTY pane runs the update's `cargo install`
- WHEN the model calls `run_in_pane`
- THEN in the first case the normal confirm opens, in the second the result is `denied` (no shell pane in the tab), and nothing is written

#### Scenario: Sixth unprompted run in a turn prompts [auto]
- GIVEN a chat in `run` where each earlier command finished
- WHEN the model calls `run_in_pane` six times in one turn with `pwd`
- THEN the first five run with no prompt and the sixth opens the normal confirm with the reason `5 unprompted runs already made in this turn`; the count starts again at the next turn

#### Scenario: Approved runs do not count toward the cap [auto]
- GIVEN a chat in `run` where five installs were approved with `y` in one turn
- WHEN the model calls `run_in_pane` with `pwd`
- THEN `pwd` runs with no prompt, and a sixth `y`-approved install in the same turn is not refused

#### Scenario: Focused target or hidden tab asks [auto]
- GIVEN a chat in `run` whose target pane has the focus, and in another run a chat whose tab is not shown
- WHEN the model calls `run_in_pane` with `ls`
- THEN the normal confirm opens and nothing is written until `y`

#### Scenario: Toggle does not answer a pending approval [auto]
- GIVEN a chat in `ask` mode with a pending `run_in_pane` confirm
- WHEN the toggle switches it to `run`
- THEN the confirm is still pending and nothing is written

#### Scenario: Pending approval blocks the next call [auto]
- GIVEN a chat in `run` mode with a pending cloud `read_file` approval
- WHEN the model calls `run_in_pane`
- THEN the result is `denied` (`another approval is pending`) and nothing is written

## ADDED Requirements

### Requirement: Read-only command classifier
A pure function of the command text alone MUST decide whether a `run_in_pane` command is read-only, and only a read-only command may run unprompted (in `run` mode, see Chat approval mode). It MUST be an allow-list, never a deny-list: a command is read-only only when it is exactly one command that matches an allow-list entry. The text is never split: any `|`, `;`, `&&`, `||`, `&`, redirect or `$()` makes it ask. At most 8 words and at most 4096 bytes in all. Anything else MUST classify as ask (not read-only) and a classification failure of any kind MUST be ask. A command classified ask takes the normal prompt, where the user's `y` runs it.

Lexical rules: words are separated by exactly one ASCII space; an unquoted word holds only ASCII letters and digits, the Latin letters and the punctuation `. _ / @ % + : , = ~ -` accepted by pane-execution, Command grammar (a quoted word holds only those characters plus the space, is inert, and is never the program). The command is ask when it holds any of: a redirect (`<`, `>`, `>>`, `2>`, `<<`, `<(`, `>(`), a command or process substitution (`$(`, `` ` ``, `<(`), any `$`, glob (`* ? [ ] { }`), `\`, `!`, `#`, a lone `&` (background), a newline, `\r`, tab or other control or format character, an env assignment as first word, or a first word that is quoted, starts with `~` or `=`, or contains `/` (a program is always a bare name).

Per-program allow-list (any flag not listed makes the command ask; short flags may be clustered only when every letter is allowed; a bare `--` is ask):

| Program | Allowed | Notes |
|---|---|---|
| `cd` | zero or one path | navigation, as today |
| `pwd` | none | no arguments |
| `ls` | `-l -a -h -1 -t -r`, up to 4 paths | never `-R` |
| `tree` | `-d`, optional `-L <1-2 digits>`, zero or one path | never `-o` |
| `cat` | `-n`, 1 to 4 paths | no `-` (stdin) |
| `head`, `tail` | `-n <N>` or `-<N>` (N up to 4 digits), 1 to 4 paths | never `-f`, `-F`, `--follow` (they never end) |
| `wc` | `-l -w -c`, 1 to 4 paths | no stdin: a read verb without a path asks |
| `rg`, `grep` | `-n -i -l -c -w -F`, plus `-r` for `grep`; one pattern word, up to 4 paths | never `--pre`, `-z`, `--search-zip`, `--hostname-bin`, `-f`, `-e` |
| `which`, `whoami`, `uname` | `uname -a`, `uname -r`; the others none | |
| `file`, `stat` | 1 to 4 paths, no flags | |
| `git status` | `-s -b --short --branch`, up to 4 paths | |
| `git --no-pager diff` | one of `--stat --name-only --name-status` is required; plus `--cached --staged`, up to 4 paths | plain `git --no-pager diff` (content) asks, as do `log -p` and `show`; never `--output`, `--ext-diff`, `--textconv` (owner decision 2026-11-22) |
| `git --no-pager log` | `-n <N>` is required; plus `--oneline --stat`, up to 4 paths | never `--output`; `log` without `-n` asks |
| `git`, `rg`, `fd`, `bat`, `jq`, `docker`, `make`, `tmux`, `curl` | exactly one argument, `--version` | a fixed list of nine (design D6), `--version` only, never `-V`; any other program (`node`, `python3`, `cargo`, `npm`…) asks, as these are often shims |

For `git`, `status` is the second word; `log` and `diff` MUST be written `git --no-pager log` and `git --no-pager diff` (a pager in a pty waits for keys). `--no-pager` is the only global option allowed before the subcommand: any other (`-c`, `-C`, `--git-dir`, `--work-tree`, `--exec-path`) makes the command ask. There are no pipes: a pipe is a shell metacharacter and the command asks. Path arguments MUST pass the path policy of pane-execution, Path policy against the real cwd, for the verbs it names, and for `cat`, `head`, `tail`, `wc`, `rg`, `grep`, `file`, `stat`, `git` paths with the same secret and credential rules (a secret path makes the command ask). `sudo`, `su`, `doas`, `env`, `xargs`, `sh`, `bash`, `zsh`, `eval`, `exec`, `time`, `nohup`, `find`, `sed`, `awk`, `tee`, `dd`, `rm`, `mv`, `cp`, `mkdir`, `touch`, `chmod`, `wget`, `npm`, `pip`, `cargo`, `apt`, `brew` and every program not in the table MUST be ask. `sudo` MUST be ask wherever it appears, as the first word. `mkdir` is no longer read-only: it asks, inside the project too.

| Command | Result | Why |
|---|---|---|
| `ls -la src` | read-only | flags in list |
| `git status` | read-only | listed subcommand |
| `git --no-pager diff --stat` | read-only | summary flag |
| `git --no-pager log -n 5` | read-only | `-n` given |
| `git diff --stat`, `git log -n 5` | ask | `--no-pager` missing |
| `git --no-pager diff`, `git --no-pager diff src/main.rs` | ask | content diff can print tracked secrets |
| `git --no-pager log` | ask | `-n` required |
| `cat README.md` | read-only | listed program, plain path |
| `node --version` | read-only | bare program with `--version` |
| `mkdir build`, `touch x`, `git add .`, `git commit -m x`, `cargo build`, `npm install` | ask | writes or installs |
| `sudo apt install jq` | ask | sudo |
| `cat x > y`, `cat x >> y`, `head -n 1 < f` | ask | redirect |
| `ls; rm -rf x`, `ls && rm x` | ask | shell metacharacter |
| `ls \| wc -l`, `ls \| sh`, `cat x \| bash` | ask | pipe (shell metacharacter) |
| `cat $(id)`, ``cat `id` ``, `cat <(id)`, `ls $HOME` | ask | substitution or `$` |
| `git -c core.pager=sh --no-pager log`, `git -c alias.status='!sh' status`, `git -C / status` | ask | option before the subcommand |
| `git --no-pager diff --output=x`, `git --no-pager log -n 5 --output=x`, `git --no-pager diff --ext-diff` | ask | writes a file or runs a driver |
| `find . -exec rm {} \;`, `find . -delete` | ask | `find` not listed |
| `rg --pre sh x`, `rg -z x`, `rg -f x y` | ask | flag not listed |
| `tail -f app.log`, `tail -F app.log` | ask | never ends |
| `sed -i s/a/b/ f`, `awk 'BEGIN{system("id")}'`, `xargs rm`, `env sh`, `tee f` | ask | program not listed |
| `ls` + newline + `rm x` | ask | newline |
| `ls&`, `ls & rm x` | ask | background `&` |
| `cat *.txt`, `ls a?`, `cat [a]` | ask | glob |
| `FOO=1 ls`, `\ls`, `"ls"`, `./ls`, `~/bin/ls` | ask | env assignment, backslash, quoted or path program |
| `ls` + NBSP + `a`, `ls` + U+202E, `ls` + U+200B | ask | non-ASCII whitespace or format character |
| `cat ~/.ssh/id_rsa`, `cat .env` | ask | secret path |
| `./evil --version`, `a.b --version`, `evil --version x` | ask | not a bare name with one argument |

#### Scenario: Table examples [auto]
- GIVEN each row of the table above
- WHEN the classifier decides
- THEN rows marked read-only classify read-only and every `ask` row classifies ask

#### Scenario: Known bypasses all classify as ask [auto]
- GIVEN every `ask` row of the bypass table, plus each of `; & | < > ( ) { } $ \` \ ! * ? [ ] #`, `\n`, `\r`, `\t`, ESC, U+0085, U+200B, U+200D, U+202E, U+FEFF and U+00A0 appended to the word of a read-only command
- WHEN the classifier decides on each
- THEN each classifies ask

#### Scenario: One command only [auto]
- GIVEN `ls | wc -l`, `ls | sort`, `ls; ls` and `ls && ls`
- WHEN the classifier decides
- THEN each is ask (one command per call, no pipes and no chains)

#### Scenario: Unknown program asks [auto]
- GIVEN `jq . data.json`, `python3 x.py`, `make`
- WHEN the classifier decides
- THEN each is ask

#### Scenario: Purity [auto]
- GIVEN the same command text with different environments, cwds and files on disk
- WHEN the classifier decides
- THEN the decision is the same (path policy may then route a listed command to the prompt, never to a run)

#### Scenario: Limits [auto]
- GIVEN `ls` with 9 words, 5 commands joined with `;`, and a command of more than 4096 bytes
- WHEN the classifier decides
- THEN each is ask

#### Scenario: Ask in the project still asks [auto]
- GIVEN a chat in `run` and `mkdir src/new`, `touch src/new.rs`, `cargo build` inside the project root
- WHEN each is dispatched
- THEN each opens the prompt and nothing is written before `y`

### Requirement: A `y` runs the model's command
In both modes `y` on a model's `run_in_pane` prompt MUST run the command: one write holding the command bytes followed by `\r`, then the settle, capture and report of pane-execution, with no message from the user needed for the chat to continue. The prompt keys MUST read exactly `y run it · n cancel · p next pane` (`RUN_KEYS`). `y` MUST NOT run, and MUST instead type the command without Enter, in exactly these cases, each with the visible outcome line `sent to pane N; press Enter there to run it (not run: <reason>)`: the policy hook returns not-allowed (see Policy hook for running on `y`), the pane's real cwd cannot be read (macOS), the command is multi-line, a non-ASCII command meets a non-UTF-8 locale, the pane is executing, line pending or dirty, the foreground is not the shell or cannot be read, or the write step refuses it. In each of those cases the prompt keys MUST read exactly `y type it (no Enter) · n cancel · p next pane` (`TYPE_KEYS`) and the reason MUST be shown in the prompt before `y`. A pane whose real cwd cannot be read (macOS has no `proc_cwd`) DOES block `y`: it only types, with the reason `the pane's directory cannot be read on this system` (design D11, `NO_CWD`; owner decision 2026-11-22, a known limitation documented in both READMEs and shown as a visible line). A `y` run is not counted toward `MAX_AUTO_PER_TURN`.

#### Scenario: y runs in ask [auto]
- GIVEN a chat in `ask` and a pending prompt for `cargo install ripgrep`
- WHEN `y` is pressed
- THEN one write holds the command and one `\r`, and the pane output reaches the model when the run ends

#### Scenario: y runs in run [auto]
- GIVEN a chat in `run` and a pending prompt for `npm install`
- WHEN `y` is pressed
- THEN one write holds the command and one `\r`

#### Scenario: Reasons it only types [auto]
- GIVEN a prompt for a two-line command, one for a pane marked executing, one for a pane whose foreground is `python3`, and one for a non-ASCII command under a non-UTF-8 locale
- WHEN `y` is pressed on each
- THEN each writes the command with no `\r` and ends with `sent to pane N; press Enter there to run it (not run: <reason>)`, and each prompt showed `y type it (no Enter) · n cancel · p next pane` first

#### Scenario: Unknown cwd makes y only type [auto]
- GIVEN a platform where the pane's real cwd cannot be read (macOS) and a prompt for `brew install jq`
- WHEN `y` is pressed
- THEN the prompt showed `y type it (no Enter) · n cancel · p next pane` with the reason `the pane's directory cannot be read on this system`, the write has no `\r`, and the outcome line ends with `(not run: the pane's directory cannot be read on this system)`

#### Scenario: n declines [auto]
- GIVEN a pending prompt
- WHEN `n` is pressed
- THEN the result is `denied` and nothing is written

### Requirement: Policy hook for running on y
Whether `y` may run a model's command MUST be decided by one pure function, `may_run_on_y`, taking the chat's mode, the origin of the call (chat or worker), the conversation's GitHub-text state and a policy input reserved for future restrictions. It MUST return not-allowed, with a short reason, for: a conversation that read GitHub text (reason `auto-run is off: this conversation read GitHub text`), a call that originates from a worker, and any restriction the policy input names. Not-allowed MUST turn the approved call into a typed-only call (see A `y` runs the model's command) and MUST never be silent: the prompt and the outcome line carry the reason. The function MUST be the only place that decides this, so the future changes #657, #658 and #659 can disable or narrow `y` runs by changing the policy input alone. In this change the policy input MUST be empty and MUST NOT be settable from any user file, model, tool result or environment variable. Setting it to forbid all runs MUST turn every approved call into a typed-only call with the reason `commands are not run in this chat (policy)`.

#### Scenario: GitHub text blocks y runs [auto]
- GIVEN a conversation that read GitHub text, in `ask` and in `run`
- WHEN the model calls `run_in_pane` with `brew install jq` and `y` is pressed
- THEN the prompt names `auto-run is off: this conversation read GitHub text`, shows `y type it (no Enter) · n cancel · p next pane`, and the write has no `\r`

#### Scenario: Worker origin never runs on y [auto]
- GIVEN the hook called with a worker origin
- WHEN it decides
- THEN it returns not-allowed

#### Scenario: Kill switch [auto]
- GIVEN the policy input set to forbid all runs (test only)
- WHEN `y` is pressed on any model command in `ask` and in `run`
- THEN each write has no `\r` and the outcome line ends with `(not run: commands are not run in this chat (policy))`

#### Scenario: Default policy allows [auto]
- GIVEN the default empty policy input, a chat origin and a conversation without GitHub text
- WHEN the hook decides
- THEN it returns allowed

### Requirement: Local engines follow the same flow
A chat on a local engine MUST get the same flow as a cloud engine: the prompt, `y` running the command, the settle, the capture and the resume without a user message (owner decision 2026-11-22). The classifier of an unprompted run on a local engine MUST be narrower: only `cd`, `pwd`, `ls`, `tree`, `grep`, `rg` and `find`, under today's grammar and flags (the seven unprompted commands of today; `mkdir` is gone), run unprompted in `run`; every other command, `cat`, `head`, `git status` and the `--version` form included, opens the prompt (design D3, tier `Base`). A local engine MUST NOT get any other unprompted run.

#### Scenario: Local engine y runs [auto]
- GIVEN a chat on a local engine in `ask` and a pending prompt for `brew install jq`
- WHEN `y` is pressed
- THEN one write holds the command and `\r`, and the output reaches the model with no user message

#### Scenario: Local engine unprompted set is narrow [auto]
- GIVEN a chat on a local engine in `run`
- WHEN the model calls `ls -l`, `rg foo src`, `find . -name x`, then `cat README.md`, then `git status`
- THEN the first three run with no prompt and the last two open the prompt

#### Scenario: Cloud engine unprompted set [auto]
- GIVEN the same calls on a cloud engine in `run`
- WHEN they are dispatched
- THEN all three run with no prompt

### Requirement: Model guidance
The model MUST be told that it runs commands itself and MUST NOT be told that the user runs them. The `run_in_pane` tool description MUST be exactly: `Run a shell command in the user's visible terminal pane. The user approves it with one key (a read-only command may run at once) and you receive its output. Never print a command for the user to copy or run: call this tool instead. If a command asks for a password, the user types it in the pane and you never see it.` and MUST be identical in every mode and on every engine. The `tools.md` guidance MUST say the same four things and nothing contradicting them: call the tool for every command; do not write shell blocks for the user to run; a password is typed by the user in the pane and never reaches the model; a long-running command returns after about 30 s with the output so far and keeps running. `RUN_HINT` is not model-facing: it is the UI hint line the user sees (`src/core/ai/chat/transcript.rs`) and MUST read exactly `Ctrl+Space e  type in pane`, because `e` types the command and never presses Enter. Neither text may mention `auto`, typing without Enter, or the user pressing Enter. A fenced shell block the model prints anyway is plain text: nothing runs and no run action is added by this change (the user's own `Prefix e` run action stays).

#### Scenario: Description is exact and mode-independent [auto]
- GIVEN requests built in `ask` and in `run`, on a cloud and a local engine
- WHEN the `run_in_pane` description is read
- THEN it equals the exact text above in each

#### Scenario: Guidance never says the user runs it [auto]
- GIVEN `tools.md`, the tool description and `RUN_HINT` (the UI hint)
- WHEN they are searched for `press Enter`, `type it`, `auto` and `you run`
- THEN none is found, and each says the model calls the tool

#### Scenario: Printed block runs nothing [auto]
- GIVEN a model reply holding a fenced bash block and no tool call
- WHEN the turn ends
- THEN nothing is written to any pane
