# Design: WSL Drive Access

## Technical Approach

One new piece of state feeds the existing gate: a `WinDrives` snapshot (the drvfs mount points and the user's own Windows profile), built once at startup under WSL and carried by `SecretRoots` next to the `$HOME` roots. `SecretRoots::judge_real` (`src/core/ai/tools/paths.rs:368`) gains one branch: a real path under a drive mount is judged by the Windows deny list instead of the outside-`$HOME` rule. Everything that already goes through that gate (reads, `list_dir`/`grep`/`glob`/`tree` filtering, the chat-folder `judge` in `src/core/ai/chat/folder.rs:149`) follows with no change of its own.

Writes, `run_command` and git ask one more question, through a new `SecretRoots::zone(real) -> Zone { Home, Drive, Outside }`: a root in `Zone::Drive` is accepted only when it equals the chat's grant. The grant is one in-memory field of the chat, set only by the key handler of the grant prompt.

Delivery is three PRs, each releasable alone (see tasks.md).

## Architecture Decisions

| # | Topic | Options | Tradeoff | Decision |
|---|---|---|---|---|
| 1 | Finding the drives | (a) any `/mnt/<letter>` · (b) `/proc/mounts` entries | (a) also matches a plain folder or a tmpfs someone mounted there. (b) costs one file read at startup | **(b)**: an entry whose mount point is `/mnt/<one ASCII letter>` and whose type is `drvfs`, or `9p` with `aname=drvfs` in its options. Read once, only when `is_wsl` (`src/core/desktop.rs:79`) is true. A read failure means no drives (fail closed) |
| 2 | The user's own profile | (a) `%USERNAME%` joined to `Users/` · (b) `%USERPROFILE%` translated to a drive path | The profile folder name can differ from the account name (a renamed account, a `name.DOMAIN` folder), so (a) can name the wrong folder. Both cost the same single `cmd.exe` call | **(b)** `cmd.exe /d /c echo %USERPROFILE%`, run with a drive root as cwd (no UNC warning), 2 s cap, in the background at startup. Accepted only when it matches `^[A-Za-z]:\\Users\\<name>$` with `<name>` free of `\`, `/`, `%`, NUL, `.` and `..`; translated to `/mnt/<letter lowercased>/Users/<name>`. This refines the Q1 wording (`%USERNAME%`) without changing its decision. Until the call answers, and when it fails, only `Public` is readable |
| 3 | Letter case | fold everywhere · fold on drives only | drvfs is case-insensitive by default; folding under `$HOME` would change today's verdicts | **Fold on drives only**: every component below `/mnt/<d>` is compared with ASCII case folding. A folder with per-directory case sensitivity on only makes the list refuse more, never less |
| 4 | Where the drive branch sits | before the outside-`$HOME` rule · a second gate | A second gate would let callers forget it | **Inside `judge_real`**, before the outside rule. The `$HOME` secret rules keep running for paths under `$HOME`, so a link from `$HOME` to a drive is judged on both sides as today |
| 5 | Secret names under a profile | reuse the `$HOME` lists relative to the profile · a new list | The same tools write the same folders on Windows (`.ssh`, `.aws`, `.docker`, `.config/gh`) | **Reuse** `~/.ssh`, `~/.gnupg`, `CREDENTIAL_DIRS` and the `.config`/`.local/share` entries relative to each readable profile, plus `AppData` and `NTUSER.DAT*`. The file-name rules (`.env`, histories…) already apply at any depth |
| 6 | Broad drive folders | refuse them · accept as explicit choice | Refusing a profile root would block a user who keeps loose files there | **Broad**, like `$HOME`: a drive root, `<drive>/Users` and a profile root are acceptable only typed in the picker or with `/cd`, never a candidate row, and never offered a grant |
| 7 | Grant storage | chat state in memory · conversation store | Q2 decided: until quit, asked again on resume | **`Chat::drive_grant: Option<PathBuf>`**, the resolved chat folder. Set only from the prompt's `y`. Cleared on every folder change. Not stored, not inherited by a new chat |
| 8 | When the prompt shows | separate prompt after the folder is set · merged into the `/cd` confirmation | A merged prompt needs three answers and does not cover the picker or resume | **Separate prompt**, `Allow writes, commands and git in <folder>? (y/n)`, shown right after the picker accepts, a `/cd` is confirmed or a conversation resumes in a non-broad drive folder. Only an unmodified lowercase `y` grants |
| 9 | `/cd` to the current folder | `already in` · reopen the prompt | Without a way back, a user who pressed `n` must leave and come back | **Reopen**: `/cd <current>` on an ungranted, non-broad drive folder shows the grant prompt. On a granted or non-drive folder it keeps `already in <folder>`. The refusal text points to this |
| 10 | The ungranted refusal | the one path text · a folder-level text | The one text hides whether a path exists. A folder-level text that depends only on the folder hides it too, and tells the model what the user can do | **Folder-level**: `path not available: the chat's folder <folder> is on a Windows drive, where writes, commands and git need your permission; ask the user to run /cd <folder>` for every write, command and git verb |
| 11 | Creating a file on drvfs | `O_EXCL` write in place · `linkat` fallback | `O_EXCL` in place leaves a half-written file visible on a crash. `linkat` keeps the temp-then-publish shape | **`linkat` fallback** in `rename_noreplace` (`src/runtime/tools/write.rs:945`): only on `EINVAL`, `linkat(fd, tmp, fd, target, 0)` then `unlinkat(fd, tmp, 0)`. `EEXIST` maps to `WriteError::Exists`, so the never-replace guarantee holds. A failed `unlinkat` leaves only the temp name, which the existing temp cleanup removes |
| 12 | Jail broad roots | add constants · a dynamic check | Drives are known only at runtime | **Dynamic**: `jail.rs` broad check (`:1056`) also refuses, case-folded, a drive root, `<drive>/Users`, any profile root and any Windows deny folder, when the root is on a drive |

## Data Flow

```
startup (WSL only)
  /proc/mounts ──► WinDrives.mounts ─┐
  cmd.exe %USERPROFILE% ─► WinDrives.own_profile (async, 2 s)
                                     ▼
                         SecretRoots { home roots, WinDrives }
                                     │
  read_file/list_dir/grep ─► judge_real ──► drive branch? ─► deny list (folded)
  chat-folder judge ───────► judge_real          │
  write/run_command/git ──► judge_real + zone(root) == Drive ─► chat.drive_grant == root?
```

## File Changes

| File | PR | Change |
|---|---|---|
| `src/core/ai/tools/paths.rs` | 1 | `WinDrives`, drive branch in `judge_real`, `zone()`, folded deny list |
| `src/core/ai/tools/windows.rs` (new) | 1 | `/proc/mounts` parser, `%USERPROFILE%` parser and translation (pure, unit-tested) |
| `src/runtime/…` startup | 1 | read `/proc/mounts`, spawn the `cmd.exe` probe, hand both to the roots |
| `src/core/ai/chat/folder.rs` | 1 | drive folders acceptable, broad drive folders |
| `docs/` `tools.md` text, README, CHANGELOG | 1-3 | reach sentence and #558 outside-folder line for drives |
| `src/core/ai/chat/*`, `src/app/*` | 2 | `drive_grant`, prompt, `/cd` reopen, clear on change |
| `src/core/ai/tools/paths.rs` `resolve_write` | 2 | drive root accepted with grant |
| `src/runtime/tools/write.rs` | 2 | `linkat` fallback |
| `src/runtime/tools/host/exec.rs` `root_of`, `git.rs` | 3 | drive root accepted with grant |
| `src/core/ai/jail.rs` | 3 | dynamic broad check for drives |

## Testing Strategy

| Layer | What | How |
|---|---|---|
| Unit | mount parsing, profile translation, folded deny list, `zone()` | pure functions on strings and temp trees; WSL simulated by passing `WinDrives` in, never by reading the real `/proc/mounts` |
| Unit | grant state machine | `TestBackend` key tests: `y`, `Y`, `n`, Esc, modified `y`, folder change clears, resume asks, inheritance does not carry |
| Unit | `linkat` fallback | the `rename_noreplace` seam forced to `EINVAL`; target exists → `Exists`; target missing → created with the bytes and no temp left |
| Manual | real drvfs | on a WSL2 PC, a scratch folder under Windows `%TEMP%`: read, create, replace, rollback, `run_command`, `git status`. PR 2 and PR 3 each start with the probe their risk names |

## Open Questions

- Replacing an existing file on drvfs (plain `renameat` over a target) and `st_nlink` on drvfs are unprobed. PR 2 starts with that probe; a failure stops PR 2 and comes back to the user.
- Landlock on 9p: answered by probe 3.1. The jail confines a granted drive folder (`ls`, writes and git work in it; the profile's `AppData`, sibling folders, `$HOME` and case variants of the root are denied), so commands and git have no fallback refusal on drives.
