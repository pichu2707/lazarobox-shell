# Modal Input Specification (delta: quit names what still runs)

## ADDED Requirements

### Requirement: The quit prompt names what still runs
Opening "Quit? (y/n)" (from `q`, or `w q` / `t c` when they quit) MUST ask the runtime what still runs in the panes. A job is a process group of a pane's session (closed panes included) other than its shell's own group. While the prompt is open and jobs run, the statusline MUST read `Quit? still running: <names> (y/n)`: at most three names, each cut to 24 columns, then `+N more`; when that does not fit, fewer names, then `Quit? N still running (y/n)`, then the plain prompt. An answer that arrives after the prompt closed MUST be ignored. Lazarobox MUST NOT signal those jobs. After lazarobox quits and restores the terminal, the jobs still running MUST be written to stderr, one per line with their process group, after a short settle so jobs ending on their hangup are not named.

#### Scenario: The prompt names the jobs
- GIVEN a pane that started `sleep 77 &` and `npm run dev &`
- WHEN `q` is pressed in PREFIX
- THEN the statusline reads "Quit? still running: sleep 77, npm run dev (y/n)" and `y` emits Quit

#### Scenario: Nothing runs
- GIVEN no job besides the pane shells
- WHEN `q` is pressed in PREFIX
- THEN the statusline reads "Quit? (y/n)"

#### Scenario: Narrow statusline
- GIVEN two jobs and a 40-column terminal
- WHEN `q` is pressed in PREFIX
- THEN the statusline reads "Quit? 2 still running (y/n)"

#### Scenario: exit in the last pane
- GIVEN the only pane started `sleep 77 &`
- WHEN its shell exits and lazarobox quits
- THEN stderr reads "lazarobox-shell: still running from your panes:" and "  sleep 77 (process group <pgid>)", and `sleep 77` keeps running
