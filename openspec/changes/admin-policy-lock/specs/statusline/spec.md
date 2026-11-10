# Statusline Specification (delta: admin policy lock)

## Base of this delta

This delta targets `openspec/specs/statusline/spec.md` on e480992e, plus the "Local-only segment" that #668 adds (openspec/changes/local-only-mode/specs/statusline/spec.md:9-25). It adds one requirement on top of that segment and changes nothing when no policy is installed. #658 adds `local only (project) · ` for a project rule (openspec/changes/project-engine-policy/proposal.md, "What the user sees"); the strongest source is named (see admin-policy, Precedence). Slice tag: [A1].

## ADDED Requirements

### Requirement: Policy mark in the local-only segment [A1]
While local-only mode is on because of the policy (see admin-policy), the prefix of the Local-only segment MUST read `local only (policy) · ` instead of `local only · `, in every pane, also when the user's file or a project rule turns it on too. With an untrusted policy file it reads the same. Everything else (drop order, no partial prefix, saved value only) is as in the Local-only segment.

#### Scenario: Policy and user both on [auto, TestBackend + insta]
- GIVEN the policy `local_only = true`, the user's `local_only = true`, a focused chat on `ollama` / `llama3:8b`, then a focused zsh pane
- WHEN each is rendered on a wide terminal
- THEN the right side reads `local only (policy) · ollama › llama3:8b`, then `local only (policy) · zsh`

#### Scenario: Narrow terminal [auto, TestBackend + insta]
- GIVEN the same, at widths 0 to 120
- WHEN the status bar is rendered
- THEN the memory and usage segments drop first, then the engine or shell text shortens, then the prefix drops whole; nothing panics
