---
name: cad-export-structure
description: Ken's SolidWorks PDFs put an entire drawing view in ONE path object (1194 subpaths) and every pdf dimension label in ONE text object (237 runs) — this shapes every selection and editing feature
metadata:
  type: project
---

Measured 2026-08-04 on Ken's own export (`cad-drawing-a.pdf`,
36 pages, page 1 has ~5900 objects):

| What | Structure |
|---|---|
| The isometric view | object 5870: **one** stroked path, **1194 subpaths**, 6681 anchors, bbox 590.7,500.2 → 1140.9,1000.3 (550×500 pt) |
| The other three views | objects with 950, 881 and 742 subpaths |
| Every **pdf dimension** label on the sheet | object 5871: **one** text object, **237 runs**, bbox 23.1,14.1 → 1564.3,1216.5 — the whole page — painted 2nd-from-last |

**Why:** this is what Ken's real work looks like, and it invalidates the
assumption most PDF-editor features are built on — that a visible line is an
object. Two of his reports traced straight to it:

- *"how do I click on individual lines and nodes to move or delete them?"* —
  there are no individual line objects. Per-object hit testing correctly
  selects the whole view. Fixed by subpath-level selection (Pass 25.0/25.1).
- *"I can't select anything"* — the page-spanning text object won every click
  until text hit testing moved to per-run bounds.

**How to apply:**
- Before designing any selection, editing, or **ce dimension** snapping
  feature, ask what it does when the object is 1194 subpaths. "Select the
  object" and "move the object" are usually the wrong granularity for this
  file, and this file is the target workload.
- The same goes for anything that reports counts to the operator — "6681
  node(s)" is technically true and unhelpful; "drawn as 1194 separate parts"
  is what explains the behaviour.
- The file is proprietary and must never be committed (`docs/LEGAL.md` §5).
  Reproduce only the *structure* in synthetic fixtures —
  `tools/gen-multi-subpath-fixture.py` and
  `tools/gen-scattered-text-fixtures.py` do exactly that.
- Verify against it with `pdfce-cli object-list … --enter N --hit X,Y` and
  the [[reference_gui_diag_harness]], not by assuming.

Related: [[project_inkscape_parity]] (the vector-editing scope this feeds),
[[feedback_dimension_terminology]] (those 237 runs are **pdf dimensions**).
