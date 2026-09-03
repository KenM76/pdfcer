---
name: a-worked-example-is-a-claim
description: A measured value inside a doc example is a claim nothing checks — one shipped the defect value it was written to replace, in the same session that fixed it, in a doc a sibling project builds against
metadata:
  type: feedback
---

**A worked example containing a measured value is a claim.** A stale one is a
**wrong claim that reads as an illustration**, and nothing in this project
checks one — not the compiler, not clippy, not a test, not any of the 29 gates.

**Why (2026-08-29):** I shipped `--probe-ink` with an example reading
`source=cmyk-buffer … srgb=24,140,108` beside `source=screen-srgb …
srgb=47,180,73`, in the CLI's own `--help` **and** in
`docs/core-api/03-capabilities.md` — the document a sibling project builds
against.

`24,140,108` is the **pre-fix defect value**. So the example restated, in
shipped operator-facing documentation, **exactly the premise that same session
had just measured away** — while a test twenty files over asserted the correct
value for the same operand. The contrast pair *looked* pedagogically useful,
which is why it survived review: it was illustrating a difference that no
longer existed.

★ Found by `pdfce-librarian`, not by me and not by a gate.

**How to apply:**
- **Take the number from a checked place**: a test assertion, or a live re-run
  at the moment of writing. Never from memory, and never from an earlier draft
  of the same session's prose.
- When an example's whole point is a *contrast*, re-measure **both sides**. The
  corrected pair here (`47,181,73` in ink vs `47,180,73` on screen) turned out
  to teach something real — a ±1 blue that is a property of the compositing
  path — which the wrong pair had been hiding.
- The remedy other stale-figure rules use (*replace the number with the command
  that derives it*) **does not apply here** — an example that stops showing a
  value stops being an example. That asymmetry is why this needs its own habit
  rather than the usual one.
- Related: [[feedback_a_claim_about_callers_is_a_measurement]] and
  [[feedback_an_extrapolation_reported_as_a_measurement]] — same family, and
  [[feedback_a_sweep_is_only_as_good_as_its_spelling]] is how you find the ones
  already shipped.
