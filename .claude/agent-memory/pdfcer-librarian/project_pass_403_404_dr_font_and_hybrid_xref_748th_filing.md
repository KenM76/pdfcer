---
name: project-pass-403-404-dr-font-and-hybrid-xref-748th-filing
description: Pass 403.0/404.0 filed together (hybrid /XRefStm second-save shadow, /DR indirect fonts); personal_rag/pdf lesson already existed pre-written
metadata:
  type: project
---

2026-09-29, 748th filing, `35e77877`/`27e97d92`. Two pdfceGUI-driven bug-fix
Passes filed in one session: `Pass 403.0` (a second incremental save of a
hybrid-reference file no longer lets the forwarded `/XRefStm` shadow the
first save's edit — §7.5.8.4 form A) and `Pass 404.0` (fonts pdfcer adds to
`/AcroForm` `/DR` are now indirect objects — Acrobat blanks a filled field
on focus-out when its `/DA` font is an inline `/DR` dict). Both are
behavioural fixes to existing capabilities — no `FEATURES.md` boxes flipped,
only sentences added to the existing "Save incrementally…" and "Write a
field's `/DA`…" rows.

**Why worth remembering:** the `C:\personal_rag\pdf\` lesson for this
finding — `lesson_20260929_acrobat_hides_text_field_value_when_dr_font_is_
inline.md` — was **already written and indexed** (both `pdf/index.md` and
the master `personal_rag/index.md`) before this dispatch, evidently by
whoever on the pdfceGUI side measured the Acrobat behaviour (the lesson
cites `O262`/`G068`/`G069` and Acrobat-automation measurement detail this
librarian has no way to have produced). **Grep `personal_rag/pdf` for the
finding BEFORE assuming a new lesson write is owed** — hard rule 4
("don't duplicate") applies even when the dispatching engineer's prompt
reads as if the lesson still needs authoring. Caught here before writing a
duplicate under a different filename; the SESSION_LOG/ROADMAP entries were
corrected in-place to point at the real file instead.

**How to apply:** on every "roadmap update — pass shipped" dispatch that
mentions a personal_rag/pdf-worthy finding, grep the subject's `index.md`
and the directory itself for the finding's keywords first — do not trust
the dispatch's phrasing ("please file it there") as evidence the file is
missing.
