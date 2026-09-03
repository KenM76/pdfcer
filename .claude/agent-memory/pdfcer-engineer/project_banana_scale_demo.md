---
name: banana-scale-demo
description: tools/gen-scale-demo is Ken's own showpiece — a banana at life size with fully-detailed mitochondria and an anniversary easter egg; treat its correctness as load-bearing, not as a toy
metadata:
  type: project
---

`tools/gen-scale-demo/` generates `banana-at-scale.pdf`: a banana at life
size, two of its cells at that same scale, and eight tiers of zoom ending
at a **10 nm ATP synthase particle readable at ~35 000 000 %**. Renders
live in `C:\Users\Ken\OneDrive\pdfTests\` as
`banana-at-scale_1-…` through `_9-…`.

**Why:** Ken named it as the first thing he wanted to work on
(2026-08-22 handoff: *"the first thing I am going to do is refine the
banana.pdf"*), and when asked whether the mitochondria could carry more
structure his answer was *"do all of it, and not just to one. I want all of
them to have the proper detail."* It is the only artefact in the repo a
non-programmer can look at and immediately understand what the renderer
does — and it contains a **personal easter egg**: a heart of mitochondria
reading `KEN ♥ EMILY` with an anniversary line, inside the pulp cell's
vacuole.

**How to apply:**

- Treat its visual correctness as real engineering, not decoration. The
  easter egg is personal; do not break it, do not "simplify" it, and check
  tier 5 renders after any change to `easter_egg.py` or
  `mitochondrion.py`.
- **It is a genuine bug-finding instrument.** In one session it found a
  renderer defect (forms executed off-screen — `Pass 74.4`), an inverted
  unit constant, and a self-crossing subpath that stroked perfectly and
  filled wrong. Reach for it when testing deep zoom, Form XObjects, or
  extreme CTMs.
- **Verify by rendering, not by reading.** All three of those bugs
  produced plausible-looking output. The self-crossing one needed a
  **160 000 000 %** render of a single crista to see at all.
- Regenerate the whole numbered render set into the OneDrive folder after
  any change — that is the copy Ken looks at, and a stale set is worse
  than none.
- It ships nothing: not in `fixtures/`, no test loads it, `cargo test`
  never runs it. So it has no gate protecting it — the only check is
  looking at the renders.

See [[screenshot-when-the-question-is-visual]] and
[[engineer-does-the-observing]].
