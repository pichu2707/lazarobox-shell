# Config Menu Specification (delta: admin policy lock)

## Base of this delta

This delta targets `openspec/specs/config-menu/spec.md` on e480992e, plus the `Local only` row that #668 adds (openspec/changes/local-only-mode/specs/config-menu/spec.md, "Local only row"). It adds one requirement and modifies none: a locked row reuses the existing "disabled row" of the Terms (drawn muted, the cursor can land on it, Enter on it does nothing). With no policy, every existing requirement and snapshot is unchanged. Slice tags: [A1] the mechanism and `Local only`; [A2] the Tools tab rows; [A3] `Orchestrator` and `Updates`; [A4] the picker.

## ADDED Requirements

### Requirement: Locked rows [A1]
A row MUST be locked when the policy bound (see admin-policy) excludes at least one of its values. A locked row MUST show its effective value followed by ` · locked by policy` (for example `on · locked by policy`). When every value but one is excluded, it MUST be drawn as a disabled row; `h`, `l`, Left, Right, Space and Enter on it MUST NOT change the draft and MUST show the footer notice `locked by policy: set by your administrator in {path}`. When two or more values remain (`Approval` with two allowed modes), the row MUST cycle among them only and is drawn enabled. While a locked row is selected, the footer MUST be `locked by policy: set by your administrator in {path}`. A locked row's key MUST NOT be written by `s` unless the user changed it to an allowed value, and every other byte of `config.toml` MUST be kept (see config-persistence). Rows: `Local only` [A1]; `Tools`, the seven family rows and `Approval` [A2]; `Orchestrator` [A3]. The `Updates` row is not a setting and is not locked; its value suffix under the policy is in admin-policy, "Updates and orchestrator" [A3]. In the picker [A4], engines the policy does not allow are not listed (see admin-policy, "Engine allow-list").

#### Scenario: Local only locked [auto, TestBackend + insta]
- GIVEN the policy `[ai]\nlocal_only = true`, the user's file without `local_only`, the Models tab and `Local only` selected
- WHEN the menu is rendered, then Space and `s` are pressed
- THEN the row is muted and reads `on · locked by policy`; Space shows `locked by policy: set by your administrator in /etc/lazarobox/policy.toml` and changes nothing; `s` writes nothing for `ai.local_only`

#### Scenario: Write locked off [auto, TestBackend + insta]
- GIVEN the policy `[ai.tools]\nwrite = false` and the user's `write = true`
- WHEN the Tools tab is rendered
- THEN `Write` is muted and reads `off · locked by policy`

#### Scenario: Approval with two modes [auto]
- GIVEN the policy `approval = ["ask", "auto"]`, the user's `approval = "ask"` and `Approval` selected
- WHEN `l` is pressed twice
- THEN the draft reads `auto · locked by policy`, then `ask · locked by policy`; `run` is never shown

#### Scenario: No policy [auto]
- GIVEN no policy
- WHEN every tab is rendered
- THEN the snapshots are byte-identical to the ones without this requirement
