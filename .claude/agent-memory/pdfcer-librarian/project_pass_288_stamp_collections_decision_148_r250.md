---
name: project-pass-288-stamp-collections-decision-148-r250
description: Pass 288.0 (custom stamp collections, Acrobat-compatible) + decision 148 + standing rule R250 (verify (c)-labelled RAG findings against a primary artifact when one is on disk), 491st filing
metadata:
  type: project
---

**2026-09-10, 491st filing, commit `554897e`.** `Pass 288.0` shipped
Acrobat-compatible custom stamp collections: `pdfcer_core::stamp_file`,
`EditSession::set_named_pages` (verb 221), CLI `stamp-list`/`stamp-pack`.
Format: one PDF per category, one page per stamp; category = `/Info`
`/Title`; stamp names live in the catalog's `/Names`→`/Pages` name tree
(lexicographic order per §7.9.6, NOT page order); `#` prefix = dynamic
stamp (read, never authored). **There is no separate interchange format —
"export" is handing over the PDF**, and that is Acrobat's own answer too.

**Why:** operator asked for stamp authoring "compatible with Adobe's,"
with "the same import/export."

**How to apply:** if asked about stamp collections again, `stamp_file`
module + verb 221 is where the logic lives; don't re-derive the format
from scratch, grep `ROADMAP.md` `Pass 288.0` / decision 148 first.

**Decision 148** (`ARCHITECTURE.md` §12): the format choice above,
sourced by reading Adobe's own shipped stamp files
(`…\Acrobat DC\Acrobat\plug_ins\Annotations\Stamps\ENU\{StandardBusiness,Dynamic}.pdf`)
directly, closing two gaps `pdfcer-acrobat-librarian`'s `(c)
convergent-secondary` RAG finding had flagged by name (where the category
name lives; whether `#` was real). `/PieceInfo` rejected as a red herring
(present only alongside `/Illustrator` data).

**Standing rule R250 minted** (this role's own synthesis, engineer offered
the finding unnumbered): a Feature-RAG entry labelled `(c)`
convergent-secondary is a pointer at what to go verify against a primary
artifact when one is on disk, not a license to build from unchecked.
Ceiling after this filing: **R250**, next free R251. Decision ceiling:
**148**, next free 149. Pass ceiling: **288.0**, next free family 289.x.

**Flagged, not acted on by this role (sibling-role / engineer territory):**
- `pdfcer-acrobat-librarian` should consider upgrading
  `markup__custom_stamp_file_format.md`'s two now-closed gaps from `(c)`
  to `(b) observed` — that RAG's own call, not mine to edit (hard rule 6).
- A line-ending-agnostic edit helper (CRLF/LF `str.replace` matched zero
  times silently for the 3rd time this session) was built in a temp dir,
  not added to `tools/` — flagged for the engineer to judge.

See also [[project_pass_286_redacted_text_per_mark_and_r247_4th_flag]] for
the preceding filing's shape (three-consumer field asymmetry) — unrelated
content, same session cadence.
