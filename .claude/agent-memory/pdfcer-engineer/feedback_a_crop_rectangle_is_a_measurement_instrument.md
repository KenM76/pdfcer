---
name: a-crop-rectangle-is-a-measurement-instrument
description: A sample region chosen by eye reports edge misalignment as colour error in both directions — find swatch bounds by scanning, and inset, before reporting any number from a render
metadata:
  type: feedback
---

**A crop rectangle picked by eye is a measurement instrument, and an
unverified one lies in both directions at once.**

**Why:** on 2026-08-27 I shipped `Pass 137.0` with a four-row table of
live-vs-reference distances. The page had **two** panels of four pairs each
and my table silently mixed them. It reported a fixed type 3 radial as an
unfixed **mesh**, and its "23.8" was **edge antialiasing on a hard-edged
circle** — not a colour error at all. In the same table it hid the identity of
the two defects that *were* real.

Re-measured by scanning for non-white runs to find each swatch's bounds, then
insetting 6–8 px so no border pixel entered the mean:

- the two genuinely-wrong pairs were **both type 7 meshes** (24.06, 16.87)
- one apparent failure was `8.74` mean-abs with **rms 21** and mean colours
  agreeing to **0.7 of 255** — the signature of geometry, not transport

**How to apply:**
- **Find the region, don't guess it.** `nonwhite = pixels.sum(2) < threshold`,
  then find runs in the row and column projections. Ten lines of numpy.
- **Inset.** Border and antialiased edge pixels dominate a small patch's mean.
- **Read mean-abs AND rms AND the two mean colours together.** High mean-abs
  with matching mean colours and high rms = misalignment. Matching rms with
  diverging means = a real colour error.
- **Never label a pair by position** ("pair d") when the page has repeated
  panels. Label by what it is (`panel A, type 7`).

⇒ Before quoting any number off a render, state how the region was chosen. If
the answer is "I looked at it", the number is not evidence yet.

Related: [[a-claim-about-callers-is-a-measurement]],
[[two-modes-one-pattern-is-one-measurement]].
