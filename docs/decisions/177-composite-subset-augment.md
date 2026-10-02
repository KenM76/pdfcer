# Decision 177 — Subset augmentation for a `/Type0` font over a `CIDFontType2`

- **Date:** 2026-10-02
- **Status:** DECIDED; shipped in `Pass 430.1` (composite slice).
- **Authored by:** `pdfcer-engineer`.
- **Trigger:** pdfcer-gui request G075 (b): the decision 173 route, for the
  composite fonts Word and Chrome emit (`/Type0`, `/Identity-H`,
  `CIDFontType2`, subset `FontFile2`).
- **Amends:** decision 173 §6. That section deferred `/Type0` + `CIDFontType2`
  to a later Pass; this decision is that Pass. Every 173 rule not restated
  here (I1-I5 identity check, hinting, tag derivation, `fsType`, settings,
  disclosure, one undo entry) applies unchanged.
- **Clauses:** ISO 32000-2 §9.7.4.2 and Table 117 (`/CIDToGIDMap`),
  §9.7.4.3 (`/W`), §9.7.6.1 and Table 121 (`/Type0` `/BaseFont`), §9.8 and
  Table 124 (`/CIDSet`), §9.9 and Table 126 (a `CIDFontType2` program
  "cmap table is not needed and shall not be present"), §9.10.3
  (`/ToUnicode`).

## 1. The program carries no cmap, so the PDF stands in for it

The identity check and "which characters does the subset already draw" come
from `(glyph, character)` pairs the document gives, not the program: each
CID the `/ToUnicode` maps, sent through `/CIDToGIDMap` to its glyph
(`ProgramAddressing::CidKeyed`). A `cmap` the program does carry is copied
unchanged and never consulted; appending to it would make two sources of
truth that can disagree. The appended glyph ids come back in
`AugmentedProgram::glyph_ids` for the caller to assign CIDs.

## 2. CID assignment

- **`/CIDToGIDMap /Identity`:** CID = GID. No choice exists.
- **A map stream:** the first CID at or above the new GID (then from 1)
  that the map leaves at GID 0, the content does not show, `/ToUnicode` does
  not map and this edit has not already taken. The map is zero-extended to
  reach it, bounded by `MAX_MAP_BYTES` (2 × 0x10000). Starting at the GID
  keeps a near-identity map near-identity. An unshown, unmapped CID is free
  by construction: nothing in the document can reach it.
- **`/W`** gains `cid [width]` per new CID; **`/ToUnicode`** gains a
  `bfchar` per new CID. Both are copy-on-write.

## 3. `/CIDSet`, re-tag, and a program another font shares

- **`/CIDSet`:** when present, a new stream with each new CID's bit set
  (`0x80 >> (cid & 7)` in byte `cid >> 3`, Table 124), referenced by the new
  descriptor. The old stream is untouched. Absent stays absent.
- **Re-tag:** the `/Type0` `/BaseFont`, the descendant `/BaseFont` and the
  descriptor `/FontName` take the same new tag. The `/Type0` name is
  re-tagged by prefix, so a `-Identity-H` suffix survives.
- **A shared `FontFile2`:** the brief proposed refusing one by name. It is
  instead **copied**: the edited font's new descriptor points at a new
  program, and every other font keeps the old object byte-identical. This is
  the simple route's behaviour already, since 173 never rewrites a program
  in place. A refusal would block an edit that harms no other font.

## 4. Refused by name

- **A CFF `CIDFontType0`:** "the descendant is a CFF-based CIDFontType0, and
  only a TrueType program (CIDFontType2) can have glyphs appended".
- **A CMap other than `/Identity-H`:** "only an /Identity-H composite font
  can be extended so far". `/Identity-V` is refused earlier, as vertical.
- **An embedded CMap stream:** refused before this route by font
  classification, with that classifier's message (a known imprecision; see
  §5).

## 5. Known gaps

- An embedded-CMap refusal names the classifier's cause, not this route's.
- Under a map stream, the typing repertoire does not offer characters the
  program already holds at an unmapped GID; the edit refuses when nothing
  needs appending.
- The disclosure lists every character the run's font was missing, not only
  those that needed a glyph appended.
