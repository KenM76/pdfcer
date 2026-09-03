---
name: project-pass-135-ocr-session-edit-and-filing-count
description: Pass 135.0 (8092e38, 2026-08-27, 278th filing) shipped OCR as an EditSession command; filing-count landmark and FEATURES row split pattern to reuse.
metadata:
  type: project
---

**Pass 135.0** (`8092e38`, 2026-08-27, 278th filing) shipped
`EditSession::add_ocr_layer` — OCR became an undoable session edit, not
only the pre-existing `ocr::layer::add_ocr_layer` free function that
returns a whole new PDF. Both writers now go through a shared
`plan_ocr_layer`/`OcrLayerPrep`.

**Why:** operator request `request_ocr_as_an_edit_to_the_open_session.md`
(2026-08-26) — every other OCR tool surveyed lets OCR happen in place;
pdfce's only made a copy. Structural tell was `grep -c ocr edit.rs` == 0.

**How to apply / reuse:**
- **FEATURES.md pattern**: when a capability grows a session-based twin of
  a pre-existing one-shot (this is the same shape as "True in-place page
  insertion" vs `pageops::insert`), add a **new row** immediately after the
  existing one rather than editing the old row's boxes — the two have
  genuinely different core/cli/gui reachability and Acrobat parity is
  usually shared.
- **`cli [ ]` / `gui [ ]` reasons must be verified, not assumed identical
  to the old row's reasons.** Here, confirmed by grep that
  `crates/pdfce-cli/src/main.rs:8673` still calls the OLD free function —
  the new session verb genuinely has zero shell callers yet. Don't
  round up cli just because an `ocr` subcommand already exists; check
  which writer it calls.
- **General shape worth citing forward:** "a correct refusal protecting a
  wrong read is a signal to move the read, not refine the refusal" — the
  free function's base-revision read made a *correct* dirty-session guard
  permanently block OCR after the first edit+save of the session. Good
  candidate phrase if a similar base-vs-live-graph bug recurs elsewhere.

**Filing-count landmark:** SESSION_LOG filings reached **278** as of this
entry (2026-08-27). Ledger at this point: rules `R218` (next free `R219`),
decisions `089` (next free `090`) — per engineer, not independently
re-run (no shell available to this role at time of filing).
