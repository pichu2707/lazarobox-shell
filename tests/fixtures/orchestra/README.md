# Orchestra failure fixtures

These fixtures are SYNTHETIC: hand-written to pin how each adapter flattens a
429 (rate limit) or 409 (conflict) and how `classify` reads the result. They
are not recordings of a real CLI or server. Recorded failures live in the
per-engine folders (`tests/fixtures/claude/`, `codex/`, `gemini/`).

The gemini 429 words an empty daily quota, so it classifies as Quota.
