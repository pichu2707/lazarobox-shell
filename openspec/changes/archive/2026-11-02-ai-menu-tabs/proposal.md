# Proposal: AI menu tabs (#429)

## Intent

The AI tab has 19 flat rows in a 25-row popup. Models sit far down the list. MCP connectors and ai-cost `Usage (alpha)` will add more. Every new row touches about 8 match sites. Split the menu into five tabs and declare each row in one place.

## Scope

### In Scope
- Tabs: `Settings │ Models │ Tools │ Connectors │ Chat`.
  - Settings: Statusline, Tab bar, Mouse, Notify, Sound, llama-server idle, Updates, Feedback. `Usage (alpha)` lands here later.
  - Models: Set all tasks, the six tasks, Hugging Face.
  - Tools: Tools, Approval, the family switches, Orchestrator gate.
  - Chat: Profile, Context, Memory.
- Connectors is hidden until it has rows (MCP).
- `1`–`5` jump keys. The footer hint is `⇥ Tab · 1-5`.
- Each draft row is declared in one place: approach A, an `AiChoice` that generalizes `Choice`.
- One `SubView` dispatch so Usage and MCP sub-views are declared once.
- Docs: orchestrator `manual-tests.md` (T02 and the AI-tab steps), `docs/assets/tapes/ai-menu.tape`, the README menu section, CHANGELOG.

### Out of Scope
- MCP rows (agent-tools 4a) and the Usage row (ai-cost 2b).
- New settings, and any `config.toml` key or write-order change.
- Popup width (62).

## Capabilities

### New Capabilities
- None

### Modified Capabilities
- `config-menu`: tab table, row placement, navigation, jump keys, footer hint, popup height, tiny-terminal thresholds.

**Spec baseline.** The main spec trails the code, and the in-flight deltas disagree with each other. For example, agent-tools lists six Settings rows and omits `llama-server idle`. The delta restates each MODIFIED requirement in full against the merged state of agent-tools, coding-tools, agent-orchestrator, auto-execute, hf-download, feedback, llama-server-install and model-marks. The code on the integrated tree is the tiebreaker. **Archive order:** ai-menu-tabs archives after all of them. If it archives earlier, a later merge reintroduces the `AI` tab.

## Approach

Exploration approach A. The `TABS` table already drives navigation and height, so the split is table entries plus `TAB_COUNT`. Behavioural rows keep a small enum: Assign, Memory, Updates, HfAccount, Feedback, Usage.

| PR | Content | Prod lines |
|----|---------|-----------|
| 1 | Tab split, `1`–`5`, hidden Connectors, docs | ≤ ~400 |
| 2 | `AiChoice` descriptor for draft rows. A characterization test pins the `ConfigEdit` output before the refactor. | ≤ ~400 |
| 3 | Second-key hook (Memory warning, Orchestrator), single `SubView` dispatch | ≤ ~400 |

Tests and snapshots do not count toward these limits. PR 1 and PR 2 land before the ai-cost 2b menu row.

## Affected Areas

| Area | Impact |
|------|--------|
| `src/core/menu.rs` | Modified: `TABS`, `ItemKind`, `on_key`, `edits`, footer |
| `src/core/ai/menu.rs` | Modified: triples become descriptors |
| `src/ui/components/menu.rs` | Modified: `tab_line`, hint |
| `src/core/config.rs` | Read: edit order pinned |
| `src/**/snapshots/` | About 40 snapshots churn |

## Risks

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Snapshot churn (~40) | High | Review in a dedicated PR 1 commit |
| Muscle memory ("Tab to AI") | Med | `1`–`5`, CHANGELOG, docs |
| The 47-cell bar breaks below ~49 columns | Med | Clip test at 48/49 columns; tiny-terminal scenarios |
| Save-order regression in PR 2 | Med | Characterization test before the refactor |
| Spec baseline drift | Med | Full restatement; archive last |

## Rollback Plan

Revert the PRs in reverse order (3, 2, 1). Each one stands alone. No config or data migration is involved: the keys and the write order are unchanged.

## Dependencies

- Integrated tree: main, agent-orchestrator, model-marks, terminal-agnostic.
- Must land before ai-cost 2b.

## Success Criteria

- [ ] Five tabs in the order above. Connectors is absent with no MCP rows.
- [ ] Models is reachable with one Tab or with `2`.
- [ ] Popup about 17 rows high (was 25), with no clipping on a normal terminal.
- [ ] The `ConfigEdit` output is identical before and after PR 2.
- [ ] Adding a draft row touches one declaration.
- [ ] No panic at any size, 0x0 included.

## Proposal question round (owner review)

The owner premises answer round one. These assumptions are still open:
1. Updates and Feedback stay in Settings.
2. Digits stay bound to fixed tab positions. `4` shows a visible notice while Connectors is hidden; it does not shift Chat to `4`.
3. With no AI section, only Settings exists. `2`–`5` show a notice instead of failing silently.
4. The Hugging Face account row moves to Models with the tasks.
