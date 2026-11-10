# Delta for chat-pane

Slice tags: S1-S4 as in chat-engines.

## ADDED Requirements

### Requirement: No-jail guidance on macOS (S1 refusal, S4 copy)
When `codex` or `gemini` is refused on macOS because Seatbelt is unavailable, the pane MUST keep the existing `<engine>: not available on this system yet` guidance, MUST add the macOS reason (Seatbelt not enforcing) in place of any "needs Linux" hint, MUST send nothing and MUST keep the composer text. A jail error (exit 71) MUST show as a jail error line, not as a CLI error. `claude` on macOS MUST never show the no-jail guidance. On Linux the guidance is unchanged.

#### Scenario: No jail on macOS [auto]
- GIVEN macOS, `chat` is `gemini`, gemini is `available` and the Seatbelt probe is Unavailable
- WHEN send is attempted
- THEN the guidance starts with `gemini: not available on this system yet`, names the Seatbelt reason, no process is started and the composer text is kept

#### Scenario: Exit 71 shows a jail error [auto]
- GIVEN macOS and a turn whose helper exits 71
- WHEN the turn ends
- THEN the transcript shows a jail error, not a CLI error

#### Scenario: claude on macOS [auto]
- GIVEN macOS, an Unavailable probe, `chat` is `claude` and claude is `available`
- WHEN send is attempted
- THEN no no-jail guidance is shown and the message is sent

#### Scenario: Linux guidance unchanged [auto]
- GIVEN Linux and an Unavailable jail
- WHEN send is attempted
- THEN the guidance equals the existing `No jail` text
