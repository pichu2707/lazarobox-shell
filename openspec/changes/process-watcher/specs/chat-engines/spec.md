# Chat Engines Specification (delta: process watcher)

Tags as in `chat-engines`: [auto] = `cargo test`; [manual] = real machine.

## ADDED Requirements

### Requirement: Nothing lazarobox started outlives it
At start, before any pane or child process exists, lazarobox MUST start a watcher (`lazarobox-shell __watch`) in its own session. Every process group lazarobox guards with `GroupKill` MUST be listed with the watcher with its leader's start time, and taken off when lazarobox reaps it. When lazarobox ends in any way, the watcher MUST send SIGHUP and SIGTERM to every group still listed, SIGKILL after 3 s to the groups that still have members, and exit. It MUST NOT signal a group whose leader is a process other than the listed one, nor groups 0, 1 or its own. `engram serve` MUST NOT be listed. A watcher that cannot start leaves lazarobox working as before.

### Requirement: Quitting is polite
When lazarobox quits (the quit key, SIGTERM or SIGHUP), right after it restores the terminal it MUST send SIGHUP and SIGTERM to every group it started and has not reaped, and SIGKILL only to the groups still listed after 2 s; a group whose leader has ended (a zombie included) MUST get SIGKILL at once for what it left behind. The chats MUST be shut down only after that, inside the existing quit bound, while the store and the memory flush; a turn whose events nobody reads any more (the loop is gone) MUST keep running until then. In the watcher, a group lives while it has a member, so what a leader left behind also gets the grace.

#### Scenario: A program that ends when asked [auto]
- GIVEN a listed group whose leader exits on SIGHUP or SIGTERM
- WHEN lazarobox quits
- THEN it exits on its own, not by SIGKILL, and the quit does not wait the grace

#### Scenario: The chats wait for the polite stop [auto]
- GIVEN an open chat turn
- WHEN lazarobox quits with a polite stop that takes 500 ms
- THEN the turn is stopped at 500 ms, not at once

#### Scenario: A streaming turn is not cut at its next line [auto]
- GIVEN a turn that keeps streaming after the loop stopped reading
- WHEN lazarobox quits with a polite stop that takes 500 ms
- THEN the turn is stopped at 500 ms, not at its next event

#### Scenario: The watcher gives a leader's children the grace [auto]
- GIVEN a listed group whose leader exits on SIGTERM and whose child needs 300 ms to end
- WHEN the watcher's input ends
- THEN the child ends on its own

#### Scenario: A crash leaves nothing behind [auto]
- GIVEN a listed group with a child of its own
- WHEN the watcher's input ends
- THEN the leader ends by SIGHUP and the child is gone

#### Scenario: A process that ignores the polite signals [auto]
- GIVEN a listed group that ignores SIGHUP and SIGTERM
- WHEN the watcher's input ends
- THEN it is killed with SIGKILL after the grace

#### Scenario: Reaped groups and reused pids are left alone [auto]
- GIVEN a group listed and then taken off, and a group listed with another start time
- WHEN the watcher's input ends
- THEN both keep running

#### Scenario: SIGKILL of lazarobox [manual]
- GIVEN lazarobox with a child running in its own group
- WHEN lazarobox gets SIGKILL
- THEN the child is gone within the grace and the watcher has exited
