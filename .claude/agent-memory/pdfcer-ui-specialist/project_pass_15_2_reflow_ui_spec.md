---
name: project-pass-15-2-reflow-ui-spec
description: Pass 15.2 reflow UI spec (docs/ui_specs/pass-15.2-reflow-ui.md) — reflow as a TextEdit sub-mode (no new CanvasTool), the recognition-options divergence finding, EditSession::reflow_block contract
metadata:
  type: project
---

Wrote `docs/ui_specs/pass-15.2-reflow-ui.md` (2026-08-01 session, decision
015's final FF-A slice — within-block reflow UI on top of Pass 14.3's
shipped text-edit tool). Read the ACTUAL shipped 14.3 GUI code
(`crates/pdfce-gui/src/main.rs`'s `TextEditState`/`PendingEdit`/
`run_text_edit_tool`, the three-phase A/B/C structure, the floating
`egui::Area` property-bar/status-strip convention) and the ACTUAL shipped
15.0 `ReflowEngine`/`ReflowPreview`/`ReflowRequest` API
(`crates/pdfce-core/src/text_edit/reflow.rs`) before designing, not just the
decision doc — this mattered, see finding 2 below.

**Key design decision: reflow is a SUB-MODE of `CanvasTool::TextEdit`, not a
new `CanvasTool` variant.** Invoked via one new "Reflow paragraph…" button
in 14.3's existing property bar, targeting "the block containing the
current caret" (resolved via already-public `EditableTextModel::line_at` +
`Line.block` — no new core accessor needed for this part). Rejected
"click a block in the read-only overlay to pick it" as the entry point
specifically because it would make plain clicks mean two different things
depending on whether the `show_block_overlay` checkbox happens to be on —
a silent mode-shift the fuzzy-never-sneaky discipline exists to prevent.
New parallel state `TextEditState.reflow: Option<ReflowState>`, mutually
exclusive with `pending: Option<PendingEdit>` (at most one uncommitted
derived state at a time — Reflow button disabled while pending, typing
suppressed while reflow active, a plain click discards an in-progress
reflow exactly like it already discards a pending edit).

**Genuine NEW finding (§0.3 of the spec, the load-bearing one to flag first
if ever asked to review the implementation): 14.3's shipped block overlay
builds its model with `BlockRecognitionOptions::default()`, but 15.0's own
reflow-preview path (both the CLI and the reflow.rs test suite) uses a
RELAXED recognition (`indent_ratio` pushed out of reach) specifically so
right/centre/justified paragraphs' ragged left edges aren't misread as
per-line indents and fragmented into single-line blocks (which would defeat
R77's alignment auto-detection outright).** `reflow_recognition_options()`
that builds this relaxed config currently lives as a PRIVATE function in
`pdfce-cli/src/main.rs` only — not reachable from `pdfce-gui`. Required core
addition: hoist it to `pdfce-core::text_edit::reflow` (public). Without
this, the GUI would either target the WRONG (fragmented) block for exactly
the paragraphs reflow is supposed to differentiate on, or duplicate the
tuning constant in a second crate (same duplication-drift shape as 14.3's
own `font_subset_stem` finding). Since this relaxation is a real trade-off
(fixes ragged-left fragmentation but LOSES traditional first-line-indent
paragraph-break detection for flush-left prose), the spec does NOT
recommend switching 14.3's whole general overlay to the relaxed model —
it keeps BOTH models (default for the general overlay, relaxed for
reflow-targeting) and adds a small, always-shown, GUI-authored disclosure
caption near the Reflow button noting the two may group paragraphs
differently. This is genuinely new UI-authored copy, not a core disclosure
rendered verbatim — flagged explicitly as such so a future reader doesn't
wonder why it breaks the "render core's disclosures verbatim" rule.

**Second core-accessor flag (§0.2, mirrors 14.3's own §0.2 precedent):**
`EditSession::reflow_block` doesn't exist yet (15.1 in progress at spec-
authoring time, confirmed by reading edit.rs directly — no `ReflowBlock` in
`CommandKind`). Designed against the announced contract (decision 015 §6:
"mirrors 14.3's edit_text/format_text integration") and made TWO concrete
asks explicit for 15.1 to confirm rather than silently guess: (1) the method
should take `req: &ReflowRequest` and re-derive/re-plan at commit time
(mirroring edit_text/format_text's own shape), NOT a pre-computed
`ReflowPreview` passed in verbatim — keeps "what you see is what you get"
structural rather than hoped-for; (2) the caller-supplied `page_cropbox` on
that request should be used as-is, not silently re-filled from the
session's own page lookup, so the overflow disclosure reviewed pre-accept
matches exactly what commits.

**Overflow/disclosure design point worth remembering:** reflow overflow
(R76) is disclosed CALMLY, in the same live-diagnostics strip the operator
reviews before clicking Accept — explicitly NOT given Pass 8's heavier
refusal-acknowledgement-gate treatment (separate checkbox blocking the
Apply button). Reasoning: reflow overflow is neither destructive nor
irreversible (pre-save, undo-able, and R76 already guarantees the content
itself is never lost/clipped) — a materially different risk class than
redaction's permanent content removal. Flagged this distinction explicitly
in the spec so the engineer doesn't over-import Pass 8's gate pattern where
it doesn't belong.

**Visual design:** ghost preview reuses 14.3's exact marked/PREVIEW-tag
convention (translucent mask + dashed border + "PREVIEW — not yet applied"
corner tag), generalized from one run's box to a whole block's box; adds a
muted "current" old-bbox outline for comparison and a wrap-width vertical
guide line for non-Left alignments. The block actually targeted by an
active/enabled reflow gets a SOLID (not dashed), thicker version of the
general overlay's existing amber stroke — shape+weight is the signal, no
new color introduced (rule 6). Alignment picker is real `selectable_value`
buttons (not a dropdown, matching 14.3's own colour-model-radio precedent);
a subtlety worth remembering if reviewing the implementation: the engine's
`ReflowRequest.alignment` must be sent as `None` (not `Some(detected_value)`)
until the operator has genuinely picked something DIFFERENT from the
detected default, because `ReflowEngine::preview` tags ANY explicit
alignment as `AlignmentSource::Overridden` — even one equal to the detected
value — which would otherwise make the "Detected" vs "you changed this"
caption lie the instant the operator re-clicks the already-selected option.

Read the full spec at `docs/ui_specs/pass-15.2-reflow-ui.md` before
reviewing or extending any part of the eventual Pass 15.2 implementation.
