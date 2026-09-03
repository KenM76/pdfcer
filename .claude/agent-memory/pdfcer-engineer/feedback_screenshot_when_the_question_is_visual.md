---
name: screenshot-when-the-question-is-visual
description: A stderr trace can prove a draw call ran; only a screenshot proves the operator can see it — and "drawn=true" hid a 680px coordinate-space bug for an entire Pass
metadata:
  type: feedback
---

**The rule:** when the operator's report is about what is or is not ON SCREEN,
the oracle is `tools/gui-shot.ps1`, not `PDFCE_DIAG`. A trace answers "did the
code run"; only a picture answers "can he see it".

**Why (2026-08-05, Pass 36.3).** Ken reported no visual cue for nodes. I found
the marks were gated behind having already selected one, un-gated them,
confirmed `node-marks part=0 count=2 drawn=true` in the trace, and would have
shipped that as the fix. It would have changed nothing he could see.

He then stepped away and offered the screen. A cropped screenshot showed the
selected part with **no node on it**. A second crop, at the vertically mirrored
position, found them: `subpath_node_points` returns PDF page space (y-up) and
the code fed it to `page_to_screen`, which takes canvas space (y-down). On a
400 pt page the marks were painted ~680 px below the line — under a comment
asserting they used "the same conversion" as the outline three lines above,
which was exactly the error.

**Hit-testing was correct the whole time** (it converts the click INTO PDF
space), so the pointer and the paint disagreed and only the paint was wrong.
That is why every headless check passed and why the bug survived being looked
at directly.

**How to apply:**
- "I can't see X" / "there's no cue for X" → screenshot FIRST, before theorising.
- A trace line saying an entity was drawn is evidence about the call, not about
  the pixels. Do not let `drawn=true` end the investigation.
- Two coordinate spaces one flip apart (PDF y-up vs canvas y-down) produce a
  wrong result that looks plausible in code and is obvious in a picture.
  Whenever a draw site takes points from a provider, check which space the
  provider documents — the `_canvas` suffix on the sibling call is the tell.
- Ken will offer the screen when he steps away. Take it, and spend it on the
  questions that are genuinely visual.

`tools/gui-shot.ps1 -Pdf … -Script … -CropX/-CropY/-CropW/-CropH` — crop tight,
compute the crop from the diag `rect=`/`zoom=` line, and remember Crop* are
SCREEN coordinates while the script's click coords are egui window coordinates.

Related: [[gui-diag-harness]] (the no-screen counterpart, still right when the
question is "did the dispatch fire"), [[engineer-does-the-observing]].
