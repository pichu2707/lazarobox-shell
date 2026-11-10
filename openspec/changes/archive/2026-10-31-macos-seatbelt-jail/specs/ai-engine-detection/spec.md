# Delta for ai-engine-detection

Slice tags: S1-S4 as in chat-engines.

## ADDED Requirements

### Requirement: Availability copy per platform (S1 reasons, S4 copy)
Detection MUST keep the refusal reason by platform. On Linux the existing copy is unchanged. On macOS, when `codex` or `gemini` is refused because Seatbelt is unavailable (`sandbox-exec` missing, probe failed) the text MUST say so and MUST NOT say "needs Linux"; before S4 enablement the macOS text MUST NOT claim the CLI is usable. From S4, with an Available probe, `codex` and `gemini` on macOS MUST be `available`. `claude` availability on macOS MUST NOT depend on the jail.

#### Scenario: Seatbelt unavailable [auto]
- GIVEN macOS, `codex` installed and a probe that is Unavailable
- WHEN detection runs
- THEN codex is not usable, the copy names the missing Seatbelt enforcement, does not say "needs Linux", and no codex process is started

#### Scenario: Available after S4 [auto]
- GIVEN macOS, an Available probe, S4 applied, `codex` and `gemini` installed
- WHEN detection runs
- THEN both are `available`

#### Scenario: claude independent of jail [auto]
- GIVEN macOS, an Unavailable probe and `claude` installed
- WHEN detection runs
- THEN claude is `available`

#### Scenario: Linux copy unchanged [auto]
- GIVEN Linux and the existing jail states
- WHEN detection runs
- THEN statuses and copy equal the current ones

### Requirement: Codex model listing under Seatbelt (S3 mechanism, S4 on)
On macOS the `codex debug models` listing MUST run through the same helper path as a chat turn, jailed by Seatbelt, with the same layout, scrubbed environment, timeout and output cap as on Linux. When the jail is unavailable, or the exit is a jail error (including 71), no process runs bare and codex keeps just `default`.

#### Scenario: Listing runs under Seatbelt [auto]
- GIVEN macOS, a fake `codex` and a recording pass-through helper
- WHEN the listing runs
- THEN the helper is called as `__jail-exec` with the codex state folder read-write and `codex debug models` after `--`

#### Scenario: No Seatbelt, no listing [auto]
- GIVEN macOS and an Unavailable probe
- WHEN codex is detected
- THEN codex is not started for the listing and keeps just `default`
