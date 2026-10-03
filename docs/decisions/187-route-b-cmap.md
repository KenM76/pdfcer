# Decision 187 — Route B never shares a font program that carries a `cmap`

- **Date:** 2026-10-02
- **Status:** DECIDED; implementation in `Pass 430.3`.
- **Authored by:** `autonomous-builder` / KenAgent.
- **Amends:** decision 172 §1 (route B "reuses the same `FontFile2` stream").
- **Clauses:** ISO 32000-2 §9.9 and Table 126 (TrueType program: a `cmap`
  "shall" be present under a simple font and "shall not be present" under a
  CIDFont), §9.7.4.2 (`/CIDToGIDMap`), §9.8 and Table 124 (`/CIDSet`).

## 1. Decision

Route B's new `/Type0` + `/CIDFontType2` resource gets its program as follows:

- **The program has no `cmap`:** share the existing `FontFile2` stream.
- **The program has a `cmap`:** write a new `FontFile2` stream: the same
  program with only the `cmap` table removed. GIDs are unchanged, so
  `/CIDToGIDMap /Identity` still holds. The original stream is untouched.

`EditOptions::cid_font_program: CidFontProgram` — `StripCmap` (default) or
`ShareStream` (option (a): always share; smallest file). CLI:
`--cid-font-program strip|share`.

## 2. Why

- Sharing a simple subset's program, which must carry a `cmap`, under a
  CIDFont breaks a "shall not". Spec fidelity forbids shipping that silently.
- Always copying adds a byte-identical duplicate whenever the program already
  has no `cmap`; this rule copies only when it must.
- R259 and R107 hold: no existing program stream is modified; the copy is a
  new object, like every other route-B object.
- `ShareStream` is defensible (readers use `/CIDToGIDMap` and ignore the
  `cmap`), so it is offered, but it is not the default.

## 3. Stripping

- Remove the `cmap` table record and its data; rewrite `numTables`,
  `searchRange`, `entrySelector`, `rangeShift`; recompute
  `head.checkSumAdjustment`. All other tables stay byte-identical.
- Done by the `pdfcer-render` sfnt writer (decision 173) through the existing
  core→render trait (`EmbeddedGlyphs::program_without_cmap`); `pdfcer-core`
  gains no dependency.
- One stripped copy per source program per document: a later route-B edit
  reuses the `/Type0` resource pdfcer already added for that program.
- New `/FontDescriptor`; same `/FontName` (same glyph set); PDF/A-1
  `/CIDSet` per decision 172 §5. Same `/Filter`; new `/Length1`.

## 4. Overrides

- XMP claims PDF/A: `ShareStream` is overridden to `StripCmap`.
- `OS/2.fsType` usage value 2 (restricted): no second copy is made; the
  stream is shared.

## 5. Disclosure (rule 4)

The decision 172 §6 route-B entry adds the program used: shared, or a
stripped copy (object number). If the shared program carries a `cmap`
(because the operator chose `ShareStream` or fsType 2 forced sharing),
`EditReport` and the CLI state the §9.9 non-conformance.

## 6. Decision 177's augmented stream (Pass 430.4)

Decision 177 §1 copied a `CIDFontType2` program's existing `cmap` into the
augmented (new) stream, breaking the same "shall not". The same mode now
governs it. The stream is always new, so there is nothing to share:
`StripCmap` and `Off` (which only switches route B off) leave the `cmap`
out, `ShareStream` keeps it and discloses the non-conformance, and a PDF/A
claim forces the strip. A program `program_without_cmap` cannot strip keeps
its `cmap`, disclosed. The identity check never read that `cmap` (decision
177 reads the document's own pairs), so dropping it changes no glyph.

## 7. As implemented (Pass 430.3)

- The stripped copy is staged uncompressed, like every program pdfcer
  writes (`/Length1` = `/Length`), not under the source stream's `/Filter`.
- A third mode, `CidFontProgram::Off` (CLI `off`), turns route B off.
- An unreadable XMP packet counts as a PDF/A claim for §4, leaning safe.
- A program that is not a single TrueType sfnt (a collection, or no `glyf`)
  cannot be stripped and is shared, disclosed as for fsType 2.
