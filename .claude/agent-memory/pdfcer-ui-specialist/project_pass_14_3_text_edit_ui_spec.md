---
name: project-pass-14-3-text-edit-ui-spec
description: Pass 14.3 text-edit UI spec (docs/ui_specs/pass-14.3-text-edit-ui.md) — the load-bearing core-accessor gap found, CanvasTool's first inhabitant, and key interaction calls
metadata:
  type: project
---

Wrote `docs/ui_specs/pass-14.3-text-edit-ui.md` (2026-08-01/02 session,
decision 014's final slice — Acrobat-style in-place text editing UI on the
Pass 12.0 canvas). This is the FIRST real inhabitant of `CanvasTool`
(confirmed by reading `crates/pdfce-gui/src/canvas.rs` directly — still
`enum CanvasTool {}`, uninhabited, at spec-authoring time; Pass 6.1/7/8's
GUI slices never actually built the canvas-tool-mode machinery despite
those Passes shipping real core/CLI capability).

**The single most load-bearing finding (§0.2 of the spec, flag first if
ever asked to review the implementation):** `pdfce-core`'s shipped
`text_edit::edit_text`/`text_edit::format::set_format` (Pass 14.1/14.2) are
FREE FUNCTIONS taking `doc: &Document` and returning ALREADY
incrementally-saved bytes (`EditOutcome{bytes, report}`) — they do NOT go
through `EditSession`'s command-log undo stack the way every other mutating
operation in the app does (`add_markup`, `add_redaction`, `fill_text_field`,
`delete_pages`, etc. all take `&mut EditSession` and push one undo-able
command). This is exactly right for `pdfce-cli`'s one-shot batch use but
WRONG for an interactive GUI Accept button — reloading the whole
Document/EditSession from `outcome.bytes` after every keystroke-edit would
be enormously wasteful AND would force the undo stack to hold two
structurally different kinds of entry (command-log vs whole-document
snapshots), a correctness hazard not just an inefficiency. **Required core
addition, named explicitly in the spec, NOT a UI workaround:**
`EditSession::edit_text`/`EditSession::format_text` — session-integrated
siblings reusing the same locate/re-encode/relayout logic, returning the
same `EditReport`/`FormatReport`, but mutating the session's in-memory
object graph as ONE undo-able command instead of eagerly saving. The free
functions stay unchanged for the CLI. **Check this landed before trusting
any Accept-button implementation** — if the engineer instead built a
reload-based workaround, that's the wrong call per this spec's own
reasoning and should be flagged back.

**Second core-accessor gap (§4.3, smaller but real):** `EditableTextModel`
(14.0) exposes `hit_test`/`resolve_range` but no `line_at`/`word_range_at`/
`line_range_at` — needed for double-click(word)/triple-click(line)
selection and Home/End nav. Recommended adding these to core rather than
having the GUI re-derive the same line/glyph-matching logic `hit_test`
already encodes internally (duplication-divergence risk, same shape as
Pass 12.0's own `canvas_to_pdf_space` finding one layer up).

**Key interaction design calls, useful if reviewing the shipped
implementation:**
1. Text selection does NOT go through Pass 12.0's `CanvasTargetProvider`/
   `canvas_selection`/`TargetId` scaffold — that's discrete-object
   selection (Pass 9a's shape), text is a contiguous caret/anchor pair, a
   genuinely different type. `TextEditState` is its own parallel state on
   `OpenDoc`, exactly as `GestureInterrupt`'s own doc comment anticipated
   a third tool supplying its own gesture-state shape.
2. **Geometry bridge correctness**: `EditableTextModel::hit_test(x,y)`
   takes genuine PDF user-space coordinates (verified by reading
   `model.rs` — bboxes are `lly/ury/llx/urx`), so the click path must use
   `viewer::canvas_to_pdf_space` (Pass 12.0's SECOND bridge, built but
   never called by any prior spec) — NOT `screen_to_page`, which produces
   device/rotated canvas space. This is the first real consumer of
   `canvas_to_pdf_space` since it shipped fully tested but uncalled.
3. **Cross-run selection is a named first-cut limit, not silently
   attempted**: `EditRequest`/`FormatRequest` anchor to ONE show operator;
   a selection spanning >1 run is detected client-side and refused with a
   reason BEFORE a doomed core call, never silently clamped.
4. **Live preview never calls core per keystroke** — draws `draft_text`
   with egui's own font as an approximate overlay (dashed border +
   "PREVIEW — not yet applied" tag), explicitly reusing Pass 8's
   marked-vs-applied visual convention (hatch+"MARKED" tag / seamless
   real result) rather than inventing a fourth "not real yet" visual
   language. Real glyph shapes only appear after a real Accept triggers a
   real re-render.
5. **GestureInterrupt policy = Discard** (Pass 6.1's class), NOT Pass 7's
   Commit policy — a pending text edit is explicitly the rule-4 "reviewable
   draft not yet accepted," so any unrelated interrupt (Undo/Save/page-nav)
   silently discards the preview with no confirmation (rule 7 — nothing
   was ever written, so there's nothing to be careful about losing).
6. **Disclosures are rendered VERBATIM from core** (`EditReport`/
   `FormatReport.disclosures: Vec<String>` — already fully-authored,
   reviewed prose per-string in `edit.rs`/`format.rs`'s
   `trust_disclosure`/`disclosure_*` functions). The GUI's only authored
   copy is the refusal "what would lift it" one-sentence appendix table
   (§8.2 of the spec, one row per `RInvTrigger`/`FormatError` variant) —
   everything else is a pure render, never a paraphrase.
7. **Property bar is the FIRST real build of the "dedicated top panel,
   tool-scoped" placement-taxonomy slot** (named by pass-6.1's own spec
   but never actually built — that Pass shipped only a menu-driven
   default-rect placement with no property bar at all, confirmed by
   reading `add_markup_shape`/`GuiMarkupKind` in `main.rs`).
8. **Block-boundary review overlay scoped READ-ONLY this Pass** — no
   split/merge/resize/reorder wiring, because core has no persistence
   mechanism for an operator's correction and nothing (reflow, FF-A) would
   consume one yet. Named explicitly as a deferred non-goal, not silently
   cut.
9. Font trust-level (Bundled vs Supplied) classification logic
   (`font_subset_stem`-equivalent) currently lives ONLY in
   `pdfce-cli/src/main.rs` — flagged the duplication risk if GUI grows its
   own copy; recommended hoisting to `pdfce_render::FontEnvironment`.

Read the full spec at `docs/ui_specs/pass-14.3-text-edit-ui.md` before
reviewing or extending any part of the shipped Pass 14.3 implementation.
