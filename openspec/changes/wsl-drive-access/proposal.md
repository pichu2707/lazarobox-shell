# Proposal: WSL Drive Access

## Intent

Under WSL, a chat whose folder lives on a Windows drive (`/mnt/c/Users/<user>/Documents/<project>`) cannot use its tools: every read, write, command and git call answers `path not available`, because the tools only reach `$HOME`. Many WSL users keep their projects on the Windows side, so for them the chat is blind. #558 (v1.5.0) made the limit visible; this change lifts it, in a controlled way.

**For:** WSL users who work in Windows folders, like the reporter of #550, who wanted the chat to read the documentation and a SQLite file of a project under `/mnt/c`.

**User decision (2026-11-04), a mix:**
- **Reads** in Windows drive folders follow a fixed policy: allowed, except Windows system and secret folders.
- **Writes, `run_command` and git** in a Windows drive folder need an explicit grant from the user for that folder.

## Scope

### In Scope
- **WSL only.** Nothing changes on plain Linux or macOS. A drive is `/mnt/<letter>` mounted as drvfs, read from `/proc/mounts`.
- **Reads by policy:** `read_file`, `list_dir`, `grep`, `glob` and `tree` reach drive folders, with the same file-name rules as today (`.env`, `.netrc`, history files…) and a new Windows deny list.
- **Windows deny list**, compared without letter case because drvfs folders are case-insensitive:
  - On every drive: `Windows`, `Program Files`, `Program Files (x86)`, `ProgramData`, `$Recycle.Bin`, `System Volume Information`, `Recovery`, and the files `pagefile.sys`, `hiberfil.sys`, `swapfile.sys`.
  - Under every Windows profile (`/mnt/<d>/Users/<name>`): `AppData`, `NTUSER.DAT*`, and the secret folders already refused under `$HOME` (`.ssh`, `.gnupg`, the `CREDENTIAL_DIRS` entries…).
- **Chat folder:** a drive folder becomes an acceptable chat folder. A drive root, `/mnt/<d>/Users` and a Windows profile root are **Broad**, like `$HOME` today: accepted only as an explicit choice.
- **The grant:**
  - **When it is asked:** when the user picks a drive folder as the chat folder (picker, `/cd`, resume), a dialog asks whether the chat may also write, run commands and use git there.
  - **What it covers:** the chat folder and its subfolders, and nothing else. It is never offered for a Broad folder or a refused one.
  - **Who can grant it:** only a human keypress grants it. The model cannot open the dialog or answer it, and neither can any text the model read.
  - **Declining:** the chat stays read-only in that folder. A write, command or git call is refused with a text that says how to grant: choose the folder again with `/cd <folder>`. Whether `/cd` to the current folder re-opens the dialog is for the design to settle.
- **Writes on drvfs:** creating a file uses `renameat2(RENAME_NOREPLACE)`, which drvfs rejects (see Evidence). When that call answers `EINVAL`, new files fall back to `linkat` plus `unlink`. That pair keeps the same never-replace guarantee, because `linkat` fails when the target exists.
- **`run_command` and git:** these run in a granted drive folder inside the Landlock jail as today. The jail's broad-root list gains the drive roots, `Users`, profile roots and the Windows system folders, without letter case.
- **Texts:** the outside-folder line from #558, the reach sentence in `tools.md`, the README and the CHANGELOG are reworded for drive folders.

### Out of Scope
- Folders outside `$HOME` on plain Linux or macOS. They are still refused, and the #558 notice stays.
- Custom WSL automount roots (`[automount] root` in `wsl.conf`), network drives, and `\\wsl$` paths.
- Grants that outlive the app (see Q2).
- Orchestra workers: they keep their staged write scope and get no grant. Their reads follow the same read policy, because they share the gate.
- `run_in_pane` and the run-mode auto-run rules, which already allow safe commands outside `$HOME`.
- Git line endings and `core.filemode` on drvfs. They are reported if they show up, not changed.
- The cause of the invented answers in #550, which is still waiting for the reporter's details.

## Approach

All the tools share one path gate, `SecretRoots::judge_real` (`src/core/ai/tools/paths.rs`), plus a second `$HOME` check in `resolve_write`. The change adds one WSL drive branch to that gate:
- **Drive check:** is this path on a drvfs drive, and is it outside the deny list?
- **Grant check:** is it under a granted root? Only writes, exec and git ask this.

The chat-folder classifier uses the same gate, so the picker, `/cd` and the tools stay in step. The grant is one field of the chat's state, set only from the dialog.

Expected delivery is three PRs, each releasable on its own:

| PR | Scope | Estimate (production lines) |
|----|-------|-----------------------------|
| 1 | Read policy on drives, deny list, chat folder accepts drive folders, reworded texts | ~200 |
| 2 | Grant dialog and state, drvfs write fallback, writes in granted folders | ~220 |
| 3 | `run_command` and git in granted folders, jail broad-root list | ~120 |

PR 1 alone answers the core of #550: the chat can read a project on `C:`.

## Capabilities

### Modified Capabilities
- `agent-tools`, Path policy: the WSL drive branch and the Windows deny list.
- `code-write`, Root policy: a granted drive folder is a valid root, and the drvfs create fallback.
- `command-runner`, Refusals before start and Jail profile: granted drive roots and the broader broad-root list.
- `git-tools`, Git root follows the chat folder: drive folders follow the grant.
- `chat-folder`, Terms (Broad), One folder policy, The `/cd` command: drive folders are acceptable, and the grant dialog.

## Risks

- **Landlock on 9p (unverified):** Landlock rules are bound to inodes. If drvfs inodes are not stable, the jail denies access, so `run_command` and git fail closed with a permission error and nothing leaks. PR 3 starts with a probe on a real WSL. If the probe fails, PR 3 refuses commands on drives with a clear text, and reads and writes still ship.
- **Rewriting existing files on drvfs (unverified):** only the no-replace rename was probed. The plain rename used to replace a file must be probed in PR 2 before anything relies on it.
- **Letter case:** a deny list compared with case can be bypassed (`/mnt/c/WINDOWS`). Every comparison on a drive must fold case, and tests must cover it.
- **Wider reach:** a model can now read any non-refused file on the user's drives, including other projects and downloads. That is the reading reach the user chose. Writes stay behind the grant.

## Evidence

- **Where the refusal comes from:** `/mnt` is refused by the outside-`$HOME` rule (`check_path`, `paths.rs:146-151`), not by `SYSTEM_DIRS`, which only judges whether `$HOME` itself is a system folder.
- **Writes have a second gate:** `resolve_write` also strips the `$HOME` prefix (`paths.rs:639`).
- **The jail would let drives through:** its broad-root list matches by exact equality (`src/core/ai/jail.rs:1052-1073`), so today `/mnt/c` would pass as a jail root.
- **No folder grant exists to reuse:** today there are only per-turn approvals (`TurnScope.allowed`), the per-run orchestra `ReadScope`, and the one-shot `/cd` confirmation.
- **Probes on WSL2 Ubuntu 24.04**, in a scratch folder under Windows `%TEMP%` on `/mnt/c` (`9p`, `aname=drvfs`), deleted afterwards:
  - `renameat2(RENAME_NOREPLACE)` → `EINVAL`
  - `openat2(RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS)` → ok, and the read returns the bytes
  - `linkat` → ok
  - `open(O_CREAT | O_EXCL)` → ok

## Open Questions

- **Q1. Other Windows users' profiles.** Should reads reach `/mnt/c/Users/<another user>`? **Recommended: no.** Only the profile of the WSL user's Windows account, plus `Public`. Finding that account costs one call to Windows at startup (`cmd.exe /c echo %USERNAME%`). If the call fails, all profiles are refused except `Public`.
- **Q2. How long a grant lasts.** **Recommended:** until the app quits, asked again on resume, the same way an explicit `$HOME` is today. This needs no new stored state. The alternative is to store the grant with the conversation, which avoids the repeated question but adds a stored permission and its revocation.
