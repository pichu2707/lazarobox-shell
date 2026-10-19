# macOS tester checklist (codex)

For the tester of issue #180. Tester machine: macOS 27.0.1, Apple M4, codex 0.153.4.
This is outside CI coverage, so every step below is manual. Takes about 15 minutes.

What is being checked: codex runs **inside the macOS jail** (`sandbox-exec`) and
completes a turn with network. gemini is not part of this check.

## 1. Before you start

Run and paste the output of:

```sh
which codex node
codex --version
sw_vers
```

Then tell us **how codex was installed**: `npm i -g @openai/codex`, `brew install --cask codex`, or other.

Make sure `codex login` already worked in a normal terminal.

## 2. Run a codex turn

1. Start lazarobox from a build that has the macOS jail.
2. Open the AI menu, set `chat` to `codex`, and check the row says `available`.
3. In the chat, send: `Say hello in one sentence.`

Expected: a short text answer.

## 3. Run a read-tool turn

Needs `[ai.tools] enabled`.

1. Send: `List the files in the current folder using your tools.`
2. Approve the call when lazarobox asks (`y`).

Expected: a text answer built from the tool result. The CLI must not run anything by itself.

## 4. What a refusal looks like

If codex does not run, one of these appears in the chat. Copy it as it is.

| Text | Meaning |
| --- | --- |
| `codex: not available on this system yet: the macOS jail is unavailable: <reason>` | The startup check of the jail failed. Nothing was started. |
| `the macOS sandbox could not be applied` | `sandbox-exec` exited with 71. The jail was not in place. |
| `the CLI did not start: its jail could not be enforced: <reason>` | The layout was refused (for example a folder that is too broad). |

Also open the menu engine list and copy the row of codex (press `y` on it for the full reason).

## 5. Collect the sandbox log during the turn

Open a **second terminal** before step 2 and leave it running:

```sh
log stream --style compact --predicate 'sender == "Sandbox"' > ~/sandbox-log.txt
```

Send the turn of step 2, then stop the log with `Ctrl+C`.
If the turn failed, the lines with `deny` name what the jail blocked.

## 6. What to paste back

- [ ] Output of step 1 (`which codex node`, versions, `sw_vers`) and how codex was installed.
- [ ] Did the codex turn answer? (yes / no) Same for the read-tool turn.
- [ ] The exact refusal or error text, if any (step 4).
- [ ] `~/sandbox-log.txt`: only the `deny` lines are enough. Remove anything private first.
- [ ] Anything odd: slow start, a pop-up from macOS, a login prompt.

Do not paste tokens, `auth.json` or your home folder listing.
