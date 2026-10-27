# Decision 203: an annotation with no /AP whose geometry fully determines its look is drawn from that geometry at render time

Recorded at the 1263rd filing, 2026-10-27, scoping `Pass 646.20`-`646.23` (request `G236`, `request_G236_draw_markup_without_appearance.md`, from `pdfcer-gui`). Scoped at that filing; `646.20` and `646.21` shipped together on `762c4d85` (amended 1264th filing, 2026-10-27), `646.22` and `646.23` not yet. Narrows `R43` again after decision 149 (`Pass 289.0`).

## Decides

1. **Rule: derived, not invented.** An annotation with no `/AP` is drawn at render time when its own entries fully determine how it looks (vertices, `QuadPoints`, `/InkList`, `/Rect` with `/RD`, `/BE`, colour, border). Anything that needs a choice the file does not make (an icon, a stamp image, a caption) stays blank. `R43` is unchanged for those.
2. **Render-only.** Nothing is written to the file. Saving a document leaves the annotation without `/AP`, byte-identical (round-trip rule).
3. **Setting.** `MarkupWithoutAp { FromGeometry (default), Blank }` on `RenderOptions`, mirroring `WidgetsWithoutAp`. CLI flags select both values.
4. **Disclosure (fuzzy-never-sneaky).** Counter `annots_geometry_drawn` appended to the END of the `render-page` metrics line (existing columns keep their position); `Diagnostics::annotations_geometry_drawn` per subtype; one stderr note per annotation. `annots_no_ap` keeps its meaning (annotations without `/AP`, drawn or not).
5. **Scope by Pass.** `646.20` Ink and the text-markup family (Highlight, Underline, StrikeOut, Squiggly). `646.21` Square and Circle (`/RD`, `/BE`), Polygon (cloud), PolyLine, plain Line. `646.22` FreeText, with disclosed font substitution. `646.23` optional `annot bake-appearances` verb.
6. **QuadPoints order.** ISO 32000-1 section 12.5.6.10 Table 179 says counterclockwise from the baseline start; Acrobat and most producers write top-left, top-right, bottom-left, bottom-right (PDF Association pdf-issues #68, #743). Both are accepted, told apart by geometry; fixtures for both.
7. **Declined and reported, left blank:** a Line with `/Cap`, `/LL` or `/IT /LineDimension` (a pdf dimension, which pdfcer must not redraw), PDF 2.0 `/Path`-only geometry, and unreadable specifications. **Out of scope:** Caret, Popup, Link, Widget.

## Why

Section 12.5.6 puts no `shall` on a reader to draw these, so decision 149's reader-duty argument does not apply; the justification is different. The geometry entries are the annotation's definition, so a reader that draws them reproduces the file rather than adding to it. A viewer that shows a Highlight and a renderer that leaves it blank disagree about the document. A setting keeps the old behaviour available.

Spec sources in the spec RAG: `iso32000__s__12.5.6.8.md`, `12.5.6.10.md`, `12.5.6.13.md`.

## Body-section effect

None beyond the Decision log. New standing rule: none; `R263` stays next free. Next free decision: `204`.
