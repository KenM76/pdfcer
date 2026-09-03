---
name: compositor-state
description: 2026-08-21 evening — the CMYK colorant buffer SHIPPED (Pass 97.1e/f); the suite transparency panels are no longer blocked on the blending space, and every remaining FAIL is an overprint/spot/ICC patch
metadata:
  type: project
---

**Supersedes the 2026-08-21 morning entry**, which said the suite
transparency panels were blocked on §11.3.4's blending colour space. They
were. They are not any more.

**`Pass 97.1e` (`a277931`) and `Pass 97.1f` (`ff4b4bf`) shipped 2026-08-21.**
A page whose group declares a subtractive blending space is composited in a
four-plane `f32` colorant buffer end to end, then converted to screen colour
once at the end.

**Measured against a worktree build of the parent commit `06aaad3`, not
against a documented number:**

| suite | baseline | 97.1e | **97.1f** |
|---|---:|---:|---:|
| pass | 26 | 28 | **29** |
| FAIL | 14 | 11 | **10** |
| trap marks | 55 | 45 | **41** |
| blends in the wrong space | 107/107 | 15/107 | **0/107** |

★★ **THE PASS COUNTS ABOVE ARE AN OVER-COUNT, AND SO IS EVERY ONE THIS
PROJECT HAS FILED.** The operator read the render cell by cell the same
evening and found failures `tools/suite-check.py` **structurally cannot
see** — it implements ONE of the suite's two criteria (a cross that should
not be there) and is blind to the other (a **check mark that should be
there and is not**, which is how seven patches score). Corrected standing:
**26 at most**. **The DELTAS survive, the LEVELS do not** — every column is
over-counted by the same family, so before/after is sound and any absolute
"N of 51" is not. See `docs/suite-operator-review-2026-08-21.md`, the
operator's cell-level calls, which are the calibration set. Fix is
`Pass 122.2`.

★ **The re-scoping fact still holds for what the harness CAN see:** every
remaining trap-criterion FAIL is an overprint, spot or ICC patch. The next
gains in this family are the n-channel spot buffer, not more compositing.

**What exists now, so it is not rebuilt:**

- `crates/pdfce-render/src/cmyk_buffer.rs` — plane-major `C,M,Y,K,α`;
  `Chan = f32` is the single place the element-type decision lives; a byte
  ceiling that **refuses and discloses** rather than failing; native Table
  149 overprint (no round trip); §11.4.7's collapse in the required
  convert-then-flatten order; knockout planes (`α_g`, `f_g` separate).
- `crates/pdfce-render/src/cmyk_paint.rs` — rasterise to a coverage mask
  with the same `tiny_skia` call an sRGB paint uses, composite here.
- `BrushSpec::cmyk` + `overprint::authored_tints` — one shared rule for
  "what colorants did the file state", read by both the buffer and Table 149.
- `compositor::{composite_element_knockout_cmyk, remove_backdrop_cmyk}`.

**Still approximated, and counted:** non-isolated ORDINARY groups on a
subtractive page are composited as if isolated (`cmyk_groups_approximated`);
images and shadings bridge through sRGB (`cmyk_bridged_pixels`) because their
colour is resolved to sRGB one layer above the canvas.

**★ Three mistakes made and fixed in this build, each measured in traps —
do not re-make them:**

1. **A transparent initial backdrop for a knockout group is WORSE than no
   knockout at all** (`PCS1_161`: 2 → 15 traps).
2. **The two CMYK↔sRGB transforms are for different jobs.** Terminal
   conversion wants the *calibrated* lattice; a ROUND TRIP wants the
   *invertible* max-GCR pair and does not care about accuracy, because the
   value never reaches a screen in that form (10 → 4 traps).
3. **`blends_in_wrong_space` had to be narrowed or the Pass read as a
   no-op.** It counted the blending SPACE, so `tools/measure-blend-space.py`
   still said 107/107 wrong after two patches started passing.

**How to apply:** re-measure with a worktree build of the parent commit
before claiming any movement — the suite harness numbers in documents drift,
and this session's baseline run cost five minutes and settled two
contradictions. `docs/NEXT_SESSION.md` (2026-08-21 evening) carries the
queue; item 1 is `97.1g`, the non-isolated group, which is a **port** of an
existing additive path rather than a design.

See [[a-correct-fix-can-be-unreachable]] and
[[feedback_a_gate_that_underreports_looks_green]].

## 2026-09-02 — the spot-colorant plane arc closed on the paint routes

Passes 228.0–239.0: every paint route deposits a spot into its own plane
(fills/strokes/text, stencil masks, sampled images direct and /Indexed,
axial/radial/function shadings, shading PATTERNS — the pattern site had
never had a native ink route at all), a process-space image under /OP true
preserves the planes, and the planes survive isolated, non-isolated and
knockout group merges BY COLORANT NAME (they had been merged by index, or
dropped, since the planes existed). The one remaining flattening route is
MESH shadings (types 4–7). Suite: 5 FAIL / 38 pass / 8 unresolved of 51; the
five that remain are the device-model adjudication (3.0, 4.0 — operator's
question (cb)), ICC RGB images (13.0 b, 17.2 — the next Pass; do NOT route
them onto the ink path, that is the measured negative), and a Lab swatch
(22.1, undiagnosed). `tools/suite-check.py` routes mark-criterion patches
past the cross detector now — a check mark IS two diagonal strokes.
