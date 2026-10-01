# Decision 173 — An embedded TrueType subset may gain glyphs from its installed face, as a new program

- **Date:** 2026-10-01
- **Status:** DECIDED; implementation in `Pass 430.1`.
- **Authored by:** `autonomous-builder` / KenAgent.
- **Trigger:** pdfcer-gui request G075(b) — a typed character whose outline
  the run's embedded subset lacks, while the shell holds the installed face
  the subset was cut from.
- **Amends:** decision 172 / `R259`. Extends `R109` to a second carrier.
- **Clauses:** ISO 32000-2 §9.6.4 (subset tag, ST1–ST4); §9.6.6.4
  (nonsymbolic TrueType: code → name → Unicode → `(3,1)`); §9.8 Table 122;
  §9.9 Tables 126–127 (`FontFile2`, `/Length1`; E3); §9.10.3. OpenType 1.9.1
  `glyf` `loca` `head` `hhea` `hmtx` `maxp` `cmap` `post` `OS/2.fsType`.
  ISO 19005-1 §6.3.5–6.3.6; ISO 19005-2/-3 §6.2.11.4–6.2.11.6.

## 1. Decision

With `EditOptions::subset_augment` set (default `None`), a character with no
outline in a simple-TrueType subset is made showable **in the same font**:

1. a shell-supplied face passes the identity check (§3);
2. a **new** `FontFile2` stream is written: the old program, plus the
   character's glyph and its composite components appended after the last
   GID;
3. a **new** `FontDescriptor` references it and carries a fresh tag (§5);
4. the font dictionary is revised as decision 172 route A does (unused code,
   `/Widths`, `/ToUnicode`, `/Encoding`), plus `/BaseFont` and
   `/FontDescriptor`.

The old stream and descriptor are never modified. Any failed check **refuses
by name** so the caller falls through to `431.0`; 430.1 never settles for a
weaker match. Unset, behaviour is unchanged. The edit is one undo entry, and
incremental save writes three new or revised objects.

**Rejected: in-place modification.** A stream may serve several
dictionaries, and rewriting it changes all of them. It also leaves one tag
naming two different subsets, which ST4 forbids.

**API.** `EditOptions::with_subset_augment(SubsetAugment)`, with these
fields:

- `source: &'static dyn SubsetAugmenter` — implemented in `pdfcer-render`
  over the shell-supplied faces (`SuppliedFace {label, bytes,
  collection_index}`), using `436.2` rung 1 only. Same injection pattern as
  `embedded_glyphs`; core never sees a path.
- `outline_check: OutlineCheck` — `AllShared` (default) or `ShownOnly`.
- `hinting_mismatch: HintingMismatch` — `Strip` (default) or `Refuse`.

## 2. Rule changes

- **`R259` amended**, replacing "program streams are never modified": *an
  existing program stream is never modified. Under decision 173 only, a font
  dictionary revision may reference a new descriptor and a new program that
  is a strict superset of the old one. In it, every existing GID keeps its
  `glyf` record byte-identical, and keeps its `hmtx` entry and every cmap
  mapping; glyphs are appended only; and the program carries a new tag.
  Other users of the old stream or descriptor keep them.*
- **New `R260`.** Before commit, an augmented program is re-parsed with the
  existing parser (`R21`) and verified:
  - `numGlyphs` = old + added;
  - old GIDs are byte-equal, and old cmap mappings are equal;
  - new mappings reach outlines equal to the face's;
  - checksums and `checkSumAdjustment` are valid.

  A failure refuses as an internal error. The check is never skipped for
  speed.
- **`R109` covers both carriers.** It runs on the face and on the subset's
  own `OS/2` when present; either failing refuses. The subset's `OS/2` is
  copied byte-identical, so OpenType C2 holds.

## 3. Identity check — all rows required; only I5's scope has a setting

| # | Check | Failure |
|---|---|---|
| I1 | Name ID 6 = `/FontName` with the tag stripped | not a candidate |
| I2 | `glyf` flavour (`0x00010000`), no `fvar`; `.ttc` member selected by I1 | `FaceNotTrueTypeOutlines` |
| I3 | `R109` passes on face and subset (§6) | the `R109` reason |
| I4 | `head.unitsPerEm` equal | `UnitsPerEmMismatch` |
| I5 | Each compared glyph has an equal **flattened outline** (contour ends, points, on-curve flags; composites resolved with transforms; instructions excluded) and an equal `hmtx` advance | `OutlineMismatch{U+,gid}` / `AdvanceMismatch` |
| I6 | At least one non-empty outline was compared | `IdentityUnproven` |
| I7 | The face maps the character in `(3,1)` or `(3,10)` | `FaceLacksCharacter` |

The compared set is every glyph the subset's cmap reaches whose Unicode the
face also maps. `ShownOnly` narrows it to glyphs reached by codes in use,
which tolerates a revision that changed an unrelated glyph. `fontRevision`
and name ID 5 are disclosed but not checked, because the outline is the
evidence. Where `/Widths` disagrees with `hmtx`, the result is a font-trust
note (172 §5).

## 4. Program surgery (`pdfcer-render`; core receives plain data)

- **`glyf`:** old bytes are copied verbatim; new records are appended at
  4-byte alignment.
- **Composites:** components are copied recursively as fresh GIDs,
  deduplicated within the edit, with GIDs remapped. Subset glyphs are never
  reused as components. A depth above 16, or a cycle, gives `MalformedFace`.
- **`loca`:** rebuilt. It switches from short to long
  (`indexToLocFormat` = 1) when an offset exceeds `0x1FFFE`.
- **`hmtx` / `hhea`:** a trailing run (`numberOfHMetrics` < `numGlyphs`) is
  expanded to full entries, then the new entries are appended.
  `numberOfHMetrics`, `advanceWidthMax`, `min{Left,Right}SideBearing` and
  `xMaxExtent` are recomputed.
- **`maxp`:** `numGlyphs` is updated, and v1.0 point, contour and component
  maxima take `max` with the new glyphs. When instructions are copied, the
  instruction, stack, function, storage and twilight maxima take
  `max(subset, face)`.
- **`cmap`:** the entry is added to every Unicode subtable present (`(0,*)`,
  `(3,1)`, `(3,10)`), and to `(1,0)` only for a Mac OS Roman character.
  Formats 0, 4, 6 and 12 are rewritten. `(3,1)` must be present, else
  `MissingUnicodeCmap`.
- **`post`:** 3.0 is unchanged. 2.0 appends the face's names (`uniXXXX` if
  the face's `post` is 3.0). 1.0, 2.5 and 4.0 give `UnsupportedPostFormat`.
- **`head`:** the bbox is unioned, and checksums and `checkSumAdjustment` are
  recomputed. `modified` and `fontRevision` are untouched, for deterministic
  output.
- **Copied raw:** `OS/2` `name` `cvt ` `fpgm` `prep` `gasp` `kern` `GDEF`
  `GSUB` `GPOS` `VDMX` `PCLT`.
- **Dropped:** `hdmx` and `LTSH` (sized by `numGlyphs`); `vhea` and `vmtx`
  (simple fonts are horizontal); `DSIG` (no longer valid).
- **Any other table** gives `UnsupportedTable{tag}`. This includes bitmap and
  colour tables, where the new glyph would paint by a different route than
  its neighbours.
- **Descriptor:** copy-on-write. `/FontBBox` and `/MaxWidth` are widened
  (× 1000/`unitsPerEm`), and every other key is kept.
- **Stream:** Flate, `/Length1` = decoded length. The size is bounded by
  `MAX_DONOR_BYTES`, else `ProgramTooLarge`.
- **Writer:** purpose-built for these tables. It adds no dependency;
  untouched tables are copied raw.

## 5. Hinting and subset tag

- **Hinting.** Instructions are copied only when `fpgm`, `prep` and `cvt `
  are each byte-equal between the subset and the face; if all three are
  absent from both, there is nothing to copy.
  - **Otherwise, `Strip` (default)** sets `instructionLength` to 0. The
    outline is identical; only low-ppem grid fitting can differ from the
    hinted neighbours.
  - **`Refuse`** refuses instead.
  - **Never offered:** copying instructions against a different `fpgm` or
    `cvt `. They call functions and CVT slots that may not exist, which is
    undefined interpreter behaviour.
- **Tag.** The tag is always new, so ST4 holds whether or not the old program
  stays reachable. It is derived deterministically, like `subset_tag_for`,
  from the stripped name, old tag and appended GIDs. It is checked against
  every `/BaseFont` and `/FontName` in the file, and stepped
  deterministically on a collision. `/BaseFont` and `/FontName` change
  together (ST3).

## 6. Scope of the first cut

- **In scope:** simple `/TrueType`, nonsymbolic, `FontFile2` with `glyf`.
- **Refused by name:** `FontFile3` (CFF/OpenType), `FontFile` (Type 1),
  Type 3, and symbolic subsets (`(3,0)`).
- **`/Type0` + `CIDFontType2`:** a later Pass. It needs `/W`,
  `/CIDToGIDMap` and a PDF/A-1 `/CIDSet`, and §9.9 forbids a `cmap` in its
  program.
- **`fsType`**, applied to the face and the subset (`R109`):
  - usage value 2: skip (fixed by `436.2`);
  - bit 8: `SubsettingNotPermitted`, because 430.1 embeds a subset of the
    face;
  - bit 9: `OutlineEmbeddingNotPermitted`;
  - bits 8–9 on `OS/2` v0/v1: inert;
  - value 4, or absent `OS/2`: proceed, as `R109` does today (question (r)).
- **Code:** allocation, `/Widths` and `/ToUnicode` follow 172 route A. The
  width is the face's `hmtx` advance × 1000/`unitsPerEm`, the same advance
  the new `hmtx` carries.
- **Guards:** every 172 §4 guard refuses here; there is no route-B pairing.
  A new guard, `ShownCodeWouldChange`, refuses when a code in use anywhere
  the dictionary is reached resolves to a cmap entry being added. That code
  paints `.notdef` today and would change.
- **Sharing:** every user of the dictionary renders its shown codes
  identically, because old GIDs are unchanged. The old stream and descriptor
  keep serving other dictionaries under their old tag.
- **Session:** one edit's missing characters go into one new program. A
  program minted earlier in the same unsaved session is superseded, not
  stacked, and must not reach the saved file; verify this against the
  writer's unreferenced-object rule.

## 7. PDF/A, signatures, licensing basis

- **Widths:** `/Widths` and `hmtx` carry the same advance, so §6.2.11.5 and
  §6.3.6 hold.
- **cmap:** the added `(3,1)` entry satisfies §6.2.11.6.
- **`/CIDSet`, `/CharSet`:** not applicable.
- **§6.2.11.4.1:** see §9 item 1.
- **Signatures and DocMDP:** a page-content-class edit; the existing gates
  decide (172 §5).
- **Licensing basis:** the outline comes from the operator's licensed face,
  not from a copy extracted from the PDF (§9.9 E3).

## 8. Disclosure (rule 4)

`EditReport` and the CLI's post-command report give one entry per augmented
font. It is labelled an inference, and nothing appears on the page:

> Added 2 glyphs to embedded subset `ABCDEF+Arial-BoldMT` (now
> `QKXRTM+Arial-BoldMT`) from installed face `<label>` (`Arial-BoldMT`,
> Version 7.00, fontRevision 7.0; subset fontRevision 7.0). Identified as the
> same font (inference): PostScript name, unitsPerEm 2048, 412 of 412 shared
> glyph outlines and advances equal (all shared). `é` U+00E9 → code 233,
> GID 4651 (components 4652, 4653), width 556 from the face's `hmtx`;
> hinting copied (fpgm/prep/cvt identical) | stripped (fpgm differs).

## 9. Open for Ken (safe default ships until answered)

1. **PDF/A §6.2.11.4.1, "legally embeddable for unlimited, universal
   rendering":** does fsType usage value 4 or 8 qualify? Default: in a file
   claiming PDF/A (XMP `pdfaid`), augment only from usage value 0;
   otherwise refuse with `PdfaEmbeddingUnconfirmed`.
2. **Value 4 and absent `OS/2`:** still question (r). 430.1 inherits
   `R109`'s current posture and adds no second policy.

## 10. Not covered

- `/Type0` + `CIDFontType2`
- CFF and Type 1
- pairing with route B
- variable-font sources
- repair of shown codes that paint `.notdef`
