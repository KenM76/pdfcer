---
name: a-rising-failure-count-can-mean-a-false-pass-was-removed
description: When a conformance score gets WORSE after a fix, measure distance-to-oracle before reverting — a renderer that starts drawing things can start failing tests it passed by drawing nothing
metadata:
  type: feedback
---

**A failure count that rises when the renderer improves is the signature of a
false pass being removed.** Do not revert on the count alone.

**Why:** `Pass 130.3` (2026-08-27) fixed a defect where a spot colour painted
**nothing at all** on a subtractive page. The print-conformance suite went from
**4 failures to 5**. Both patches involved moved measurably **closer** to
Acrobat:

| patch | mean abs distance | rms |
|---|---|---|
| `PCS2_020` | 24.76 → **19.88** | 63.06 → **55.68** |
| `PCS2_040` | 41.40 → **28.52** | 79.68 → **63.25** |

Five of six cells on `PCS2_040` had been rendering **completely blank**, and the
harness scored it `ok` — a white trap cross on white paper has no contrast for a
detector to find. **They were passing because they were rendering nothing.**

**How to apply:**
- When a fix makes a conformance score worse, compute **distance to the oracle**
  (mean/rms against reference renders) *before* deciding. Trap-count and
  pixel-distance can move in opposite directions and the count is the weaker
  signal.
- Look for the specific shape: *did the renderer previously draw nothing there?*
  A blank region cannot trip a contrast-floored detector, so "blank" and
  "correct" are indistinguishable to it.
- Say so in the commit and the filing, or the next reader sees a regression.

**The sibling discipline — ABLATE, do not argue.** In the same Pass, two
"obvious" repairs for a neighbouring spot-under-overprint question were built
and **both were refuted by measurement**:

| behaviour for a spot-only source under `/OP true` | suite failures |
|---|---|
| preserve the backdrop (shipped, unchanged) | **4** |
| ink union, `max(c_b, c_s)` | 6 |
| paint the flattened tint normally | 8 |

My reasoning for "paint normally" was confident and wrong: three patches exist
whose entire subject is that a white spot set to overprint must **not** knock
out what is under it. The refuted `ComponentRule::MoreInk` was **removed**
rather than left callable-and-uncalled, and the table is recorded on
`overprint::erases_the_paint` so nobody re-derives it.

⇒ In this area, build the candidate and run the corpus. An argument that sounds
decisive is worth less than one number.
