---
name: project-pass-312-line-split-clear-space-573rd-filing
description: Pass 312.0 (G032) fixed SplitGranularity::Line welding SolidWorks BOM rows/zone letters via a new horizontal clear-space test; same filing closed a register-size overage the 572nd filing left in FEATURES.md
metadata:
  type: project
---

2026-09-22, 573rd filing, commit `d5b23d66` (relayed, not independently git-shown — no
shell this dispatch): `runs_share_a_line` compared orientation+baseline only, so a
SolidWorks BOM table (row-major, one operator per cell) and a sheet border's zone
letters (one operator each) — both sharing a baseline with unrelated neighbours —
welded into single "lines" (565 across one 36-page drawing, widest gap 709.4pt).
Fixed with a horizontal gap/backward-jump test, exposed as new public
`LineSplitOptions{max_gap,max_backward}` (defaults 1.0/0.5 line heights), skipped
under `TextBoundsBasis::EmBox`. New Pass family (312), no decision-log entry (not a
crate-boundary/invariant call).

**Also fixed in the same filing:** the 572nd filing (`Pass 311.0`/`G028`) had left
`docs/FEATURES.md`'s "Edit text across show operators" row at 1,676 chars against
the 1,200-char register-entry-size cap — trimmed to a verdict + short paragraph
citing `0c0145ac`. **Verification method with no shell:** `Grep` pattern
`^.{1200,}$` over the whole file in content mode returns every over-cap line number
directly — used this to confirm both the trimmed row and the newly-noted "Split one
text object into several" row stayed under cap post-edit, without running
`tools/check-register-entry-size.py` itself. Reusable technique for any future
no-shell register-size check.

**Why:** demonstrates the size-rule discipline working end-to-end — a gate failure
surfaced from a prior filing was closed in the very next one, and the same filing
avoided repeating the mistake on its own new FEATURES.md edit.

**How to apply:** when dispatched with no shell and asked to keep a register entry
under a character cap, use `Grep` with `^.{N,}$` as a substitute for running the
Python gate directly — it is exact (same length semantics) and does not require
Bash. Distinguish PRE-EXISTING over-cap rows (in `tools/register-entry-size-baseline.txt`,
accepted debt) from a NEW violation your own edit would introduce.

See [[project_pass_136_form_recursion_and_filing_count]] and
[[project_features_md_concision_rewrite_20260811]] for related FEATURES.md/size
discipline precedent.
