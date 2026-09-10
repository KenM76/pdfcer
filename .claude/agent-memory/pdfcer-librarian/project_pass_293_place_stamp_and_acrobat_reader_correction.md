---
name: project_pass_293_place_stamp_and_acrobat_reader_correction
description: Pass 293.0 (place-stamp, 496th filing) shipped with no new decision; Acrobat Reader (not just Pro) can place an existing custom stamp — a standing project premise was wrong in its implication, corrected 2026-09-10.
metadata:
  type: project
---

**Pass 293.0** (`56c5e55`, 2026-09-10, 496th filing) shipped
`EditSession::place_page_artwork` / `pdfcer place-stamp` — a custom
stamp's artwork can be placed onto a page as a form XObject behind a
`/Stamp` annotation. Closed the last of five `pdfcer-gui` requests filed
2026-09-10. No new architectural decision (the form-XObject-behind-
annotation shape is §12.5.5/§8.10, already documented elsewhere in
`ARCHITECTURE.md`; the rejected-raster-alternative rationale routed
through the existing decision-058 channel). New `FEATURES.md` row:
`core [x] / cli [x] / gui [ ]` — the requesting shell (`pdfce-gui`,
external, per decision 058) asked for the core API only.

**Correction worth carrying forward**: the global auto-memory entry
"Acrobat Reader is available; Pro is not" (this machine has Acrobat
*Reader* installed, not Pro) had been read by this project as implying
*no stamp-placement artifact is obtainable here*. A
`pdfcer-acrobat-librarian` dispatch this session found that inference
false — **Acrobat Reader itself can place an existing custom stamp**;
only *authoring a new stamp category* needs Pro. So a
Reader-placed-and-saved PDF is obtainable on this machine and could
settle the open `/Name`-on-a-custom-stamp round-trip question (`R250`,
open since `Pass 288.0`). **Do not re-assert "no stamp artifact
obtainable" from the Reader/Pro memory alone — check what the specific
operation actually needs.**

**Why this matters beyond this Pass**: the Reader/Pro memory is a fact
about the machine; what follows from it is a claim about a *specific
capability*, and those are not the same kind of fact. The general
lesson — verify what a licensing/availability constraint actually
gates before treating it as blocking a whole class of verification —
generalizes to any future "we don't have X installed" reasoning in this
project.

See [[project_pass_288_stamp_collections_decision_148_r250]] for the
stamp-collection format decision and `R250`'s origin.
