# tab-order — provenance and attribution

Three minimal PDFs for the **tab-order derivation** —
`crates/pdfcer-core/src/edit.rs`'s `EditSession::page_tab_sequence` and the
`pdfcer tab-order` command that shells it. ISO 32000-1:2008 §12.5.1 (the
`/R`, `/C` and `/S` bullets) and §7.7.3.3 Table 30, plus ISO 32000-2:2020's
`/A` and `/W` values and §12.5.3's annotation flags.

Each file isolates one axis of that derivation — what the file *states*,
what has to be computed from *geometry*, and which annotations a reader
*visits at all* — so a failing test names the clause it broke rather than
reporting that "tab order is wrong".

## Source material and license (LEGAL.md §5)

**Nothing here derives from a third-party file.** These are `LEGAL.md` §5
category (a): **wholly synthetic**, authored for this project, generated
byte by byte by a committed script (`tools/gen-tab-order-fixtures.py`) with
no PDF library behind it — so the fixtures cannot inherit a bug (or a
normalisation) from the very code they test, and no attribution is owed or
claimed.

Every file uses a classic §7.5.4 cross-reference table and US-Letter
(612×792) pages with **no content streams**: the annotations and the
`/Tabs` entry are the entire subject, and page content would only add bytes
no test reads. Every annotation carries `/Border [0 0 0]` so that a test
which ever renders one of these files does not end up depending on
border-drawing code as well.

Regenerate with:

```
python tools/gen-tab-order-fixtures.py
```

## ★ Why the array order is never the expected answer

On every page below, `/Annots` is listed in an order that is **none** of
the orders any test expects — not row order, not column order, not widget
order, not the reverse of any of them.

That is the whole design. `/Tabs /R` and `/Tabs /C` are *derived* orders,
and the cheapest wrong implementation is one that returns the array it was
given. A fixture listed in row order cannot tell that implementation from a
correct one. The project has a standing lesson for this shape: **a fixture
whose default value equals the expected value cannot falsify anything.**

The same reasoning is why `modes.pdf` carries the identical four
annotations on all seven of its pages. The only thing that differs between
page 3 and page 4 is `/Tabs /R` against `/Tabs /C`, so **those two pages
returning the same sequence is a test failure with one possible cause.**

## Why hand-authored bytes, specifically

Most of what this derivation must survive is something no authoring tool
will produce on request:

- a `/Tabs` value **outside** Table 30/31's closed set (`/Q`), which is a
  producer defect and must be reported verbatim rather than normalised
  away;
- an annotation dictionary written **directly into `/Annots`** rather than
  as an indirect reference — Table 164 permits it, producers almost never
  do it, and it is the one entry that can only ever be disclosed because it
  has no object identity to be named by;
- a `NoView` annotation that **also** sets `ToggleNoView`, the bit pattern
  under which ISO 32000-2 makes an otherwise-invisible annotation appear
  *when it is selected* — and tabbing to it is what selects it, so it is
  the one "invisible" annotation that stays in the sequence;
- a `/TrapNet` with the exact `/F` §14.11.6.2 requires (`Print` +
  `ReadOnly`, everything else clear);
- a page carrying `/Tabs /S` with **no structure tree at all**, which is
  the case pdfcer declines to answer for rather than falling back to the
  array.

## The files

### `modes.pdf` — seven pages, one `/Tabs` state each

| page | `/Tabs` | what it pins |
|---|---|---|
| 1 | `/A` | the file states the order outright; **no disclosure is owed**, and a note here would be noise |
| 2 | `/W` | widgets in array order first, then the tail whose order ISO 32000-2 contradicts itself about |
| 3 | `/R` | row order, computed from geometry |
| 4 | `/C` | column order — a **different** answer on the same four annotations |
| 5 | `/S` | structure order: no sequence at all, deliberately |
| 6 | *(absent)* | array order used as a reader convention, disclosed as one |
| 7 | `/Q` | outside the closed set; the value is reported verbatim |

Two widgets (the top row) and two non-widgets (a `/Link` and a `/Text`, the
bottom row), so `/Tabs /W`'s two passes have something to separate and a
mix-up between "widgets first" and "row order" is visible rather than
accidentally correct.

⚠ The four annotations are **shared objects referenced from all seven
pages**. A real document should not do that — an annotation belongs to one
page — but here the `/Annots` array is the thing under test, and every page
listing the identical four in the identical order is exactly what makes
page 3's answer and page 4's answer comparable. Do not copy this structure
into a fixture that is about anything else.

### `geometry.pdf` — three pages for the computed half

**Page 1 — `/Tabs /R` with `/Rotate 90`.** §12.5.1's descriptions *"assume
the page is being viewed in the orientation specified by the `Rotate`
entry"*, while §12.5.3 says `/Rect` *"continues to describe the
annotation's relationship with the unscaled, unrotated user space"*. An
implementation that groups rows on raw `/Rect` y is silently wrong here and
correct on every other page in this directory — which is the point of the
page. Rotating clockwise turns the left-hand column into the top row, so
the two left-hand annotations must come first.

**Page 2 — the annotations a reader never reaches.** `Hidden` (`/F 2`),
`NoView` (`/F 32`), `NoView + ToggleNoView` (`/F 288`, which **stays**), a
`/Popup`, a `/TrapNet`, one ordinary widget, and one annotation dictionary
written straight into the array. Six references plus one direct entry, of
which two are visited, four are skipped and one can only be counted.

**Page 3 — two widgets whose tops are two points apart**, with the higher
one on the right. One row or two, depending entirely on the grouping
tolerance — so `--row-tolerance` has something to move, and the two answers
differ in *order*, not merely in grouping.

### `rtl.pdf` — one page, and a document that reads right to left

`/Tabs /R` with `/ViewerPreferences << /Direction /R2L >>` in the catalog.
§12.5.1 makes the direction within a row a `shall` determined by that
entry, so sorting left-to-right here is a conformance defect rather than a
preference.

A **separate file** because `/Direction` is document-level: it cannot vary
per page, and a fixture that tried to put an `/R2L` page beside an `/L2R`
one would be testing nothing.
