---
name: inkscape-parity-scope
description: 2026-07-30 Ken expanded pdfce scope to include Inkscape-level PDF vector editing on top of Acrobat Pro parity
metadata:
  type: project
---

pdfce's parity target is **Acrobat Pro AND Inkscape's PDF-editing capabilities** (Ken, 2026-07-30). Inkscape-parity means deep vector-object editing of PDF page content: node/Bézier path editing, boolean path ops (union/difference/intersection/exclusion), stroke/fill/gradient editing, transforms, alignment/distribution, z-order, text-to-path, object grouping — treating a PDF page's content stream as an editable vector canvas, not just Acrobat's coarser "edit object" tool.

**Why:** Ken wants one tool covering both the document-workflow side (Acrobat) and the vector-art side (Inkscape) of PDF editing.

**How to apply:** Filed in ROADMAP Backlog as its own bucket. Inkscape itself is GPL — behavioral reference ONLY, never a dependency or code source (same rule as MuPDF/Ghostscript in [[docs/PRIOR_ART.md]]). This scope raises the bar on `pdfce-core`'s content-stream model: it must support full round-trip decomposition of path/graphics operators into editable objects — factor that into parser/writer design decisions from Pass 1 onward.
