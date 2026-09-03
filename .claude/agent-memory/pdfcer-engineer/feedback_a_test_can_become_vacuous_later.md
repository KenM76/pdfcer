---
name: a-test-can-become-vacuous-later
description: A fix upstream can make a downstream test start passing for a different reason than it was written for. Only sabotage tells the two apart — and it found a second copy of one bug in a second module.
metadata:
  type: feedback
---

# When a fix upstream makes a downstream test pass, it may now be passing for a DIFFERENT reason

Recorded 2026-08-27, from `Pass 139.1`/`139.2`.

## What happened

`Pass 139.1` fixed text extraction so a rotated line came out as **one run**
instead of one run per letter. The hit-test tests written against
`EditableTextModel` then passed.

They were passing **for the wrong reason**. `EditableTextModel`'s own
Stage-1 clustering had a *second copy* of the same page-axis assumption, so
it immediately re-fragmented the clean runs — 16 lines for four lines of
text. Every glyph of a vertical run had become **its own line**, so every
probe trivially found "its" line and resolved to the only slot in it.

**Reverting the glyph-cell fix changed nothing.** That is what gave it away.

## Why this is not just "R162 again"

`R162` is *an assertion that cannot come out false*. This is different and
sharper: **the test was sensitive when it was written**, and was rendered
insensitive later by somebody else's fix in a different module. Nothing in
the test changed. Nothing in the test's own file changed. It went from
meaningful to vacuous while sitting still.

A green suite says nothing about this. Neither does re-reading the test.

## How to apply

**When a Pass fixes something upstream of an existing test, sabotage the fix
you just made and check that the test you expect to fail actually does.**
It costs one build.

Concretely, across this Pass, six sabotages failed 5 / 6 / 1 / 1 / 1 / 1
tests, no two overlapping. Two of them existed *only* because an earlier
sabotage revealed a hole:

| sabotage | what it revealed |
|---|---|
| delete the direction-change rule | **all nine tests still green** — the fixture's blocks sat far apart, so the perpendicular clause separated them regardless |
| revert `perp`/`gap` to page axes | **all fourteen still green** — within a run the glyphs abut and `Δy` is zero, so both formulas agree |
| `RawLine::push` back to page axes | still green — `hit_test`'s nearest-line **fallback** silently rescued it |

Each hole needed a **new fixture that removes one more signal**, until only
the rule under test could do the separating. Three fixtures where one looked
sufficient.

## The corollary about fallbacks

The third row is its own lesson: `hit_test` has a nearest-line fallback, so a
*wrong box* was invisible through that door. A wrong box is not invisible to a
shell drawing a selection highlight from `Line::bbox`. **Assert the artefact
directly, not only through a caller that has a fallback.**

Filed cross-project at
`D:\dev\rag\rust\a_test_can_become_vacuous_later_when_an_upstream_fix_changes_why_it_passes.md`.

Related: [[feedback_verify_each_instance_not_the_class]],
[[feedback_a_gate_that_underreports_looks_green]],
[[feedback_fixing_one_route_makes_the_others_look_broken]] — that one is the
same phenomenon in the *source*; this is it in the *tests*.
