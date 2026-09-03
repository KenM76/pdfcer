---
name: a-wrong-expectation-returns-as-a-result
description: A number identified as wrong in a plan reappears later as a measured outcome, where nothing invites checking it — restate the correction, not just the number
metadata:
  type: feedback
---

**When you find a planned number wrong, write down that it was wrong —
not merely the right number.** Otherwise the wrong one comes back, in
prose, as a result.

**Why.** On 2026-08-23 a roadmap table gave `Pass 74.7`'s acceptance
criterion as *"11 of 11 forms render at every scale"*. That was
**unachievable**: above a certain zoom only two are in the viewport,
because `Pass 74.4`'s exact `/BBox` cull removes the rest by design. A
change satisfying the criterion would have been a **regression**. I
caught it, said so in the dispatch, and the librarian minted `R215` for
it.

Then I wrote, in the demo README: *"the table above is superseded: **11 of
11 forms survive at every scale tested**."* The same wrong number, in the
same session, hours after having it named — now dressed as a **measured
outcome**.

**★ The direction of travel is the whole lesson.** An expectation is
something a reader tests. A result is something a reader *uses*. Moving a
number from the first slot to the second removes the only mechanism that
would have caught it, and it happens naturally: writing up a fix, you
reach for the criterion you were working against and report it as met.

Found by the librarian **reading the file**, not by any gate — the number
is correct-looking, has no stale word in it, and appears nowhere near the
document that recorded the correction.

**How to apply:**

- When you correct an expected value, **the correction is the artefact**,
  not the corrected value. Write "this said X; X is impossible because Y;
  it is Z" and keep the wrong one visible. A bare Z gets re-derived back
  to X by the next person reasoning from the same faulty intuition —
  including you, an hour later.
- **Never quote an acceptance criterion as a result without re-measuring
  it.** If the criterion was wrong, quoting it launders the error.
- Suspect any sentence of the form *"the table above is superseded"* /
  *"now passes"* / *"all N survive"*. Those are the shapes that carry a
  planned number into a factual slot.
- The same applies to ratios: quoting `93 s → 1.3 s` measured the shipped
  code against a **rejected draft**, not against the baseline (`31 s`).
  A speed-up is a claim about *what it replaced*.

See [[gates-i-owe-myself]] and
[[feedback_a_gate_that_underreports_looks_green]] — same family: the
check that would have caught it was the one nobody was running.
