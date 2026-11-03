# `fixtures/synthetic/xfa/fonts/` — provenance

**Generator:** `tools/gen-xfa-font-fixtures.py` (fontTools), run from the repo
root with no arguments. Head timestamps are pinned, so a rerun reproduces the
same bytes.

**Rights (rule 7, `docs/LEGAL.md` §5):** synthetic and self-authored. Every
face has one box glyph for U+0021–U+007E, a space of half the advance, upem
1000, ascent 800, descent −200, cap height 700.

**Consumed by:** `pdfcer_render::font::xfa_fonts` unit tests and the
`pdfcer xfa-materialize --font-dir` CLI test.

| File | Family / style | Advance | Pins |
|---|---|---|---|
| `XfaTestSans-Regular.ttf` | Xfa Test Sans Regular | 800 | glyf → `FontProgramKind::TrueType` |
| `XfaTestSans-Bold.ttf` | Xfa Test Sans Bold (weight 700) | 900 | bold match |
| `XfaTestSans-Cond.ttf` | Xfa Test Sans Condensed (width 3) | 400 | normal width preferred |
| `XfaTestSerif-Regular.otf` | Xfa Test Serif Regular (CFF) | 700 | name-keyed CFF → `OpenTypeCff` |
| `XfaTestLocked-Regular.ttf` | Xfa Test Locked Regular, fsType 2 | 600 | Restricted → embedding refused |
