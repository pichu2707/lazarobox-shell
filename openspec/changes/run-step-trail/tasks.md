# Tasks

Slice A, `feat/run-trail-core` (pure, nothing wired):

- [x] `Trail` type: cap, one line per step, call text and outcome cut apart so
      a long call keeps how it ended, text round trip, `shown` report,
      `absorb` of steps taken before the run was known.
- [x] `scrub`: best-effort masking of JSON members, `name=value`,
      `--name value`, headers, URL query values, curl `-u`, `sshpass -p` and
      `mysql -pVALUE` for secret-looking name segments; quoted values go whole.
      Handoff reasons are scrubbed too.

Slice B, `feat/run-step-trail` (stacked on A):

- [ ] `runs.trail` nullable column, save with COALESCE, read on recovery.
- [ ] Trail keyed by run id; steps of a fresh goal's turn wait until an event
      names its run (not the run the pane had before); a run with no worker
      still gets its store row; only the coordinator's turn feeds the trail.
- [ ] Show the trail on stop (turn cap, timeout, cancel) and on interrupted runs.
- [ ] Failed run save says the step trail may be missing; steps of a run that
      never started are not dropped silently.

Later:

- [ ] Engine error of the coordinator turn with the run still open (the
      ipfing-like case): say the trail is available and show it; the stop
      report only fires on stop, timeout and cancel today. Next task.
- [ ] Show the trail of a run that finished cleanly (a closing line or view).
- [ ] A resumed run (Prefix r r) starts an empty trail: the old run's trail is
      not carried over (the old row keeps it until retention drops it).
- [ ] Worker tool calls: summarise per worker into the trail if diagnosis needs
      more than the handoff lines.
- [ ] Acceptance-judges S2: push judge steps through `Trail::call` / `handoff`.
