---
name: project-image-drop-not-inline-embed-in-either-reference-product
description: neither Acrobat Pro nor PDF-XChange Editor embeds a dropped image file inline on the page it's dropped onto — both open it as a new document instead; the operator's "drag and drop images" request has no direct parity precedent
metadata:
  type: project
---

Sourced 2026-08-08 (`text_edit__image_placement.md` +
`core_ops__drag_and_drop_surface.md`, both in
`D:\Dev\Rag-Specialized\Acrobat_Features\`). When the operator asked for
"drag and drop images ... onto the pdf workspace ... among all the
other things that work this same way with other software," the
research found that **neither reference product does what a naive
reading of that request implies**:

- **PDF-XChange Editor** (directly sourced, developer forum reply):
  dropping an image file onto an OPEN document's canvas creates a
  brand-new, separate PDF document from that image — it does NOT embed
  the image into the currently-open page. Dropping the same image onto
  the thumbnails/pages panel instead inserts it as a new WHOLE PAGE
  (still not an inline placement within existing page content).
- **Acrobat**: the exact canvas-drop behavior is unconfirmed (a sourcing
  GAP), but the closest analogue found (Reader treats any file dropped
  on an open window as an OPEN request) points the same direction —
  not toward inline embedding.
- **Both products** reserve true inline-page image placement for a
  distinct, deliberate "Add Image" tool: file-open dialog, then
  click-to-place or click-drag-to-size. Never raw OS drag-and-drop onto
  an existing page.

**Why this matters:** the request cannot be scoped as a parity-match
acceptance criterion ("do what Acrobat does") for the canvas-drop case,
because Acrobat doesn't do it either. It has to be scoped and reported
to `pdfce-engineer` explicitly as a **capability decision pdfce makes
independently** — a genuine exceed-both-reference-products feature, not
a parity target. Both new RAG files flag this explicitly in their
pdfce-parity-notes sections precisely so a future session doesn't
misread "pdfce should do X" as "Acrobat does X."

**How to apply:** when a future operator request sounds like "make
pdfce do [thing] the way other PDF software does," don't assume the
reference products actually converge on that behavior — verify first.
This is the second time in this RAG's history (after the redaction
Apply-vs-Sanitize removal-scope conflict) that a seemingly-obvious
"just match the reference products" request turned out to require
fresh sourcing rather than recall, and the answer changed the framing
of the acceptance criteria rather than just filling in details.

See also [[feedback-helpx-fetch-reliability]] for this same session's
fetch-reliability data point, and
`D:\Dev\Rag-Specialized\Acrobat_Features\core_ops__drag_and_drop_surface.md`
for the full sourced drop-target catalog (page-panel PDF drops DO have
genuine parity ground — only the image-onto-canvas case lacks it).
