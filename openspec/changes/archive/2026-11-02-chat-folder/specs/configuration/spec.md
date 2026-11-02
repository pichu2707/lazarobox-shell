# Delta for Configuration

Note: the change `coding-tools` also adds a requirement to configuration (family toggle keys); this delta adds a separate one.

## ADDED Requirements

### Requirement: Workspace key
The file MAY hold a table `[workspace]` with `dir`, a string: an absolute path, or a path starting with `~` which expands to an absolute `$HOME`. A missing key and an empty string both mean unset. `dir` is the start folder of the app when no `--dir` flag is given (see start-dir). Invalid values fall back to unset for that key only and report a problem under the existing startup-notice rules (`config: <problem>`, or `config (N problems): <first problem>`, the other keys respected): `workspace.dir: expected a string` for a non-string, `workspace.dir: expected an absolute path` for a relative path, and `workspace: expected a table` for a non-table `workspace`. A well-formed path that does not exist or is not a directory at startup is not a parse problem: the start dir falls back to the launch cwd and the notice is `config: workspace.dir: folder not found`. Unknown keys under `workspace` MUST be ignored. A valid `dir` never raises a notice. The key is read once at startup; there is no hot reload.

#### Scenario: Default [auto]
- GIVEN a file with no `[workspace]`
- WHEN it is parsed
- THEN `dir` is unset and no notice is produced

#### Scenario: Absolute and tilde [auto]
- GIVEN `dir = "/w/proj"`, and in another file `dir = "~/code"` with `HOME=/h`
- WHEN each is parsed
- THEN the values are `/w/proj` and `/h/code` and no notice is produced

#### Scenario: Empty string is unset [auto]
- GIVEN `dir = ""`
- WHEN it is parsed
- THEN `dir` is unset and no notice is produced

#### Scenario: Relative path [auto]
- GIVEN `dir = "proj"`
- WHEN the app starts
- THEN `dir` is unset and the notice is `config: workspace.dir: expected an absolute path`

#### Scenario: Wrong types [auto]
- GIVEN `dir = 3`, and in another file `workspace = 3`
- WHEN each is parsed
- THEN the notices are `config: workspace.dir: expected a string` and `config: workspace: expected a table`

#### Scenario: Counted with other problems [auto]
- GIVEN `dir = 3` and `[statusline] position = "middle"`
- WHEN the app starts
- THEN the notice starts with `config (2 problems): ` and both keys use their defaults

#### Scenario: Unknown keys ignored [auto]
- GIVEN `[workspace]` with `colour = "red"` next to a valid `dir`
- WHEN it is parsed
- THEN `dir` applies and no notice is produced
