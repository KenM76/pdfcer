# Decision 178 — The replacement-face matching ladder

- **Date:** 2026-10-02
- **Status:** DECIDED; shipped in `Pass 436.2`.
- **Authored by:** `pdfcer-engineer`.
- **Trigger:** `edit-text --fallback-font NAME` could resolve only a page
  resource or a standard-14 name, and the decision 175 retype route always
  fell to Helvetica. Passes 430.1, 431.0 and 436.0 each needed "which
  installed face stands in for this run's font", answered once.
- **Amends:** decision 175 (the retype route's floor is no longer always
  Helvetica, §5). The `Pass 431.0` fallback route (no decision record of
  its own) gains a ladder choice, §4.
- **Clauses:** ISO 32000-2 §9.8 (font descriptors), ISO 32000-1 Table 122
  (`/FontFamily`, `/FontStretch`, `/FontWeight`, `/ItalicAngle`) and
  Table 123 (flags: FixedPitch bit 1, Serif bit 2, Italic bit 7), §14.8
  Table 332 (`/StemV` and `/ForceBold` are not a weight); OpenType `OS/2`
  `fsType`, `usWeightClass`, `usWidthClass`, `panose`, `sFamilyClass`,
  `fsSelection`; `post.isFixedPitch`; `name` IDs 1, 6, 16.

## 1. The rungs

A candidate qualifies only when it carries every character the edit needs
from it. Qualifying candidates are ranked, best first:

1. **Exact name** — the face's PostScript name equals the run font's
   `/BaseFont` with any six-uppercase-letter subset tag stripped
   (`postscript_name_matches`, shared with 430.1's augmenter identity check).
2. **Family + class** — same family, compared case-, space- and
   hyphen-insensitively with an `MT`/`PS`/`PSMT` suffix dropped.
3. **Metric equivalent** (Pass 469.0) — the run is a standard-14
   Helvetica/Times/Courier face and the candidate's family is one of its
   metric-compatible equivalents, in this preference order: Arial, Liberation
   Sans, Nimbus Sans / Times New Roman, Liberation Serif, Nimbus Roman /
   Courier New, Liberation Mono, Nimbus Mono. Preference is compared before
   class. Without this rung, a non-embedded standard-14 run (no descriptor, so
   no `/FontFamily`) put every covering face on Coverage, and file order
   decided the tie: Helvetica was replaced by Berlin Sans FB because
   `BRLNSR.TTF` sorts before `arial.ttf`.
4. **Coverage** — any other face.
5. **Standard 14** — the floor, always available: Times, Helvetica or Courier
   by serif/fixed class, with Bold/Italic (Oblique) by weight ≥ 600 and the
   italic flag.

Within a rung the nearest class wins: fixed-pitch mismatch 100, serif
mismatch 50, italic mismatch 20, |Δweight|/20, 3 per width step; ties keep
the shell's order. Fixed pitch dominates because a monospaced run set in a
proportional face (or the reverse) breaks column alignment, which no weight
difference does.

The request's class comes from the font dictionary (the descendant's
descriptor for a `/Type0`): `/Flags`, `/ItalicAngle` (non-zero ⇒ italic),
`/FontStretch`, `/FontFamily`, and the weight text extraction already derives
(`/FontWeight`, else the name); a standard-14 run without a descriptor takes
its built-in descriptor. `/StemV` and `/ForceBold` are not read (§14.8 says they shall not set
weight). An unknown serif bit compares as neither serif nor sans.

## 2. The seam

Core stays filesystem-free and wasm-clean. A shell implements
`text_edit::ReplacementFaces { candidates(chars), plan(candidate, chars) }`;
core ranks (`rank_replacement_faces`, pure) and walks the ranking, asking the
shell to subset each pick until one succeeds (`plan` failing, e.g. a CFF face
the embedder cannot subset, is recorded in `FaceMatch::failed` and the next
face is tried). `EditOptions` is `Copy`, so it holds
`Option<&'static dyn ReplacementFaces>`; the CLI leaks one per process. The
shipped provider is `pdfcer_render::font::InstalledFaces`, which describes
each face from its own `name`, `OS/2`, `post` and `cmap` and expands
collections.

A face with no `name` ID 6 is given a derived name (the family's ASCII
alphanumerics, else `Face<n>`), because a subset `/BaseFont` must have a name
after its tag. The derived name may match rung 1 by coincidence; that is a
correct match by the only name the face has.

## 3. `fsType`: skipped, disclosed, never overridden

A face is skipped when its `OS/2.fsType` says:

- **Restricted License** (usage 2) — no embedding at all.
- **Preview & Print** (usage 4) — embeddable for viewing only, "no edits".
  An edit is exactly what is being done, so it is skipped.
- **Ambiguous** usage (more than one usage bit set) — skipped, not guessed.
- **No subsetting** (bit 8) or **bitmap embedding only** (bit 9) — pdfcer
  embeds a subset of outlines, so either forbids it. Both apply only from
  `OS/2` version 2; a version 0–1 table's high bits are ignored
  (`FsTypeBits::decode(raw, version)`).

Installable (0) and Editable (8) qualify. A face with **no `OS/2` table**
qualifies: there is no restriction to honour.

**There is no override.** This is a licensing call — the font vendor's
stated terms — not a spec ambiguity and not an open operator question; the
"ship both, pick the default" rule does not apply to it. A skipped face is
disclosed with its rung, file and reason so the operator can see why their
preferred face was passed over and supply another.

## 4. When the ladder runs

- **Fallback route (431.0):** an explicit `--fallback-font NAME` or
  `--fallback-font-file PATH` (`EditOptions::fallback`) always wins. The
  ladder runs only when the shell sets `replacement_faces` and no explicit
  face is given — in the CLI, `--fallback-font auto`, over the `--font-dir`
  folders plus the settings file's font folders. Opt-in, like the fallback
  route itself.
- **Retype route (436.0):** always uses the ladder; with no provider it goes
  straight to the standard-14 floor.
- `run-repertoire --fallback-font auto` is refused by name: a repertoire
  count over "whatever face the ladder would pick" would be a different
  number per machine, and the command names one face.

## 5. The retype floor changes

Decision 175's retype set an un-encodable run in non-embedded Helvetica.
It now sets it in the standard-14 face nearest the run's class (a serif bold
run gets Times-Bold, a monospaced run Courier). Helvetica remains the floor
for a sans or unknown run, so most existing behaviour is unchanged; the
choice is disclosed either way.

## 6. Disclosure

`FallbackUse::chosen_by: Option<FaceMatch { rung, face, source, skipped,
failed }>`; `FaceMatch::disclosure()` reads
`replacement face: '<face>' (<rung label>) from <source|the standard 14
fonts>` and names up to five skipped faces (`'<name>' (<file>: <reason>)`)
then "and N more", plus any that failed to subset. Only skips ranked **at
or above** the pick's rung are named: a restricted face that would have lost
to the pick anyway is not why the pick was made. The CLI prints
`  face_match=<face> rung=<label> skipped=<n> failed=<n> source=<path|standard-14>`
(source last, since a path may hold spaces) and the disclosure line.

## Known limits

- Only a TrueType (`glyf`) face can be subset for embedding; a CFF face is
  ranked, fails to plan, is disclosed in `failed`, and the ladder falls past
  it.
- `auto` re-reads the font folders the CLI's font environment already read.
- No settings key selects `auto`; it is a per-invocation flag.
