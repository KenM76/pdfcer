# Dimension fixtures — provenance (Pass 12.M2, decision 011 §2.3/§2.4)

**All files here are 100% synthetic** — every byte is emitted by pdfcer's own
generator + `pdfcer`. No real-world PDF of unknown provenance is used
(project rule 7 / `LEGAL.md` §5). Fixtures cover the scaled measurement /
dimensioning subsystem: linear + radius/diameter dimensions, named groups with
per-group scale/units, feet-inches formatting, the per-group OCG layer, and the
hybrid storage (native `/Line`+`/AP`, portable `/Measure` mirror, authoritative
`/PieceInfo` sidecar).

## Base geometry (the input the dimensioning tools measure)

Produced by `tools/gen-dimension-fixtures.py` (raw hand-authored PDF bytes):

| File | What it is |
|---|---|
| `linear-base.pdf` | one horizontal stroked line, 100,200 → 300,200 (200 pt) |
| `short-arc-base.pdf` | a **short (40°) arc** of a circle centred (200,200) r=100, drawn as 12 small line segments — the operator's "small line segments approximating an arc" case (the Taubin regime) |
| `two-region-base.pdf` | two lines in two page regions (for a two-group, different-scale page) |
| `plain-base.pdf` | a blank page (dimensions authored by explicit coordinates) |

Regenerate: `python tools/gen-dimension-fixtures.py`

## Authored dimensioned fixtures (produced by `pdfcer` on the bases)

Reproduce with the release CLI (`cargo build --release -p pdfcer-cli`);
`$C = target/release/pdfcer`, `$D = fixtures/synthetic/dimension`:

| File | Commands | Demonstrates |
|---|---|---|
| `linear-dim.pdf` | `$C dimension-add $D/linear-base.pdf --kind linear --points "100,200 300,200" -o $D/linear-dim.pdf` then `$C group-set-scale $D/linear-dim.pdf --group 0 --real-length 5 --drawn 200 --unit m -o $D/linear-dim.pdf` | a linear dimension; real-length scale back-calc → **5.00 m** |
| `short-arc-radius.pdf` | `$C dimension-add $D/short-arc-base.pdf --kind radius --points "<12 arc points>" --group 0 -o $D/short-arc-radius.pdf` | **Taubin best-fit** recovers radius **100.00 pt** from the short arc |
| `feet-inches-dim.pdf` | `dimension-add … --points "72,300 216,300"` then `group-set-scale --real-length 12.5 --drawn 144 --unit ft-in` | architectural **feet-inches** formatting → **12'-6"** (exceeds Acrobat) |
| `two-group.pdf` | `group-add --name "Right Wing" --unit cm`; `dimension-add … --group 0`; `dimension-add … --group 1`; `group-set-scale --group 0 --real-length 10 --drawn 100 --unit m`; `group-set-scale --group 1 --ratio 1:50 --unit cm` | **two groups, different scales/units** on one page (10.000 m vs 176.39 cm) — non-geometric per-group scale (exceeds Acrobat's `/Viewport` partition) |
| `ocg-hidden.pdf` | `group-add --name Annotations`; `dimension-add … --group 1`; `layer-toggle --group 1 --hide` | the per-group **OCG layer** moved into `/OCProperties /D /OFF`; pdfcer's render hides it |

The 12-point arc list for `short-arc-radius.pdf` is:
`300.000,200.000 299.799,206.342 299.195,212.659 298.193,218.925 296.795,225.115 295.007,231.203 292.837,237.166 290.293,242.979 287.385,248.620 284.125,254.064 280.527,259.291 276.604,264.279`

## Hybrid-storage / PieceInfo-round-trip / /Measure-mirror

Every authored fixture is a self-contained hybrid file: `grep -a` any of them
for `/LineDimension` (native annotation intent), `/Measure` (portable §12.9
scale mirror — feet-inches shows the two-element `ft)/C … in)/C` array),
`/OCProperties` (§8.11 layer registration), and `/PieceInfo` (§14.5
authoritative sidecar). `pdfcer dimension-list <file>` reads the sidecar back
after reload — proving the `/PieceInfo` round-trip (e.g. `two-group.pdf` lists
both groups with their exact scales/units).

## R59 render-fidelity (pdfium differential)

`python tools/annot-pdfium-diff.py fixtures/synthetic/dimension` renders each
fixture with pdfcer and pdfium and compares ink bounding boxes:

- **All visible dimensioned fixtures AGREE with pdfium** (`linear-dim`,
  `feet-inches-dim`, `short-arc-radius`, `two-group`) — the baked `/AP`
  (leader + arrowheads + value label) is visually faithful.
- `ocg-hidden.pdf` **intentionally diverges**: pdfcer correctly HIDES the
  OFF-layer dimension (renders only the base line), while pdfium with
  `draw_annots=True` paints the annotation regardless of its OCG state. This is
  pdfcer honouring §8.11.3.3 (annotation visibility = flags AND OC state), not a
  fidelity defect — corroborated by the headless render test
  `pdfcer-render::annot::tests::an_annotation_on_an_off_layer_is_not_painted`.
  Do NOT "reconcile" it by painting the hidden dimension.
