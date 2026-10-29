# Decision 209 — the font of an XFA caption with no `font` element is a setting, with the grammar default `own`

Recorded 2026-10-29 (1294th filing), at `Pass 647.4` fourth cut `358dd5ce`. Applies decisions 203, 204, 207 and 208; gap G-D11 in `D:\Dev\Rag-Specialized\PDF_Spec\adobe_ext\adobe_ext__xfa_appearance_drawing.md`.

**Question.** A `caption` with no `font` element can take the font defaults (Courier, 10 pt) as the grammar states, or inherit the font of the field or draw that holds it, which XFA 3.3 p.43 allows. The spec does not say which a conforming processor must do.

**Decides.** Both readings ship, and the grammar's is the default. The setting is `CaptionFont { Own (default), Field }` on `PaintOptions` (`with_caption_font`) of `pdfcer_core::xfa::paint_page`, and the CLI flag is `xfa-layout --png-dir DIR --caption-font {own,field}`. A caption's own `font` element always overrides it.

**Why this default.** The grammar is normative, as in decisions 207 and 208.

**Not a setting.** The depth of a caption's region when `reserve` is omitted, 0 or the grammar default -1 (read as omitted) is derived: just enough for the unwrapped caption text plus the caption's margin (gap G-D7). Hidden and inactive captions take no space (p.68); an invisible caption keeps its region and draws nothing.

**Disclosure.** Inline captions and rich-text or non-text captions are not drawn and are reported as `PaintNoteKind::Content`; a rich caption still carves its reserve. Nothing is silent (rule 4).

**Code.** `crates/pdfcer-core/src/xfa/paint.rs` (the setting) and the caption carving beside the text code under `paint/`. Tests are in `paint/tests.rs` and `crates/pdfcer-cli/tests/xfa_layout.rs`. Documented in `docs/core-api/03-capabilities.md` under "Drawing a laid-out XFA page".
