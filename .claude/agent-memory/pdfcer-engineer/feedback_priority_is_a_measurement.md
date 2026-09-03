---
name: priority-is-a-measurement
description: A queue item's priority is a measurement, not a reading — check the target file actually exercises the feature before spending a Pass on it
metadata:
  type: feedback
---

Before working a queued item, **measure that the thing you are optimising
for actually needs it.** A priority inherited from a handoff is a claim, and
claims get verified.

**Why:** on 2026-08-17 the pdfce handoff's suite queue read
*shading patterns → tiling patterns → transparency → overprint*. I shipped
shading patterns, then checked what the operator's suite X-4 file still
needed and found **`pattern_spaces=0` on all six pages** — that file uses no
`scn` patterns at all. The Pass I had just shipped did not move it, and
tiling patterns, queued next, would not have either. Transparency, queued
third, turned out to be **113 ignored blend modes and 36 ignored soft
masks**, with the worst page being one that looked clean by every other
counter.

The check cost thirty seconds. Believing the queue would have cost a whole
Pass aimed at a file that does not use the feature.

**Two things made the error possible, and both generalise:**

1. **The ordering was never measured, only propagated.** It survived three
   handoffs because each one restated it faithfully. Faithful restatement of
   an unverified claim looks exactly like corroboration.
2. **The gap that mattered was UNCOUNTED, so it could not compete.**
   `apply_ext_gstate` silently dropped `/BM` and `/SMask`, so pdfce's own
   diagnostics could not see the largest remaining defect on the file. **A
   gap nothing counts cannot be prioritised** — which is the argument for
   shipping a disclosure before an implementation, not after.

**How to apply:**
- On picking up any queued item: run the target artefact through the tool
  and confirm the relevant counter is non-zero. If there is no counter,
  that absence is itself the finding — add the counter first.
- Distinguish *"this feature is unimplemented"* from *"this feature is
  needed here."* Only the second is a priority.
- State the measurement in the commit and the handoff, so the next session
  inherits a number instead of an ordering.

Related: [[two-modes-one-pattern-is-one-measurement]] and
[[absence-needs-an-unscoped-query]] — the same family, where something that
looked like evidence was actually an unexamined assumption.
