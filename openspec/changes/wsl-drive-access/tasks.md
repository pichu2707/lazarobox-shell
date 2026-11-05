# Tasks: WSL Drive Access

## Review Workload Forecast

| Field | Value |
|-------|-------|
| Estimated changed lines | ~540 production, plus tests and docs |
| 400-line budget risk | Low (no PR above ~220 production lines) |
| Chained PRs recommended | Yes |
| Suggested split | PR 1 -> PR 2 -> PR 3 |
| Delivery strategy | ask-on-risk |
| Chain strategy | stacked-to-main (each PR from main after the previous one merges) |

Decision needed before apply: No
Chained PRs recommended: Yes
Chain strategy: stacked-to-main
400-line budget risk: Low

### Suggested Work Units

| Unit | Goal | PR | Est. (production) | Risk |
|------|------|----|------|------|
| 1 | Read policy on drives, deny list, drive chat folders, texts | PR 1 | ~200 | High (path policy) |
| 2 | Grant prompt and state, drvfs create fallback, writes in granted folders | PR 2 | ~220 | High (write root) |
| 3 | `run_command` and git in granted folders, jail broad check | PR 3 | ~120 | High (sandbox) |

Each PR gets an independent verifier and is released on its own (patch through `release.yml`).

## PR 1: Reads on Windows drives

- [x] 1.1 RED: `/proc/mounts` parser: `drvfs` and `9p aname=drvfs` at `/mnt/c` kept; `/mnt/wsl` tmpfs, `/mnt/cd` and `/mnt/c/x` dropped; unreadable input gives no drives.
- [x] 1.2 RED: `%USERPROFILE%` parser: `C:\Users\javier.lazaro` → `/mnt/c/Users/javier.lazaro`; `\\server\x`, `C:\Users\a\b`, `C:\Users\..`, `%USERPROFILE%` (unexpanded) and empty refused.
- [x] 1.3 RED: drive branch of `judge_real`: a project under the own profile and under `/mnt/d/work` allowed; `Windows`, `Program Files`, `ProgramData`, `$Recycle.Bin`, `pagefile.sys`, `AppData`, `NTUSER.DAT`, `.ssh` under the profile, another user's profile refused, each also in other letter case (`/mnt/c/WINDOWS`); `Public` allowed; no own profile known → only `Public`; not WSL → every drive path refused as today.
- [x] 1.4 RED: `list_dir` on a drive folder omits denied entries; `grep`/`glob` do not descend into them.
- [x] 1.5 RED: chat-folder judge: a drive project folder is acceptable; drive root, `/mnt/c/Users` and the profile root are broad (explicit only, picker again on resume); a deny folder gives `not allowed`.
- [x] 1.6 GREEN: `windows.rs` parsers, `WinDrives` in `SecretRoots`, startup read of `/proc/mounts` and the background `cmd.exe` probe (2 s cap).
- [x] 1.7 Writes, `run_command` and git in a drive folder still refused (regression test), with the one path text.
- [x] 1.8 Docs: `tools.md` reach sentence, the #558 outside-folder line, README WSL section, CHANGELOG `[Unreleased]`.
- [ ] 1.9 Manual (real drvfs read path checked on WSL2 by a throwaway probe: own profile found, README read, `.env` and `C:\Windows`/`C:\WINDOWS` refused; the TUI check is pending): on WSL2, the chat opened in a project under `/mnt/c/Users/<me>/Documents` reads a file and lists the folder; `C:\Windows` is refused.

## PR 2: Grant and writes

- [ ] 2.1 Probe first (manual, WSL2 scratch folder under `%TEMP%`): `renameat` over an existing file, `st_nlink` of a fresh file, `openat2` no-symlink write open. Any failure stops PR 2 and goes back to the user.
- [ ] 2.2 RED: grant prompt after picker accept, `/cd` confirm and resume in a non-broad drive folder; only unmodified lowercase `y` grants; `Y`, `n`, Esc and `Alt+y` leave it read-only; never shown for a broad or home folder.
- [ ] 2.3 RED: grant cleared on folder change; not inherited by a new chat; not stored (resume asks again); `/cd <current>` on an ungranted drive folder reopens the prompt, on a granted one shows `already in`.
- [ ] 2.4 RED: the model cannot reach the prompt: no tool and no message text opens it (a turn whose text is `/cd …` from the model changes nothing).
- [ ] 2.5 RED: ungranted drive folder: every write returns the folder-level text, whatever the path; granted: writes in the folder stage and apply, writes outside it give the one path text.
- [ ] 2.6 RED: `rename_noreplace` seam forced to `EINVAL`: missing target created with the bytes and no temp left; existing target → `Exists`, its bytes unchanged.
- [ ] 2.7 GREEN: `drive_grant` state, prompt, `resolve_write` drive root, `linkat` fallback.
- [ ] 2.8 Docs: README (grant), CHANGELOG.
- [ ] 2.9 Manual: create, edit and roll back a file in a granted drive folder on WSL2.

## PR 3: Commands and git

- [ ] 3.1 Probe first (manual): the Landlock jail with a granted drive folder as root runs `ls` and `git status`, and cannot read the profile's `AppData` or another drive folder. A failure turns PR 3 into the refusal `commands cannot run in a Windows drive folder on this system` for commands and git on drives.
- [ ] 3.2 RED: jail broad check refuses `/mnt/c`, `/mnt/c/Users`, a profile root, `/mnt/c/Windows` and `/mnt/C/windows`-style case variants as roots.
- [ ] 3.3 RED: `run_command` and git in an ungranted drive folder give the folder-level text; in a granted one they run with the folder as cwd.
- [ ] 3.4 GREEN: `root_of` and git root with the grant, dynamic broad check.
- [ ] 3.5 Docs: README, CHANGELOG; tick this file and archive the change after the last PR merges.
