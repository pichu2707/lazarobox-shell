# Local-Only Mode Specification (delta: admin policy lock)

## Base of this delta

This delta targets the `local-only-mode` capability that #668 adds (openspec/changes/local-only-mode/specs/local-only-mode/spec.md, "The switch is the user's"), once that change is archived into `openspec/specs/local-only-mode/spec.md`. If this change is implemented before #668 is archived, the block below replaces that requirement in #668's spec. It is a full copy where only the sentences about who decides the value change, plus two scenarios. Slice tag: [A1].

## MODIFIED Requirements

### Requirement: The switch is the user's [S1]
The mode MUST be read from `[ai] local_only` (see configuration, The local-only key) and MUST be changed only by the user: by the `Local only` menu row (see config-menu) or by editing the file, **unless the system policy bounds it on (see admin-policy), in which case the mode is on and the user cannot turn it off**. lazarobox MUST NOT turn it on or off by itself, and MUST NOT change any task assignment when it changes. The rest of the code MUST read the mode through one accessor, which MUST return on when the policy or the user's file says so, and a second accessor MUST name the source: `Policy` when the policy bounds it on, else `User`. No refusal text MUST tell the user how to turn the mode off.

#### Scenario: Off by default [auto]
- GIVEN a file without `local_only` and no policy
- WHEN the config is read
- THEN the mode is off and every engine behaves as today

#### Scenario: Assignments are untouched [auto]
- GIVEN `general` is `claude` / `sonnet`, no policy, and the mode is turned on from the menu and saved
- WHEN the file is read back
- THEN `[ai] engine = "claude"` and `model = "sonnet"` are byte-identical and only `local_only = true` was added

#### Scenario: Locked on by the policy [auto]
- GIVEN the policy `[ai]\nlocal_only = true` and the user's `local_only = false`
- WHEN the config is read
- THEN the mode is on, its source is `Policy`, and every rule of this capability applies as when the user turned it on

#### Scenario: The policy cannot turn it off [auto]
- GIVEN the policy `[ai]\nlocal_only = false` and the user's `local_only = true`
- WHEN the config is read
- THEN the mode is on, its source is `User`, and the policy problem says that `ai.local_only = false` locks nothing
