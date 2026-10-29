# Decision 207 — XFA border edge placement and a missing fill are settings, with spec-grammar defaults

Recorded 2026-10-29 (1290th filing), at `Pass 647.4` first cut `4463b8e3`. Applies decisions 203 (render-time synthesis) and 204 (dynamic XFA reopened); gaps G-D1, G-D2 in `D:\Dev\Rag-Specialized\PDF_Spec\adobe_ext\adobe_ext__xfa_appearance_drawing.md`.

**Question.** Two points of drawing a laid-out XFA page are open in the spec: where a border edge sits relative to the item's rect (grammar vs Ch.2 prose), and whether a `border` with no `fill` element paints white (p.39 note) or nothing (grammar).

**Decides.** Both ship; one is the default each (the operator's standing rule on spec ambiguity). Set on `PaintOptions` of `pdfcer_core::xfa::paint_page`.
- `EdgePlacement { Grammar (default), Inward }`: `Grammar` centres the edge on the rect; `Inward` draws it inside the rect (Ch.2 prose).
- `MissingFill { None (default), White }`: `None` follows the grammar (no fill element, no fill); `White` paints white (p.39 note).
- Non-drawn or approximated content is reported in `PaintedPage::notes` (`PaintNoteKind`: Content, Stroke, FillType, Corner, OpenBreak, Color), deduplicated by path (rule 4).

**Why these defaults.** The grammar is the normative definition; the prose and the note are secondary readings, kept reachable as settings.

**Code.** `crates/pdfcer-core/src/xfa/paint/`; tests in `paint/tests.rs`; documented in `docs/core-api/03-capabilities.md` ("Drawing a laid-out XFA page"). Core only; renderer integration and the CLI path are the remainder of `Pass 647.4`/`647.5`.
