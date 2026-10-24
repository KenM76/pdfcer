# Decision 200: visual-comparison marks and rules

Recorded at `Pass 645.2` (`6ee68bab`), 2026-10-24. Scoped by `D:\Dev\Rag-Specialized\Acrobat_Features\compare__visual_pixel_scanned_and_page_matching.md`.

## Decides

1. A changed region is an orange (`REPLACED_COLOR`, decision 199 palette) `/Square` revision cloud (`/BE /S /C /I 1`), border 1.5 pt, drawn 3 pt outside the changed pixels, note "Changed appearance (compared with old page N; P px)".
2. A NEW page with no counterpart in OLD gets a blue (`INSERTED_COLOR`) `/Square` frame 4 pt inside its crop box, note "Inserted page: ...". Deleted pages cannot be marked on NEW; they are listed in the report and the summary page.
3. Pages of different size are compared anchored top-left, white beyond the smaller page, and flagged "page sizes differ" (the Acrobat RAG requires a stated rule).
4. Defaults, all printed: scale 1.5 (108 dpi), threshold 48, merge gap 6 pt, min pixels 6, match threshold 0.5.
5. An explicit `--map` overrides automatic page matching.
6. No OCR; scans are compared as pixels.

## Why

Acrobat offers coarse whole-sheet rectangles and automatic page matching only in its scanned/drawings preset, with no sensitivity control and no mapping override. Region clouds keep the mark small and reviewable; stating every default and every non-in-order pairing keeps the inference disclosed rather than silent.

## Body-section effect

None beyond the Decision log. New standing rule: none; `R263` stays next free. Next free decision: `201`.
