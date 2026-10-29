# Decision 208 — the weight of an unweighted XFA `font` is a setting, with the grammar default `normal`

Recorded 2026-10-29 (1293rd filing), at `Pass 647.4` third cut `16a386c4`. Applies decisions 203, 204 and 207; gap G-T1 in `D:\Dev\Rag-Specialized\PDF_Spec\adobe_ext\adobe_ext__xfa_appearance_drawing.md`.

**Question.** XFA 3.3's `font` grammar defaults `weight` to `normal`. The p.58 prose describes the default font as bold. A `font` with no `weight` attribute can be read either way.

**Decides.** Both readings ship, and the grammar's is the default. The setting is `FontWeight { Normal (default), Bold }` on `PaintOptions` (`with_font_weight`) of `pdfcer_core::xfa::paint_page`, and the CLI flag is `xfa-layout --font-weight {normal,bold}`. An explicit `weight` attribute overrides it.

**Why this default.** The grammar is normative, as in decision 207.

**Code.** `crates/pdfcer-core/src/xfa/paint.rs` (the setting) and `paint/text.rs` (where it is read). Tests are in `paint/tests.rs`. It is documented in `docs/core-api/03-capabilities.md` under "Drawing a laid-out XFA page".
