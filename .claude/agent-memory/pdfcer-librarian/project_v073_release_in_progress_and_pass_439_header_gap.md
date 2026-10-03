---
name: project-v073-release-in-progress-and-pass-439-header-gap
description: v0.73.0 RELEASE IN PROGRESS filed (889th filing); Pass 439.0 was missing its own ### header in ROADMAP.md despite being correctly filed and cross-referenced
metadata:
  type: project
---

`v0.73.0` — version-bump commit `574a63bc`, RELEASE IN PROGRESS filed
2026-10-02 (889th filing). Batches `436.2`/`439.0`/`440.0`/`441.0`/`442.0`/
`442.3`/`444.0`/`443.0`/`445.0`/`442.2`/`447.0`/`448.0`/`442.1`/`446.0`.
Headline: first release shipping non-PaddleOCR engines as separate
`<build>-ocr-addon-<name>.zip` assets. OneDrive target `pdfcer2`. Tag,
gates, GitHub release, OneDrive deploy, smoke test, `verify-release.py`
all still owed — a `RELEASED` filing will follow.

**Finding: `Pass 439.0`'s `ROADMAP.md` entry had no `### ` header line.**
Body (G089/G090, unsigned-`/Sig` widget redraw + opt-in foreign check-box/
radio rebuild, commit `4d8ea504`) was correctly filed at the 876th filing
and correctly cross-referenced from the file's own later index section —
but the heading itself was missing, so a direct `Grep "Pass 439\.0"` with
the usual `### \`Pass N\`` header pattern returned nothing. Only found by
grepping `^### ` across the whole Shipped section and diffing the Pass-ID
sequence against the batch list I'd been handed. Fixed by inserting the
header in place (dated note, no content change).

**Why this matters for future release filings:** when asked to "verify
each [batched Pass] against ROADMAP's Shipped entries," don't just grep
for the literal Pass ID string — grep `^### ` and walk the header list,
because a header can be missing while the body and every *other* reference
to that Pass ID is intact. A targeted grep for the ID alone would have
missed this one entirely (it would have found the index-section mentions
and silently concluded the entry existed).

See also [[feedback_verify_relayed_corrections_via_grep_before_accepting]].
