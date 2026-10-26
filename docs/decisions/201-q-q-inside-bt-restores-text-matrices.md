# Decision 201: `q`/`Q` inside a text object restore Tm and Tlm, on every PDF version

Recorded at `Pass 646.9` (`X004`), 2026-10-26. Withdraws item 1 of request `X002`, pinned by `Pass 646.3` (`5bdc277d`).

## Decides

1. Inside a text object, `q` saves Tm and Tlm with the graphics state and the matching `Q` restores them, in all six routes that track the pen (render, text extraction, redaction measurement, object decomposition, the text-edit walker; the content-rounding optimizer already abandons rounding there).
2. The saved pen is restored only into the SAME text object (matched by BT serial). A `q` outside BT restores no pen.
3. No setting; the behaviour is the same for every PDF version.

## Why

The PDF Association's ISO-approved errata to ISO 32000-2:2020 (`pdf-issues` #368, closed 2024-02-18) append to section 9.4.1 that inside a text object `q` and `Q` additionally push and pop Tm and Tlm. The errata text is 2.0-only, but in PDF 1.x `q`/`Q` inside BT is forbidden (special graphics state operators, section 8.2), so a conforming 1.x file never reaches it. For a non-conforming file, every reference reader (Acrobat Reader, Chrome) restores the pen, so there is no second defensible answer to make a setting of.

## Consequence

In `BT ... (AAAA) Tj q /F2 24 Tf (B) Tj Q (SECRET) Tj ET` SECRET is drawn over the 24 pt B, so search-redact of SECRET removes the B too (covered content goes).

## Body-section effect

None beyond the Decision log. New standing rule: none; `R263` stays next free. Next free decision: `202`.
