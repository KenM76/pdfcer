# Decision 172 — An existing embedded-subset font dictionary may be extended by addition only

> **Renumbered 2026-10-01 by `pdfcer-librarian`.** This record was first
> committed as `docs/decisions/040-additive-font-dict-extension.md`
> ("decision 040"), minted by reading the highest filename in
> `docs/decisions/` (039) rather than `ARCHITECTURE.md` §12's own ceiling
> — exactly the mistake this directory's `README.md` names and warns
> against. §12 already has a decision 040 (2026-08-11,
> `print_render_options`). Renumbered to **172**, §12's real next-free
> slot at filing time. Content below is otherwise unchanged from the
> original record. See `ARCHITECTURE.md` §12's decision 172 entry for the
> full renumbering note.

- **Date:** 2026-10-01
- **Status:** DECIDED; implementation in `Pass 430.0` (route A, simple
  TrueType, existing code) with the remaining routes split into later Passes
  (`430.2`, `430.3`; `430.1` separately, needs its own decision — §7 below).
- **Authored by:** `autonomous-builder` / KenAgent.
- **Trigger:** pdfcer-gui request G075(a) — characters whose glyph outline is
  present in an embedded subset program are refused by the embedded-subset
  floor (`R-INV-1`) because the page never shows their code.
- **Amends:** decision 021 / `R107`.
- **Clauses:** ISO 32000-2 §9.6.6.4 (TrueType encodings; the `(3,1)` cmap
  route for a nonsymbolic font), §9.6.2 (simple font dictionary `/FirstChar`,
  `/LastChar`, `/Widths`), Table 122 (`/MissingWidth`), §9.10.3
  (`/ToUnicode`), §9.7 (`/W`, `/CIDToGIDMap`).

## 1. Decision

**Route A is the default:** make the character showable through the existing
font dictionary — use a code that is unused everywhere that dictionary is
reached, extend `/Widths` (or `/W`) from the program's `hmtx`, extend
`/ToUnicode` — and never modify the font program.

**Route B is the automatic fallback** when any route-A guard fails: a new
`/Type0` + `/CIDFontType2` dictionary that reuses the same `FontFile2` stream
under a new resource name, reaching the glyph by GID through `/CIDToGIDMap`.
An `EditOptions` override may force B or refuse.

**Refuse (C)** only when the glyph has no outline in the embedded program —
with a reason per character.

## 2. Rule changes

- **R107 narrowed:** FF-C (donor-face embedding) only ever adds font
  resources and never modifies an existing font **program**. Decision 021's
  reason — a subset cannot gain an outline without changing program bytes —
  is unchanged.
- **New rule (`R259`):** an existing font dictionary may gain a new
  revision only by addition. Only codes/CIDs with no user anywhere the
  dictionary is reached are assigned; no existing
  code→glyph, width or Unicode mapping changes; program streams are never
  modified; `/Encoding`, `/Widths`, `/ToUnicode` and `/CIDToGIDMap` are written
  as new objects (copy-on-write) so a sub-object shared with another font can
  never carry the change to it.

## 3. Why

- Rule 3 (round-trip) protects objects pdfcer did not logically touch. This
  edit logically touches the font dictionary, so no §5 exception is needed.
- Assigning only unused codes leaves every existing show rendering and
  extracting identically on every page sharing the dictionary.
- B's literal R107 purity buys nothing for signatures — it still rewrites the
  page `/Resources` and content stream — and splits the run across two fonts,
  which hurts later edits. So B is the fallback, not the default.
- B is still needed: GID addressing covers symbolic `(3,0)` subsets,
  cmap-stripped subsets without `post` names, and simple fonts whose 256 codes
  are full.
- Acrobat refuses here; parity is a floor. Answers the operator's request for
  "ways to edit text in all cases".

## 4. Guards that route A to B

- the font is symbolic, or a simple TrueType font has no `/Encoding`;
- no unused code remains, or the glyph is reachable only by GID;
- a content stream reaching the dictionary cannot be parsed, so "unused"
  cannot be proven;
- a remap is needed and the font is in AcroForm `/DR`;
- no `/ToUnicode`, and adding one would change extraction of codes in use;
- a CID already shown elsewhere would need a different `/W`.

## 5. Implementation constraints

- One planner serves both `run_repertoire` and `edit_text`, so they agree by
  construction, including the route chosen.
- Width = `hmtx` advance × 1000 / `unitsPerEm`. Extending
  `/FirstChar`/`/LastChar` fills the gap with `/MissingWidth` (0 if absent),
  the value readers already used for those codes.
- An `/Encoding` name becoming a dictionary keeps the same `/BaseEncoding`.
  Prefer a code the base encoding already maps to the target glyph.
- Permissions/DocMDP: the existing encryption and certification gates decide;
  A and B are both page-content edits.
- PDF/A-1: B's new descriptor needs its own `/CIDSet`.
- `/Widths` disagreeing with `hmtx` for glyphs already shown is stated as a
  font-trust note, not refused.

## 6. Disclosure (rule 4)

`EditReport` and the CLI list each added character with its route
(extended dictionary / sibling resource), the code or CID assigned, how the
glyph was found (cmap / `post` name / GID) and where the width came from. A
`post`-name match is labelled an inference. Nothing appears on the page.

## 7. Not covered

G075(b) — copying an outline from an installed face — changes program bytes
and needs its own decision.
