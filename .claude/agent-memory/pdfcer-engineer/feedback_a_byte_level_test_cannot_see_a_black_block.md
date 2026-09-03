---
name: a-byte-level-test-cannot-see-a-black-block
description: When the property is visual (nothing painted here / this region is blank), assert it on a raster via a pdfce-render integration test — 22 green byte-level redaction tests agreed with a black block inside a "transparent" mark
metadata:
  type: feedback
---

When the claim is about what a page LOOKS like — a region renders blank, a
mark leaves its interior transparent, nothing survives under a redaction —
write the assertion against pixels (`crates/pdfce-render/tests/*.rs`,
`render_page_with` + a pixel count in a page-space rectangle), not against
the content stream or the sample buffer.

**Why:** 2026-09-03, Pass 245.0. Twenty-two unit tests asserted destroyed
image cells were "cleared to 0" and all passed. The first render-level test
(`redaction_leaves_no_ink.rs`) counted 6,241 black pixels inside a region
whose mark had no `/IC` — because 0 IS black for DeviceGray/RGB, and Table
192 says a no-`/IC` mark is transparent. Every byte-level test had encoded
the design choice it should have questioned. The same test then caught two
sabotages (never-cut strokes: 3,071 leaked; never-clipped fills: 9,623) that
the unit tests only partly saw.

**How to apply:** for any redaction, overlay, mask, clip or "this must not
paint" change, add or extend a `pdfce-render/tests` case that rasterises the
OUTPUT document and asserts ink counts in named rectangles, both the region
that must be empty and a region that must still paint (so blanking the whole
page cannot pass). Core cannot rasterise; that is why the test lives in
render. Related: [[screenshot-when-the-question-is-visual]] for the GUI
harness form of the same rule.
