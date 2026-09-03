# links — provenance and attribution

Five minimal PDFs for the `/Link`-annotation **destination reader** —
`crates/pdfcer-core/src/annot.rs`'s `page_link_destinations` and
`crates/pdfcer-core/src/outline.rs`'s `DestinationReader`. ISO 32000-1:2008
§12.5.6.5 (link annotations, Table 173) resolving into §12.3.2
(destinations) and §12.6.4 (go-to actions).

Each file isolates **one** claim from that reader's contract, so a failing
test names the clause it broke rather than reporting that "links are
wrong".

## Source material and license (LEGAL.md §5)

**Nothing here derives from a third-party file.** These are `LEGAL.md` §5
category (a): **wholly synthetic**, authored for this project, generated
byte by byte by a committed script (`tools/gen-link-fixtures.py`) with no
PDF library behind it — so the fixtures cannot inherit a bug (or a
normalisation) from the very code they test, and no attribution is owed or
claimed.

Every file uses a classic §7.5.4 cross-reference table and US-Letter
(612×792) pages with **no content streams**: the links are the entire
subject, and page content would only add bytes a test never reads. Every
link carries `/Border [0 0 0]` so that no test which ever renders one of
these files ends up depending on border-drawing code as well as
link-reading code.

Regenerate with:

```
python tools/gen-link-fixtures.py
```

## Why hand-authored bytes, specifically

Most of what a link reader must survive is something no authoring tool
will produce on request. A link whose `/Dest` **and** `/A` are both
present — which Table 173 forbids outright — is the residue of a
sanitiser, not an export option. A link pointing at an object that is no
longer a page is what a page delete leaves behind, and no tool offers to
make one. A `/GoToR` whose destination name **collides** with a name this
same document defines is the single input that can tell a correct remote
resolver from one that silently resolves remote names locally, and it
exists nowhere in the wild on demand.

A generator emitting through a PDF library would have every one of those
normalised away before it reached disk — the same argument
`tools/gen-outline-fixtures.py` and `tools/gen-annot-fixtures.py` record
for their own corpora.

## ★ Why every file has at least three pages, and none links to page 1

A destination resolver's most likely defect is returning a defaulted `0`
page index. A one-page fixture cannot detect it, and neither can a
multi-page fixture whose links all happen to target the first page: both
pass against an implementation that resolved nothing whatsoever and
returned a default.

So every explicit destination here targets page **2 or later**, and the
expected page index is asserted as a value rather than as "is `Some`".
This is the project's standing lesson that *a default-valued fixture
cannot falsify a carry*, applied at the point where it bites.

## What each file pins

| File | Pages | Pins |
|---|---|---|
| `goto-actions.pdf` | 4 | The happy path, four ways. Four `/GoTo` actions with explicit destination arrays covering `/Fit`, `/XYZ` **with a null zoom**, `/FitH` and `/FitR`. The null zoom is the load-bearing one: Table 151 defines it as *retain the current magnification*, and a reader that reaches for a number and falls back to `0` zooms the page to nothing on a wholly valid file. |
| `named-links.pdf` | 3 | Both §12.3.2.3 namespaces and their failure mode. A byte string into the PDF 1.2 `/Names → /Dests` **name tree**; a name object into the PDF 1.1 catalog `/Dests` **dictionary**, written as a direct `/Dest` with no action wrapper (the older spelling, a separate code path); and a name **neither** namespace defines, which must survive as a reported unresolved name rather than vanish. |
| `broken-links.pdf` | 3 | The four malformations. An object that exists but is **not a page** (a reader that only checks "did this resolve to a dictionary" accepts it); a genuinely **dangling** reference to a free xref slot, which §7.3.10 makes null — a *different* failure that must not be collapsed with the first; a link with **neither** `/Dest` nor `/A`, which must be counted rather than dropped; and a link with **both**, pointing at **different pages** so precedence is observable rather than a coin flip (`/Dest` wins, matching the outline path). |
| `non-navigation-links.pdf` | 3 | Actions that are not page jumps — `/URI`, `/JavaScript`, `/Launch` — which must be *disclosed and never executed*, plus a `/GoToR` into `other.pdf`. ★ That document **deliberately defines the `/GoToR`'s destination name in its own name tree**, pointing at page 3. §12.6.4.3 puts a remote destination's name in the *target* file's namespace, so a resolver that consulted the local one would report a confident, entirely wrong local page jump — a failure a fixture without the collision cannot detect. |
| `no-links.pdf` | 2 | The zero-result control: one `/Square` annotation, no links. A tool reporting nothing here must stay distinguishable from a tool that failed to look, which is why `list-links` prints its summary line even when both counts are zero. |

## A defect these fixtures found in themselves

The first cut of `broken-links.pdf` numbered its "not a page" object `9`
by hand, which collided with the fourth annotation's own object number.
The annotation was overwritten and **silently disappeared from
`/Annots`**; the file then exercised three cases while its docstring
claimed four, and nothing failed.

It was caught by running `pdfcer list-links` against the fixture and
counting the output lines — not by any test, because the tests had been
written against the same wrong assumption. The generator now derives that
object number from the same counter the annotations use.
