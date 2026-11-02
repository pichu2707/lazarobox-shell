# AI Engine Detection Specification (delta: llamacpp pick model)

Tags as in `ai-engine-detection`: [auto] = `cargo test`; [manual] = real machine.

## ADDED Requirements

### Requirement: Local llama.cpp models
When llama-server is installed (running or not), detection MUST also list the `.gguf` model files in, in this order: `$XDG_DATA_HOME/llama.cpp/models` (default `~/.local/share/llama.cpp/models`); `$LLAMA_CACHE` (default `$XDG_CACHE_HOME/llama.cpp`, then `~/.cache/llama.cpp`); on macOS `~/Library/Caches/llama.cpp`; `$HF_HUB_CACHE`, else `$HF_HOME/hub`, else `~/.cache/huggingface/hub`. Only absolute folders count. A model file ends in `.gguf` (any letter case), is not a vision projector (`mmproj` in its name) and, of a split model `-NNNNN-of-NNNNN`, only part 1 is listed. The walk MUST NOT follow links to folders, MUST stop at 6 levels and 20 000 entries, and accepts links to files (the Hugging Face snapshots). Files are listed once, sorted by file name without letter case.

#### Scenario: A Hugging Face snapshot is listed [auto]
- GIVEN `hub/models--x/snapshots/abc/q.gguf` linking to a blob
- WHEN detection runs with llama-server installed
- THEN the snapshot path is listed

#### Scenario: Nothing is read without llama-server [auto]
- GIVEN llama-server not installed
- WHEN detection runs
- THEN no folder is read and the list is empty
