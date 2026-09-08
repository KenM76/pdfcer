---
name: project-cross-reference-existing-spec-primaries-over-web-search
description: When a web search returns an unverifiable/inconsistent factual claim (e.g. exact character codes, byte mappings), check whether a primary Adobe file already sits in PDF_Spec's _sources/ before treating the web claim as final — cross-referencing two already-on-disk primaries can resolve at higher confidence than any fresh fetch.
metadata:
  type: project
---

Discovered 2026-09-07 during the checkbox/radio-button cataloging session
(`forms__checkbox_radio_technical_model.md`). The operator's brief asked
for the exact ZapfDingbats character-code mapping behind Acrobat's six
checkbox/radio check styles (check/cross/star/diamond/circle/square).
WebFetch/WebSearch results on this point were genuinely inconsistent
across sources (different pages assigned different letters to circle vs.
diamond vs. cross).

**The fix wasn't a better web search — it was checking whether the
primary data already existed on disk.** `D:\Dev\Rag-Specialized\PDF_Spec\_sources\core14_afm\ZapfDingbats.afm`
(Adobe's own Core-14 font metrics file) gives character-code → glyph-name
(`a20`, `a24`, `a35`, `a71`, `a73`, `a78`, ...). `D:\Dev\Rag-Specialized\PDF_Spec\_sources\agl\zapfdingbats.txt`
(Adobe Glyph List's ZapfDingbats mapping) gives glyph-name → Unicode code
point. Chaining the two (grep the AFM for the ASCII code, grep the AGL
file for that glyph name) produced an exact, byte-level-grounded answer —
code `4`→`a20`→U+2714 (check), `8`→`a24`→U+2718 (cross), `H`→`a35`→U+2605
(star), `l`→`a71`→U+25CF (circle), `n`→`a73`→U+25A0 (square), `u`→`a78`→
U+25C6 (diamond) — that then INDEPENDENTLY matched a WebSearch synthesis
run before the cross-check, which raised confidence further (two
unrelated methods agreeing) rather than requiring a tie-breaker.

**Why this matters going forward:** `PDF_Spec/_sources/` holds several
primary Adobe reference files (Core-14 AFMs, the AGL, standard encoding
tables) that were staged for spec-clause sourcing but are equally usable
as ground truth for ACROBAT-FEATURE questions that happen to route
through the same underlying font/encoding data (check-style glyphs,
symbol-font character assignments, Base-14 metrics generally). Before
spending a WebSearch/WebFetch budget trying to resolve a conflicting or
uncertain byte-level claim, grep `PDF_Spec/_sources/` and
`PDF_Spec/fonts/` for a file that already settles it directly. This is
the mirror image of the standing rule "don't duplicate PDF_Spec content
here" — it's "don't re-guess from the web what PDF_Spec already has on
disk."

**How to apply**: whenever an Acrobat_Features cataloging session hits a
character-code, encoding-table, or font-metric question with
inconsistent web sourcing, check `PDF_Spec/_sources/` and `PDF_Spec/fonts/`
first. If a primary file resolves it, cite that as the RESOLVED source
(higher confidence than any COMMUNITY-tier web result) and treat any
independently-matching web synthesis as corroboration, not the primary
citation itself.

See also [feedback-helpx-fetch-reliability](feedback_helpx_fetch_reliability.md)
for the broader fetch-reliability context this technique partially routes
around.
