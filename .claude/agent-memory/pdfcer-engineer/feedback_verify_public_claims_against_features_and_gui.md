---
name: verify-public-claims-against-features-and-gui
description: README/homepage capability claims must be sourced from docs/FEATURES.md every time; the gui column must be reconciled against pdfcer-gui's OWN FEATURES.md, not trusted from second-hand CONSUMED notices
metadata:
  type: feedback
---

Two claim-accuracy failures Ken caught on 2026-09-05, one axis each.

**Axis 1 — README vs my own FEATURES.md.** Refreshing `README.md` I left "OCR"
in the *Not built yet* list. OCR shipped in `Pass 129.0` and is `core [x] cli
[x] gui [x]` in `docs/FEATURES.md` (rows for `ocr::layer` / `add_ocr_layer`; CLI
`ocr` + `ocr --in-place`). I even told myself "OCR — roughly still true" from
stale memory instead of grepping FEATURES. Ken: *"on pdfcer's homepage you say
that ocr isn't built yet, but it was ages ago."*

**Why:** The claim-bearing-copy rule (global CLAUDE.md) says lift the real
status from the source of truth, not memory. For pdfcer the source of truth for
"can it do X" is `docs/FEATURES.md`, full stop. I sourced encryption and
signature claims from it in the SAME edit but skipped OCR — a per-line
discipline, not a per-file one.

**How to apply:** Every capability sentence in README (or any public/user-facing
artifact) is grepped against `docs/FEATURES.md` before it ships — each claim,
including the ones I "know". Never restate a build-status from memory.

**Axis 2 — the gui column is second-hand.** `docs/FEATURES.md`'s `gui` column
was updated from pdfcer-gui's "CONSUMED" notices in
`D:\Dev\FeatureRequests\pdfce_FeatureRequests`, NOT from auditing their code, so
it drifts. Ken: *"do you check pdfcer-gui's feature set against your claims on
what is missing from it?"* — the honest answer was no.

**Why it matters:** `D:\Dev\pdfcer-gui\FEATURES.md` states in its own header that
the engine's `gui` column *"is this project's acceptance criteria"* and is
re-measured against their running binary. So THEIR FEATURES.md (+ their
`NO_SURFACE.md`) is authoritative for what the GUI actually has; my column is a
lagging copy that can be stale in both directions (claim-missing when shipped,
claim-shipped when not).

**How to apply:** Periodically reconcile `docs/FEATURES.md`'s `gui` column
against `D:\Dev\pdfcer-gui\FEATURES.md` + `NO_SURFACE.md` (a subagent diff is
the right tool — read-only, both directions) and fix drift via the librarian.
Do it at least when touching the gui column or making any "missing from the GUI"
claim. A CONSUMED notice is a signal to tick, not proof of current state.

Related: [[a-doc-comment-can-be-shipped-ui]], [[an-unticked-box-is-unfalsifiable]],
[[documented-accurate-gate-green-and-unfindable]].
