# Archive Report: google-connect

Archived: 2026-11-13.

## Outcome

Implemented by PR #736 (closes #734), released in v1.25.0: the **Connect Google** row and screen in the Connectors tab set up Search Console and Google Workspace with the user's own Desktop OAuth client, append the catalog entries to `config.toml` (byte-prefix append, `.bak`, re-parse check) and save the values in one `secrets.toml` write.

## Specs

The change ran without delta specs in this folder: PR #736 updated `openspec/specs` directly (`config-persistence`, `configuration`, `mcp-secrets`, `config-menu`), so there is nothing to merge here.

## Follow-up

The proposal's out-of-scope item "a lazarobox-owned OAuth client" continues as `openspec/changes/gsc-lazarobox-client` (issue #733), for Search Console only (`webmasters.readonly` is non-sensitive).
