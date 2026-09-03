# render-profile — the standing answer to "where does the render time go?"

Out-of-tree profiling harness (mirrors `tools/roundtrip` and
`tools/font-parity`). Loads a PDF, renders one page at a series of scales,
and reports the load/render split, the **scaling curve**, and what the
page's content actually looks like to the renderer.

Enables `pdfcer-render/profile`, a feature that is off in every shipping
build and compiles to nothing without it.

## Why this is committed rather than a scratch file

On 2026-08-07 three throwaway probes were written into `interpret.rs` and
deleted within hours. **Two produced figures wrong by two orders of
magnitude, and both were believed and acted on:**

| claim | actual | how it went wrong |
|---|---|---|
| `Mask::new` is 10.1 s of an 18 s render | **1.02 s** | measured by an ablation that skipped `intersect_clip` entirely — which also makes every `q` cheap and lets tiny-skia skip mask sampling. Construction *plus* use, attributed to construction (**R164**) |
| mean clip bbox is 0.663% of the page | **66.36%** | a fraction printed as a percent |

The second error was written into `intersect_clip`'s own doc comment as
"clips in real drawings are SMALL relative to the paper", and became the
stated premise of a follow-on optimization that was scoped, dispatched,
and killed only when the number was measured again.

Neither survived contact with a second measurement. Both survived for
hours because **there was no second measurement to make** — the probe that
produced them no longer existed. A harness that must be rewritten each
session is one nobody runs, and an unrepeatable number ages into a fact.

## Usage

```
cd tools/render-profile
cargo run --release -- <file.pdf> [--page N] [--scales 0.25,0.5,1,2] [--repeat N]
```

`--repeat` takes the **fastest** of N runs, which is the right statistic
for a deterministic workload perturbed by scheduling noise.

## Reading the output

**The scaling curve is the diagnostic, not any single row.** A cost
quadratic in area rises by the same factor at every doubling; one that
jumps at a single step is a cache boundary. On the reference CAD sheet the
steps ran 3.23× / 3.14× / 14.1× — three smooth steps then a cliff, which
identified a working set crossing L3 rather than an algorithmic term. **A
single before/after pair could not have told those apart.**

`load` is `Document::from_bytes` only — the object graph and xref. When it
is a rounding error, optimizing the reader is wasted effort; on the
reference sheet it is ~0.005% of the total. The render column necessarily
includes content-stream interpretation, because the interpreter paints as
it walks and the two are not separable from outside.

The content block reports counts and geometry rather than timings.
**Timer calls inside a loop that runs 148,517 times perturb the thing
being measured**, and per-phase timings invite exactly the
subtract-two-totals reasoning that produced the `Mask::new` error.

It prints an explicit note when clips cover a large share of the page, so
that the "clips are tiny" premise cannot be re-adopted silently.

## Fixture rule

Per `docs/LEGAL.md` §5, files fed to this tool are rights-cleared or
operator-supplied and are **never committed**. The reference CAD sheet is
a measurement input, not a fixture.
