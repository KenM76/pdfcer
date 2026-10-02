# Decision 174 — A replacement the run's font refuses may be set in a same-face sibling resource

- **Date:** 2026-10-02
- **Status:** DECIDED; shipped in `Pass 430.1` (sibling slice).
- **Authored by:** `pdfcer-engineer`, under the operator's direction that
  text be editable in every case through workarounds the operator opts into.
- **Trigger:** pdfcer-gui request G075 — a typed character the run's
  embedded subset cannot carry, while another font on the same page (the
  same face, subset differently) already shows it. Word and Chrome emit
  exactly this: one face as a `/Type0` subset and a simple subset side by
  side.
- **Amends:** nothing. Runs after decisions 172 and 173, never instead of them.
- **Clauses:** ISO 32000-2 §9.3.1 (`Tf` sets font and size in the text
  state), §9.4.3 (`Tj`, `TJ`), §9.4.4 (advance), §9.6.4 (subset tag),
  §7.3.5 (name escaping), §7.8.3 (resource names).

## 1. Decision

With `EditOptions::sibling_fonts` (default `false`), when every route that
keeps the run's own font refuses, the edit tries each other `/Font`
resource on the content stream's resources whose `/BaseFont`, subset tag
stripped, equals the run's. Candidates are taken in resource-key order; the
first that encodes the whole replacement wins. A candidate must:

1. be a different dictionary;
2. pass `classify_font`;
3. share the run's writing mode;
4. encode the replacement, extensions (decisions 172/173) included, under
   its own resource name.

The anchor operator is rewritten as
`pre Tj/TJ  /Sib size Tf  (new) Tj  /Own size Tf  post Tj/TJ [pin]`.
No font object is written; only the content stream changes.

## 2. Scope

- One `Tj`/`TJ` holds the whole match. A match spanning operators or text
  objects, or shown by `'`/`"` (whose line move would separate from its
  text), is not split.
- The **whole replacement** goes into the sibling. A replacement mixing
  characters only the run's font carries with characters only a sibling
  carries refuses; splitting one replacement over two fonts is not done.

## 3. Geometry

The run's own font measures the glyphs already on the page (span shifts,
the matched part's old advance); the sibling measures the replacement. The
size restored is the anchor's `Tf` size. A pinned follower's compensating
number is emitted after the restore, so it is scaled by the run's own font.

## 4. Disclosure (rule 4)

Same face is inferred from the name only, so the edit reports:
`'{own}' cannot carry the replacement, so it is set in '{sibling}' (font
resource /X), another font on this page taken to be the same face from its
name; the text around it stays in '{own}'`. The CLI prints it.

## 5. Surfaces

- Preview: `TextEditPreview.font_resource`/`font`/`base_font` name the
  sibling, so the preview draws in it.
- Repertoire: `run_repertoire_with` adds the characters any sibling accepts.
- CLI: `edit-text --sibling-fonts`.

## 6. Rejected

- **Matching by outline comparison.** Two subsets of one face share only
  the glyphs both kept; comparing those proves little and costs a program
  parse per candidate. The name is the producer's own claim, and it is
  disclosed as such.
- **Adding a new font resource.** That is `Pass 430.3` (route B), which
  builds a new font over the same program; this decision reuses what the
  page already has.
