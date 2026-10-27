# Delta for AI Engine Detection (owned llama-server)

Slice tag: [S3]. Existing detection behavior for engines already on PATH is unchanged.

## ADDED Requirements

### Requirement: Owned directory on detection PATH
Detection MUST append the owned `llama-server` bin directory as the LAST PATH entry, so any system binary found earlier wins. Without an owned install, PATH and detection MUST be byte-identical to today.

#### Scenario: Owned appended last [auto]
- GIVEN an owned install and a PATH `/usr/bin:/bin`
- WHEN the detection PATH is built
- THEN it is `/usr/bin:/bin:<owned bin>`

#### Scenario: No owned install changes nothing [auto]
- GIVEN no owned install
- WHEN the detection PATH is built
- THEN it equals the process PATH

### Requirement: Runtime PATH refresh and forced re-detection
After an install or remove the system MUST refresh the PATH snapshot used by detection and spawn and MUST force re-detection, bypassing any cache, so the engine state updates in the running session.

#### Scenario: Stale snapshot refreshed [auto]
- GIVEN a PATH snapshot taken before install
- WHEN install completes
- THEN the snapshot includes the owned directory and detection reports the engine

#### Scenario: Forced re-detection bypasses cache [auto]
- GIVEN a cached `not installed` result
- WHEN install or remove completes
- THEN detection runs again and the state reflects the new reality
