# Configuration Specification

## Purpose

A user config file read once at startup. For now it only places the statusline and the tab bar. Tags: [auto] = `cargo test`; [manual] = real Kitty.

## ADDED Requirements

### Requirement: File location
The config MUST be read from `$XDG_CONFIG_HOME/lazarobox/config.toml`. When `XDG_CONFIG_HOME` is unset, empty or relative, it MUST be read from `$HOME/.config/lazarobox/config.toml`. The file MUST be read once at startup; there is no hot reload.

#### Scenario: Path resolution [auto]
- GIVEN `XDG_CONFIG_HOME=/x` and `HOME=/h`
- WHEN the path is resolved
- THEN it is `/x/lazarobox/config.toml`; without a usable XDG value it is `/h/.config/lazarobox/config.toml`; without either there is no path

### Requirement: Schema and defaults
The file is TOML with `[statusline] position` and `[tabbar] position`, each `"top"` or `"bottom"`, and a top-level `mouse` boolean. Defaults: statusline `"bottom"`, tabbar `"top"`, `mouse = false`. Any key may be omitted. Unknown keys and tables MUST be ignored. All four position combinations are valid.

#### Scenario: Defaults [auto]
- GIVEN an empty file
- WHEN it is parsed
- THEN statusline is bottom, tabbar is top and mouse is false

#### Scenario: Each value [auto]
- GIVEN a file that sets one position
- WHEN it is parsed
- THEN that position changes and the other keeps its default

#### Scenario: Unknown keys ignored [auto]
- GIVEN a file with unknown keys and tables next to valid ones
- WHEN it is parsed
- THEN the valid keys apply and no error is raised

### Requirement: Fallback and startup notice
A missing file MUST silently give the defaults. An unreadable file or unparseable TOML (a syntax error) MUST give all the defaults and a startup notice `config: <short error>` (one line) shown through the existing statusline notice until the next key.

An invalid value or type MUST fall back to the default for THAT KEY ONLY; every other key in the file is respected. The notice names the failing key, for example `config: statusline.position: expected "top" or "bottom"`; when several keys fail it reports the first and appends ` (+N more)`. A non-table section (`statusline = 3`) reports `statusline: expected a table`, and a non-boolean `mouse` reports `mouse: expected true or false`.

`mouse = true` MUST be accepted with no effect and the notice `config: mouse is not supported yet` (a key problem notice wins over it).

#### Scenario: Missing file [auto]
- GIVEN no config file
- WHEN the app starts
- THEN the defaults apply and no notice shows

#### Scenario: Invalid value falls back per key [auto]
- GIVEN `[statusline] position = "middle"` and `[tabbar] position = "bottom"` and `mouse = true`
- WHEN the app starts
- THEN the statusline is at the default (bottom), the tab bar is at the bottom, mouse is true, and the notice is `config: statusline.position: expected "top" or "bottom"`

#### Scenario: Several invalid keys [auto]
- GIVEN invalid values for `mouse`, `statusline.position` and `tabbar.position`
- WHEN the app starts
- THEN all three use their defaults and the notice reports the first key followed by ` (+2 more)`

#### Scenario: Unparseable file [auto]
- GIVEN a file with a TOML syntax error
- WHEN the app starts
- THEN all defaults apply and the notice is `config: <short parse error>`

#### Scenario: Mouse reserved [auto]
- GIVEN `mouse = true`
- WHEN the app starts
- THEN it is accepted and the notice is `config: mouse is not supported yet`

#### Scenario: Positions in Kitty [manual]
- GIVEN each of the four combinations in `~/.config/lazarobox/config.toml`
- WHEN the app runs with one and with three tabs
- THEN the bars sit where configured and nvim fills the rest
