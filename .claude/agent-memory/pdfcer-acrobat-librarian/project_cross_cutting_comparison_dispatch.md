---
name: project-cross-cutting-comparison-dispatch
description: a different dispatch shape from bucket-building — a wide, shallow verdict pass across every FEATURES.md row rather than a deep single-bucket catalog; how it was handled and what it left behind
metadata:
  type: project
---

2026-08-05: the operator asked for a fourth column ("Acrobat") on
`D:\Dev\pdfce\docs\FEATURES.md`, a concise capability scan
`pdfce-librarian` maintains (see that file's own header — deliberately
short, one screen-scannable pass, NOT the roadmap). This is a distinct
dispatch shape from every prior session logged in
[[project-bucket-building-pattern]]: instead of deep-cataloging one
`roadmap_bucket` into N per-feature files, the task was a **verdict per
existing FEATURES.md row across every bucket at once**, plus two
open-ended enumeration asks ("what does Acrobat have that we never
listed" and a CANNOT/WILL-NOT split for the bottom of that document).

**How this was handled**: wrote ONE new file,
`comparison__pdfce_feature_column.md`, explicitly NOT following the
per-feature `_TEMPLATE.md` shape (no single `roadmap_bucket`, no single
`feature` — it's a cross-cutting index, same spirit as the existing
`*_permissions_signature_interaction.md` cross-cutting files but scoped
to the WHOLE corpus rather than one bucket). Frontmatter's
`roadmap_bucket` field states plainly that it spans every bucket rather
than picking one arbitrarily — do this again for any future "compare
across everything" ask rather than forcing it into a single-bucket
frontmatter that would be misleading.

**Sourcing mix, worth reusing as a template for the next such ask**: of
~70 rows verdicted, roughly half rested on ALREADY-BUILT, dated RAG
files (core_ops, text_edit, forms, redaction, measure — all built
2026-07-31 through 2026-08-03) and could be cited with full confidence;
the other half touched buckets this RAG has ZERO files for (Digital
signatures, Encryption, Bates, OCR, Accessibility, Comparison,
Portfolios, Optimization, PDF/A, Prepress) plus the still-flagged
vector/object-editing gap in Text & object editing. Rather than either
(a) declining to answer the zero-coverage half, or (b) silently
answering from training-data recall, the right move — confirmed working
well here — was: run FAST, targeted WebSearch passes (2-3 queries per
uncovered bucket, not a full cataloging session) to get a verdict with
a source, explicitly mark every such row as **FRESH** (this session's
search only, not a cataloged file) vs **RAG** (dated file citation) vs
**GK** (general low-drift-risk knowledge, e.g. "Acrobat can open a
PDF" — not worth burning a search on), and end the file with an
explicit "RAG coverage gaps surfaced" section naming every zero-file
bucket for `pdfce-engineer` to decide whether a dedicated cataloging
session is warranted before treating any FRESH-sourced row as
`must_have`-grade. This three-way confidence-tagging (RAG/FRESH/GK) is
worth reusing verbatim for any future wide-shallow dispatch — it lets
the consuming agent (here, whoever merges into FEATURES.md) know
exactly how much to trust each row without re-deriving it.

**The single most valuable finding this session produced** wasn't from
new research at all — it was CONNECTING two already-cataloged facts
that had never been placed next to each other: `measure__perimeter_and_area_tools.md`
already established Acrobat's Measure tool is an exhaustive 3-tool model
(Distance/Perimeter/Area, no radius/diameter), and a fresh search this
session established Acrobat's Edit PDF has ZERO node/subpath/Bézier
editing at all (whole-object only, Adobe's own workaround is "leave
Acrobat, use Illustrator"). Neither fact alone was new, but juxtaposing
them against pdfce's already-SHIPPED node/subpath/Bézier vector-editing
model (Passes 25.x-30.x) produced the comparison's highest-confidence
exceed claim (5 of 8 vector-object rows a clean, sourced `[ ]`). Worth
remembering for future wide-shallow dispatches: the value isn't only in
fresh lookups, it's in juxtaposing scattered existing facts that a
narrower, single-bucket session never had reason to connect.

**What was deliberately NOT done, and should be flagged if asked
again**: no dedicated per-feature files were created for any of the
ten zero-coverage buckets named above — this was explicitly a
verdict-and-flag pass, not a cataloging pass, consistent with the
"quick corpus-building pass now if small and clearly scoped, otherwise
report the gap" instruction in this agent's own role description. If a
future session is asked to build any of those ten buckets out properly,
treat it as a fresh bucket-building dispatch per
[[project-bucket-building-pattern]], not an extension of this file.
