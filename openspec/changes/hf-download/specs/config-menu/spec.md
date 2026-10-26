# Config Menu Specification (delta: Hugging Face downloads)

## ADDED Requirements

### Requirement: Hugging Face row in the llamacpp picker
The model level of the `llamacpp` picker MUST end with a row `download from Hugging Face…` (key `PickKey::HfDownload`, mark Plain, always enabled). It is shown whenever llama-server is installed (status not `not installed`), with or without local models. While a download runs, the row reads `↓ <percent>% <file> · <license>` instead, and Enter on it asks `cancel the download? (y/n)`.

#### Scenario: The row is last [auto]
- GIVEN llamacpp stopped with two local models
- WHEN its model rows are built
- THEN the last row is `download from Hugging Face…`, enabled, and the local models come before it

#### Scenario: The row shows a running download [auto]
- GIVEN a download of `a-Q4_K_M.gguf` (license `apache-2.0`) at 42 %
- WHEN the model rows are built
- THEN the last row reads `↓ 42% a-Q4_K_M.gguf · apache-2.0`

### Requirement: Hugging Face screens
Enter on `download from Hugging Face…` opens, in the picker, these levels in turn. Esc on each goes back one level; no level writes a config edit.
1. **Query**: a text field like `custom…`, with the hint `search the Hub or owner/repo`. Enter with text of the form `<owner>/<repo>` (one `/`, both sides non-empty, no space) opens the files of that repo. Any other non-empty text searches. Empty text does nothing. The footer reads `Enter search · Esc back`.
2. **Repos**: up to 20 rows `<owner>/<repo>  <downloads> ↓`, sorted by downloads. No result reads the info row `no GGUF models found for "<text>"`. Enter opens the repo's files.
3. **Files**: one row per GGUF choice of model-download, `<quant>  <size>  <file>` (or `(<n> parts)` after the size of a split model). The first row is the info row `license: <id>`. No choice reads the info row `no .gguf files in <owner>/<repo>`. Enter opens the confirmation.
4. **Confirm**: info rows, then the footer `y download · n back`:
   - `download <file> (<size>)`
   - `from <owner>/<repo>`
   - `license: <id>`
   - `read it: <link>`
   - `to: <hub folder>`
   - `free: <free space>`

   `y` starts the download and returns to the llamacpp model level, whose last row then shows the progress. `n` or Esc goes back to Files. When the free space is short, the footer shows the disk space error and `y` does nothing.

While a request runs, the footer reads `asking Hugging Face…` and keys other than Esc are ignored. A failed request shows its model-download message as the footer error and stays on the level that asked. A reply that arrives after the user left that level is dropped. Every label from the Hub is sanitized like the other picker labels (control and format characters drawn as `?`).

#### Scenario: Paste a repo id [auto]
- GIVEN the Query level with `Qwen/Qwen2.5-0.5B-Instruct-GGUF` typed
- WHEN Enter is pressed
- THEN an effect asks for that repo's files, no search effect is emitted, and the footer reads `asking Hugging Face…`

#### Scenario: Search then pick [auto]
- GIVEN the Query level with `qwen 0.5b` typed
- WHEN Enter is pressed, a reply with two repos arrives, Enter opens the first, its files reply arrives, and Enter is pressed on `Q4_K_M`
- THEN the Confirm level shows the file, its size, `license: apache-2.0`, its link, the hub folder and the free space, and no download effect has been emitted yet

#### Scenario: Nothing downloads before y [auto]
- GIVEN the Confirm level
- WHEN `n`, Esc, `j`, Enter and `s` are pressed
- THEN no download effect is emitted and no config edit exists; only `y` emits it, once

#### Scenario: Error stays on its level [auto]
- GIVEN the Query level with `gated/model` typed
- WHEN the reply is a 401
- THEN the footer error reads `gated/model needs a Hugging Face token: set HF_TOKEN` and the Query level keeps its text

#### Scenario: Late reply dropped [auto]
- GIVEN a files request in flight
- WHEN Esc is pressed and then its reply arrives
- THEN the picker stays on the level Esc went to

### Requirement: Download in the status bar
While a download runs, the status bar MUST show `↓ <percent>%` after the other right-hand marks. When it ends, the status bar shows `downloaded <file>` (or the failure message) as a notice until the next key, and the engines are detected again. A cancel shows `download cancelled · run it again to resume`.

#### Scenario: Progress then done [auto]
- GIVEN a download started from the picker
- WHEN progress 10 %, 55 % and the end arrive
- THEN the status bar reads `↓ 10%`, then `↓ 55%`, then `downloaded <file>`, and a redetect effect is emitted once

### Requirement: Hugging Face account row
The AI tab MUST end with a static row `Hugging Face`, after `Tools` (this modifies "AI rows end with Tools": Tools is now the last row before it). Its value is:
- `not connected` when no token is found;
- `<name>` once an account is known (from a connect in this session, or from a whoami check made when the row is opened);
- `token set (HF_TOKEN)` while `HF_TOKEN` is set.

Enter on the row opens a field with the hint `paste a Hugging Face token` that draws one `•` per character. Enter checks the token, with the footer `asking Hugging Face…`, and Esc goes back. On a known account, Enter offers instead `disconnect <name>? (y/n)`, which deletes the token file on `y`. The row writes no config edit.

#### Scenario: Account row [auto]
- GIVEN the AI tab
- WHEN rendered with no token
- THEN the last row reads `Hugging Face` with `not connected`, after `Tools`

#### Scenario: The field hides the token [auto]
- GIVEN the token field
- WHEN `hf_abc` is pasted
- THEN the field shows `••••••` and no rendered cell holds `hf_abc`
