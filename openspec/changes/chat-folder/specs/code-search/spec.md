# Delta for Code Search

Dependency: written against the unarchived change `coding-tools`; this change MUST be archived after it. The general rule (root = current chat folder) is in agent-tools.

## ADDED Requirements

### Requirement: Search root follows the chat folder
`glob`, `grep` and `read_file` MUST resolve relative patterns and paths against the chat folder at the moment of the call. After a folder change, results from the old folder MUST NOT appear, and the staged view (empty at the time of a change, since a change is refused while writes are staged) applies to the new folder only.

#### Scenario: Glob in the new folder [auto]
- GIVEN `/w/a` with `a.rs`, `/w/b` with `b.rs` and a change from `/w/a` to `/w/b`
- WHEN `glob` is called with `*.rs`
- THEN only `b.rs` is returned

#### Scenario: Grep does not leak the old folder [auto]
- GIVEN a term present only in `/w/a` and a change to `/w/b`
- WHEN `grep` is called for it
- THEN no match is returned
