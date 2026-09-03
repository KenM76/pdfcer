---
name: a-census-counter-that-omits-a-producer
description: A counter that silently omits one producer is a DIFFERENT question, not a smaller number — and a page with a second producer cannot detect the missing one
metadata:
  type: feedback
---

A census counter that omits one producer is **not a smaller number. It is a
different question**, and nothing in the counter's name says which.

**Why:** 2026-08-27, `Pass 140.0`. I read `tint_applied=292` off a spot-colour
page, attributed it to the image's `TintCache`, and **wrote that number into a
commit message as a measured cost**. The counter had **zero** image
contribution — `image::decode` built its `ColorDiagnostics` locally and dropped
them, so the counter had never seen an image at all. It was the page's path
fills. I attributed a counter to a producer that could not have reached it, and
the reason I could not tell is the defect I then had to fix (`Pass 140.2`).

**★ The half that makes this hard to catch:** every other fixture on hand
carried a **fill beside the image**, and a fill's conversions *are* counted. So
the counter read a plausible non-zero number whether or not the image
contributed anything.

> **A page with a second producer cannot detect a missing producer.**

The fixture had to be **image-only** before the zero was visible. Same shape as
[[feedback_a_gate_that_underreports_looks_green]] — an under-reporting
instrument reads as a working one — but the isolation requirement is the new
part.

**How to apply:** before quoting a counter as evidence about a specific
producer, build the case where that producer is the **only** thing on the page
and check the counter is non-zero. If it cannot be isolated, do not attribute
the number to it. And when adding a new producer to a subsystem, check it
reaches the subsystem's counters — a diagnostics struct constructed inside a
function and never returned is the silent failure mode, and it compiles.

Related: [[feedback_an_extrapolation_reported_as_a_measurement]],
[[feedback_a_claim_about_callers_is_a_measurement]].
