# Proposal: OSC 8 Links in Panes

Status: shipped in the pull request that adds this file. Refs #796.

## Intent

AI CLIs such as Claude Code and OpenCode print links as OSC 8 hyperlinks when the host terminal supports them (panes inherit `TERM_PROGRAM`, for example `WezTerm`). The text shown is often not the URL, for example an icon such as `⧉`. pane-links (#597) only finds URLs in the text, so these links cannot be opened in a pane. This change makes them links, for the click and for PREFIX `u`.

## Decision

pane-links D2 and D6 said "the target is exactly the text shown", and kept OSC 8 out of scope so that a click never opens a URL the user has not seen. This change replaces them for OSC 8 links only:

- A link whose text is its URL opens at the first click, as before.
- A link whose text is not its URL shows its URL in the notice at the first click (`Link to <url>: click it again to open it`), and opens only when the next click is on that same link. The owner chose this over opening at once, and over listing such links in PREFIX `u` only.
- PREFIX `u` lists the link by its URL, so the user reads the target before choosing it.
- Only `http` and `https` targets are links (`is_link_url`, unchanged). `file://`, `vscode://` and every other scheme start nothing.

## Design

- vt100 does not keep OSC 8; it hands it to `Callbacks::unhandled_osc` with the screen. The pane's callbacks record where each link starts and ends, in stable lines, and the text its cells show when it ends (`src/core/pane/hyperlinks.rs`, `src/core/osc8.rs`).
- A recorded link whose cells now show other text was drawn over (CLIs redraw their lines), and is no link. A link a later one overlaps is forgotten at once.
- Every recorded link is forgotten when line numbers stop being comparable: a resize, a switch to or from the alternate screen, a lost eviction count. The program draws again in those cases.
- A link must be one run of cells: a line break inside it or a cursor jump backwards makes it no link. Bounds: a URI of 8192 bytes or more, more than 32 rows, or a payload the VT parser may have cut (16 params) start nothing; a pane keeps at most 256 links; a new link ends the open one.
- Where an OSC 8 link and a URL found in the text share a cell, the OSC 8 link wins.

## Out of Scope

- OSC 8 links in the chat transcript (the chat sanitizes escape sequences).
- The `id=` parameter that joins separate runs of cells into one link: each run is its own link.
- A link drawn in the same chunk of output that switches screens: it is forgotten with the switch.
