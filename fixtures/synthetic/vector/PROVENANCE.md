# vector — provenance and attribution

Four minimal PDFs for **Pass 9a** — the read-only vector object/selection
model + centerline derivation
(`docs/decisions/011-first-beta-scaled-measurement-dimensioning-tool.md`
Appendix A). Each isolates the shapes the acceptance criteria pin: every
path-construction/painting operator the model decomposes, a Bézier circle,
a text object, an image XObject, and thin filled "line" bars for the
centerline derivation (with a genuine square and a below-threshold bar as
the false-positive guard).

## Source material and license (LEGAL.md §5)

**Nothing here derives from a third-party file.** These are `LEGAL.md` §5
category (a): **wholly synthetic**, authored for this project, generated
byte by byte by a committed script (`tools/gen-vector-fixtures.py`) with no
PDF library behind it — so the fixtures cannot inherit a bug (or a
normalization) from the very code they test, and no attribution is owed or
claimed. Each file is a single page with a classic §7.5.4 cross-reference
table, and its content stream is written verbatim so the geometry is known
exactly and the object model is assertable from the tokens alone.

## What each file pins

`paths.pdf` (300×300)
: One content stream exercising every path shape the model decomposes: an
  open stroked polyline (`m`/`l`/`S`), a filled rectangle (`re`/`f`), a
  **thin filled bar** line (`re`/`f`, aspect 25:1 — a centerline
  candidate), a stroked closed triangle (`m`/`l`/`l`/`h`/`S`), an even-odd
  donut (two `re` / `f*`), a stroked cubic (`c`/`S`), and the `v` and `y`
  implicit-control-point operators. The primary **geometry cross-check**
  fixture (pure paths — no fonts/images) for
  `pdfcer-render/tests/vector_cross_check.rs`, and the object-count /
  selection / centerline fixture for `pdfcer-core/tests/vector_model.rs`.

`curves.pdf` (300×300)
: A circle drawn as four κ≈0.5523 cubic Béziers (`m`/`c`×4/`h`/`S`) — the
  shape a radius/diameter dimension fits — plus explicit `v` and `y`
  subpaths, so the shared cubic primitives are cross-checked against the
  renderer on real Bézier control points.

`mixed.pdf` (300×300)
: A stroked line + a text object (`BT`…`ET`, non-embedded Helvetica) + an
  image XObject (`Do` on a 2×2 DeviceGray image). Pins that text and image
  objects decompose as **selectable-for-move/delete** (bbox + token range
  only, not node-editable) while the stroked path still cross-checks
  against the renderer.

`centerline.pdf` (400×400)
: Thin filled bars at aspect 50 — horizontal, vertical, and a 45° rotated
  bar via `cm` — plus a genuine 60×60 square and a below-threshold aspect-4
  bar. The centerline derivation must offer a candidate for each thin bar
  (rotation-correct in page space) and **none** for the square or the
  aspect-4 bar — the Z3 false-positive guard.

`edit.pdf` (300×300)
: Three isolated, **index-predictable** objects in one content stream, for
  the **Pass 9c-min** basic-editing surgery (move / delete / drag-node,
  decision 011 §2.5): object 0 = a stroked line (`m`/`l`/`S`, two anchors),
  object 1 = a filled rectangle (`re`/`f` — the `re`-corner node-refusal
  case), object 2 = a stroked closed triangle (`m`/`l`/`l`/`h`/`S`, three
  anchors). The single, uncompressed content stream makes the "exactly one
  changed stream" content-identity gate (R46/§5.7) directly observable, and
  the obvious paint-order indices give the CLI (`object-move`/`object-delete`/
  `node-move`) and the render-fidelity test (`pdfcer-render/tests/
  vector_edit_render.rs`) a stable target. Used by `pdfcer-core/tests/
  vector_edit.rs`, `pdfcer-cli/tests/vector_edit.rs`, and the render-fidelity
  test.

`overlap.pdf` (300×300)
: Three **concentric filled squares**, painted outermost first — object 0 =
  20,20..280,280, object 1 = 70,70..230,230, object 2 = 120,120..180,180.
  The click-through / all-hits fixture (ui-spec `pass-17-dock-and-layer-
  tree.md` §C.3): at the page centre all three are under the pointer, and a
  topmost-only `hit_test_point` can only ever return object 2 — objects 1
  and 0 are **unreachable by any click** without
  `pdfcer_core::vector::hit_test_point_all`, which is precisely the gap that
  query exists to close. The nesting also gives stacks of three (150,150),
  two (85,85) and one (35,35), so a hit list's LENGTH is a real answer about
  the geometry rather than a constant an implementation could fake by
  returning every object on the page. Distinct fill colours (blue / amber /
  green) so a rendered check, and a human, can see which square a cycle step
  landed on. Used by `pdfcer-cli/tests/object_list.rs` and
  `pdfcer-core/tests/vector_model.rs`.

## Regenerating

```
python tools/gen-vector-fixtures.py
```

The script is deterministic; regenerating over the committed files is a
no-op unless the script itself changed.
