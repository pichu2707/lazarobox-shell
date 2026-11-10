# Delta for Chat Engines (owned llama-server spawn)

Slice tag: [S3]. Baseline: main spec plus pending llamacpp-tools deltas; nothing there changes.

## ADDED Requirements

### Requirement: Managed spawn resolves the owned binary
When llamacpp is managed and no system `llama-server` is on PATH, the managed spawn MUST run the owned binary by absolute path. A system binary MUST win. The upstream build resolves its shared library through `$ORIGIN`, so no library environment variable MUST be set; if a verified build lacks an rpath, the library path MAY be set for that child process only and MUST NOT leak to other processes or the user's shell.

#### Scenario: Owned binary spawned [auto]
- GIVEN managed llamacpp, no system binary, an owned install
- WHEN the server is started
- THEN the spawned program is the absolute owned `llama-server` path

#### Scenario: System binary preferred [auto]
- GIVEN a system binary and an owned install
- WHEN the server is started
- THEN the system binary is spawned

#### Scenario: No global environment leak [auto]
- GIVEN an owned install
- WHEN the server is spawned
- THEN no library path variable is set in the app's own environment
