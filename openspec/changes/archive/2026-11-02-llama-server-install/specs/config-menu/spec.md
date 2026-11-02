# Delta for Config Menu (llama-server install)

Baseline note: written against the PENDING `hf-download` change (`openspec/changes/hf-download/`), not the main spec. Archive after `hf-download`. Slice tags: [S1] HF row decoupling, [S4] install/remove entries.

## ADDED Requirements

### Requirement: HF row independent of llama-server [S1]
The `download from Hugging Face…` row on the llamacpp engine MUST be shown whenever the engine is selectable, including when its state is `not installed`, because the download needs only `curl`. When `llama-server` is not installed the row MUST carry a note that `llama-server` is needed to run downloaded models. Nothing else about the download flow changes.
(Previously, in the pending hf-download delta: the row was hidden when llamacpp was `not installed`.)

#### Scenario: Row visible when not installed [auto]
- GIVEN llamacpp `not installed`
- WHEN the llamacpp menu is built
- THEN the HF download row is present with the "llama-server needed to run" note

#### Scenario: No note when installed [auto]
- GIVEN llamacpp `not running` or `running`
- WHEN the menu is built
- THEN the row is present without the note

#### Scenario: Download works without llama-server [auto]
- GIVEN llamacpp `not installed` and `curl` available
- WHEN the user starts the HF download
- THEN the flow proceeds exactly as with llama-server present

#### Scenario: Row still hidden without curl [auto]
- GIVEN `curl` missing
- WHEN the menu is built
- THEN the row behaves as defined by hf-download for the missing-curl case

### Requirement: Install and remove entries [S4]
When llamacpp is `not installed`, the llamacpp row MUST offer `install llama-server…` opening the wizard. When an owned install exists, it MUST offer `remove llama-server`. With a system binary on PATH and no owned install neither entry is shown.

#### Scenario: Install entry when not installed [auto]
- GIVEN llamacpp `not installed`
- WHEN the row menu is built
- THEN `install llama-server…` is listed and opens the wizard

#### Scenario: Remove entry with owned install [auto]
- GIVEN an owned install
- WHEN the row menu is built
- THEN `remove llama-server` is listed

#### Scenario: No entries with system binary only [auto]
- GIVEN a system binary and no owned install
- WHEN the row menu is built
- THEN neither entry appears
