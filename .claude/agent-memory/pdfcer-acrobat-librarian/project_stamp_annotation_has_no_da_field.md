---
name: stamp-annotation-has-no-da-field
description: /Stamp (ISO 32000-1 §12.5.6.12) has no /DA key at all — a documented spec absence, not a research gap; drives where pdfcer stores stamp font size and what custom-stamp-file compatibility actually requires
metadata:
  type: project
---

ISO 32000-1 §12.5.6.12's `/Stamp`-specific table has exactly ONE
subtype-specific key, `/Name` (an icon name, default `Draft`). It has
**no `/DA`/`/Q`** — that "variable text" machinery (§12.7.3.3) belongs
only to `/FreeText` (Table 174, `/DA` Required) and AcroForm widgets
(Table 222, `/DA` Required/inheritable). `/Stamp` is never named as a
member of that family in either PDF edition.

**Why:** confirmed 2026-09-10 while grounding a pdfcer bug (stamp label
font size was being re-derived from `rect_height * 0.42` on every box
resize, with nowhere durable to store an operator-set size). The
question "where does Acrobat store a Stamp's font size" turned out to
have the answer "nowhere — there is no key for it," which is itself the
useful, actionable finding: pdfcer is free to pick its own storage
location with zero Acrobat convention to match or risk conflicting
with. Full writeup:
`D:\Dev\Rag-Specialized\Acrobat_Features\markup__stamp_text_size_and_resize_behavior.md`.

**How to apply:** any future stamp-related dispatch (including the
still-uncataloged "Bates numbering / stamping" `roadmap_bucket`, which
is a *different* bucket but shares the same underlying `/Stamp`-family
appearance-authoring question) should start from this fact rather than
re-deriving it: `/Stamp` has no spec-defined variable-text field, so any
"how does Acrobat store X about a stamp" question likely resolves to
either (a) a documented absence like this one, or (b) an
Acrobat-proprietary `/PieceInfo` key that must be independently
searched for, not assumed. On that second point: `/PieceInfo
/ADBE_CompoundType /Private Watermark|Header|Background` IS confirmed
(via pdflib.com's pCOS Cookbook + MonkeyBread Software's DynaPDF plugin
docs) for Acrobat's separate Watermark/Header-Footer feature — but
targeted searching (including guessed key names) found **no** evidence
this or any other `/PieceInfo` key is used for the Comment ▸ Stamp
feature specifically. Don't assume Stamp inherits Watermark's
convention; it's an unconfirmed, actively-searched-for negative.

Also confirmed same session, useful for any custom-stamp-authoring
dispatch: a custom Acrobat stamp is a plain PDF, one page per stamp, one
file per category, with a page-template name `<internal>=<display>` on
each page (leading `#` = "re-run calculation scripts on every
placement," not part of either name) — dynamic stamps are ordinary
AcroForm calculation-script fields, no special stamp-field type. Full
writeup: `markup__custom_stamp_file_format.md` (same directory).
See also [[project_bucket_building_pattern]] for how this session's
two-file addition fits the RAG's usual extension-session shape.

**UPGRADED 2026-09-10 (follow-up, same day) — the `/PieceInfo`
negative above is now MEASURED, not just searched-for-and-not-found.**
A follow-up dispatch read Acrobat DC's own shipped
`StandardBusiness.pdf`/`Dynamic.pdf` stamp collection files directly
(licensed local install,
`C:\Program Files\Adobe\Acrobat DC\Acrobat\plug_ins\Annotations\Stamps\ENU\`).
`StandardBusiness.pdf` DOES carry `/PieceInfo` (12×) but it is
Illustrator authoring metadata, unrelated to stamp identity/naming —
confirms the "don't assume Stamp inherits Watermark's `/PieceInfo`
convention" line above by direct observation rather than absence of
evidence. Same pass also **closed** two GAPs `markup__custom_stamp_file_format.md`
had flagged `(c)`: category name = the file's `/Info`/`/Title` (not
filename, not a preferences mapping), and the `#`-prefix dynamic
convention is confirmed on every entry of Acrobat's own Dynamic
category. New structural fact not previously recorded anywhere: the
page-template names live in a catalog `/Names`→`/Pages` **name tree**
(ISO 32000-1 §7.7.4 Table 31), in **required lexicographic key order**
(§7.9.6), inside a `/Names` dict shared with unrelated keys like
`/JavaScript` — a reimplementer must not wholesale-replace that
dictionary or emit entries in page order. The round-trip `/Name`
GAP (does a *placed* stamp annotation retain a link back to its
source stamp) is explicitly **still open** — this follow-up inspected
collection files only, never a placed-and-saved annotation.
