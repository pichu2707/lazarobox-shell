# Configuration Specification (delta: project engine policy)

## Base of this delta

This delta targets `openspec/specs/configuration/spec.md` after the change `local-only-mode` is archived (it adds "The local-only key"). It adds one array of tables under `[ai]`, an owned table (see Unknown keys in owned tables), so the rule "every key that lazarobox reads MUST NOT be reported" covers it once it is read; no requirement needs a full MODIFIED copy. Slice tag: [P1]. The repository file `.lzr/policy.toml` is not part of the user's configuration; it is specified in project-engine-policy.

## ADDED Requirements

### Requirement: Project entries
The file MAY hold an array of tables `[[ai.projects]]`. Each entry has `root`, a string: an absolute path, or a path starting with `~` which expands to an absolute `$HOME` (as `[workspace] dir`); and `local_only`, a boolean, default `false` (see project-engine-policy). An entry whose `root` is missing, not a string, or not absolute after expansion MUST be skipped and reported as `ai.projects[N].root: expected an absolute path`, the other entries kept: an entry whose place is unknown cannot be applied anywhere. A `local_only` that is not a boolean MUST count as `true` (fail closed) and MUST produce the problem `ai.projects[N].local_only: expected true or false (local only is on for this project until this is fixed)`; the entry is kept. A non-array `projects` is reported as `ai.projects: expected an array of tables` and gives no entries. Unknown keys inside an entry MUST be ignored, the entry kept, and each one reported as `unknown key: ai.projects[N].<key>`. Two entries may have the same root; neither is reported. Problems go through the rules of Fallback and startup notice. The list is read once at startup (no hot reload). The menu does not write it before P6 of the change; a save that does not change it keeps its bytes. A build that predates it and reads owned tables reports `unknown key: ai.projects` and does not enforce it.

#### Scenario: Default [auto]
- GIVEN a file with no `[[ai.projects]]`
- WHEN it is parsed
- THEN there are no entries and no notice

#### Scenario: Entry parsed [auto]
- GIVEN `HOME=/h` and `[[ai.projects]]\nroot = "~/clients/acme"\nlocal_only = true`
- WHEN it is parsed
- THEN one entry exists with root `/h/clients/acme` (written as `~/clients/acme`) and `local_only` true, and no notice

#### Scenario: Bad root skipped, others kept [auto]
- GIVEN two entries, the first with `root = "rel/p"`, the second valid
- WHEN the app starts
- THEN only the second is kept and the notice is `config: ai.projects[0].root: expected an absolute path`

#### Scenario: Wrong local_only fails closed [auto]
- GIVEN an entry with `root = "/w/acme"` and `local_only = "yes"`
- WHEN it is parsed
- THEN the entry is kept with `local_only` true and the problem is `ai.projects[0].local_only: expected true or false (local only is on for this project until this is fixed)`

#### Scenario: Unknown key in an entry [auto]
- GIVEN an entry with `root = "/w/acme"`, `local_only = true` and `engines = ["ollama"]`
- WHEN it is parsed
- THEN the entry is kept and the problem is `unknown key: ai.projects[0].engines`

#### Scenario: Not an unknown key [auto]
- GIVEN a file that sets every key lazarobox reads, `[[ai.projects]]` with `root` and `local_only` included
- WHEN it is loaded
- THEN no `unknown key` problem is reported
