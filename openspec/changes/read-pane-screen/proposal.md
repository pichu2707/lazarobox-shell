# Proposal: Read Pane Screen (`read_pane` for the chat)

Status: owner decisions final on 2026-11-07 (engram `sdd/read-pane-screen/owner-questions`, #3742; prior approval #3277, read-only sharing, main use case SSH). Facts are in engram `sdd/read-pane-screen/explore` (#3740).

## Intent

The chat sees only folder context. `list_panes` sends number, focus, folder and an allow-listed program, and the README promises no screen content. Over SSH, the files the user cares about are not on this machine, so the screen is the only context. The user wants to ask "what went wrong here?" and have the chat read what is on the pane, while seeing and approving exactly what leaves.

## Decisions (owner, binding)

1. New tool `read_pane` in `Family::Pane`, chat only. ORCH workers and the coordinator never get it.
2. Visible screen by default; an optional argument adds scrollback, capped at about 200 lines / 8 KiB, keeping the tail.
3. Asks on EVERY call, on every engine, local included. No grant, no toggle, and no approval mode (`ask`, `auto`, `run`) answers it.
4. The prompt previews the EXACT text to be sent, after redaction, scrollable.
5. The App freezes the text when the prompt opens; the approved text is the sent text.
6. Pipeline: `memory::redact::redact(&sanitize::clean(..))`, then `label::wrap` with a fresh nonce.
7. Any pane `list_panes` lists may be targeted. Denied, gone pane and empty screen each produce a visible notice.

## Scope

### In Scope
- `read_pane` schema and registration (pane number, optional scrollback lines).
- App-side read on the UI loop at prompt open, frozen and answered like `Answer::Ran`.
- Scrollable exact-preview approval ask.
- Notices: denied, gone, empty, truncated, no scrollback in a full-screen app.
- Served through the lazarobox MCP shim to CLI engines, asking inside lazarobox.
- Golden `tools_list.json` update; README screen-content section rewritten.

### Out of Scope (non-goals)
- Writing to panes, sending keys, or any change to `run_in_pane`.
- Worker or coordinator access; persistent or per-chat grants; a toggle.
- Automatic screen attachment, live streaming, or a `/screen` composer command (option C, possible later).
- Stronger redaction rules than `memory::redact`.

## Capabilities

### New Capabilities
None.

### Modified Capabilities
- `agent-tools`: new `read_pane` requirement; default tool list and its pinned fixture gain it; Pane family still excluded from worker toolsets.
- `lazarobox-mcp-server`: Tools served gains `read_pane`.
- `chat-pane`: scrollable exact-preview prompt; the notices above.

## Approach

Build the text in a pure core function from `pane.rs` lines (`read_lines`, scrollback), clamp, then clean, redact and cap. The App calls it when it opens the prompt, keeps the frozen text in the pending ask, and answers with that same text, wrapped. No roster snapshot, so screen text never sits in shared state or goes stale.

## First-slice boundary (stacked-to-main, 400 production lines each)

| PR | Delivers | Est. | Rollback |
|---|---|---|---|
| 1 | Core builder (visible + scrollback clamp, tail cap, clean/redact) and the scrollable preview ask, tested but NOT offered to the model | ~200 | Revert; tool list byte-identical meanwhile |
| 2 | Registration, app read and freeze, answer and wrap, notices, MCP shim, fixture, README | ~250 | Revert; `read_pane` disappears, PR 1 inert |

## Affected Areas

| Area | Impact |
|---|---|
| `src/core/ai/tools/{mod,registry,panes}.rs` | Modified: tool, schema, builder |
| `src/core/ai/chat/approval.rs` | Modified: new ask with frozen text |
| `src/app/tool.rs`, `src/app/run.rs` | Modified: read at prompt, answer |
| `src/runtime/tools/host.rs` | Modified: dispatch |
| `src/core/ai/tools/golden/tools_list.json`, `README.md` | Modified |

## Risks

| Risk | Likelihood | Mitigation |
|---|---|---|
| Secrets on screen (`cat .env`, keys in an editor); bypasses `read_file` path policy | High | Ask every call on every engine; exact redacted preview; user sees what leaves. Redaction is partial; the README says so |
| Prompt injection from screen text | High | `label::wrap` with nonce; text is untrusted data; tool has no side effects |
| Full-screen apps (alternate screen) | Med | Visible grid read; scrollback request yields a notice, not silence |
| Preview differs from sent text | Low | Single frozen value used for both; test pins equality |
| Oversized screens exceed 8 KiB | Med | Tail kept, truncation marked in text and preview |

## Rollback Plan

Each PR is one revert. After PR 2 revert the tool list returns to the current fixture; no config, stored data or migration is involved.

## Dependencies

None external.

## Success Criteria

- [ ] On a cloud and a local engine, `read_pane` asks in every approval mode and shows the exact redacted text, scrollable.
- [ ] Approved text equals sent text, wrapped as untrusted.
- [ ] A secret assignment on screen arrives redacted.
- [ ] Scrollback over 200 lines or 8 KiB keeps the tail and says it was cut.
- [ ] Denied, gone pane, empty screen: each shows a notice.
- [ ] Workers and coordinator never list `read_pane`.
- [ ] `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check` pass on each PR.

## Proposal question round

Answered by the owner (#3742) and confirmed by the orchestrator.

### Settled assumptions (owner, 2026-11-07, binding)

The three assumptions left open above are settled:
- The 8 KiB cap also bounds a large visible screen; the tail is kept and the cut is marked.
- Only panes `list_panes` lists may be read, excluding the chat's own pane.
- A pane that changes or closes after the freeze does not matter: the approved frozen text is sent.

Also settled: no paging and no persistence in this change; the schema stays open to a later `page` argument. Rejected: storing screen text in SQLite as chunks to page through, because it would keep secrets at rest and later pages would leave without the user seeing them (a consent bypass).
