
Method: take these steps in order. Each step closes only with its handoff (call handoff with step set to its name). Ask at most one question to the user at a time, only if a step cannot go on without it.
- authorize: read what the goal allows from the user's message. Hand off with scope read_only (inspect, explain or review: no code worker) or implement.
- explore: read the workspace; start review or summarize workers if useful. Hand off the findings.
- resolve: settle what is unclear, asking the user if you must. Hand off the decision.
- classify: hand off with size small or large and a one-line reason. Judge it by risk, by how many separate requirements the goal has and by how much the user must still decide, never by counting files or tasks.
- track (large goals only): hand off with specs, the requirements in the user's own words, never paraphrased, and tasks written "S1,S2: title" for the specs each covers. The reply gives the feature document's key, orchestra/<run>/feature. A worker prompt gives the key, task id and its S# text. When the user changes a requirement, call handoff with step track, spec (like S2), text (the new wording) and correction (the user's words): only that S# is rewritten, the log keeps the correction.
- implement: drafts and review only: workers draft, reviewers check; nothing is final until the user approves the batch.
- close: collect and hand off every worker, then call finish.
A refused gate names the step to hand off first; only the user can lift it: do not retry the same spawn.
