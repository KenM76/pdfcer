---
name: project-pass-265-and-v046-release-470th-filing
description: Pass 265.0 (form-field /Q, /DV, NoExport writers) shipped + v0.46.0 released in one filing (470th); Pass 266.0 minted for residue
metadata:
  type: project
---

2026-09-08 (470th filing), `4319279`: `Pass 265.0` — three previously
readable-but-unwritable form-field properties get writers: `/Q` quadding,
`/DV` default value, `Ff` bit 3 NoExport. `/Q`/`/DV` are
`Option<Option<T>>` (set-or-removed); `--clear-*` CLI flags conflict with
their own setters at the clap level. Residue (`/DA`, `/TM`, `/AA`, `/CO`,
four flags) minted as `Pass 266.0` in Backlog, not a silent drop.

Same filing also recorded `v0.46.0` RELEASED (tag `0591f1a`, covering the
469th filing's `Pass 262.0`–`263.0`) — a release owed from a prior filing's
ledger, filed alongside a brand-new Pass, same shape as the 373rd filing's
`Pass 232.0` + `v0.20.0` precedent ([[project_pass_232_and_r217_fifth_amendment]]).

**Notable:** the engineer's dispatch had already cited `Pass 265.0` in
`docs/core-api/02-editing-and-saving.md` ("126 variants at Pass 265.0")
*before* this filing minted the ID — grepped the ceiling first
(`263.0`, family `264.x` spent, next free `265.x`) and confirmed the
pre-filing citation and the ID minted here agree, per the standing
grep-before-minting discipline
([[project_decision_slicing_list_is_not_id_allocation]]). No correction
was owed, but the check is still mandatory every time — an engineer
citing an ID pre-filing is not itself proof the ID is free.

**Why this is worth keeping:** two more data points that (a) a release and
a fresh Pass legitimately share one filing when the timing lines up, and
(b) verifying a pre-cited Pass ID against the live ledger before minting is
cheap and catches nothing here but has caught real collisions before —
don't skip it just because the citation looks confident.
