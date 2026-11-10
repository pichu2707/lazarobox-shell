# Delta for config-menu

## MODIFIED Requirements

### Requirement: Ollama screens
Enter on `download from Ollama…` opens, in the picker, these levels in turn. Esc on each goes back one level, and no level writes a config edit.
1. **Popular.**
   - Title: `Ollama: most popular for this PC`.
   - Footer: `j/k move · Enter choose · Esc back`, and `asking ollama.com…` while asking.
   - Rows, up to 20: `<name>  <pulls> pulls  <sizes that fit>`, for example `llama3.2  86.3M pulls  1b 3b`, followed by a tool mark when the rules of Tool marks on Ollama download screens give one.
   - Nothing that fits reads the info row `no popular model fits this PC`.
   - The fallback adds its info row first.
2. **Sizes.**
   - Title: `Ollama: <name>`.
   - Rows: `<name>:<tag>  <exact size>`, followed by a tool mark when the same rules give one. A tag too big for this PC ends with ` · too big for this PC` and stays choosable.
   - Enter opens the confirmation.
3. **Confirm.** Info rows, then the footer `y download · n back`:
   - `download <name>:<tag> (<size>)`
   - `license: <title>`, one per license layer, or `license: not stated`
   - `read it: https://ollama.com/library/<name>:<tag>`
   - `to: <Ollama models folder>`, or `to: the Ollama server's models folder`
   - `free: <free space>`, or `free: unknown`

   `y` starts the pull and goes back to the ollama model level. `n` or Esc goes back. The confirmation carries no tool mark.

#### Scenario: Pick without typing [auto]
- GIVEN the Popular reply with `llama3.2` (`1b`, `3b`) and 8 GB of RAM
- WHEN Enter, Enter on `llama3.2`, Enter on `llama3.2:3b`, then `y`
- THEN the effects are the library ask, the sizes ask, the license ask and one pull of `llama3.2:3b`, in order, and nothing is pulled before `y`

#### Scenario: Late reply dropped [auto]
- GIVEN the Sizes request in flight
- WHEN Esc is pressed and its reply arrives
- THEN the reply is dropped and the Popular screen stays

## ADDED Requirements

### Requirement: Tool marks on Ollama download screens
The Popular and Sizes rows MUST carry the same tool marks as installed models (see ai-task-models, Tool marks on installed Ollama models), taken from the library data: whether the model card declares `tools`, and the parameter size. No request is added for this; the marks use data the screens already read, and a card whose data is missing gets no mark.
- Sizes row: decided by that tag's size. Declared tools and 3B or more reads `🔧`; declared tools and under 3B reads `tools: unreliable`; tools not declared, or size unknown: no mark.
- Popular row: decided by the LARGEST size shown on the row (the largest size that fits this PC). Declared tools and that size is 3B or more reads `🔧`; declared tools and that size is under 3B reads `tools: unreliable`; tools not declared, or no size known: no mark. A row with mixed sizes such as `1b 3b` therefore reads `🔧`.

Exactly 3B counts as 3B or more. The marks use the text fallback `tools` on `TERM=linux` or a non-UTF-8 locale (see ai-task-models, Tool mark text fallback). A mark MUST NOT change the row's selectability, the row order or the pull flow.

#### Scenario: Popular row with tools [auto]
- GIVEN the Popular reply with `llama3.2` (`1b`, `3b`, tools declared) and `smol` (`1b`, tools declared) and `chat-only` (`7b`, no tools)
- WHEN the Popular screen is built
- THEN `llama3.2` carries `🔧`, `smol` carries `tools: unreliable`, and `chat-only` carries no mark

#### Scenario: Popular row with mixed sizes [auto]
- GIVEN the Popular reply with `mix` (sizes `1b 7b`, only `1b` fits this PC, tools declared) and `mix2` (sizes `1b 3b 8b`, all fit, tools declared)
- WHEN the Popular screen is built
- THEN `mix` carries `tools: unreliable` (largest fitting size 1B) and `mix2` carries `🔧` (largest fitting size 8B)

#### Scenario: Sizes rows [auto]
- GIVEN the Sizes screen of `llama3.2` with tools declared, tags `1b` and `3b`
- WHEN it is built
- THEN `llama3.2:1b` carries `tools: unreliable`, `llama3.2:3b` carries `🔧`, and a `too big for this PC` suffix still ends its row where it applies

#### Scenario: Missing card data [auto]
- GIVEN a Popular card with no capabilities data, and the fallback list
- WHEN the screens are built
- THEN the rows carry no tool mark and no row is dropped

#### Scenario: Text fallback on download screens [auto]
- GIVEN `TERM=linux` and the Popular reply above
- WHEN the Popular screen is built
- THEN `llama3.2` carries `tools` in place of `🔧`

#### Scenario: Marks do not change the pull [auto]
- GIVEN marked rows
- WHEN the pick flow of `Pick without typing` is run on a marked row
- THEN the effects and order are the same as for an unmarked row
