# Config Menu Specification (delta: Ollama downloads)

## ADDED Requirements

### Requirement: Ollama download row
The model level of the `ollama` picker MUST end with a row `download from Ollama…` (key `PickKey::OllamaDownload`, mark Plain). It is shown when Ollama is `available` (its server answered detection) and is enabled then. While a pull runs, the row reads `↓ <percent>% <model>:<tag> · <license title>` instead, and Enter on it asks `cancel the download? (y/n)`. While a Hugging Face download runs, the row stays but Enter on it says `a download is already running`.

#### Scenario: Row only with a running server [auto]
- GIVEN ollama `available`, then ollama `not running`
- WHEN its model level is opened
- THEN the last row is `download from Ollama…` in the first case, and absent in the second

### Requirement: Ollama screens
Enter on `download from Ollama…` opens, in the picker, these levels in turn. Esc on each goes back one level, and no level writes a config edit.
1. **Popular.**
   - Title: `Ollama: most popular for this PC`.
   - Footer: `j/k move · Enter choose · Esc back`, and `asking ollama.com…` while asking.
   - Rows, up to 20: `<name>  <pulls> pulls  <sizes that fit>`, for example `llama3.2  86.3M pulls  1b 3b`.
   - Nothing that fits reads the info row `no popular model fits this PC`.
   - The fallback adds its info row first.
2. **Sizes.**
   - Title: `Ollama: <name>`.
   - Rows: `<name>:<tag>  <exact size>`. A tag too big for this PC ends with ` · too big for this PC` and stays choosable.
   - Enter opens the confirmation.
3. **Confirm.** Info rows, then the footer `y download · n back`:
   - `download <name>:<tag> (<size>)`
   - `license: <title>`, one per license layer, or `license: not stated`
   - `read it: https://ollama.com/library/<name>:<tag>`
   - `to: <Ollama models folder>`, or `to: the Ollama server's models folder`
   - `free: <free space>`, or `free: unknown`

   `y` starts the pull and goes back to the ollama model level. `n` or Esc goes back.

#### Scenario: Pick without typing [auto]
- GIVEN the Popular reply with `llama3.2` (`1b`, `3b`) and 8 GB of RAM
- WHEN Enter, Enter on `llama3.2`, Enter on `llama3.2:3b`, then `y`
- THEN the effects are the library ask, the sizes ask, the license ask and one pull of `llama3.2:3b`, in order, and nothing is pulled before `y`

#### Scenario: Late reply dropped [auto]
- GIVEN the Sizes request in flight
- WHEN Esc is pressed and its reply arrives
- THEN the reply is dropped and the Popular screen stays
