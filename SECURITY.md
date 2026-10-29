# Security policy

## Supported versions

Only the latest release published on [crates.io](https://crates.io/crates/lazarobox-shell)
receives security fixes. Update with `cargo install lazarobox-shell --locked`.
Linux is the only supported platform; macOS is not officially supported in 1.0.

## Reporting a vulnerability

Please do not open a public issue. Report it privately through GitHub's private
vulnerability reporting (security advisories) for
[pichu2707/lazarobox-shell](https://github.com/pichu2707/lazarobox-shell/security/advisories/new):
open the **Security** tab and choose **Report a vulnerability**. Include what
you found, the version you run (the **Updates** row of the menu shows it) and
how to reproduce it.

## Scope

In scope: how lazarobox handles your files and processes, for example escaping
the chat folder or the CLI jail, running a command or writing a file without the
approval it asks for, sending secrets to a model provider, and unsafe handling of
`config.toml`, `chat.db` or the MCP servers. The README section
"Seguridad / modelo de amenazas" indexes the documented risks.

Out of scope: risks the README already documents as accepted (for example, a
command you approve with `y` that reads the project's `.env`), bugs in the AI
CLIs, Ollama, engram or an MCP server you configured, and macOS-only behaviour.
