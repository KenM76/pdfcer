---
name: stamp-placement-spec-normative-stretch-and-reader-can-place
description: two upgrades from a stamp-PLACEMENT dispatch (2026-09-10) — resize/drag stretch is spec-normative (a), not inference (d); and Acrobat Reader (not just Pro) can PLACE an already-existing custom stamp, giving a Pro-free path to settle the /Name round-trip GAP
metadata:
  type: project
---

Follow-up to [[project_stamp_annotation_has_no_da_field]], same bucket,
same day. A dispatch scoping stamp-artwork PLACEMENT (as opposed to the
prior session's stamp-collection-FILE format) produced two findings
worth carrying forward to any future stamp or general appearance-
placement work:

**1. Anisotropic stretch on `/BBox`/`/Rect` aspect mismatch is spec
NORMATIVE, not merely inferred — cite `iso32000__s__12.5.5.md` line 164
directly.** The clause states outright: "Step b maps LL→LL and UR→UR of
`Rect` independently in x and y ⇒ non-uniform (anisotropic) scale. The
appearance is stretched to fill `Rect` exactly; aspect ratio is not
preserved. This is normative, not a bug." This retroactively upgraded
`markup__stamp_text_size_and_resize_behavior.md`'s resize-stretch
conclusion from a (d)-graded architectural inference to (a) documented
fact — the file was edited in place to record the upgrade rather than
left stale (see its own "UPGRADED 2026-09-10" addendum). **Generalizes**:
any annotation subtype whose `/AP` lacks a §12.7.3.3-style
regenerate-on-geometry-change hook (i.e., isn't FreeText or a form
widget) will exhibit this same normative stretch on resize — check this
clause first before treating a resize-distortion question for any other
annotation type as unsourced.

**2. A WebSearch-synthesized "Acrobat preserves aspect ratio, centers the
stamp" claim was checked directly against its cited primary source
(blog.adobe.com's "Adding a Circle Stamp" post) and found ABSENT from
that page** — the synthesis had conflated Stamp placement with the
unrelated AcroForm pushbutton Icon Fit algorithm (`/IF` dictionary
family, §12.7.4.2.4). **Lesson: a WebSearch tool's own synthesized
summary is not a citation** — when a search-engine synthesis makes a
specific mechanical claim, fetch the actual cited page and check for the
sentence before recording the claim as sourced. This is a second,
independent instance of the "WebSearch synthesis mis-attributes a spec
mechanism to the wrong feature" failure mode — treat any confident-
sounding synthesis sentence with a specific mechanical claim (numbers,
"always," "preserves X") as needing a direct-fetch check before it enters
a `must_have`-grade file.

**3. Acrobat READER (not just Pro) can PLACE an already-existing custom
stamp — it just can't AUTHOR new custom-stamp categories.** This
corrects a premise a dispatch was given under: "settling the /Name
round-trip GAP needs a PDF stamped by Acrobat Pro, which is not on this
machine." Per convergent (WebSearch-synthesized, not yet direct-fetch-
verified) tutorial sourcing, free Reader's commenting tools can place any
built-in stamp AND any custom category already present in the Stamps
folder — and the operator's own custom collection file already sits on
disk at
`C:\Users\Ken\AppData\Roaming\Adobe\Acrobat\DC\Stamps\YTV_yyfVN1TzJ0_6oei-GB.pdf`.
**How to apply**: before accepting "we don't have Acrobat Pro, so this
GAP needs a live Pro session" as a dead end, check whether the specific
action needed is AUTHORING (Pro-only, per this same finding) or merely
USING/PLACING already-existing content (Reader-capable). The general
"Acrobat Reader is available; Pro is not" fact (pdfcer-librarian's own
memory) is more useful for placement-behavior questions than it first
appears — it isn't only good for render-parity checks.

Full writeup:
`D:\Dev\Rag-Specialized\Acrobat_Features\markup__custom_stamp_placement_and_appearance_authoring.md`.
Sibling file updated same session:
`markup__stamp_text_size_and_resize_behavior.md`.
