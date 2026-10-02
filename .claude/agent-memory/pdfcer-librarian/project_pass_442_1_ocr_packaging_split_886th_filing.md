---
name: project-pass-442-1-ocr-packaging-split-886th-filing
description: Pass 442.1 (OCR packaging split, only paddle bundled) filed 886th filing, be16b008 not yet pushed at filing time
metadata:
  type: project
---

Pass 442.1 shipped 2026-10-02 (886th filing), `be16b008`, on `main` but
**not yet pushed** at filing time per the dispatching engineer's own
report (not independently checked — no shell, hard rule 8). Only
`paddle` ships bundled in the portable folder and is now the CLI
default; `ocrs`/`ocrcer`/`tesseract` became add-on zips via
`tools/package-portable.py`'s new `split_ocr_addons()`.

Closes the packaging half of the `Pass 442.x` OCR-add-on family begun
at `Pass 442.0` (880th-ish filings). Only `Pass 442.4` (`pdfcer-gui`'s
model drop-down) remains queued in that family.

**Why this matters for future filings:** this Pass answers a factual
sub-question of open operator question `(bl)` (does the portable
folder ship a CC-BY-SA-4.0 file) without resolving `(bl)` itself — after
this Pass the portable folder ships none of that licence, but Ken still
rules on `(bl)`. Don't read "packaging split shipped" as "`(bl)` closed"
in any future sweep.

**FEATURES.md rows touched** (both under *Implemented → Text*, not
moved sections): "Choose the OCR engine" (default/bundling claims
corrected — was stale, said `default ocrs`/"ocrcer ships built in by
default") and "Packaging split: only PaddleOCR …" (boxes → `cli [x]`,
marked SHIPPED; core/gui stayed `[ ]`/`[ ]`, not rounded up). Neither
row crossed the 1200-char cap (verified via `Grep ^.{1200,}$` — no
shell available for the official checker script).

**Verify before trusting:** confirm `be16b008` actually reached
`origin/main` before citing it as pushed in any later filing.
