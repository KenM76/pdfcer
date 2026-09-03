# Region rasterisation — measured cost model

**Measured 2026-08-13** on the operator's own benchmark drawing, in answer to
`D:\Dev\FeatureRequests\pdfce_FeatureRequests\request_region_rasterisation.md`
from the `pdfcer-gui` session. Re-runnable:

```text
cargo run --release -p pdfcer-render --example region_bench -- <file.pdf>
```

**Subject:** `D:\Dev\temp\pdfce\ncored-benchmark-cad-drawing.pdf` — **A3
landscape** (1190.55 × 841.89 pt), 5.6 MB, dense vector site plan,
148,517 paints · 24,128 clip ops. Release build. Load: ~5 ms.

---

## The measurements

| case | pixmap | pixels | time |
|---|---|---:|---:|
| **full page**, scale 1 | 1191 × 842 | 1,002,822 | **877 ms** |
| **full page**, scale 2 | 2382 × 1684 | 4,011,288 | **1,422 ms** |
| **region**, scale 1 | 401 × 301 | 120,701 | **699 ms** |
| **region**, scale 2 | 401 × 301 | 120,701 | **855 ms** |
| **region**, scale 8 | 401 × 301 | 120,701 | **801 ms** |
| **region**, scale 32 | 401 × 301 | 120,701 | **1,067 ms** |
| **★ floor**, a 1 × 1 **point** region | 1 × 2 | **2** | **691 ms** |

## ★ The finding

**A two-pixel render costs 691 ms; a 120,701-pixel render costs 699 ms.**

So on this document the cost is **~99 % resolution- and area-independent**.
It is content-stream interpretation and path construction, paid in full
regardless of how few pixels come out. Fill is nearly free by comparison — the
whole 1,002,822-pixel page adds only ~180 ms over the floor.

The requesting session predicted this ("~0.74 s of that is
resolution-independent") from `tools/render-profile`'s scale sweep. **The
prediction was correct, and the floor is slightly higher than estimated.**

## What follows from it, in order of importance

### 1. Do not tile. One region per viewport.

A 3 × 3 tile ring costs **9 × ~0.7 s ≈ 6.2 s**, against ~0.7 s for a single
region covering the same area. Tiling on this engine is not an optimisation,
it is a 9× regression. The requesting session's instinct — *"a bare
`render_page_region` that re-parses would be worse than nothing, and I would
rather know that up front"* — was right, and this is the up-front answer.

Tiling remains legitimate for **bounding memory** on an enormous viewport. It
is never a way to save time.

### 2. Region rendering buys REACHABILITY, not speed.

This is the part the timing table understates. At scale 32 the whole page would
be 38,112 × 26,940 px — **3.8 GiB, and over `MAX_PIXMAP_EDGE` regardless**, so
it is not slow, it is *impossible*. The region renders in 1.07 s.

| a true A1 landscape @ 2× DPR | whole-page | region |
|---|---|---|
| max zoom | **3.4×** (guard-bound) | limited only by region size |
| memory at max | 1.00 GiB | ~0.5 MB for a 400 × 300 pt viewport |

So the operator requirement — *"zoom in as much as feasibly possible,
preferably further than other software allows"* — is now reachable. It simply
costs ~0.7–1.1 s per zoom step on a document this dense, not less.

### 3. Cost grows mildly with zoom, and that is fine.

699 ms at 1× → 1,067 ms at 32×, for a constant pixel count. The growth is path
geometry getting larger relative to the clip, not fill. Deep zoom is affordable.

### 4. A display-list cache would remove ~99 % of the repeat cost.

Because the floor is interpretation, a reusable parsed representation the shell
holds and replays against N regions would take the second and subsequent
renders of the same page from ~700 ms to roughly the fill cost — tens of
milliseconds. That is the single highest-value optimisation available to this
crate, and this measurement is the evidence for funding it.

**It is not built.** `render_page_region` shares `render_page_with_view`'s
implementation exactly, differing only in pixmap size and a translation on the
base CTM. Nothing is cached between calls.


### 4a. ★ The floor decomposed by ABLATION, and the ~99 % claim is confirmed rather than corrected

**Measured 2026-08-18**, scoping `Pass 75.0`. The claim above — *"the floor is
interpretation"* — was inferred from an **area** comparison (a 2-pixel render
costs what a million-pixel one does). That is sound evidence for
area-independence, but it does not by itself say **what** the area-independent
work is. Interpretation and `fill_path`'s *setup* are both area-independent:
tiny-skia builds an edge list per path whether the pixmap is a megapixel or
two pixels.

So it was measured directly, by **ablation**: every `fill_path` / `stroke_path`
call in the paint path was env-gated off (8 sites), and the FLOOR case re-run.
Nothing else changed, so nothing else is confounded.

| FLOOR (1 × 1 pt region, 2 px) | run 1 | run 2 | run 3 | median |
|---|---:|---:|---:|---:|
| **normal** | 665 ms | 667 ms | 722 ms | **667 ms** |
| **every paint call removed** | 591 ms | 580 ms | 611 ms | **591 ms** |

⇒ **Painting is ~11 % of the floor (~76 ms). Interpretation and path
construction are the other ~89 % (~591 ms).**

**This confirms §4 rather than qualifying it**, and the confirmation is worth
more than the original inference because it rules out the specific alternative
that would have sunk the design: had the split been the other way round, a
cached parse would have removed only a tenth of the cost and `Pass 75.0`'s
acceptance criterion 1 — *"~700 ms to tens of ms"* — would have been
unreachable by construction.

**Ceiling this sets on `Pass 75.0`:** a handle that skips interpretation and
path construction removes ~591 ms of the ~667 ms floor. The residual ~76 ms is
`fill_path` setup for all 148,517 paints, and **a bbox cull at replay is what
removes that** — a cull is cheap against a display list because the bounds are
already computed, whereas during interpretation they are not known until the
path has been built, which is the expensive part. **So culling belongs in the
replay path and is not an alternative to the handle.**


#### ★ Where inside the 89 % — a second measurement, because "interpretation" was still a bucket not a location

**Measured 2026-08-18**, same session, by timing `Interpreter::paint` directly
(the single chokepoint every path fill and stroke funnels through) rather than
by ablation.

```
FLOOR 742.4 ms   of which paint() = 126.6 ms   = 17.1 %
```

⇒ **`paint()` is ~17 % of the floor; ~83 % is spent OUTSIDE it.**

The 83 % is content-stream tokenizing, operator dispatch, graphics-state
handling and — the part that is easy to mis-locate — **`PathBuilder` pushes**.
A path is built **incrementally by the `m` / `l` / `c` / `re` operators**, so
its construction cost is spread across the operator loop and is *not* inside
`paint()`; only `builder.finish()` is.

**Why this matters for `Pass 75.0`'s shape rather than just its funding.** The
expensive thing is producing a finished `Path`, and it is expensive *before*
the paint call, distributed over thousands of operators. So the display list
must store **finished paths**, and the natural recording point is `paint()`,
where the finished path first exists as a value. A cache keyed anywhere
earlier would have to cache the operator stream and rebuild the path — which
is most of the cost it was supposed to avoid.

⚠️ **Read the first number of a run and discard it.** The counter accumulates
across renders, so a report taken after the FULL cases attributes their
`paint()` time to the FLOOR case and can exceed 100 %. The figure above is the
**second** FLOOR iteration, after the counter was zeroed. Recorded because the
first reading said *164 %*, which is obviously wrong and would have been
quietly halved into something plausible by anyone in a hurry.

**Both instrumentations were reverted.** They are measurement hacks; the tree
is byte-clean against `HEAD` apart from this document.

#### ★ A cull at the PAINT site was already measured and correctly rejected

`paint_is_cullable` exists in `interpret.rs` and feeds **only** a counter. That
is deliberate, and `profile.rs` says so at the field:

> *"Measured at 1.34 % on the reference CAD sheet, which is why no such cull
> was built. Kept as a counter so the next person to propose one gets the
> number instead of the intuition."*

It worked: this session proposed exactly that cull and got the number instead
of the intuition. **Recorded because a predicate that gates nothing looks like
dead code to a reader who has not found its rationale** — and the rationale is
one file away, in a doc comment on the counter it feeds.

Note also that the two culls are against **different rectangles** and are not
substitutes: `paint_is_cullable` tests against the **clip's** bbox (1.34 %
hit rate, because on this sheet clips average 66 % of the page), whereas a
replay-time cull would test against the **region**, which for a zoomed viewport
is a small fraction of the page and would hit far more often.

#### Method note, stated because it nearly produced a wrong number

The first ablation run reported the split as **36 % painting**, and that figure
would have been written down had it not been repeated. The machine was
concurrently running `cargo build`, and the contention landed unevenly across
the two cases. **Three runs per case, medians reported, both cases interleaved
under the same load** is what made the number stable. A single-run ablation
measures the load as much as the code.

## ★ A SECOND DOCUMENT, and the caveat below was right

**Measured 2026-08-13 by the `pdfcer-gui` session** on `iso32000-2-preview.pdf`
— the PDF 2.0 spec preview, 689 KB, text-heavy A4 — in answer to the
one-document caveat this section originally carried:

```text
FULL   scale 1   596x842   =   501,832 px    8.97 ms
FULL   scale 2  1191x1684  = 2,005,644 px   13.94 ms
FLOOR  1x1pt      1x2      =         2 px    3.21 ms
REGION scale 1   401x301   =   120,701 px    6.08 ms
```

| | dense CAD (A3) | text-heavy (A4) |
|---|---:|---:|
| interpretation floor | **691 ms** | **3.2 ms** |
| full page, scale 1 | 877 ms | 8.97 ms |
| floor as share of full page | **~99 %** | **~36 %** |

**Both conclusions survive, but only one magnitude does.** *Never tile for
speed* holds on both. *Tiling is a catastrophe* holds only where
interpretation dominates: the 3×3 penalty is **~9×** on the CAD sheet and
**~1.9×** on the text page, where the absolute numbers (29 ms vs 55 ms) put
nothing about interactivity at stake.

**The real finding is not about tiling at all.** It is that one document type
costs **700 ms** per render and the other costs **9 ms** — a spread of nearly
three orders of magnitude on the same code path. Any strategy tuned for one is
mistuned for the other, which is the argument for the display-list handle
below rather than for any particular render granularity.

## Honest limits of this measurement

- ~~**One document.**~~ **Discharged** — see the section directly above. The
  original caveat read: *"A dense CAD sheet is the worst case for
  interpretation cost and the best case for the argument above. A text-heavy
  office page would show a much lower floor and a relatively larger fill share;
  the 'do not tile' conclusion weakens as the floor falls."* That is exactly
  what the second measurement found. Kept rather than deleted, because a caveat
  that turned out to be correct is evidence about how much to trust the next
  one.
- **One machine, single-threaded.** `pdfcer-render` uses no `rayon` and no
  threads. Region fills are embarrassingly parallel once a display list exists;
  without one, parallelism would duplicate the floor per worker rather than
  divide it.
- **`--release` matters.** Debug ratios are not shipped ratios.
- The `scale 8` row (801 ms) is *faster* than `scale 2` (855 ms). That is run
  noise, not a trend — the harness reports a single run per case, deliberately,
  because the floor's magnitude is the finding and it is not a close call.

## Not measured in THIS section

The requesting session's question 3 — the honest upper bound on magnification —
is answered in the second half of this document, below.

---

# The magnification ceiling — measured, and it is not where anyone would guess

**Added 2026-08-13**, answering question 3 of the same request (*"the honest
upper bound on magnification before the output stops being meaningful"*), which
the first version of this document recorded as owed.

```text
cargo run --release -p pdfcer-render --example zoom_ceiling
```

**Method.** A 1 pt black bar whose left edge sits at **x = 2999.7373 pt** on a
3370 pt (A0) sheet. At each scale a tight region is rendered around that edge
and the first ink column is compared against where the arithmetic says it must
land. Error is in **device pixels**.

Position matters as much as scale: `f32` carries a 24-bit mantissa, so it is
the **absolute magnitude** of `coordinate × scale` that consumes precision. A
bar near the origin would show nothing.

| scale | region px | error px |
|---:|---|---:|
| 1.3 | 1 × 1 | 0.755 |
| 21 | 3 × 2 | 0.076 |
| 336 | 28 × 15 | 0.222 |
| 672 | 54 × 28 | 1.444 |
| 2,690 | 216 × 107 | 0.775 |
| 5,381 | 431 × 214 | 0.551 |
| 10,762 | 862 × 428 | 0.102 |
| 21,524 | 1724 × 856 | 0.797 |
| **43,047** | 3448 × 1712 | **2.594** |
| 86,095 | 6896 × 3424 | 5.187 |
| 172,189 | 13792 × 6848 | 11.374 |

## Reading it

Below ~5,000× the error is **sub-pixel and non-monotonic** — it wanders
between 0.02 and 1.4 px with no trend. That is anti-aliasing and threshold
rounding on where the ink crosses the detection cut, **not** precision decay.

Beyond ~43,000× it **doubles as the scale doubles** (2.59 → 5.19 → 11.37).
That is the signature of `f32` mantissa exhaustion, and the arithmetic agrees:
`2999.7373 × 43,047 ≈ 1.29 × 10⁸`, well past the 1.677 × 10⁷ above which the
`f32` spacing exceeds 1.0, so the representable gap — and hence the error —
scales with the coordinate from there.

## ★ The conclusion, which is the useful part

**Numerical precision is not the binding constraint on magnification, and is
not close to being one.** On the worst realistic case — a coordinate near
3,000 pt on an A0 sheet — device coordinates stay sub-pixel accurate to roughly
**5,000×**, three orders of magnitude beyond any plausible viewing zoom.

**So `MAX_ZOOM` must be set from performance and usability, not from
numerics.** The real limit is the ~0.7–1.1 s per-render interpretation floor
measured above. Setting it from `f32` would be picking a number for a reason
that does not apply — the same class of error `MAX_PIXMAP_EDGE`'s original
justification made.

---

## ★★ AMENDMENT 2026-08-23 — the MEASUREMENTS above are unchanged; the CONCLUSION was a PREDICTION and it has been refuted

**Nothing measured on 2026-08-13 is wrong, and no number in this file is
corrected.** What is amended is the section immediately above, and the defect is
one clause: ***"three orders of magnitude beyond any plausible viewing zoom."***

**That is `R193`/`Pass 74.0`'s exact anti-pattern** — *a limit justified by
"beyond any plausible use" is a prediction, not a fact* — and the prediction was
falsified within eleven days by this project's own artefact. `tools/gen-scale-demo`
draws a banana and, at 1:1 with no scale break anywhere between them, the water
molecules inside one of its cells. **Reading that page requires `scale` up to
`1.6 × 10⁹`**, five and a half orders of magnitude past the `~5,000×` this file
calls the outer edge of plausibility. **The plausible viewing zoom was a
property of the documents that existed when the sentence was written.**

**So the headline sentence — *"numerical precision is not the binding constraint
on magnification, and is not close to being one"* — is FALSE above roughly
`5 × 10⁶`, and was false when written for any document finer than its own page
units.** Three `f32` limits were subsequently measured under page-space content
(`ARCHITECTURE.md` §4, *Numerical reach of PAGE-SPACE CONTENT*): a path
coordinate near `x = 540 pt` quantised to **21.5 µm**; a `cm` carrying a page
coordinate drifting by `≈ page_x × scale / 16 700 000` px; and the content's
device position moving in **~500 px steps at `scale = 8.1 × 10⁶`**. **All three
were raised on 2026-08-23 by `PASS 74.7`** (`1d6db9e` + `5b0d885`), in two
algorithms.

**★ Why this file's method could not have seen it, which is the transferable
half.** The measurement above is **error in pixels against zoom on one CAD
sheet**, and its own reading section is careful and correct: sub-pixel wander
below `~5,000×`, then doubling past `~43,000×`. **The extrapolation it refuses
to make is the one that mattered** — it stops at the zooms it measured and then
declares the region beyond them uninteresting. **An error curve measured over
the range you consider plausible cannot tell you the range is plausible.**

**What is still exactly right and should not be discarded:** the per-render
interpretation floor, the cost model, the region-vs-whole-page arithmetic, and
the ruling that `MAX_ZOOM` is set from **performance and usability** rather than
from numerics. That ruling survives its stated reason — the numerics were the
binding constraint after all, and are no longer, so the conclusion is now true
for a better reason than the one given.

**Cross-references:** `ROADMAP.md` `R213` (a magnitude claim is a claim about
one quantity — this file's original defect was the *subject* of its ceiling),
`R193`/`Pass 74.0` (a prediction filed as a fact), `ARCHITECTURE.md` §4 and
§4.1 (AB). Filed by the two-hundred-and-thirty-second filing.

## What is still NOT covered here

Hairline strokes (`0 w`), which render at a device-minimum width regardless of
scale and therefore get relatively *thinner* the deeper the zoom, and text
hinting at extreme sizes. Both are **appearance** questions rather than
correctness ones, and neither is measured by this harness. Stated so the
"measured" claim above is not read wider than it is.


---

# The display list — `Pass 75.0`, measured

Everything above measures rendering a region **from the content stream**, and
its conclusion was that interpretation dominates. `Pass 75.0` acts on that:
[`pdfcer_render::record_page`] interprets a page once into a
`DisplayList`, and `DisplayList::replay_region` rasterises any region of it
without re-interpreting.

This section is the acceptance measurement, taken **2026-08-18** on the same
machine, same file, same harness — `crates/pdfcer-render/examples/region_bench.rs`
in `--release`, extended with `RECORD` / `MEMORY` / `PFLOOR` / `PAN` cases:

```bash
cargo run -q --release -p pdfcer-render --example region_bench -- \
  D:/Dev/temp/pdfce/ncored-benchmark-cad-drawing.pdf
```

**Three runs, medians reported**, because §"Honest limits" above already
records what a single run measures: the machine's load as much as the code.

## The numbers

`ncored-benchmark-cad-drawing.pdf` — A3 landscape, 148,517 paints, 24,128 clip
ops. Recorded: **127,267 ops, 40 distinct clips, ~29.5 MiB held.**

| case | from the content stream | from a display list | ratio |
|---|---:|---:|---:|
| **FLOOR** — 1 × 1 pt region, 2 px | **636 ms** | **1.06 ms** | **600×** |
| region 400 × 300 pt, scale 1 (120,701 px) | 680 ms | 83.5 ms | 8.1× |
| region 400 × 300 pt, scale 8 (120,701 px) | 819 ms | 10.5 ms | **78×** |
| recording the page itself | — | 618 ms | — |

## ★ Read the FLOOR row first — it is the one that says what happened

A 1 × 1 point region costs **636 ms to render and 1.06 ms to replay**.

That is the whole claim, stated without any confound: the floor case has
essentially no fill in it, so what it measures is *interpretation and nothing
else*, and interpretation is now **gone from the second render**. The 1.06 ms
that remains is the op walk plus the bounding-box cull over 127,267 recorded
ops — the only part of a replay that still scales with the **page** rather
than with the **viewport**.

## Why the two full-size region rows differ so much, and why that is correct

Both render 120,701 pixels. The scale-8 row replays 13× faster than the
scale-1 row, and the reason is not the raster: at scale 8 the viewport covers
50 × 37.5 pt of the sheet, at scale 1 it covers 400 × 300 pt — **64× more
drawing**.

So the cost model has changed shape. It used to be *proportional to the page*;
it is now *proportional to what is actually in view*. That is exactly what
acceptance criterion 1 asked for ("cost roughly **fill** rather than
**interpretation**"), and it is why the scale-1 figure being 83.5 ms rather
than "tens of ms" is not a shortfall: those 83.5 ms are 15,000-odd real paints
plus ~40 region-sized clip masks, which a direct render pays too and on top of
680 ms of interpretation.

## ★ The regression this harness caught, and it would have shipped

The **first** working implementation recorded one clip definition per `W n` —
24,128 of them — instead of deduplicating. A replay builds one mask per
*distinct* definition, so it was building 24,128 region-sized masks per frame.

Measured then:

| | direct | replayed | |
|---|---:|---:|---|
| region 400 × 300 pt, **scale 1** | 706 ms | **1.79 s** | the cache was **2.5× slower than no cache** |
| region 400 × 300 pt, **scale 8** | 912 ms | 10.4 ms | 88× faster |

**The scale-8 number was already excellent while the feature was a net loss at
scale 1**, because at deep zoom almost every op culls before its clip is ever
requested — so the expensive path was never taken. A harness that measured only
the motivating case would have reported an 88× win and shipped a regression.

The fix is `RecorderState::push_clip`'s dedup table, keyed on the *same*
`ClipCache::build_key` the painting path deduplicates on, so recorder and
painter agree by construction about what "the same clip" is. Result: **40
definitions**, matching the ~41 the design predicted from the live cache's
99.83 % hit rate.

**The transferable part:** a cache's win and a cache's cost do not live in the
same case. Measure the one where the cache does the *most* work, not the one
that motivated it.

## Cheap documents — criterion 5, no first-render regression

Recording is **cheaper than rendering**: it builds no clip masks and fills no
spans. On documents where interpretation is already cheap this shows up as
recording costing about the interpretation floor and nothing more.

| document | interpretation floor | recording | ops |
|---|---:|---:|---:|
| `fixtures/synthetic/addtext/plain.pdf` (A4 text) | 416 µs | **394 µs** | 16 |
| `PDF 2.0 UTF-8 string and annotation.pdf` | 12.8 µs | **14.4 µs** | 1 |
| the A3 CAD sheet | 636 ms | **618 ms** | 127,267 |

So a first render *through* a handle costs `record + replay` ≈ `interpretation
+ fill` ≈ what a direct render costs. There is no document where adopting the
handle makes the first frame materially slower, which was the criterion.

*(`iso32000-2-preview.pdf`, the text-heavy document measured in the section
above, is **not on this machine** — that measurement came from the `pdfcer-gui`
session. The two documents above stand in for it, and the substitution is
stated rather than glossed.)*

## Memory — criterion 4

**~29.5 MiB for 127,267 ops**, i.e. roughly **240 bytes per op**: the `Op`
enum itself plus each path's points and verbs.

Reported at runtime by `DisplayList::memory_bytes`, and it is the *same
accumulated number* that enforces `MAX_DISPLAY_LIST_BYTES` (256 MiB) — a guard
and a report computed twice are a guard the caller cannot see.

A shell holding one list per open page should budget from that figure. Note
what it excludes and why: image texels and stroke parameter blocks are
`Arc`-shared, and attributing a shared buffer wholly to one list would
overstate the cost of holding a second list for the same page — which is
precisely the decision the number exists to inform.

## What this does NOT cover

- **Pages the recorder refuses** — shadings, overprint composites, soft masks,
  tiling patterns. They are refused **by name** and render normally through
  `render_page_region`; none of the figures above apply to them. The reference
  CAD sheet contains none (`images=0 shadings=0 patterns_unpainted=0
  soft_masks_applied=0 groups_composited=0`), which is why it is the
  motivating case and not a representative one.
- **A change of scale**, which invalidates a list by design (`display_list`
  module docs §2.1). Panning at a fixed zoom is what these numbers measure.
- **Multi-page holding.** Every figure here is one page.
