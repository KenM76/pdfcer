---
name: dimension-terminology
description: Always say "pdf dimensions" for CAD-exported dimensions already in the file, and "ce dimensions" for the ones pdfce authors — never bare "dimensions"
metadata:
  type: feedback
---

Never write bare **"dimensions"** in pdfce work. Always disambiguate:

- **pdf dimensions** — dimensions that were already in the PDF, exported by
  CAD or another authoring tool. Not pdfce's. Read-only page content (or
  foreign annotations) as far as pdfce is concerned. The `55 5/8"` printed on
  a drawing is a *pdf dimension*.
- **ce dimensions** — the dimension objects **pdfce authors**: the
  `/Line` + `/IT /LineDimension` annotations with baked `/AP`, their groups,
  scale, `/Measure` dict and `/PieceInfo` sidecar. Everything under
  `pdfce-core/src/dimension/`.

**Why:** Ken set this on 2026-08-04 after reading analysis he could not
decode — the word "dimension" appeared throughout without ever saying which
kind, and the two have opposite properties (one is existing content pdfce
must not touch; the other is pdfce's own authored, editable, deletable
object). He named the failure in **both directions**: ambiguous analysis is
hard for him to act on, *and* an ambiguous report from him can send
troubleshooting down the wrong path. This is a mutual-intelligibility rule,
not a style preference.

**How to apply:**
- In every reply, commit message, doc comment, decision record and subagent
  dispatch. Dispatches especially — a subagent that inherits the ambiguity
  writes a whole analysis in it, which is exactly how it reached Ken.
- When Ken says "dimension" without a qualifier, work out which he means
  from context and **echo back the qualified term** so a mismatch surfaces
  immediately rather than after the work.
- The distinction is about **provenance**, not about representation. A ce
  dimension is still a ce dimension after saving and reopening; a pdf
  dimension does not become a ce dimension because pdfce can see it.
- Watch the near-misses: "the dimension tool" is the *ce* dimension tool;
  "dimension groups", "re-measure", "dimension scale" are all ce concepts;
  "the extension lines of that 55 5/8″ dimension" is a *pdf* dimension.

Related: [[project_inkscape_parity]] (Ken's parity scope), and the
selection-model work where the distinction is load-bearing — pdf dimensions
are page content or foreign annotations, ce dimensions are pdfce-authored
annotations with a sidecar record that must be pruned on delete.
