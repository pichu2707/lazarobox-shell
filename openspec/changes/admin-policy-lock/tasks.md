# Tasks: Admin policy lock

Order follows the proposal's slices. Each slice is one PR under about 400 production lines (tests and docs may exceed; the size is an advisory heuristic). Every RED/GREEN task writes the failing test first through the public interface, then the code. No test opens the real policy path, `/etc`, `/Library` or `~/.config/lazarobox/` (admin-policy, Testable without root). Each slice adds its README and CHANGELOG lines. Open questions are decided before the slice that needs them: Q1, Q3, Q4, Q6, Q7 before A1; Q2 is the slice order; Q5 before A6.

Dependencies: A1 needs #668 S1 (the `local_only()` accessor and `local_only_source()`, local-only design D1) and #668 S2 (the `Local only` row). A3 shares `update_check()` with #668 S5, A4 needs #668 S1 and S2, A5 sits on #668 S6's filter point; when one of those has not landed, the slice adds the hook itself. #658 P1 and A1 touch the same input of `availability_for_with`: whichever lands second makes the policy the first input of `LocalRule`.

## 0 README now (optional, docs only, any time)

- [ ] 0.1 Docs: in README "🛡️ Tus datos: qué sale de tu máquina" (README.md:36-51), replace the bold sentence of README.md:38 and the #659 bullet of README.md:50 with the text below, so the README says that today everything is the user's choice and a lock comes only with this goal (owner, 2026-11-10). If #668 S1 ships first, merge this text with its task 1.9.

  Replacement for README.md:38:

  > Pensado para equipos a los que les importa dónde van sus datos. **Hoy todo es elección tuya y nada está bloqueado:** cada ajuste vive en tu propio `config.toml` y lo cambias desde el menú o editando el fichero (el motor de cada tarea, las herramientas, el modo de aprobación, la comprobación de versión y tus servidores MCP). lazarobox no impide usar un motor en la nube si lo eliges. Que un administrador pueda bloquear algunos ajustes es un objetivo planificado ([#659](https://github.com/pichu2707/lazarobox-shell/issues/659)), no algo que exista ya.

  Replacement for README.md:50:

  > - un fichero de política del sistema con el que un administrador podrá bloquear ajustes (por ejemplo, el modo local estricto) para que no se puedan relajar; hasta que llegue, todo lo puedes cambiar tú ([#659](https://github.com/pichu2707/lazarobox-shell/issues/659));

## A1 The file, `local_only`, notices, locked rows (Q1, Q3, Q4, Q6, Q7) — 230-330 lines

- [ ] 1.1 RED/GREEN: `POLICY_PATH` per OS, a string test only (admin-policy, Fixed location; design D1).
- [ ] 1.2 RED/GREEN: pure `judge(facts, root_uid)`: trusted; file or folder with group or other write → untrusted with `{at}`; sticky folder with a root-owned child → trusted; wrong owner → untrusted (admin-policy, Trust check; design D2).
- [ ] 1.3 RED/GREEN: `read_policy(path, root_uid)` at the runtime edge: `O_NONBLOCK | O_NOCTTY`, judged on the handle, at most 64 KiB + 1 read; `ENOENT`/`ENOTDIR` → no policy; FIFO, 65 KiB file, symlink to an untrusted file, `chmod 000` (skipped as root) (tests in a temporary folder with `geteuid()` as the trusted uid). Precedent: src/runtime/mcp_secrets.rs:29-62.
- [ ] 1.4 RED/GREEN: `Policy::parse`: `[ai] local_only`, the policy's own unknown-key walk (fail closed), wrong type (that key strictest), a loosening value ("locks nothing"), the strict baseline as one table (admin-policy, Format; A file that cannot be used fails closed; design D3, D8).
- [ ] 1.5 RED/GREEN: `AiConfig::with_policy`; `local_only()` = policy OR user; `local_only_source()` returns `Policy` (local-only-mode delta, The switch is the user's; design D12). The raw user value is kept.
- [ ] 1.6 RED/GREEN: start-up: the policy read before the user file in `startup` (src/runtime.rs:937-955), with a `policy: Option<&Path>` parameter; policy problems first; `config: {key} = {value} is overridden by the policy in {path}`; the production entry returns "no policy" under `cfg!(test)` (admin-policy, A user value the policy overrides; Testable without root).
- [ ] 1.7 RED/GREEN: the generic locked-row mechanism (design D6) and the `Local only` row: muted, ` · locked by policy`, footer and notice `locked by policy: set by your administrator in {path}`, `s` writes nothing for it (config-menu delta, Locked rows).
- [ ] 1.8 RED/GREEN: status bar `local only (policy) · ` in chat and shell panes, widths 0-120 (statusline delta).
- [ ] 1.9 Characterization: with no policy, every existing test green, menu save byte-identical, statusline and menu snapshots unchanged.
- [ ] 1.10 Docs: README. In "🛡️ Tus datos" (README.md:36-51), replace the text of task 0.1 (or the current README.md:38) with:

  > Pensado para equipos a los que les importa dónde van sus datos. **Por defecto, todo es elección tuya:** cada ajuste vive en tu propio `config.toml` y lo cambias desde el menú. **Bloqueo opcional por un administrador** (desde la versión {X.Y}): si existe `/etc/lazarobox/policy.toml` (en macOS, `/Library/Application Support/lazarobox/policy.toml`), propiedad de root, sus valores marcan un límite: puedes elegir algo más estricto, nunca algo más permisivo. El menú muestra esas filas en gris con «locked by policy» y la ruta del fichero. Sin ese fichero, nada cambia.
  >
  > - **Qué protege:** los cambios desde el menú, desde el chat (`/write`, `/github`, `Prefix a`), editando tu `config.toml`, y los que intente un modelo o un fichero del repositorio.
  > - **Qué no protege:** no es una barrera de seguridad. Quien tenga root, use otro binario (una versión anterior o una compilada por su cuenta) o lance `claude` en un panel de shell, se lo salta. En WSL, un usuario de Windows suele poder entrar como root en su distribución.

  Remove the #659 bullet from "Planificado" (README.md:50). Add a section "### Política del sistema (administradores)" with: the paths per OS; owner root, mode `644`, folder `755`; an example `policy.toml` with `[ai] local_only = true`; "solo puede endurecer"; what happens with a broken file (todo al máximo de restricción, con aviso); "no pongas secretos: lo puede leer cualquiera"; "despliega una clave nueva solo cuando todas las máquinas tengan una versión que la conozca"; read at start only. Add a row to "Seguridad / modelo de amenazas" (README.md:1548-1561): `| Fichero de política | Opcional; lo instala root y limita los ajustes; no frena a root ni a otro binario | [Política del sistema](#política-del-sistema-administradores) |`. CHANGELOG.

## A2 Tools and grants — 200-300 lines

- [ ] 2.1 RED/GREEN: parse `[ai.tools] enabled`, the seven family keys and `approval` (string or array, `ask` always allowed) (admin-policy, Format).
- [ ] 2.2 RED/GREEN: bounds in `tools_enabled`, `toggle`, `families`, `approval` (src/core/ai/config.rs:629-660); a family bounded off is never offered to an engine or a worker.
- [ ] 2.3 RED/GREEN: `/write` and `/github` refused first with the policy reason (`write_blocker`, src/app/write.rs:101-111; src/app/github.rs:27-41) (admin-policy, Runtime grants are refused).
- [ ] 2.4 RED/GREEN: `/exec` refused the same way, if orchestrator-worker-flow PR16 has landed; otherwise add this task to PR16.
- [ ] 2.5 RED/GREEN: `Prefix a` cycles only allowed modes; one allowed mode gives the notice (`toggle_approval`, src/app/tool.rs:507-530) (admin-policy, Approval modes).
- [ ] 2.6 RED/GREEN: no WSL drive grant offered while Write is bounded off (`offer_drive_grant`, src/app/folder.rs:395-414).
- [ ] 2.7 RED/GREEN: Tools-tab rows locked (`Tools`, families, `Approval` with two modes cycling) (config-menu delta).
- [ ] 2.8 Docs: README "Política del sistema" lists the tool keys; CHANGELOG.

## A3 Updates and orchestrator — 80-140 lines

- [ ] 3.1 RED/GREEN: `[updates] check = false` bounds the start check (src/runtime.rs:408-409), `r` (src/core/menu.rs:1564-1566) and the llama-server newer build (src/app/install.rs:12-22); row suffix ` · checks off: policy`; `r` notice (admin-policy, Updates and orchestrator).
- [ ] 3.2 RED/GREEN: `[ai.orchestrator] enabled = false`: `Orchestrator` row locked; Ctrl+O as with the gate off.
- [ ] 3.3 Docs: README "Avisos de actualización" (README.md:450) and "Política del sistema"; CHANGELOG.

## A4 Engine allow-list — 150-230 lines

- [ ] 4.1 RED/GREEN: parse `[ai] engines` (ids of `CATALOG`, src/core/ai/mod.rs:90-105; an unknown id is a wrong value; `[]` refuses all).
- [ ] 4.2 RED/GREEN: refusal in `availability_for_with` (src/core/ai/chat/engine.rs:479-560) next to #668's `LocalOnly`, for every task, with the guidance of design D7; never another engine (admin-policy, Engine allow-list).
- [ ] 4.3 RED/GREEN: picker hides engines not allowed, note row `engines hidden: not allowed by policy`; task-row mark ` · off: policy` (src/core/ai/picker.rs:208-279, 650-656).
- [ ] 4.4 Coordinate with #658: if its `engines` list exists, the allowed set is the intersection.
- [ ] 4.5 Docs: README; CHANGELOG.

## A5 User MCP servers — 80-150 lines

- [ ] 5.1 RED/GREEN: parse `[ai.mcp] user_servers`; `false` keeps every user entry from starting at the chat set start (src/runtime/chat/external.rs:107-150); the notice once per set (admin-policy, User MCP servers).
- [ ] 5.2 RED/GREEN: the Connectors area, if mcp-connectors has shipped, shows `locked by policy` on its install and probe actions.
- [ ] 5.3 Docs: README "Tus propios servidores MCP" (README.md:1266); CHANGELOG.

## A6 `--show-policy` (Q5) — 60-100 lines

- [ ] 6.1 RED/GREEN: the flag beside `--dir` and `--check-prefix` (src/core/start.rs:10, 13); no argument; pure formatter of path, verdict, problems and bounds; status 0; `no policy at {path}` (admin-policy, Show the policy).
- [ ] 6.2 Manual: on a real machine with root, the [manual] scenario of admin-policy, Show the policy.
- [ ] 6.3 Docs: README "Política del sistema" shows how to check a machine; CHANGELOG.

## Later (not specced here)

- Lock keys of design D3, "Later keys": memory, drive writes, the full write grant of `run` mode, downloads, administrator-declared MCP servers, a contact string, #570.
- macOS Managed Preferences and a Windows-side file for WSL (proposal Q1 c, d).
