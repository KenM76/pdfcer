---
name: local-acrobat-install-as-primary-source
description: Acrobat DC's own shipped PDF assets on this machine are directly-readable PRIMARY evidence, outranking community-sourced (c) findings — check the local install before settling for convergent secondary sourcing
metadata:
  type: reference
---

Acrobat DC is installed locally (see also
[[reference_adobe_font_tech_notes]] and the sibling "Acrobat Reader is
available; Pro is not" memory in `pdfcer-librarian`'s own memory — this
is a **different** fact: it's the shipped **asset files**, not the
running application, that are the resource here). Several Acrobat
features ship their own built-in content as ordinary PDF files on disk,
readable directly with the `Read` tool — no running Acrobat instance,
no COM automation, no screenshot needed.

**Confirmed example, 2026-09-10:** Acrobat's built-in stamp categories
are literally
`C:\Program Files\Adobe\Acrobat DC\Acrobat\plug_ins\Annotations\Stamps\ENU\*.pdf`
(`StandardBusiness.pdf`, `Dynamic.pdf`, etc.) — reading these directly
closed two GAPs a prior (c)-convergent-community-sourced pass had
flagged by name (stamp category-name storage, the `#`-prefix dynamic
convention) and surfaced a structural fact (the catalog `/Names`→
`/Pages` name tree) no community source had described at all. Full
writeup: `markup__custom_stamp_file_format.md` (this RAG).

**How to apply:** before spending a research session on convergent
community-sourced reconstruction `(c)` of an Acrobat file-format
mechanic, check whether Acrobat's own install directory (typically
under `C:\Program Files\Adobe\Acrobat DC\Acrobat\...`) ships a directly
relevant asset file — stamp collections, watermark/header-footer
templates, form-field presets, security-policy templates, etc. are all
plausible candidates by the same logic (Acrobat frequently implements
its own "built-in" content using the same file mechanism a user-authored
custom version would use). If such a file exists, reading it directly is
**(b) observed**, strictly stronger than any amount of `(c)` community
convergence, and should be tried first — not just as a tiebreaker when
community sourcing conflicts.

**Caveat, don't over-claim:** this only answers "what does Acrobat's own
shipped content look like," which is evidence for but not automatically
identical to "what Acrobat requires to recognize equivalent
user-authored content" (e.g. the round-trip placed-annotation `/Name`
GAP in the stamp file — collection files don't show placement behavior
at all). State explicitly which question a given local-file read
actually answers.
