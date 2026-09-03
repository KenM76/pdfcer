---
name: a-rising-failure-count-measure-the-oracle
description: The four-step method that proved a suite regression was a false pass being removed — ablate, segment by exact level, run the detector on the reference, measure distance-to-oracle
metadata:
  type: feedback
---

When a fix makes a pass/fail count **worse**, do not revert on the count.
Establish which it is by measurement, in this order.

**Why:** 2026-08-27, `Pass 140.1` took the print-conformance suite from 5 FAIL
to 6. Reverting would have restored a matched pair of errors and made two other
patches worse. The four steps that settled it:

1. **Ablate.** Build each half of the change separately. `140.0` alone scored
   identically to the baseline, so every movement was `140.1`'s. Without this I
   would have been arguing about a change I had not localised.
2. **Segment by exact value, not by means.** A mean over the failing region
   said the page got *closer* to Acrobat, which looked like a contradiction.
   Segmenting the trap box by exact grey level split it into two objects and
   showed one had become correct and the other had not:

   | object | Acrobat | before | after |
   |---|---|---|---|
   | surround | `84,120,34` | `127,127,127` | `127,127,127` |
   | trap X | `84,120,34` | `128,128,128` | `76,117,31` |

   Both were the same **wrong** colour before, so the mark was invisible and
   the patch scored clean. That is what a false pass looks like from inside.
3. **Run the detector on the REFERENCE.** Zero marks on Acrobat's render,
   three real ones at diagonality 1.00 on mine — so the marks were adjudicated
   rather than detector noise. Skipping this leaves "maybe the detector is
   wrong" unresolved, and it is one line to check.
4. **Measure distance-to-oracle on the pixels that CHANGED**, not on the whole
   page. Whole-page moved 57.0 → 45.1 (dominated by unchanged content);
   changed-pixels-only moved **108.6 → 25.0**, which is the actual claim.

**How to apply:** any time a headline number moves the wrong way. Also note the
generative cause — the same one `tests/shading_ink.rs` records: **several
routes agreeing wrongly is silent; fixing one converts it into a visible
disagreement.** That is an argument *for* fixing one, but it makes the
remaining routes urgent, and it means a count can get worse while every pixel
gets better.

Related: [[feedback_a_rising_failure_count_can_mean_a_false_pass_was_removed]]
(the earlier, shorter form of this — this entry is the method),
[[feedback_fixing_one_route_makes_the_others_look_broken]],
[[feedback_a_crop_rectangle_is_a_measurement_instrument]].
