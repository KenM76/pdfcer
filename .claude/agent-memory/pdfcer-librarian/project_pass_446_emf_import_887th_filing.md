---
name: project-pass-446-emf-import-887th-filing
description: Pass 446.0 (EMF import as vector content) shipped, closes the O279 family; 887th filing, 2026-10-02
metadata:
  type: project
---

Pass 446.0 (`ea8ea9cd`, cherry-picked from worktree `4fb2dbc2`, on `main`,
**not yet pushed** as of this filing) — `G094`: EMF import as vector
content via new `emf_import` module + `EditSession::add_emf`/
`add_emf_stamp` (2 new verbs, `EditSession` now 315 public) + CLI
`add-emf`. **Closes the `O279` family** — `443.0`/`444.0`/`445.0`/`446.0`
are now all SHIPPED (no open Passes remain in that family).

**Why this matters for future filings:** this Pass also fixed a defect in
the *already-shipped* `export-emf` writer (header internally inconsistent
by ~1.4%, shrinking output in every reader) — found and fixed in the same
commit as an unrelated import feature. Confirms the pattern (`Pass
444.0`, `Pass 447.0`, etc.) that pdfcer sessions routinely find and fix a
sibling defect while building something else; always check the dispatch
report for an unrelated "fixed on discovery" clause before filing, it is
not an error in the report.

**FEATURES.md mechanics worth remembering:** the EMF-export row (Export
section, right after the EMF/SVG keep-text rows) got a short in-place
correction appended to its existing sentence (header-scale fix) rather
than a new row — correct per the "replace, don't append a running note"
rule, since this was folded into the existing sentence as a factual
correction, not a growing history. The EMF-import *Planned* row was
deleted outright and replaced by a new *Implemented* row directly after
the EMF-export row (Export section groups all MS-EMF-touching rows
together: SVG keep-text, EMF keep-text, EMF export, EMF import).

**No shell available this filing** (contrary to the environment prose
claiming one) — same situation as the 576th filing
([[project_pass_315_text_run_set_move_576th_filing]]). Could not run
`tools/check-register-entry-size.py` or confirm `ea8ea9cd` is at `HEAD`
independently; both relayed from the dispatching engineer /
git-status snapshot (which predates this commit). Used
`Grep "^.{1200,}$"` over `FEATURES.md` as a cap-check substitute, but
found the raw-character-count technique noisy: dozens of pre-existing,
presumably-passing rows already exceed 1200 raw characters (e.g. lines
414, 507, 519 before my edit), so the actual checker evidently measures
something other than raw line length (stripped markdown? a baseline
exception like the code-structure gate?). Don't trust a bare `grep` hit
count against this cap as a pass/fail signal — only as a rough
size-parity check against neighboring rows of similar vintage.
