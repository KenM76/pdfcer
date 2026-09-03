---
name: project_export_and_copy_out_arc
description: 2026-09-03 state of the Pass 248 family (export to PNG/JPEG/SVG + copy-out to Word/Inkscape) — what shipped, what the oracles were, what is deliberately not done
metadata:
  type: project
---

Pass 248.0 (raster export, c549219), 248.1 (SVG, 80f1c3e) and 248.2
(`copy-page` to the Windows clipboard) shipped 2026-09-03 for Ken's ask
"export page(es) to png, jpg, svg … full support (including transparency
where supported!) … copy and paste vector graphics into word or inkscape".

**Why:** the whole arc hangs on one design choice — SVG comes from the
renderer's own display-list recording via an EXPORT recorder mode that never
refuses (decision 132), not from the vector editing model. Anything that
builds a second SVG/EMF writer over `vector::PageObjects` re-opens the
"second interpreter" trap.

**How to apply:**
- Oracles that exist: `tests/export_svg.rs` (resvg pixel parity, pinned
  `=0.45.1` because 0.48 duplicates tiny-skia/png and the duplicates guard
  sees dev-deps) and an Inkscape end-to-end test that runs when
  `C:\Program Files\Inkscape\bin\inkscape.exe` exists. Word paste was
  measured by hand through combridge (`word run-script` into a NEW document,
  closed unsaved) — type 17 + `svgBlip` = SVG stored; type 3 = raster.
- Deliberately NOT done: `CF_ENHMETAFILE` (only LibreOffice 24.x needs it),
  a `copy-selection` CLI verb, true `<linearGradient>` for axial/radial
  shadings (they are rasterised at `--dpi` and counted in `ExportTally`).
- Inkscape's CLI has no `paste` action — do not try to drive a paste
  headless again.
- The clipboard round-trip test is gated on `PDFCER_CLIPBOARD_TESTS=1`
  because it overwrites the developer's clipboard.
