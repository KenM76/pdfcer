---
name: project_export_and_copy_out_arc
description: 2026-09-03 state of the Pass 248 family (export to PNG/JPEG/SVG/EMF + copy-out to Word/Inkscape/LibreOffice) — what shipped, which oracles are real, what is deliberately not done
metadata:
  type: project
---

Pass 248.0 (raster export), 248.1 (SVG), 248.2 (`copy-page`) shipped and
released as v0.29.1; 248.3 (native SVG gradients) and 248.4 (EMF writer +
`CF_ENHMETAFILE`) shipped and released as v0.30.0 — all 2026-09-03, for
Ken's ask "export page(es) to png, jpg, svg … full support (including
transparency where supported!) … copy and paste vector graphics into word
or inkscape", plus his "yes" to the two follow-ons.

**Why:** the whole arc hangs on one design choice — every writer (SVG, EMF,
raster) consumes the renderer's own display-list recording via an EXPORT
recorder mode that never refuses (decision 132). Anything that builds a
writer over `vector::PageObjects` re-opens the "second interpreter" trap.

**How to apply:**
- Oracles that are REAL: resvg (`=0.45.1`, no raster-images) and Inkscape
  for SVG; **real GDI via `PlayEnhMetaFile`** (PowerShell P/Invoke embedded
  in `tests/export_emf.rs`) for EMF. **`System.Drawing.Imaging.Metafile` is
  NOT an oracle** — GDI+ mis-plays `EMR_ALPHABLEND` (half-alpha premultiplied
  pixel → nearly opaque). Word paste measured through combridge into a NEW
  document: default paste stores the SVG (`svgBlip`); `PasteSpecial(EMF)`
  via COM hung and was not measured.
- EMF reference is `D:\dev\rag\emf\` (from [MS-EMF] v18 + consumer sources);
  LibreOffice 24.x ignores the fill rule and renders PS_USERSTYLE solid, so
  dashes are pre-applied and multi-subpath nonzero fills are counted.
- Deliberately NOT done: a `copy-selection` CLI verb; SVG 2 `fr` for
  two-circle radials; Inkscape headless paste (its CLI has no `paste`).
- The clipboard round-trip test is gated on `PDFCER_CLIPBOARD_TESTS=1`.
