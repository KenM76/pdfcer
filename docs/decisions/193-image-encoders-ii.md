# Decision 193: Image encoders II, R28 narrowed (Pass 616.5)

Consultant: `autonomous-builder` (KenAgent protocol). Narrows standing rule
**R28** (decision 005 §6.1). Builds on decisions 190, 191 and 192.

## Decision

- R28 is narrowed. pdfcer-core gains these encoders, each selectable through
  `image(encode=...)` on all images, a rule, or one object:

  | `encode=` | Implementation | Applies to | Version | Inline |
  |---|---|---|---|---|
  | `runlength` | in-house, ISO 32000-2 §7.4.5 | any image | any | yes |
  | `ccitt` | in-house Group 4 (`/K -1`), ITU-T T.6 + T.4 tables | 1-bpc, 1-component | any | yes |
  | `jbig2` | in-house generic region, MQ arithmetic, template 0, TPGDON, embedded format, no `/JBIG2Globals` (T.88 §6.2, Annex E) | 1-bit | 1.4 | **no** (§7.4.7, §8.9.7) |
  | `lzw` | `weezl` `Encoder::with_tiff_size_switch(Msb, 8)`: `/EarlyChange 1`, key omitted | any image | any | yes |
  | `auto` | smallest eligible lossless candidate | any image | the document's current version | per candidate |

- **LZW is never chosen; it is written only on request.** It is written only
  when `encode=lzw` is named. No preset, `auto` or fallback ever picks it. It
  is refused by name when the document claims PDF/A. Its report row gives the
  Flate size for the same samples.
- The shipped presets set `encode=auto` on their image rules. An `image(...)`
  with no `encode` key keeps decision 192's base (Recompress gives Flate,
  Leave gives Keep).
- JPX and Brotli are not decided here. Each has its own record, and each
  record decides whether `auto` may use that filter.
- Lossy JBIG2 symbol matching is not part of this record.

## Reasoning

- Ken, 2026-10-21: change images to "whatever PDFs support", on all images
  or per image. An operator request overrides an engineer-written "never".
  Decision 190 narrowed W14 for RC4 to "never chooses" in the same way, and
  R28 names RC4 as its parallel.
- §7.4.4.1 NOTE 1 still holds: Flate is more compact. So pdfcer never picks
  LZW for the user, and it shows the Flate size whenever the user picks LZW.
- The Unisys LZW patents expired in 2003 (US) and 2004 (elsewhere). `weezl`'s
  encoder is already compiled in, so there is no new dependency or licence.
  This is an engineering call, not a legal one.
- The legitimate use of LZW is pre-1.2 readers and TIFF-strip pipelines.
  PDF/A is the opposite case: ISO 19005-1 §6.1.10, 19005-2 §6.1.7.2/§6.1.10
  and 19005-4 §6.1.6.2/§6.1.9 all forbid LZWDecode.
- In-house code is right for RunLength, CCITT and JBIG2. Decision 005 §5.3
  chose `weezl` over in-house code because a decoder faces hostile input. An
  encoder sees only pdfcer's own pixels. The hayro decoders already in the
  tree are an independent test oracle. No mature pure-Rust JBIG2 encoder
  exists, and a C one breaks the static-binary and WASM invariants.
- `auto` goes past parity, where the user picks a mono codec by hand. It is
  safe by construction: lossless, guarded, and within the document's current
  version.

## Defaults

| Setting | Default |
|---|---|
| Presets' image encode | `auto` |
| `auto` candidates | Flate, RunLength, plus CCITT G4 and JBIG2 for 1-bit 1-component images |
| `auto` excludes | LZW, Brotli or any extension, JPEG, JPX (until its record), lossy anything, any filter above the document's version, JBIG2 for inline images |
| `auto` tie-break | Flate, then CCITT, then RunLength, then JBIG2 |
| `auto` raises the version | never |
| Explicit encoder above the document's version | raise the header and disclose; `version-raise=refuse` is the strict option |
| Guard | only-if-smaller plus `min-saving` against the current stream, for every encoder |
| CCITT polarity | the adapter sets `/BlackIs1` (R26 exception) |
| Lossy JBIG2 symbol matching | not shipped |

## Validation and disclosure

- Refused by name: `lzw` under a PDF/A claim, and any filter the declared
  PDF/A part forbids (any override belongs to Pass 616.10); `jbig2` on an
  inline image; `ccitt`/`jbig2` as a per-object override on a source that is
  not 1-bit when the patch has no bilevel step. A rule match on such an image
  is skipped with "not bilevel".
- Each image's report row names the encoder. For `auto` it lists every
  candidate's size. If no candidate passes the guard, the image is kept with
  "not smaller" and the sizes (rule 4).

## Replacement text for R28

> **R28 — Image encoders enter only under a decision record (005, 192, 193).**
> Shipped: JPEG, Flate, RunLength, CCITT G4, JBIG2 generic, LZW. pdfcer never
> *chooses* LZW. It is written only when named explicitly, never by a preset,
> `auto` or a fallback, and it is refused under a PDF/A claim.

## Conditions

- Rule 1: before code, `pdfcer-spec-librarian` files the T.4/T.6 code tables
  and T.88 §6.2 plus the Annex E MQ encoder. Doc comments cite the clauses.
- Round-trip property tests per encoder against the in-tree decoders, a fuzz
  target asserting the encode-then-decode identity, and an Acrobat Reader
  render check on one fixture per encoder.
- No new dependency: `cargo tree -p pdfcer-core` unchanged, rule 2 rechecked.
  The JBIG2 code is split into an MQ module and a generic-region module, each
  within the structure-gate limits.
- Owed fixes at filing: R28 in 005 and the ROADMAP standing-rules index; the
  "never write LZW" line in `filter__lzw.md`; the `weezl` encoder comments in
  `crates/pdfcer-core/Cargo.toml` and `crates/pdfcer-model/Cargo.toml`;
  `FEATURES.md`.

## Alternatives rejected

- **Keep LZW refused, with a CLI explanation:** goes against the operator's
  explicit request, and the request carries more weight than the reasoning
  for a self-imposed "never".
- **LZW as an `auto` candidate:** Flate beats it everywhere NOTE 1 covers,
  so it could only win on noise, and choosing LZW is exactly what R28 guards
  against.
- **Third-party CCITT/JBIG2 encoder crates:** no pure-Rust JBIG2 encoder is
  mature enough; for CCITT a crate saves little over a fixed set of tables,
  and the encoder input is trusted.
- **`auto` allowed to raise the version or use extensions:** that would make
  a preset change compatibility without being asked.
- **Lossy symbol matching in this record:** it can swap characters (Xerox
  scanners, 2013). It needs its own record and a heads-up to Ken.

## Risks

- `auto` costs time on large scans, since every candidate is encoded.
  Measure before adding any speed option.
- Some lightweight viewers decode JBIG2 poorly. The tie-break order and the
  rule that `auto` stays within the document's version limit the exposure.
- Polarity: CCITT `/BlackIs1` and JBIG2's 1-is-black convention must match
  the source image's `/Decode`. The round-trip oracle catches a mistake here
  only if the test compares rendered pixels, not raw samples.

## References

Decisions 005 (R28, §5.3), 190, 191, 192. ISO 32000-2 §7.4.4.1 NOTE 1,
§7.4.5, §7.4.6, §7.4.7, §8.9.7. ISO 19005-1 §6.1.10, -2 §6.1.7.2/§6.1.10,
-4 §6.1.6.2/§6.1.9. ITU-T T.4, T.6, T.88. Spec RAG `filter__lzw.md`,
`filter__jbig2.md` (note 5).
