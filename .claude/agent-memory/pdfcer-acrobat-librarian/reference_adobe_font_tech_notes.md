---
name: reference-adobe-font-tech-notes
description: adobe-type-tools.github.io/font-tech-notes hosts Adobe-PRIMARY font policy/tech notes (incl. AcrobatDC_FontPolicies) — the authority for Acrobat font-embedding/editing questions, but PDF-only so reachable via search index, not WebFetch
metadata:
  type: reference
---

**`https://adobe-type-tools.github.io/font-tech-notes/`** — Adobe's own
archive of font technical notes, published by the Adobe Type Tools team.
This is **Adobe-primary sourcing**, a tier above helpx help pages, for
anything about font embedding permissions, subsetting policy, `fsType`,
or what Acrobat is allowed to do with an embedded font.

The load-bearing document for the Acrobat feature RAG is
`pdfs/AcrobatDC_FontPolicies.pdf` (*Font Embedding Guidelines for Adobe
Third-party Developers*). It carries the only Adobe-primary statement
found of the rule that governs Acrobat text editing: the OS-install check
is applied **even when the font is fully embedded and its `fsType`
permits editable embedding**, and editing fails outright when the face is
not installed.

**How to actually read it:** everything in that archive is a **PDF**, and
`WebFetch` on it returns structure-only with no extractable text layer
(same failure as every other raw-PDF fetch — see
[[helpx-fetch-reliability]]). **Reach the content through `WebSearch`
instead**: search a distinctive phrase plus the filename; the search
index has the full text and returns usable synthesis. Facts obtained this
way must be flagged in the RAG file as search-index-surfaced and marked
for direct-read re-verification.

**How to apply:** when an Acrobat question is really a *font policy*
question (may Acrobat embed this / may it edit with this / what does
`fsType` gate), check here before settling for helpx or community
sourcing — helpx describes the feature, this describes the rule the
feature obeys. First used 2026-09-08 for
`text_edit__subset_glyph_coverage_and_novel_characters.md`, where it
refuted the working hypothesis outright.
