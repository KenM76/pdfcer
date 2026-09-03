# cmyk-calibration — deriving pdfcer's `DeviceCMYK` → sRGB conversion

The measurement and fitting harness behind `crates/pdfcer-core/src/color/`.
This file is the **logic**; the three scripts are the syntax that enacts it. A
competent engineer should be able to rebuild the whole thing — and re-derive
the shipped table — from this note alone.

| Script | Role |
|---|---|
| `cmyk_probe.py` | Ground truth: render known CMYK patches in a reference engine, read the pixels back. |
| `fit.py` | Analysis: score candidate conversions, fit the shipped node grid, emit it as Rust. |
| `compare.py` | Verification: pdfcer-vs-pdfium divergence on a real page, by decision 006 §3.7's exact method. |
| `corpus_cmyk.py` | Verification at scale: the same statistic pooled over every corpus file containing `/DeviceCMYK`. |

Nothing here ships. pdfcer depends on none of it; the fitted numbers are pasted
into `color/cmyk_table.rs` as source, so a release binary carries a table and
no data file, no parser, and no I/O. `pypdfium2` is tooling-only, exactly as in
`tools/render-parity` (`docs/LEGAL.md` §6).

---

## 1. The problem this exists to solve

ISO 32000-1 §8.6.4.4 defines `DeviceCMYK` as four subtractive ink
concentrations in 0.0–1.0 and **mandates no conversion to RGB at all**. §8.6.4's
whole premise is that device colour spaces are device-dependent. So there is no
"correct" screen colour for an untagged CMYK value to be measured against —
only other implementations' choices:

- **Acrobat** runs it through a user-configurable working-space ICC profile,
  defaulting to **U.S. Web Coated (SWOP) v2** — a house default, changeable by
  any user (the one documented exception being a PDF/X file's own
  `/OutputIntents`).
- **pdfium** ships a fixed calibrated table (`AdobeCMYK_to_sRGB1`) in the same
  SWOP-derived family, not configurable at all.
- **pdfcer, before 2026-08-08**, used naive additive `1 − min(1, x + k)`. Not a
  model of anything — the formula you write when you need one and have no data.

pdfcer is therefore **choosing**, not matching, and this harness is how the
choice is made from measurement instead of assertion.

## 2. Why measurement rather than a profile

The accuracy ceiling for a fixed choice is an extract from a real CMYK ICC
profile. That path was not taken, and the reason is licensing, not accuracy:

- SWOP v2's tone-response and gamut data is **proprietary and not published as
  an algorithm**. Knowing the profile's name does not let anyone reproduce its
  numbers.
- The freely-downloadable alternatives carry terms of their own. The ECI
  profiles (ISOcoated_v2 and family) may be redistributed only **with their
  licence attached and at no fee** — a non-MIT obligation travelling with a
  pdfcer release, which is the operator's call under rule 13, not an engineer's.

Rendering a patch and reading the pixel back sidesteps all of it: the result is
a **measurement of program output**, not a copy of a licensed artifact. No ICC
profile is read, shipped, or redistributed by pdfcer or by this harness.

## 3. Method

### 3.1 Probe (`cmyk_probe.py`)

Emits a synthetic PDF with one solid rectangle per sampled CMYK point, painted
with the `k` operator — no images, no ICC, no shading, so nothing can route the
colour through a different code path than a plain vector fill. Patches are 6
PDF units square, laid out on a near-square grid, and the page is rendered at
**scale 1.0** so one PDF unit is one device pixel. Only the **centre** pixel of
each patch is sampled, which puts it ≥2 px from any anti-aliased boundary.

```sh
# fit set: a 9-level lattice, 6,561 points
python cmyk_probe.py --levels 9 --engine pdfium --out out/fit-pdfium.tsv

# validation set: 4,000 uniformly-random points, seeded
python cmyk_probe.py --random 4000 --engine pdfium --out out/val-random.tsv
```

`--engine pdfcer` runs the same probe through the built `pdfcer`, which is
how you check that the Rust implementation reproduces the fit.

**Use `--random` for validation, not a lattice.** A lattice validation set
silently coincides with the model's own grid nodes whenever the two resolutions
share a divisor, which degenerates validation into "can it reproduce points it
was handed" and flatters the fit. This was not hypothetical: a 6-level lattice
would have validated the shipped 6-level grid at exactly its own nodes.

### 3.2 Fit (`fit.py`)

The model is **quadrilinear interpolation over a uniform `L⁴` grid of sRGB
nodes**. Fitting the nodes is a *linear* least-squares problem — the
interpolation weights are the design matrix and the node colours are the
unknowns — so the optimum is closed-form: no iterations, no seed, no knob. That
is what keeps this clear of the project's W14 rule against tuning a threshold
until a number turns green. There is no threshold; there is a solution and a
reported error.

Afterwards the **16 hypercube corners are snapped to their directly measured
values** (0.0 and 1.0 are members of every lattice, so the fit set measures them
exactly), and paper white is forced to exactly 1.0 with four-colour solid to
exactly 0.0. Rationale and the measured cost of snapping (none) are in
`fit_nodes`' docstring.

```sh
python fit.py --fit out/fit-pdfium.tsv --validate out/val-random.tsv --sweep
python fit.py --fit out/fit-pdfium.tsv --levels 6 --emit-rust
```

### 3.3 Regenerating `cmyk_table.rs`

`--emit-rust` prints the `GRID_L` constant and the `NODES` array. Paste the
array body into `crates/pdfcer-core/src/color/cmyk_table.rs` between its
existing header and closing bracket, keeping that file's module documentation
intact, then run `cargo fmt` and `cargo test -p pdfcer-core --lib color::`.

## 4. Choosing L

Per-pixel cost is **independent of L** — a quadrilinear lookup touches exactly
16 nodes however many the table holds — so L trades table size against accuracy
and nothing else. Measured on the 4,000-point random validation set (mean /
p95 / max per-channel Δ out of 255, and the fraction of samples where some
channel is off by more than 8):

| L | nodes | table | mean | p95 | max | >8/255 |
|---|---|---|---|---|---|---|
| 2 | 16 | 192 B | 5.06 | 20.7 | 57 | 43.5 % |
| 3 | 81 | 972 B | 3.29 | 14.4 | 46 | 17.0 % |
| 4 | 256 | 3.1 KB | 2.24 | 10.5 | 30 | 9.8 % |
| 5 | 625 | 7.5 KB | 1.41 | 7.4 | 27 | 4.1 % |
| **6** | **1,296** | **15.5 KB** | **1.16** | **5.8** | **17** | **2.6 %** |
| 7 | 2,401 | 28.8 KB | 0.96 | 5.3 | 19 | 1.7 % |
| 9 | 6,561 | 78.7 KB | 0.48 | 2.2 | 8 | 0.0 % |

**L = 6 is the shipped value.** Returns flatten after it — L = 7 buys 0.2 of a
mean unit for nearly double the table, and L = 9 only looks dramatic because
its nodes coincide with the fit lattice, so it is reproducing measurements
rather than generalising. 15.5 KB stays comfortably resident and 2.6 % of
samples beyond 8/255 is already an order of magnitude inside the *benign
renderer noise* band the parity harness measures.

For reference, on the same validation set:

| conversion | mean | p95 | max | >8/255 |
|---|---|---|---|---|
| naive additive `1 − min(1, x+k)` (before) | 32.48 | 77 | 100 | 97.0 % |
| multiplicative `(1−x)(1−k)` | 14.88 | 50 | 100 | 90.9 % |
| **quadrilinear L=6** | **1.16** | **5.8** | **17** | **2.6 %** |

The multiplicative row is the answer to "how far can data-free arithmetic get?"
— better than half the divergence for zero bytes, but still 91 % of samples
outside 8/255, because it keeps the additive form's assumption that the inks
are ideal sRGB primaries. It stays in `fit.py` as a scored baseline so the
question is answered rather than re-litigated.

## 5. Verification on a real page (`compare.py`)

Decision 006 §3.7 measured pdfcer against pdfium on one 300×232 `DeviceCMYK`
JPEG at 1:1 and produced the figure the project quotes. `compare.py`
reproduces that measurement exactly, so before/after are the same method.

```sh
cargo build --release -p pdfcer-cli
python compare.py --label after
```

Measured on `fixtures/synthetic/cmyk-variants/v2.pdf` (the same codestream
decision 006 used):

| metric | before (naive) | after (calibrated) |
|---|---|---|
| max abs Δ per channel | `[11, 37, 30]` | `[3, 2, 2]` |
| 95th percentile per channel | `[5, 27, 18]` | `[1, 1, 1]` |
| mean abs Δ per channel | `[2.47, 9.61, 6.82]` | `[0.63, 0.30, 0.12]` |
| **pixels differing > 8 in some channel** | **37.40 %** | **0.00 %** |

The "before" column was re-measured by temporarily reverting `cmyk_to_srgb` to
the naive formula and rebuilding — it reproduces decision 006's published
numbers digit for digit, which is what establishes that the method matches.

### 5.1 At corpus scale (`corpus_cmyk.py`)

Same statistic, pooled over all 83 files in `fixtures/external` whose bytes
contain `/DeviceCMYK` (81 measured, 2 skipped as unloadable by pdfcer), page 1
of each at 125 DPI — the `render-parity` baseline resolution:

| | before | after |
|---|---|---|
| pooled pixels > 8/255 | 5.88 % | **5.45 %** |
| mean-per-page pixels > 8/255 | 9.99 % | **8.28 %** |
| pages with >1 % of pixels beyond 8/255 | 51 / 81 | **41 / 81** |

The **paired per-file** comparison is the informative one, and it is
unambiguous: **18 pages improved, 63 unchanged, 0 worse.** Largest movers:

```
100.00 % ->  1.78 %   pdfium/testing/resources/bug_1646.pdf
 15.22 % ->  0.73 %   qpdf/qtest/qpdf/image-streams-small.pdf
 15.22 % ->  0.97 %   qpdf/qtest/qpdf/image-streams.pdf
  1.88 % ->  0.23 %   veraPDF 6-2-4-3-t02-pass-a.pdf
```

The pooled figure moves far less than the per-page one, and that is expected
rather than disappointing: "the file contains `/DeviceCMYK`" is a byte-scan,
not a claim that the page is mostly CMYK. Most of the 63 unchanged pages
diverge for reasons this change cannot touch — the ~99.8 % pages are dominated
by a transparency/shading gap that is its own filed item. **Zero regressions
across the set is the load-bearing result**; the conversion is strictly closer
to the reference everywhere, which is what a monotone improvement in a fitted
transfer function should look like.

### 5.2 Why the standing `render-parity` gate was not re-run to completion

Two blockers, both **pre-existing and unrelated to this change**, are recorded
here because the next render-touching Pass will hit them:

1. **The harness aborts on a corpus file that crashes pdfium.**
   `fixtures/external/pdfium/testing/resources/bug_457855936.pdf` terminates
   the process with exit `0x80000003` (STATUS_BREAKPOINT — a pdfium internal
   `CHECK`), which is precisely what a fixture named after a bug report is for.
   `render_parity.py` renders pdfium in-process, so that abort takes the whole
   sweep down at ~file 300 of 4,023 with no traceback and no partial report.
   `corpus_cmyk.py` avoids it by rendering pdfium in a child process and
   counting a dead child as a skip; the same isolation would fix the harness.
2. **The recorded baseline is stale.** `out/summary.json` was recorded over
   2,914 files; the corpus is now 4,023. Bucket counts are not comparable to it
   even once (1) is fixed, so the baseline needs re-recording rather than
   comparing against.

A bounded 60-file smoke run of the harness completes and is clean
(47 benign / 8 known-gap / 1 unexplained, the unexplained one a pre-existing
`unencrypted.pdf` at `frac32 = 0.088`, no DeviceCMYK involved).

## 6. Cost

Measured end to end on a deliberately **incoherent** 4-megapixel full-page
`DeviceCMYK` image — 2000×2000 pseudo-random samples, worst case for cache
locality and for any run-length or memoisation shortcut. Generate it with
`python cmyk_probe.py --emit-cost-fixture out/big-cmyk.pdf`, then time
`pdfcer render-page --scale 1` on it. Median of five runs:

| conversion | whole-page render |
|---|---|
| naive additive | **290 ms** |
| calibrated, 16-corner weighted sum | ~610 ms |
| **calibrated, strided nested lerp (shipped)** | **495 ms** |

So ≈ **+51 ns per converted sample** in the pathological case, and +71 % on a
page that is *nothing but* a full-bleed CMYK raster. Note that this is a
whole-page figure — it includes inflating 16 MB of Flate, rasterising, and
encoding a 4-megapixel PNG — so the conversion's share of a realistic page is
smaller still.

The restructuring from sixteen four-factor weight products to fifteen nested
lerps with constant strides was worth roughly a third of the added cost and is
what ships. (Run-to-run variance on this machine is ±10 %; the 610 ms figure is
a best-of-three from the earlier shape and should be read as "clearly worse",
not as a precise number.)

For **vector** content — which is what CAD exports, the motivating case, are
made of — the conversion runs once per `k`/`K` operator rather than per pixel,
and the cost is unmeasurable.

Headroom, if it is ever needed: a one-entry memo keyed on the previous sample
would collapse the cost on flat art and CAD rasters (long runs of identical
CMYK) while doing nothing for photographs. Not implemented — it adds a branch
for a case that is not currently hurting.

## 7. Re-targeting

The grid is data and the tool that produced it is committed, so pointing pdfcer
at a different printing condition is a re-run of §3, not a code change. If
pdfcer ever grows a user-selectable working CMYK space (Acrobat's model), this
table becomes its default entry rather than being replaced.
