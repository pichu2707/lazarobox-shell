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

- [x] `runs.trail` nullable column, save with COALESCE, read on recovery.
- [x] Trail keyed by run id; steps of a fresh goal's turn wait until an event
      names its run (not the run the pane had before); a run with no worker
      still gets its store row; only the coordinator's turn feeds the trail.
- [x] Steps of a turn that has not named its run are held per turn, so a
      coordinator-only run ending after the next goal keeps its steps and goal.
      Only an end or stop may name an earlier turn's run (the run's end can
      arrive after the next goal: `Done` is sent before `turn_ended`, and
      swapping them would break the Undelegated/Waits notices that wait for
      an idle pane). Limits: a worker-less run that ends after the next turn
      already took steps of its own is misfiled, and held steps of a turn that
      never made a run are dropped, with a notice, two goals later.
- [x] Show the trail on stop (turn cap, timeout, cancel) and on interrupted runs.
- [x] Failed run save says the step trail may be missing; steps of a run that
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
