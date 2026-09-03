---
name: two-shipping-functions-are-not-an-inverse-pair
description: A writer and a reader that both ship and are both tested are not thereby inverses — measure the round trip before building a clipboard, cache or undo on it
metadata:
  type: feedback
---

**Before reusing an existing writer + reader as a round trip, MEASURE that
they are inverses.** Both shipping, both tested, both correct at their own
jobs proves nothing about the composition.

**Why:** 2026-08-29, `Pass 169.0`. Carrying a markup annotation on the
clipboard looked free: `annot_author::build_appearance(spec).annot` writes the
annotation dictionary and `spec_from_dict` reads one back, both exercised on
every real document. I wrote the round-trip test **first**, against all eight
variants, and it failed on one:

    cloudy Square  10,20,110,90  →  2.5,12.5,117.5,97.5

`build_appearance` computes a `/Rect` that bounds **what is drawn** (§12.5.2
requires it) and a cloudy border's scallops bulge outside the nominal
rectangle. Reading it back yields the expanded rect. A clipboard built on that
pair **grows a cloudy box by 7.5 pt in every direction on every copy/paste
cycle, compounding, with no error at any step** — drift that only shows after
enough repetitions, by which time the operator cannot say when it started.

Neither function is at fault. `spec_from_dict` reads *foreign* annotations,
where the stored `/Rect` **is** the truth and shrinking it would be an
invention. **Nothing had ever required it to be the author's inverse**, so
nothing had ever checked.

**How to apply:** when a design's appeal is *"both halves already exist"*,
that is the signal to write the round-trip property test before writing
anything else — over **every** variant, with values that are not defaults
([[feedback_a_default_valued_fixture_cannot_falsify_a_carry]]). If it fails,
write the explicit codec; it is smaller than the drift is expensive. **Keep
the failing measurement as a test** so the cheap route is not re-proposed —
including by a later session of mine reading the codec and wondering why it
exists.

Same shape as [[feedback_a_claim_about_callers_is_a_measurement]]: a property
nobody stated is a property nobody checked.
