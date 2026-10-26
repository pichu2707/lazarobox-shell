# Chat Engines Specification (delta: process watcher)

Tags as in `chat-engines`: [auto] = `cargo test`; [manual] = real machine.

## ADDED Requirements

### Requirement: Nothing lazarobox started outlives it
At start, before any pane or child process exists, lazarobox MUST start a watcher (`lazarobox-shell __watch`) in its own session. Every process group lazarobox guards with `GroupKill` MUST be listed with the watcher with its leader's start time, and taken off when lazarobox reaps it. When lazarobox ends in any way, the watcher MUST send SIGHUP and SIGTERM to every group still listed, SIGKILL after 3 s to the groups that still have members, and exit. It MUST NOT signal a group whose leader is a process other than the listed one, nor groups 0, 1 or its own. `engram serve` MUST NOT be listed. A watcher that cannot start leaves lazarobox working as before.

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
