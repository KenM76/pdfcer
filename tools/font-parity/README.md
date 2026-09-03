# font-parity — embedded-font-program parse/routing regression gate

Out-of-tree corpus gate (mirrors `tools/content-identity` and
`tools/roundtrip`). Guards the **routing correctness** of
`FontProgram::parse` (`crates/pdfcer-render/src/font/program.rs`) across the
external corpus — the targeted guard that catches a font-program *misroute*
at the source, before it reaches the render-parity gate (R59) as the vague
symptom "text missing on a whole class of files."

## The bug it guards

`FontProgram::parse` selects one of three parsers by matching binary magics
on the **raw decoded** font-program bytes:

| Magic (raw first bytes) | Framing | Variant |
|---|---|---|
| `00 01 00 00` / `OTTO` / `true` / `ttcf` | sfnt | `FontProgram::Sfnt` |
| `01 00` | bare CFF | `FontProgram::Cff` |
| `80 01` (PFB) or `%!` after ASCII-whitespace trim (PFA) | Type 1 | `FontProgram::Type1` |

The fixed regression: a leading-whitespace trim that **included NUL** ate the
`0x00` of a TrueType `0x00010000` version tag, leaving `01 00 …`, which then
matched the bare-CFF magic → valid TrueType handed to the CFF parser → "offset
out of bounds" → every standard-version embedded TrueType rejected (real
SolidWorks / AutoCAD / Office `CIDFontType2` files). The fix matches magics on
raw bytes; only the Type-1 *text* path trims whitespace.

## What it asserts

For **every distinct embedded program** (`FontFile`/`FontFile2`/`FontFile3`,
simple fonts and composite/descendant fonts alike) in **every loadable**
corpus file, the program must either:

- **route correctly** — parsed variant agrees with the framing its magic bytes
  imply (an independent oracle, `magic_of`, replicates `program.rs`'s ladder
  exactly and is compared against the parser's actual verdict), or
- **fail clean** — a named `ProgramError` (`UnknownFormat` / `Parse`).

A **misroute** (parsed variant disagrees with magic — the bug signature) or a
**panic** fails the gate. Clean parse failures are reported (real coverage
picture) but do not fail it. Every program parse is `catch_unwind`-wrapped;
each file runs on a worker thread under a 30 s budget so no hang stalls the
sweep.

## Coherence with `fonts_unsupported`

`text.rs` maps any `FontProgram::parse` failure to the single
`UnsupportedFont::UnusableProgram` reason key (→ `Diagnostics::fonts_unsupported`).
This harness's `UnknownFormat` / `Parse` reasons are a strict **refinement** of
that one bucket — never a contradiction. The taxonomies agree.

## Re-run (standing gate — run on any change to `program.rs` or `font/`)

```sh
cd tools/font-parity
cargo test                                   # 5 targeted unit tests
cargo run --release -- --gate ../../fixtures/external   # corpus gate
```

`--gate` exits non-zero on any misroute or panic (the CI-less standing form).
Without `--gate` it always exits 0 after printing the report (measurement run).
Exit 2 = usage; exit 3 = corpus dir unwalkable.

## Baseline (2026-07-31, corpus = `fixtures/external`, 4023 files)

```
files loadable:       3321
embedded programs:    1526   (distinct, deduped by stream obj id)
  by magic:  sfnt 1326 (86.89%) | cff 171 (11.21%) | type1 26 (1.70%) | unknown 3 (0.20%)
  by parse:  Sfnt 1326 | Cff 171 | Type1 26 | failed-clean 3 (all UnknownFormat)
  stream decode-failed: 2
  MISROUTES: 0    PANICS: 0    TIMEOUTS: 0
GATE: PASS
```

Routing is exact: each magic-framing count equals its parsed-variant count
(1326 sfnt→Sfnt, 171 cff→Cff, 26 type1→Type1), the post-fix expectation. The 3
`unknown`-magic programs are Isartor deliberate-fail fixtures
(`isartor-6-3-2-t01-fail-{a,b,c}`) — correctly `UnknownFormat`, not misrouted.

## Standing rule

Intended **R62** (librarian assigns the number): *embedded font programs route
to the correct parser or fail clean; a magic/variant disagreement is a gate
failure.* The font-layer analogue of R46 (content-identity) and R59
(render-parity).
