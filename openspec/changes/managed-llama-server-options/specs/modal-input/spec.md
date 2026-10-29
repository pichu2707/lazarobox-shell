# Modal Input Specification (delta: managed llama-server options)

## ADDED Requirements

### Requirement: The quit prompt names a shared llama-server that is answering
When "Quit? (y/n)" opens, lazarobox MUST also ask, in the background, whether the managed llama-server it started is answering a request that is none of its own turns (another lazarobox that uses it: `GET /slots` shows more slots processing than its turns in progress). When it is, the prompt MUST name it after the jobs, once, as `llama-server (shared)`, in the same list and with the same limits as the jobs (see The quit prompt names what still runs), whatever arrives first. A server it only uses (started by another lazarobox or by hand), one that is not running, or one whose `/slots` does not answer, is never named. An answer that arrives after the prompt closed MUST be dropped. Tabs and panes of one lazarobox share its server, so they never count.

#### Scenario: Another lazarobox is being answered [auto]
- GIVEN a pane that started `sleep 77 &`, and the managed server answering another lazarobox
- WHEN `q` is pressed in PREFIX
- THEN the statusline reads "Quit? still running: sleep 77, llama-server (shared) (y/n)" and `y` emits Quit

#### Scenario: Only our own turn [auto]
- GIVEN the managed server answering one request, which is a turn of this lazarobox
- WHEN the runtime is asked
- THEN the server is not named

#### Scenario: Two lazarobox [manual]
- GIVEN two lazarobox, the second using the first one's llama-server, with a reply streaming in the second
- WHEN `q` is pressed in the first
- THEN its prompt names `llama-server (shared)`; with no reply streaming, it does not
