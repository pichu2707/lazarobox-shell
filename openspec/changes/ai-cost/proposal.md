# Proposal: AI usage and cost visibility

## Intent

Every engine stream already reports usage (tokens, cache, timings; claude also dollars), and every decoder throws it away. Users cannot tell what a turn, a day or an engine costs, nor whether a subscription is being burned. Goal (#440): capture what engines report, keep it, show it honestly, warn passively. Premises are binding (engram `sdd/ai-cost/premises`).

## Scope

### In Scope
- Capture engine-native usage for all 5 engines; one turn = one user send, summed over its requests.
- Persist usage rows: counts and timestamps only, no text; survive chat deletion (link dropped); kept 90 days.
- `/cost` user-only composer command: text view (today, 7 days, per engine/model).
- Per-answer usage line and status-bar usage segment, both ON by default, toggled in the AI menu.
- Per-engine billing mode: `per-token` (dollars = spend) or `subscription` (tokens + dollars labelled "list-price equivalent").
- Thresholds: per turn (tokens), per day (tokens or $); empty = off; passive note, never blocks.
- Charts in `/cost` (sparkline/bars).
- Optional worker/role key on rows for #435.

### Out of Scope
- Price tables or computed dollars; dollars only when the engine reports them.
- OxideGate source (separate change; read-only, never re-routes engines; needs loopback spec amendment).
- Spend report with hints (follow-up).
- Blocking sends or budgets.

## Capabilities

### New Capabilities
- `ai-usage`: TurnUsage model, capture rules, summing, "no data" semantics, billing modes, thresholds, `/cost`, charts.

### Modified Capabilities
- `chat-engines`: stream parsers emit usage; usage parse failure never fails a turn.
- `chat-history`: schema v4 usage table, allowed content, delete unlinks, 90-day retention.
- `chat-pane`: `/cost` command, per-answer line, threshold note.
- `statusline`: usage segment in chat pane segments.
- `config-menu`: AI rows for billing mode, toggles, thresholds.
- `config-persistence` / `configuration`: owned keys.

## Approach

Approach A: optional-field `TurnUsage` reaches the chat loop with the final event (variant vs `Done` payload decided in design); summed per turn; stored off the UI thread in `chat.db` v4 `turn_usage`. Missing = `None`, rendered "no data", never 0.

## Slices (~400 production lines each, tests excluded)

1. Capture + persist + text `/cost` (split 1a/1b if forecast exceeds).
2. Per-answer line + status segment + billing mode + AI menu toggles.
3. Thresholds.
4. Charts.
- Follow-ups: spend report; OxideGate change.

## Affected Areas

| Area | Impact |
|------|--------|
| `src/core/ai/chat/{engine,stream,claude,codex,gemini}.rs` | Modified: usage decode |
| `src/core/ai/chat/mod.rs`, `src/runtime/chat.rs`, `src/app.rs` | Modified: turn summing |
| `src/runtime/chat_store.rs` | Modified: v4 table, prune |
| `src/core/ai/chat/folder.rs` (`parse_command`) | Modified: `/cost` |
| `src/ui` statusline, AI menu, README | Modified |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Subscription users read list price as a bill | High | Label "list-price equivalent"; per-engine mode |
| StreamEvent contract widening touches every match | Med | One optional payload; decided in design |
| Multi-request turns double- or under-count | Med | Sum per send; partial marker when any part lacks data |
| Gemini fail-closed decoder rejects new `stats` shapes | Med | Usage decode is lenient and separate; failure = no data |
| Privacy: rows outlive chats | Med | No text; link nulled on delete; 90 days; README states it |

## Rollback Plan

`git revert` per slice. The v4 table is additive; older builds ignore it; deleting the table loses only usage history. UI pieces are toggles.

## Proposal question round

Premise round done. Open for review: default billing mode per engine (proposed: subscription for claude/codex/gemini CLIs; local engines never show dollars); "day" = local midnight; `/cost` scope all chats (current chat highlighted).

## Success Criteria

- [ ] A claude turn shows tokens and a labelled dollar figure; ollama/llama.cpp show tokens, never dollars.
- [ ] Missing values show "no data", never 0.
- [ ] Usage survives chat delete, pruned after 90 days, contains no text.
- [ ] Thresholds produce a note and never block a send.
