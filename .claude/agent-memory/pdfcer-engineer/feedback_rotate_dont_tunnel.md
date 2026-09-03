---
name: rotate-dont-tunnel
description: Ken 2026-08-31 — when several independent defects are queued, rotate between them or work them in parallel; never sink hours into one and deliver nothing
metadata:
  type: feedback
---

When a queue of **independent** defects exists, work them **in rotation or in
parallel** — attempt one, move to the next, come back. Do not sink the session
into a single defect.

**Why:** Ken, 2026-08-31, on the colour-conformance work, verbatim: *"cycle
through each colour section after each attempt to fix or do in parallel as I
don't want none to be fixed after many hours because you got stuck trying to fix
one of them."* The failure he is guarding against is **zero delivered progress
after a long session** — not slowness. One unfixed defect out of five is a
result; five unfixed because all the time went into the first is not.

**How to apply:**
- On a queue of independent defects, **dispatch read-only diagnostic agents in
  parallel**, one per defect, then apply fixes serially (writes to the same file
  must not be parallel).
- Cap the effort on any single one. When an attempt fails, **record what was
  ruled out and move on** — a refuted hypothesis is real progress and belongs in
  the record (see [[a-rising-failure-count-measure-the-oracle]] on ablation).
- Prefer **breadth-first**: get every defect to "root cause named" before taking
  any to "fixed".
- This does **not** license shallow work. It licenses *stopping* on one and
  *returning* to it, which is different from doing each badly.

Related: [[fan-out-agents-in-parallel]] (the mechanism), and
[[the-unreported-route-was-the-whole-fix]] (why the first hypothesis is often
wrong, which is precisely why tunnelling on it is expensive).
