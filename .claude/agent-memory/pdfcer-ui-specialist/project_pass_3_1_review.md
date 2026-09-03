---
name: project-pass-3-1-review
description: Findings from the Pass 3.1 review (first pdfce-gui editing UI — document properties, rotate, undo/redo, Save a copy) — 2026-07-31
metadata:
  type: project
---

Reviewed Pass 3.1's first editing surfaces in `crates/pdfce-gui/src/main.rs` +
`ui_text.rs` (dispatched by the engineer 2026-07-31, same day Pass 3.1 shipped
per `docs/ROADMAP.md`).

**Highest-priority finding (❌, cited to the engineer): non-atomic save
write.** `PdfceApp::save_dialog` (`main.rs`) calls `std::fs::write(&path,
&bytes)` directly against the operator-chosen path — a direct truncate+write,
not the write-to-temp-then-rename pattern Standing Rule 5 (crash-safe
autosave/atomic writes) requires. This is live today on "Save a copy," not
just a future in-place-Save risk: a crash mid-write can corrupt the
destination, and if the operator re-saves over an existing "(edited).pdf"
from an earlier attempt in the same session, truncation destroys the good
prior copy before the new bytes finish landing. Flagged as must-fix,
independent of whether/when true in-place Save ships.

**Standing gap, not Pass-3.1-specific: no autosave/crash-recovery scratch
file exists yet anywhere in pdfce-gui.** Rule 5's other half. Not a
Pass-3.1/3.2 blocker per se (scheduling is the engineer's call), but must
land before any true in-place Save ships — in-place Save without
autosave/crash-recovery would be strictly more dangerous than today's
copy-only model.

**Unresolved pattern decision: floating window vs. docked panel for
secondary tool surfaces.** Pass 3.1's Properties panel is a floating,
non-modal `egui::Window` (correct fit for an occasional short task) — but
`main.rs`'s own module docs say the window's right side is deliberately
*reserved for a future contextual/tool panel*, implying a dock was the
anticipated shape. Recommended the engineer settle floating-vs-docked as a
one-time convention (not per-panel) before Pass 3.2's page-operations UI is
designed — page insert/delete/reorder/merge/split is a much better fit for
a persistent panel (or the existing thumbnail rail, drag-to-reorder) than a
flat toolbar row of 5-6 more icon buttons or another floating window. If
this becomes a real three-styles-and-counting situation, it's the kind of
pattern [[toolbar-growth-risk]] the librarian should record as a decision.

**Toolbar growth risk (progressive disclosure, rule 3).** The primary
toolbar is one `ui.horizontal` row, separator-divided into groups (file /
view / navigation / zoom / edit / history), with an explicit module-doc
commitment that future Passes "insert a group instead of rewriting the row."
That works at 5 groups; it does NOT have any escape valve for Bates
stamping/OCR/redaction/forms/portfolios/PDF-A conversion, which Rule 3
explicitly says belong in a "more tools" area, not appended to the primary
row. Flagged this now, before Pass 3.2 adds ~6 page-operation buttons, as
the moment to decide a real secondary/"more tools" surface rather than
discovering the toolbar is Acrobat-ribbon-shaped after five more Passes.
Small tactical note found in the same review: the trailing status-summary
label in the toolbar row is not right-aligned (no
`egui::Layout::right_to_left`), so it will drift further right and could
wrap/crowd as more groups are appended — cheap, concrete fix regardless of
the larger toolbar-growth decision.

**What's working well, worth replicating exactly as new features land:**
- The properties Apply/Revert model (apply-on-button, draft text kept out
  of the session until Apply) is the right choice — validated against
  Standing Rule 2 and the "keystroke ≠ edit" principle. Do not recommend
  per-keystroke or on-close-apply alternatives.
- `properties_lossy_warning()` wording in `ui_text.rs` is a strong template
  for "fuzzy, never sneaky" text-decode-uncertainty framing — names the
  U+FFFD substitution, states exactly what Apply will overwrite. Only gap
  found: it's panel-wide (one bool), not per-field, so the operator has to
  hunt visually for which of N fields is actually lossy. Recommended
  per-field marking as a small, absorbable follow-up (the per-field
  `.exact` data already exists in `OpenDoc::seed_properties_draft`, it's
  just being collapsed into one bool today).
- Color-never-sole-signal (Rule 6) is consistently honored throughout
  `main.rs`/`ui_text.rs` — every colored_label pairs a glyph (⚠/✖/✔) with
  full-sentence text. Told the engineer to keep this exact pattern for
  every future warning/error surface (OCR confidence, Bates suggestions,
  redaction confirmations, etc.).
- The R20 status-bar diagnostics collapsing-header is a clean model:
  always-present affordance (never conditionally hidden), one-line summary
  + expandable plain-English detail, honest "skipped, not approximated"
  framing throughout. No violations found.

**Smaller discoverability gaps flagged:** Rotate (a real, repetitive,
document-mutating action — rotating many scanned pages is a common
workflow) has a toolbar button and tooltip but **no keyboard shortcut** in
`collect_keyboard_actions`; recommended adding one (an unclaimed chord, not
copying Acrobat's own binding — that would violate the
"never copy Acrobat's GUI mechanics" rule, this is just picking a free
chord). Also asked whether `EditSession`'s command log carries a
human-readable per-command label — if it does, the undo/redo tooltips
should say what will be undone ("Undo: Rotate page 3"), not just "the last
change." If it doesn't, this needs a `pdfce-core` addition, not a pure UI
fix — flagged as contingent, not asserted as already-buildable.

**Save-vs-Save-a-copy verdict (asked to judge explicitly):** a true
in-place Save should eventually exist (Ctrl+S muscle memory is close to
universal, and it gets more valuable as annotations/forms/signing land),
gated on: (1) atomic-write fix above landing FIRST and unconditionally,
(2) the autosave/crash-recovery scratch file existing, (3) in-place Save
staying incremental by default (never silently promoting to full rewrite —
R33/R35), (4) Ctrl+S rebinding to true Save when it ships, with
"Save a copy" demoted to Ctrl+Shift+S to match near-universal Save/Save-As
convention, (5) no new confirmation dialog for the ordinary case — reuse
the existing persistent status-bar `SaveOutcome` pattern rather than
inventing a fourth confirmation style. Recommended surfacing "this file
retains prior revisions and can be recovered further than a normal in-place
save" as an honest selling point once shipped, paired with a narrator-line
disclosure when the incremental-revision count grows unboundedly (so
"arguably safer" doesn't become "silently bloating" — Rule 4).
