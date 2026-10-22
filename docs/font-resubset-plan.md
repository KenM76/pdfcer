# Font re-subsetting inside optimize — plan (Pass 616.6 slice 2)

Shrink an embedded TrueType (`FontFile2`) or CFF (`FontFile3`) program to the
glyphs the document draws. Typical target: Word/LibreOffice files that embed whole fonts.

## Method: GID-preserving glyph zeroing

Every table and every glyph ID stays. Unused `glyf` records become empty
(`loca` rewritten in its original format, so `head` is unchanged); GID 0 and
the composite closure of every kept glyph stay. Content streams, `/Widths`,
`/W`, `/Encoding`, CIDToGIDMap and ToUnicode are untouched: only the program
stream is rewritten.

Rejected: `subsetter` with renumbering. It drops `cmap`, which a simple
TrueType font needs to reach its glyphs (ISO 32000-1 §9.6.6.4); renumbering
also forces a CIDSet rewrite. Small extra saving, much more risk.

## Glyph-usage enumerator (core, `optimize/glyph_usage/`)

A new small interpreter, not text_extract's `Walk`. `Walk` is page-only, emits
characters rather than raw codes, and misses annotation appearances, patterns,
soft masks and Type3 CharProcs.

- **Roots:** every page; every annotation `/AP` N/R/D stream and state; every
  resource-owning stream `fontinfo::inventory()` lists that no invocation
  reached. These run with no inherited font.
- **Operators:** q/Q (font stack capped at 256), `Tf`, `Tj ' " TJ` (raw
  codes keyed by font dict id), and `Do` form (font carried in, §8.10.1;
  cycle guard, depth cap `MAX_FORM_DEPTH`).
- **Run in their own resources, with no inherited font:** `scn`/`SCN`
  tiling patterns, `gs` `/SMask /G` groups, and every Type3 CharProc.
- **Codes:** 1 byte for simple fonts; 2 bytes for Type0 Identity-H/V. Any
  other CMap is refused.
- **Referrer check:** every object referring to a candidate font dict must
  be a walked owner, or a `/Font`/`/Resources` dict whose own referrers all
  are. For a CIDFont, its Type0 parent is checked in turn. The kept set for
  a shared program is the union of all its users' glyphs.

## Refusals (token: reason, off-canvas)

| Token | When |
|---|---|
| `acroform-dr` | The font is reachable from AcroForm `/DR`. `/DA` names resolve against `/DR`, so this also covers every `/DA` font without tokenising `/DA`. |
| `type3` | A Type3 font. |
| `unreadable-content` | Any undecodable stream, limit hit or unattributed show. This refuses all programs. |
| `unreferenced-user` | A user of the program was not walked, or is an orphan. |
| `font-not-indirect`, `program-direct` | The font dict or program is not an indirect object. |
| `non-identity-cmap` | A Type0 font whose CMap is not Identity-H/V. |
| `unmapped-code` | A used code that no GID rung resolves. |
| `fstype-restricted`, `fstype-no-subsetting` | The program's fsType forbids it. |
| `collection` | The program is a `ttcf` collection. |
| `format-deferred` | `FontFile` (Type 1). |
| `malformed-glyf` | The `glyf` data is malformed. |
| (note, no token) | The document claims PDF/A: the whole re-subset step is skipped with one note. What PDF/A requires of a subset is sourced in the spec RAG (`pdfa/pdfa__ref__font_subset_cidset_charset.md`); lifting this skip is the next slice. |
| `unembedded-this-run` | The same run unembeds the font. |
| `not-smaller` | The rewritten program is not smaller. |

## Render (`font/resubset/`)

- **Simple TrueType keep-set:** for each code, the union of every candidate:
  - the shipped ladder (`LoadedFont::gid`);
  - Branch A: name → AGL → (3,1);
  - name → Mac code → (1,0);
  - the `post` name;
  - Branch B: (3,0) with high byte 00/F0/F1/F2 → (1,0);
  - a direct (3,1) lookup;
  - GID 0.

  Over-keeping costs only bytes.
- **Composite keep-set:** CID through Identity or the CIDToGIDMap stream; an
  out-of-range CID gives GID 0.
- **Rebuild and verify:** take the composite closure (`glyf::closure`),
  rebuild `glyf`/`loca`, then `sfnt::assemble`. Verify by re-parsing, checking
  `numGlyphs` is unchanged, and checking every kept record is byte-identical.
- **Hook:** a core trait `FontResubsetter` (the same pattern as
  `SubsetAugmenter`), held in `OptimizeSettings::resubset` as a
  `&'static dyn`. It has a hand-written `PartialEq` using `ptr::addr_eq`.

## Core apply (`edit/optimize_resubset.rs`)

- **`plan_resubset`:** drops the rewritten program ids from `changes`,
  `writes` and `skipped`.
- **`check_resubset_fresh`:** re-plans and compares the whole cut, so an
  edit that draws other glyphs, or changes a rewritten object, is stale.
- **`resubset_writes`:** writes the Flate program with `/Length1` and gives
  every cut program a fresh tag unique in the file, written over the stem of
  each font `/BaseFont`, CIDFont `/BaseFont` and descriptor `/FontName`; it
  folds into the Optimize undo entry. A different subset under an old tag
  would break ISO 32000-2 §9.6.4.
- **`removes_content` stays false**, so the never-larger rule applies. A note
  says typing new text in these fonts later needs augmentation.

## Fixtures (`tools/gen-resubset-fixtures.py` → `fixtures/synthetic/resubset/`)

The donor has glyphs A–H plus a composite glyph that uses B.

- `simple`: WinAnsi; composite closure.
- `symbolic`: (3,0) F0xx with `/Differences`.
- `type0-identity`: with a CIDToGIDMap.
- `form-inherited-font`: a form inherits the page's font.
- `ap-states`: the font is used only in an appearance state.
- `pattern-smask`: the font is used in a pattern and a soft mask.
- `type3-charproc`: the font is used inside a Type3 CharProc.
- `shared-program`: two font dicts share one program.
- `orphan-user` (refused): an unreferenced user of the program.
- `unattributed` (refused): a show with no font.
- `acroform-dr` (refused): the font is in AcroForm `/DR` and named in a `/DA`.

The existing `unembed/*` fixtures and the fsType and cycle donors cover the
other refusals.

Built: `simple` and `type0-identity`, both over the synthetic
`text/augment/face.ttf` donor (glyphs A–E plus Eacute = E + acute); the
render test checks both render pixel-identical after the cut. The other
cases are inline core tests instead (`optimize/glyph_usage/tests.rs`,
`optimize/resubset_plan_tests.rs`), which build their documents in code;
`shared-program` (two fonts, two descriptors, one program) and `symbolic` (a (3,0)-only cmap, /Flags 4, no /Encoding) are built fixtures, and the render test checks both.

## Steps

1. Core types: the settings, plan and error.
2. The interpreter (done: `optimize/glyph_usage/`).
3. Roots and referrers.
4. Render GID sets.
5. Render zeroing and verification.
6. Core wiring.
7. CLI `--resubset-fonts`, with pixel-identical render tests.

Steps 1–7 are built. A program the cutter keeps whole is refused
`every-glyph-used`, so it is not renamed for nothing.

Slice 3, merging copies of one face, is built. Fixture `pair`: one
face embedded as a TrueType font and as a Type0/CIDFontType2, which is
Word's pattern. A corpus measurement over 2,915 files found 22 that carry
one face more than once, with up to 31.6% of a file's bytes duplicated.
The merge rule is documented in `docs/core-api/03-capabilities.md`.

CFF zeroing is built (`font/resubset/cff.rs`, `cff_glyphs.rs`), for a bare
CFF (`/Type1C`, `/CIDFontType0C`) and the `CFF ` table of an OpenType
program (`/OpenType`, `DSIG` dropped):

- Each unused charstring becomes a bare `endchar`. Glyph IDs, global and
  local Subrs, charset, Encoding, FDSelect and Private DICTs are copied.
- Kept: glyph 0, the roots, and each kept glyph's `seac` base and accent
  (Adobe TN5177 Appendix C, resolved by StandardEncoding name the way the
  renderer draws them), found by evaluating every kept charstring.
- Rewritten: the CharStrings INDEX, the Top DICT INDEX, and a CID font's
  FDArray. Every absolute offset (charset > 2, Encoding > 1, CharStrings,
  Private, FDArray, FDSelect) is re-encoded as a 5-byte integer and moved
  by the size change of the structures before it.
- Refused `cff-layout`: more than one font in the Name INDEX, a
  CharstringType other than 2, an offset into a rewritten structure, or a
  Private DICT and its Subrs on two sides of one.
- Verified: the cut re-parses with the same glyph count, names, built-in
  encoding and subfonts, and every kept glyph draws the same path and
  advance; otherwise `malformed-glyf`.
- Merging copies stays TrueType-only.

Fixtures `type1c` (seac Eacute) and `opentype`, from `tools/gen-cff-face.py`.

## CIDSet and CharSet

A descriptor's `/CIDSet` (ISO 32000-1 §9.8.3 Table 124) and `/CharSet`
(§9.8.1 Table 122) are left byte-identical. A glyph is present in a program
by its structure (an ID below `numGlyphs`, a CFF charset entry, a Type 1
`CharStrings` key), never by its outline; an empty `glyf` record and a bare
`endchar` charstring are still present (veraPDF issue #811 ruling; spec RAG
`pdfa/pdfa__ref__font_subset_cidset_charset.md`). So a zeroing cut leaves
the present set, and with it a correct list, unchanged. Rewriting a list to
the drawn glyphs makes it under-claim, which fails PDF/A-2/-3
(ISO 19005-2 §6.2.11.4.2: a present list must be exact; measured on veraPDF
1.30.2). Fixtures `type0-cidset` and `type1c-charset` pin it.

Deferred: PDF/A, merging CFF copies, Type 1 `/FontFile`, and switching to
the short `loca` format.
