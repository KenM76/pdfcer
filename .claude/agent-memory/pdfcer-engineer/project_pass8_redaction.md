---
name: pass8-redaction
description: Pass 8.0 redaction design decisions and the ui-spec P0 constraints (mark-vs-apply, refusal-ack gate, scrub-rides-full-rewrite)
metadata:
  type: project
---

Pass 8.0 (Redaction mark+apply, text+region) — design record.

**Cardinal rule:** pdfce must NEVER claim content redacted when it is not. Disclosed/refused under-redaction OK; silent under-redaction = catastrophic. Headline gate = ABSENCE PROOF (grep whole saved file for redacted string → zero).

**Architecture chosen:**
- `crates/pdfce-core/src/redact.rs` — self-contained advance-preserving surgery interpreter (mirrors Pass-4 page.rs math, kept separate for auditability). Reuses `text_extract::font::ExtractFont` (`codes`/`width`/`bytes_per_code` exposed pub(crate)) for exact segmentation + text-space widths. Security guarantee (bytes gone) is INDEPENDENT of width accuracy (only cosmetic advance-preservation depends on width).
- Apply = build ONE DirtySet (content-stream replacements, page-dict rewrites, annot deletions, container decomposition, /Info+XMP scrub) → `writer::save_full` (forced FULL REWRITE, R35). Never incremental.
- Content-stream rewrite: replace FIRST content obj with redacted+overlay rewrite, empty the rest (avoids delete/sharing traps + leaks). Old glyph bytes gone via save_full re-serialization (content streams are File-provenance, never ObjStm).
- Container decomposition (§7.5.7 Strategy B): for every dirty object with ObjStm provenance, promote ALL container members (replace with current value) + delete container. Triggered mainly by page-dict rewrite when page dict lives in an ObjStm.
- Image intersecting region → DESTROYED since Pass 245.0 (2026-09-03, `redact_image.rs`): decode, clear covered cells, Flate re-encode; wholly covered → `Do` removed + object tombstoned to a 1×1 PAPER sample (white for Gray/RGB, no ink for CMYK — Pass 246.0); shared → copy-on-write clone under `/pdfceRd<obj>_<n>`; undecodable → the MARK IS RETAINED (unapplied, counted in `marks_retained`) and only all-retained refuses (`RedactError::ImageUndestroyable`; `ImageRegion` is GONE). Form XObject intersecting → disclose. Vector paths → CUT at the boundary since Pass 246.0 (`redact_vector.rs`); `vector_paths_intersecting` is now only the malformed-object residual. Never overlay-and-leave-pixels.

**ui-specialist P0 (docs/ui_specs/pass-8-redaction.md) — binding:**
1. GUI P0 = CLI-first + placeholder. Do NOT build a parallel drag-tool (Pass 6.1 canvas tool-mode never shipped). ONE non-negotiable GUI item: persistent status-bar disclosure of UNAPPLIED /Redact marks, computed from the doc's ACTUAL annotation census (not a session counter) — targets the #1 failure (saving marked-but-not-applied believing it's done).
2. Mark-vs-apply must NEVER share a render path: pre-apply = translucent hatch + red outline + "MARKED" tag (NEVER solid fill); post-apply = real black fill baked into content. So the /Redact mark's /AP preview = red outline, NOT solid fill.
3. Refusal-acknowledgement gate (§4.4): Apply REFUSED or gated behind explicit per-residual acknowledgement — no path where partial reads as complete. CLI: report + non-zero/refusal exit unless explicit `--acknowledge-residuals`. Apply gets NO keyboard shortcut.
4. Correctness finding (standing-rule-worthy): EVERY actual scrub (incl. Sanitize/carrier-scrub) must ride the forced full-rewrite — incremental would leave "removed" carrier content recoverable in the prior revision. Mine already does (all scrubs in the one save_full DirtySet).

**SHIPPED 2026-07-31 (Pass 8.0):** ABSENCE PROOF passes (CLI demo: search "SECRET" → 3 marks → apply → grep output = 0 occurrences; render shows black box + "dossier"/"line and PUBLIC text." correctly repositioned). 8 redact unit tests (absence, advance-preservation numeric, forced-full-rewrite/no-Prev, container-decomposition via ObjStm /Info, image-refuse, /Info scrub, struct-tree disclose, nothing-to-apply). Fuzz target 15 redact_apply 9,262 runs/61s/0 crashes. Workspace 1018 tests. All gates green (fmt/clippy -D/wasm32/gui-free/no-network/ui-strings/dup). ZERO new deps. New: crates/pdfce-core/src/redact.rs (surgery interpreter + apply orchestration + count_redaction_marks). font.rs exposed codes/width/to_unicode/bytes_per_code/width_estimated/base_font_name pub(crate). edit.rs add_redaction + mark_redactions_by_search/_by_pattern. annot_author.rs RedactSpec + build_redact_mark (RED OUTLINE preview). CLI redact-mark/redact-apply(--acknowledge-residuals, exit 10 REDACTION_RESIDUALS)/list-redactions. GUI ui_text::redaction_marks_pending status-bar disclosure. DEFERRED (disclosed): image pixel-clear (refuse instead), /RO+OverlayText burn-in (fill only), form-XObject in-region surgery (disclosed note), XFA/StructTree/attachments (detect+disclose not scrub), GUI apply-button+canvas-mark (ui follow-up per P0).
