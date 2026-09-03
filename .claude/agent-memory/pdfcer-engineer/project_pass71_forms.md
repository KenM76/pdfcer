---
name: project_pass71_forms
description: Pass 7.1 (form flatten + FDF/XFDF + choice fields + regenerate-all + JS histogram) — key design decisions
metadata:
  type: project
---

Pass 7.1 completed the AcroForm subsystem (choice fill, regenerate-all +
/NeedAppearances, flatten, FDF/XFDF, JS-disclosure histogram, CLI).

**Load-bearing design decisions (so a future session doesn't relitigate):**

- **Flatten appends a NEW overlay content stream to the page `/Contents`
  array** (`q cm /pdfceFmN Do Q`, invoking the widget's existing `/AP /N` as a
  page `/Resources /XObject`), rather than rewriting the existing page content
  stream. Consequence: existing content streams stay **byte-verbatim**, the
  R46 gate is fully unperturbed (NO flattened-page exceptions to enumerate),
  and it's more minimal-diff-friendly than an in-place rewrite. **Why:** in-place
  rewrite would violate the minimal-diff invariant and force filter decode/re-encode.
- **R48 destructive semantics via object DELETION:** flatten deletes the field
  + widget dicts (§7.5.4). Full rewrite omits deleted objects (field data
  physically gone); incremental keeps them recoverable in the prior revision
  (the R35 sibling). Verified: full-rewrite output has no `/FT /Tx` but still
  renders the burned value. **Do NOT** try to make full-rewrite GC unreachable
  objects — it re-emits every object verbatim by design (R32); deletion is the
  mechanism.
- **Flatten uses the STRICT cert gate** (`check_certification`), not the `/P>=2`
  fill gate — it's structural (removes fields), so a certified doc refuses it.
- **Fit matrix (§12.5.5):** flatten emits only the A matrix (fit BBox→Rect) as
  `cm`; `Do` re-applies `/Matrix` itself. Emitting A×Matrix is the double-apply trap.
- **FDF/XFDF = zero new deps** (rule 13): FDF reuses `crate::parser::Parser`;
  XFDF has a hand-rolled ~200-line scoped XML reader (`fdf.rs`). FormData is
  value-only (FQN→values); import dispatches by the TARGET field's modelled type.
- **JS histogram (decision 009 posture A):** `forms::scan_javascript` —
  recognition-only, NEVER executes. Counts field `/AA` C/F/V/K JS hooks, doc
  `/Names/JavaScript`, `/OpenAction`, and flags `/URI`//SubmitForm//ImportData
  (R12) + `/Launch` (R13) action hazards. Surfaced on `list-fields` summary line.

**Shipped:** 620 core lib tests (from 601), fdf_parse fuzz target 14
(624k runs/61s, 0 crashes), R46 gate PASS, wasm32 core+render clean.

**Residual:** corpus flatten-burn coverage is thin (sampled forms were
certified/pushbutton/no-`/AP`); synthetic fixtures + tests carry the burn path.
Also: list-box multi-select appearance is a simplified display-text join (not
the §12.7.4.4 highlight-rectangle rendering) — named simplification.

Related: [[clap-windows-stack]]
