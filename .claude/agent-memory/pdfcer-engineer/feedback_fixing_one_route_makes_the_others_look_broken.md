---
name: fixing-one-route-makes-the-others-look-broken
description: When several routes reach one behaviour and all are wrong the same way, they look consistent — fixing the first is what exposes the rest, so enumerate every route in the same Pass
metadata:
  type: feedback
---

**A system where every route to a behaviour is wrong in the SAME way looks
correct.** Fixing one route is what makes the others look broken. So when a
Pass fixes one route, **enumerate the other routes in that same Pass and say
explicitly which are left** — do not wait for the bug reports.

**Why:** three independent instances in seventy-two hours (2026-08-25 → 27):

| first fix | what it exposed |
|---|---|
| `Pass 130.1` — images keep their ink | shadings still round-tripped, so an image and a shading of the same colour now visibly disagreed (`137.0`) |
| `Pass 137.0` — analytic shadings keep their ink | mesh shadings were then the only pairs on the page still wrong (`137.1`) |
| `Pass 136.0` — the CLICK reaches inside a form | the marquee and the line-pick did not, and both were reported within a day (`138.0`) |

In every case the second half was **not a regression** and had been broken
since it shipped. It was invisible because everything agreed — wrongly.

The third one is the sharpest: `pick_line_in_page` was **inert**, not
degraded, on any CAD drawing whose geometry sits inside a form (129,758 page
objects, 1 form, **10,256 invisible leaves**), and nobody noticed for months
because *selection* was equally blind, so an operator met the page-sized form
long before they met the measure tool.

**How to apply:**
- Before shipping a fix, grep for the other consumers of whatever the fix
  touched — the other paint routes, the other queries over the same list, the
  other shells. Name them in the commit even if you do not fix them.
- Treat the disagreement a half-fix creates as **information**, not as a
  reason to defer: it is the only thing that made these visible at all.
- Say so in the operator briefing. Ken was told two shading pairs would still
  disagree; a day later they did not, and the retraction was owed to him.

**The sibling shape, same days:** a doc comment that **enumerates a
population** decays every time the population changes and nothing compiles it.
`cmyk_bridged_pixels`'s description was wrong three times in two days, each
correction written by somebody who had just read it and believed it.

---

## 2026-08-31 — CONFIRMED, and this time it paid off BEFORE the bug reports

Applied deliberately rather than learned again, on two Passes in one session,
and the ratio is the point: **pdfceGUI reported one defect and there were
four.**

`edit_widget` — reported: a `/Btn` appearance was never redrawn on resize.
Enumerating the verb's other routes found three more of the identical shape:

- **Text and Choice WERE redrawn — at the OLD size.** The regeneration existed
  and was a no-op with respect to geometry. Nobody had looked because an empty
  text field's stretched border reads as a border.
- **A push button's caption change redrew nothing**, though the caption is
  painted into the plate.
- **★ On a multi-widget field the resize was silently DISCARDED**, while the
  artwork was rebuilt at the new size. The outcome reported `resized: true`
  and a `rect_after` that was never written.

The form-geometry Pass repeated it: building the feature surfaced that
`page_objects` returned no leaves at all, and that its memo could not see a
form edit. Neither was the thing being built.

**What made the sweep productive rather than open-ended:** enumerate the
verb's TRIGGERS, not its callers. `edit_widget` rebuilds on three conditions;
checking each against "does the artwork actually follow?" is a finite list and
found all four in minutes.

★ **And the reported one was the ONLY visible one.** A tick is a shape; a
stretched border on an empty box is a border; a discarded resize returns `Ok`.
The defect that gets reported is the one that draws something wrong, which is
a poor proxy for the one that matters — #4 was corrupting geometry silently
and had been since `Pass 134.0`.
