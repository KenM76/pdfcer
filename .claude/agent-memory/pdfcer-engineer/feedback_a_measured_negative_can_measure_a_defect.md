---
name: a-measured-negative-can-measure-a-defect
description: A recorded "tried it, 3x worse, refused" can be a measurement of a bug on the route, not of the route; probe the intermediate (ink probe on a vector/image pair) before believing or re-deriving a refusal
metadata:
  type: feedback
---

Before accepting a prior session's measured negative about a ROUTE, probe
the route's intermediate on one pixel. A refusal recorded with real numbers
can be measuring a defect that sat on the path being tried.

**Why:** Pass 214.0 (2026-09-01) tried managing ICCBased RGB images onto the
ink path, measured two patches 3x and 1.8x worse, and recorded a refusal in
its commit, in FEATURES.md and in NEXT_SESSION §D as "do not re-derive".
The numbers were real. The cause was that a direct ICC image was outside
the texel loop's cached route, so its ink arm wrote the RAW samples as
C,M,Y,K — RGB written as ink is the 3x. On 2026-09-02 an agreement test
(vector cell vs image cell of one patch) plus `render-page --probe-ink` on
each found it in under an hour; the retracted route then fixed both
remaining ICC patches and improved every ICC patch on the sweep.

**How to apply:** when a handoff says "measured worse, do not retry", ask
what the measurement passed THROUGH. If the route has an intermediate the
project can probe (the colorant buffer, a decoded palette, a diagnostics
counter), read it on one pixel of a vector/image pair of the same authored
colour before treating the refusal as settled. Do not re-run the whole
sweep to re-derive it; probe first. And when recording a negative yourself,
name the intermediate you did NOT probe.
