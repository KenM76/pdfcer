---
name: a-default-valued-fixture-cannot-falsify-a-carry
description: When the feature is "preserve X across an operation", a fixture whose X equals what the code would write anyway passes against an implementation that carried nothing — build the fixture off-default first
metadata:
  type: feedback
---

When the feature under test is **"carry X through an operation"**, the fixture's
value of X must differ from **whatever the code would have written on its own**.
Otherwise the test passes against an implementation that carries nothing.

**Why:** 2026-08-29, `Pass 167.0` (the form-field clipboard). Field *creation*
chooses every value itself, so `demo-form.pdf` — whose `/DA` is `/Helv 0 Tf 0 g`,
whose `/Q` is 0 and whose `/MK /BC` is black — is a perfectly good fixture for
`add_text_field`. It is **useless** for `copy_field`/`paste_field`, because every
one of those values is exactly what a re-author writes anyway. A paste that
silently dropped `/DA`, `/Q` and `/MK` would have produced a byte-identical
result and every assertion would have been green.

Building `rich-field-form.pdf` with **nothing at its default** (14 pt blue
centred, dashed 2 pt, blue border, cream background, `DoNotSpellCheck` set,
`/V` ≠ `/DV`, a `/DA` naming a font resource no destination has) found **two
real defects in the first run** that 21 other passing tests had not:

1. a merged Shape A field's `/T` leaked into the clip's *widget* half, so every
   paste came back under the source's name — a well-formed, wrong document;
2. the same leak with `/DA` **undid a font rename the code had just performed
   and disclosed** — the disclosure said `/TB_1`, the field said `/TB`. **A
   disclosure can be true about what the code intended and false about what it
   wrote.**

A sibling fixture is often needed too: `rival-font-form.pdf` exists only so a
paste that *clobbered* the destination's own resource would fail — its `/TB` is
Courier where the source's is Helvetica-Bold. Without a fixture that
**disagrees**, "install under a free name" and "overwrite" are the same test.

**How to apply:** before writing tests for any preserve/carry/round-trip
feature, ask *"what would the code write if it carried nothing?"* and make the
fixture differ on every field. If an existing fixture already matches the
defaults, generate a new one (`tools/gen-*-fixtures.py`, byte-authored, plus a
`PROVENANCE.md` entry) rather than testing against it. Same reasoning as
[[feedback_a_test_can_become_vacuous_later]] and
[[feedback_sabotage_catches_false_comments]] — the question is always *what
would still pass if the feature were absent?*

**Three more instances in one Pass, 2026-09-02 (Pass 239.0), each a
different construction and each found only by sabotaging the code under
the green test:**
- a spot shading vs a spot fill **on white paper** cannot tell a deposited
  plane from a flattened tint -- the plane's curve is sampled through the
  very conversion the flattened route takes, so both collapse to the same
  sRGB by construction. The discriminator is the same pair OVER A PROCESS
  MARK under overprint, where a deposit preserves the mark and a flatten
  knocks it out.
- a spot painted inside a group whose colorant the parent already holds at
  the same index merges correctly BY LUCK; a decoy colorant painted first,
  and the group painted BEFORE the direct fill, is what makes the merge
  have to allocate and map.
- under Normal blend a knockout group's backdrop removal hides a missing
  initial spot EXACTLY, and a non-isolated group with a Normal interior
  never takes the two-walk route at all (`groups_backdrop_reruns=0`).
  Multiply, which reads the backdrop, is what makes either route matter.

**How to apply:** before believing a new agreement test, ask what the
WRONG implementation would render for this fixture. If the answer is "the
same thing", the fixture is not measuring; change the geometry, not the
threshold.
