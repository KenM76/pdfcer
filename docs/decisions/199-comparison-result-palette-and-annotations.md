# Decision 199: the comparison-result palette and annotation shapes

Recorded at `Pass 645.1` (`fcf02195`), 2026-10-24.

## Decides

1. A comparison result is the NEW document plus one review annotation per change, saved content (not a provisional layer), each removable and reviewable:
   - inserted: `/Highlight`, blue (0.25, 0.55, 1.0)
   - replaced: `/Highlight`, orange (1.0, 0.6, 0.1)
   - formatting: `/Underline`, purple (0.6, 0.2, 0.8)
   - deleted: `/Caret`, red (0.9, 0.1, 0.1), at the change's new-side anchor
2. `/Contents` carries the old text where there is one: "Inserted: ...", "Replaced. Was: <old>", "Deleted: <old>", "Formatting changed: ...".
3. A summary page (counts, colour key) is inserted before page 1, default ON, `--no-summary` to omit.
4. Marks go through the existing author verbs, each its own undo entry. A change that cannot be marked is listed in `ResultReport::unmarked` with a reason and does not fail the rest.

## Why

Acrobat's tutorial uses blue for insertion and a marker for deletion; the rest of the palette is free design. Text-markup and caret annotations keep marks reviewable and removable, as opposed to burned into page content. The summary page is default-on to match Acrobat's result.

## Limitation

A deletion's caret sits at its content-order position, not its visual position; on a drawing whose edit moved text in the stream it can land far from the words. Stated on the summary page.

## Body-section effect

None beyond the Decision log. New standing rule: none; `R263` stays next free. Next free decision: `200`.
