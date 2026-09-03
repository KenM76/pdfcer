# render-parity — full-page pdfium pixel-parity harness (Pass 11)

The standing **render-fidelity verification gate** (decision 010, candidate
C). It proves pdfce's render stack against an *independent* reference
renderer — pdfium, via `pypdfium2` — at corpus scale, replacing the
self-comparison round-trip oracle ("pdfce agrees with pdfce") with a
measured, bucketed, by-file/by-reason fidelity report.

This file is the **logic**; `render_parity.py` is the syntax that enacts it.
A competent engineer should be able to rebuild the harness from this README.

---

## 1. Why this exists (the forcing consumer)

The self-comparison oracles pdfce already ships — `tools/roundtrip` (object
identity, R34) and `tools/content-identity` (content-stream identity, R46) —
prove pdfce agrees with *itself*. That is sufficient for **additive**
authoring (annotations, form fill, flatten-by-append): the page content
stays byte-verbatim, so a self-comparison holds. It is **not** sufficient
for content-stream **surgery** that re-renders an edited page — the vector /
Inkscape-parity editing arc (candidate A, Pass 9). A's acceptance test is
"does the edited page still render *correctly*?", and *correctly* means
"like an independent production renderer", which pdfce comparing to pdfce
structurally cannot answer.

Decision 010 therefore sequences C (this harness) **before** A, so A's
content-stream edits inherit a standing full-page fidelity gate. This
harness is the generalization of `tools/annot-pdfium-diff.py` (an ink-bbox
differential on 7 annotation fixtures) to full-page, per-channel, per-pixel
comparison over the whole loadable corpus — `fixtures/external`, **4,023
files** as of 2026-08-08. (The recorded baseline in `out/` measured an
earlier 2,914-file corpus and is **not comparable** to a run over today's;
see §11.)

## 2. What it does

For each `*.pdf` under the corpus dir(s), for each sampled page:

1. Render in **pdfce** via `pdfce-cli render-page --page N --scale S`
   (scale = DPI/72), capturing the stdout **diagnostics tally**.
2. Render in **pdfium** via `pypdfium2` at the same scale.
3. Composite both onto white (normalizing transparency), crop to the common
   top-left extent, and compute the per-pixel **max-channel absolute delta**
   `d ∈ [0,255]`.
4. Reduce to per-page metrics: `mean`, `p95`, `dmax`, and
   `frac_over_T` = fraction of pixels with `d > T` for T ∈ {16, **32**, 64}.
5. Tag the page with pdfce's disclosed gaps (from the diagnostics tally) and
   a file-level DeviceCMYK byte-scan.
6. Classify into one of three buckets (§4).

Outputs (`out/`, deterministic + locale-invariant):

| File | Contents |
|---|---|
| `per-page.tsv` | one row per (file, page): dims, metrics, bucket, reason, gaps |
| `per-page.partial.tsv` | the **streamed, crash-surviving** copy (no `bucket` column). Written row-by-row *during* the sweep and deleted on a clean finish — its presence means the run did not complete (§10) |
| `progress.json` | mid-sweep checkpoint: files done, ETA, status counts, aborted files so far |
| `summary.txt` | distribution + bucket counts + DeviceCMYK char. + unexplained tail + the aborted-file list |
| `summary.json` | same, machine-readable (the gate/CI artifact); carries `run.complete` and `corpus_fingerprint` (§11) |
| `summary.superseded-*.json` | a prior baseline archived by `--rebaseline` (never deleted) |
| `diffs/*.png` | `[pdfce ǀ pdfium ǀ 8×-amplified delta]` panels for the worst pages |

### 2.1 The reference renderer runs in a CHILD PROCESS

`render_parity.py` **must not import `pypdfium2`**. All PDFium contact goes
through `pdfium_worker.py`, a persistent child process. This is not
defensive style — it is the difference between the harness running and the
harness being unusable at corpus scale. See §9.

## 3. The tolerance band — empirical, never tuned (decision 010 Y1 / W14)

**The central problem.** Two independent renderers *always* differ at the
pixel level: anti-aliasing, font hinting, sub-pixel glyph positioning, and
image interpolation are implementation choices, not bugs. Demanding
pixel-for-pixel agreement is a category error. Separating benign noise from
real divergence **is** the analytical core of this Pass, and there are two
forbidden failure modes:

- **W14** — tuning a threshold until a number turns green; and
- declaring benign anti-aliasing noise a "bug".

**Why `frac_over_32` is the discriminator.** Benign AA/hinting noise is
confined to a *thin sub-pixel band around edges*. Individual edge pixels can
swing the full 0..255 (a glyph edge that is black in one renderer and white
in the other one pixel over), so **max-delta and mean-delta are dominated by
edge noise and are poor discriminators**. But that noise touches only a
*small fraction of the page's area*. A real divergence — a missing shading
fill, a wrong DeviceCMYK colour, a shifted or dropped glyph run — touches a
*large contiguous area*, i.e. a large fraction. So **fraction-of-area over a
moderate per-pixel threshold (32/255 ≈ 12.5%)** is the noise-robust page
metric. (Empirically confirmed: the very first band-edge "unexplained" page
was a blank sheet with a single 1px-stroked square whose *only* divergence
was AA on the four border edges — max-delta 143, but `frac_over_32` ≈ 0.0017.
See `docs/SESSION_LOG` Pass 11 and the diff panel.)

**How the band is derived (not picked).**

1. Every page is tagged **clean-by-construction** iff pdfce discloses **zero**
   gaps for it (no substituted/notdef glyphs, no deferred `sh`/BDC-EMC/Type3
   ops, no unsupported font, no image-codec shortfall, no DeviceCMYK JPEG)
   **and** the file contains no `/DeviceCMYK` **and** the page boxes agree.
   Whatever such a page diverges by *can only be renderer noise*, because
   pdfce itself claims to render it in full.
2. The band is the **p99.9 of `frac_over_32` over the clean-by-construction
   population**. Principle: that population is benign *in full*, so the band
   covers essentially all of it. The percentile is chosen to *cover the
   benign population*, **not** to hit a target unexplained count — that is
   the W14 line. Any page above a percentile of its own benign peers is,
   by construction, anomalous *relative to benign noise* and worth a look.
3. The band is a property of the **known-benign** population, so it **cannot
   be tuned to make a bug pass**: a bug lives either on a page pdfce
   discloses a gap for (bucket ii) or in the residual tail of clean pages
   above their own noise floor (bucket iii) — never below the band.

The report always prints the **distribution** (mean / p50 / p95 / p99 / max
of `frac_over_32`, separately for all / clean / DeviceCMYK pages) — never a
bare pass/fail. The band is re-derived from the data on every run and its
source string is recorded in `summary.txt`/`.json`.

## 4. The three buckets (decision 010 deliverable 3; R20 by-file-and-reason)

Each measured (file, page) is exactly one of:

| Bucket | Definition |
|---|---|
| **(i) benign-renderer-noise** | `frac_over_32 ≤ band`. AA / hinting / sub-pixel / interpolation. Characterized, **not** chased to zero (a non-goal). |
| **(ii) known-disclosed-gap** | `frac_over_32 > band` **and** pdfce disclosed a gap that explains it — cross-referenced against pdfce's **existing** `Diagnostics` tally so an already-counted gap is **subtracted, not re-reported**. Reasons: `font-unsupported` (Type3 / exotic CMap), `font-substituted` (substitute face ≠ embedded shapes), `glyph-notdef`, `deferred-op` (`sh` shading, `/OC` marked content, Type3 procs, clip modes 4–7), `image-*` (codec/feature/geometry), `devicecmyk-*` (colorimetry, §6). |
| **(iii) unexplained-divergence** | `frac_over_32 > band` **and** no disclosed gap explains it. The genuine **bug candidates** — the residual after subtracting (i) + (ii). Every one is enumerated by file + reason and either **fixed** (if cheap and clearly a pdfce render bug) or **filed as a named, counted render-gap** (R20/R27). |

Three side classifications that are **not** pdfce errors:

- **reference-divergence** *(only in `--annots` mode)* — the page carries a
  `/Widget` or a no-`/AP` annotation. pdfium needs `FPDF_FFLDraw` to draw
  widget appearances, and it **synthesizes** some no-`/AP` looks (e.g.
  `/Circle /IC` interior fill) that **R43 makes pdfce correctly refuse**
  (Pass 6.0 finding). Bucketed reference-side so pdfium's own quirks are
  never misattributed to pdfce (deliverable 5 / risk Y2). **The default run
  is content-only** (annotations off on both engines), which structurally
  removes this confounder — the vector-editing oracle cares about page
  *content*, which is exactly what an edit re-renders.
- **reference-aborted** — pdfium did not *fail*, it **died**: an internal
  `CHECK`, a segfault, a heap corruption, or a hang. A reference renderer
  that ceases to exist has told us nothing about pdfce, so this can never be
  a pdfce bucket. Reported alongside the three, with the fault **named**
  (`exit 0x80000003 (STATUS_BREAKPOINT)`, `signal 6 (SIGABRT)`, …) and each
  file re-verified in isolation. See §9.
- **skipped** — pdfce could not load/render (e.g. a conformance `fail-*`
  file with a broken header/trailer/stream — legitimately out of scope, as
  in the roundtrip gate), or pdfium reported a *catchable* failure. Counted
  with a reason histogram, never silently dropped.

**`skip` vs `abort` is a load-bearing distinction.** A skip is information
about the *file* ("this thing cannot be loaded"), and belongs in a histogram
of malformed fixtures. An abort is information about the *reference tool*
("PDFium killed itself on this input"), and belongs in its own named,
enumerated category. Collapsing the second into the first would bury a
reference-tool crash among 400 broken conformance fixtures.

## 5. Known reference-divergences encoded (deliverable 5 / Y2)

pdfium quirks that must never be scored against pdfce:

- **`FPDF_FFLDraw` widgets** — a bare `page.render(draw_annots=True)` does
  **not** draw `/Widget` form-field appearances; pdfium needs its form-fill
  environment. So in `--annots` mode a widget-bearing page whose `/AS`-
  selected appearance pdfce *does* paint is a reference-side gap
  (`pdfium-fflodraw-widget`), not a pdfce error.
- **Synthesized no-`/AP` appearances** — pdfium invents a look for some
  annotations that lack an appearance stream (the `/Circle /IC` fill);
  R43 forbids pdfce from inventing one (`pdfium-synthesized-noap`).

Both are detected from pdfce's own annotation diagnostics
(`annots_widget`, `annots_no_ap`) and bucketed `reference-divergence`. The
**default content-only run avoids them entirely**, which is why it is the
primary mode.

## 6. DeviceCMYK colorimetry characterization (deliverable 7)

Decision 006 §3.7 established that pdfce's `Rgb::from_cmyk` is the naive
additive `1 − min(c+k, 1)`, whereas pdfium uses its calibrated
`AdobeCMYK_to_sRGB1` table — a real, systematic, visible divergence
affecting **all** DeviceCMYK fills/strokes (not just JPEGs; measured 37.4%
of pixels >8 delta on the corpus CMYK JPEG). This harness **characterizes
and quantifies it corpus-wide** but does **not** fix it here (that would
confound the colour change with the harness build — Y5; and decision 006
revisit-trigger 7 requires re-pinning the §3.4 polarity matrix *before* any
colour change). It is filed as the harness's **first named residual** — a
follow-up colour Pass, promotable via `pdfce-acrobat-librarian`'s already-
filed "what does Acrobat do for uncalibrated DeviceCMYK→screen" question.

The `summary` reports the `frac_over_32` distribution for **DeviceCMYK-only**
pages (DeviceCMYK present, no *other* disclosed gap) against the clean
baseline, so the colorimetry effect is isolated and sized. DeviceCMYK
presence is detected by a tooling-side **file byte-scan** for `/DeviceCMYK`
(no render-side counter is added — adding render capability is a non-goal;
observing is not applying).

## 7. Usage

```sh
cargo build --release -p pdfce-cli          # prerequisite

# default: content-only, 150 DPI, ≤4 sampled pages/file, full corpus
python tools/render-parity/render_parity.py

# bounded subset with more diff panels
python tools/render-parity/render_parity.py --max-files 200 --emit-diffs 12

# full-corpus breadth, first page of every file (fast sweep)
python tools/render-parity/render_parity.py --pages-per-file 1

# one specific page's diff panel (demo / triage)
python tools/render-parity/render_parity.py --diff "6-2-2-t01" --diff-page 1

# gate mode for a render-touching Pass
python tools/render-parity/render_parity.py --gate --max-unexplained <baseline>

# full corpus, pinned binary, results kept out of the baseline dir
cp target/release/pdfce-cli.exe /tmp/pinned.exe
python tools/render-parity/render_parity.py \
    --pages-per-file 1 --dpi 125 --cli /tmp/pinned.exe \
    --out tools/render-parity/out-<label>
```

Key options:

| Option | Default | Effect |
|---|---|---|
| `--dpi` | 150 | render DPI; `scale = dpi/72` on both engines |
| `--pages-per-file` | 4 | sampled pages per file (`0` = all) |
| `--max-files` | 0 | cap the file list (`0` = all) |
| `--annots` | off | compare *with* annotations (turns the reference-divergence confounder on) |
| `--band` / `--band-pct` | — / 99.9 | band override / clean-population percentile |
| `--emit-diffs N` | 8 | diff panels for the worst pages |
| `--timeout` | 120 s | per-page **pdfce** render timeout |
| `--pdfium-timeout` | 120 s | per-request **reference** timeout — the only place a PDFium *hang* can be caught (§9.2) |
| `--checkpoint-every` | 100 | files between `progress.json` checkpoints (`0` = never) |
| `--verify-aborts` / `--no-verify-aborts` | on | re-run each aborted file alone to confirm the abort reproduces (§9.3) |
| `--cli PATH` | `target/release/pdfce-cli` | **pin the measured binary.** A full sweep takes minutes-to-tens-of-minutes; in a repo under active development a concurrent `cargo build` can relink `target/release` *mid-sweep*, turning healthy pages into spurious `pdfce:` skips. Pass a **copy** of the binary. The path used is recorded in `summary.json`. |
| `--rebaseline` | off | deliberately re-record a stale baseline, archiving the old one (§11) |
| `--out DIR` | `tools/render-parity/out` | output dir — *and* the dir whose `summary.json` is the baseline |

Exit codes: `0` success / gate PASS · `1` gate FAIL · `2` setup error
(no CLI, no corpus, nothing measurable) · `3` **stale baseline refused**, or
the run did not complete (gate INDETERMINATE).

## 8. Gate role — required on every render-touching Pass (deliverable 6; R34/R46 pattern)

This is the standing render-fidelity gate. Like `tools/roundtrip` (R34) and
`tools/content-identity` (R46) it is an **out-of-tree local corpus gate** —
it is **not** in `.github/workflows/ci.yml` because pypdfium2 is not a CI
dependency (and pdfce ships no runtime dependency on it). It **MUST be
re-run** on every Pass that touches `pdfce-render`, `pdfce-core`'s content-
stream interpretation, colour, fonts, or images — **especially the vector-
editing Pass (Pass 9)**, whose content-stream edits re-render the very pages
this harness measures.

Procedure for a render-touching Pass:

1. Run the harness over the loadable corpus at a fixed DPI.
2. **Check the corpus fingerprint matches the baseline's** (§11). If it does
   not, the counts are not comparable and no amount of care in step 3 makes
   them so — the harness enforces this and will refuse rather than let you
   find out afterwards.
3. Confirm the three-bucket counts against the recorded baseline
   (`out/summary.json`): the **unexplained** count must not rise without a
   named, filed reason (a new render-gap item, R20/R27), and the **band**
   derivation must be reported (never a bare pass/fail).
4. `--gate --max-unexplained <baseline>` returns non-zero if the unexplained
   count exceeds the recorded baseline — the mechanical enforcement. It
   returns **INDETERMINATE (exit 3)** rather than PASS if the sweep did not
   complete (§10).

The band is re-derived every run, so it tracks the current renderer; a
regression shows up as a *new* page crossing from benign/known-gap into
unexplained, enumerated by file+reason.

## 9. Crash isolation — why PDFium runs in a child process

### 9.1 The failure

`fixtures/external/pdfium/testing/resources/bug_457855936.pdf` is a 759-byte
fuzzer artefact: no `%PDF-` header, a `startxref` pointing at garbage, two
`trailer` tokens, and runs of filler bytes. It trips an internal `CHECK()`
inside PDFium's own C++ **during `FPDF_LoadDocument`** — at *open*, before any
page is touched. A firing `CHECK` calls `abort()`.

Measured 2026-08-08 (`pypdfium2` in-process, no render, just the open):

```
$ python -c "import pypdfium2 as p; p.PdfDocument('...bug_457855936.pdf')"
(no output at all)
ExitCode: 0x80000003          # STATUS_BREAKPOINT
```

That is **not a Python exception**. `try/except Exception` does not see it.
`finally` does not run. No traceback is printed. Every result held in memory
is lost. The file sorts at **index 234 of 4,023**, so the harness died about
6 % into a corpus sweep and produced *nothing* — which is why full-corpus
render verification had been blocked rather than merely inconvenient.

**pdfce handles the same file correctly**, which is what makes the
attribution unambiguous:

```
$ pdfce-cli render-page .../bug_457855936.pdf --page 1 --scale 1.7361 -o out.png
pdfce-cli: ...: not a PDF: no %PDF- header in the first 759 bytes
exit 4
```

pdfce refuses it cleanly with a named reason. The fault is entirely
reference-side, and the harness must be able to *say so* rather than die.

### 9.2 The fix

`tools/cmyk-calibration/corpus_cmyk.py` already proved the technique in this
repo: run PDFium in a **child process**, so an abort kills the child and the
parent merely observes an exit code. This harness ports that and adds the one
thing a 4,023-file sweep needs that a per-file `python -c` does not — the
child is **persistent**. `corpus_cmyk.py` spawns a fresh interpreter per file,
which is fine for a few dozen DeviceCMYK files; over the full corpus it would
add a Python startup *plus* a `pypdfium2` import (which `dlopen`s the PDFium
binary) to every file, on the order of 0.3–0.5 s each — 20–35 minutes of pure
overhead. So `pdfium_worker.py` is a long-lived request/response server:

| Layer | Responsibility |
|---|---|
| `pdfium_worker.py` (child) | the **only** thing that imports `pypdfium2`. Newline-delimited JSON on stdin/stdout: `ping` / `count` / `render` / `quit`. Rasters go out as raw top-left-origin **RGBA8** (no PNG — nothing but the parent reads them, and an encode+decode per page over ~4,000 pages buys nothing). |
| `PdfiumWorker` (parent) | spawns once, reuses across the corpus, respawns only on death. A daemon reader thread feeds reply lines into a `Queue` so `request()` can apply a wall-clock timeout — the **only** place a PDFium *hang* can be caught, since a wedged child cannot report its own wedge. |

Three outcomes, all distinguishable, which is the whole point:

| Worker behaviour | Parent sees | Result |
|---|---|---|
| `{"ok": true, …}` | reply | measured |
| `{"ok": false, "error": …}` | reply | `skip` (a *catchable* PDFium failure — information about the file) |
| **pipe closes** | EOF + exit code | `reference-aborted` (information about PDFium) — worker respawned, sweep continues |
| no reply within `--pdfium-timeout` | queue timeout | `reference-aborted` (hang) — worker killed and respawned |

**Measurement is unchanged.** The parent rebuilds the raw buffer with
`Image.frombytes("RGBA", (w, h), data)` and runs it through the *same*
`to_white_rgb()` compositing the in-process version used. The process
boundary changed the failure mode, not a single pixel.

### 9.3 Naming the fault, and verifying it

The report never says merely "it crashed". Exit codes are decoded —
`STATUS_BREAKPOINT` (a deliberate `CHECK`/assert) is a very different finding
from `STATUS_ACCESS_VIOLATION` (a memory-safety fault) or
`STATUS_HEAP_CORRUPTION`, and POSIX signal deaths are named too
(`signal 6 (SIGABRT)`).

Attribution is honest about its own limits. A death observed while servicing
request *N* is attributed to request *N* — sound, because the worker answered
request *N−1*, but not a proof: a heap-corrupting file could in principle kill
the worker on a later allocation. So `--verify-aborts` (**on by default**,
cheap because aborts are rare) re-runs every aborted file **alone, in its own
dedicated child**, and the report marks each one:

- `[CONFIRMED]` — reproduced in isolation. A real reference-renderer abort.
- `[NOT-REPRODUCED]` — did *not* die alone. Attribution is uncertain and the
  report says so, rather than quietly upgrading a guess to a fact.
- `[UNVERIFIED]` — `--no-verify-aborts` was passed.

## 10. Partial results survive

A 4,023-file sweep is long enough that "it died, so you get nothing" is itself
a defect — it is the reason the original crash *blocked* corpus verification
instead of merely annoying someone. Three mechanisms, in increasing order of
how violent a death they survive:

1. **`out/per-page.partial.tsv`** is opened before the sweep and **flushed
   after every row**. It survives a `SIGKILL`, a power loss, anything. It has
   no `bucket` column *by construction*: bucketing needs the benign band,
   which is a percentile over the whole clean-by-construction population and
   therefore cannot exist until the sweep ends. A completed run deletes this
   file — **its presence means the run did not finish**.
2. **`out/progress.json`**, checkpointed every `--checkpoint-every` files
   (default 100): files done, elapsed, ETA, per-status counts, worker respawn
   count, and the aborted-file list so far.
3. **Ctrl-C or an unexpected parent-side exception** is caught, the sweep
   stops, and the **entire reporting pipeline still runs** on what was
   collected — band derivation, bucketing, distributions, diff panels.

A partial report is stamped, loudly and first:

```
******************************************************************************
** PARTIAL RUN -- THIS IS NOT A FULL SWEEP
** interrupted (Ctrl-C) after 1180/4023 files
** Every count below is a LOWER BOUND. ...
******************************************************************************
```

and carries `"run": {"complete": false, "stop_reason": …}` as the **first key**
in `summary.json`. `--gate` on an incomplete run returns **INDETERMINATE**
(exit 3), never `PASS`: a partial sweep can only *under*-count unexplained
pages, so a low number from one is not evidence of anything.

## 11. Comparability — the stale-baseline guard

`out/summary.json` doubles as the **recorded baseline** the gate compares
against (§8). Bucket counts are only meaningful between runs over the *same*
files at the *same* settings — and this corpus has already grown:

| | files | note |
|---|---|---|
| recorded baseline (`out/summary.json`, 2026-07-31) | **2,914** | veraPDF (2,907) + pdf20examples (7) |
| corpus today | **4,023** | `+1,109`: pdfium (331), qpdf (639), pdfbox (139) |

"unexplained went 1 → *n*" across that gap is **not** a regression signal; it
is an arithmetic fact about measuring 1,109 additional files. Nothing in the
old report said so, and a reader diffing the two numbers would have been
silently wrong.

### 11.1 It is worse than a count mismatch — the BAND MOVES

The corpus-size argument above is the obvious one. The measured 4,023-file
run (2026-08-08) exposed a sharper one, and it is the reason this guard is a
refusal rather than a warning.

The band is `p99.9` of `frac_over_32` **over the clean-by-construction
population** (§3). That population is a property of the corpus. Grow the
corpus, and the band moves:

| | clean pages | band |
|---|---|---|
| 2,914-file baseline | 1,728 | `0.029416` |
| 4,023-file run | 2,015 | `0.088205` |

The band is the bucket boundary, so **pages can change bucket with no change
whatsoever in pdfce**. Worked example — `veraPDF-corpus/TWG test files/TWG
test suite A019-pdfa2-pass-a.pdf` p1, the baseline's *only* unexplained page:

```
baseline  … 16.316  207.0  207  0.07961  0.07947  0.07919 …  bucket=unexplained
new run   … 16.316  207.0  207  0.07961  0.07947  0.07919 …  bucket=benign
```

Every measured value is **identical to the last decimal** — same mean, same
p95, same dmax, same `frac_over_32`. pdfce renders that page in 2026-08-08
exactly as it did in 2026-07-31. Only the *band* changed, and the page fell
below it.

So the two reports do not merely have different totals; they use **different
partitions of the same measurement space**. `unexplained: 1` and
`unexplained: 3` are not two values of one quantity. This cuts both ways and
the dangerous direction is the quiet one: a corpus addition can push the band
*up* and silently reclassify a genuine bug candidate as benign. Hence:

- the band and its derivation are printed on every run and stored in
  `summary.json` (they always were), **and**
- the fingerprint makes a cross-corpus comparison an explicit, refused act
  rather than an easy mistake.

**The mechanism.** Every report now carries:

- `corpus_fingerprint` — `{n_files, sha256_of_sorted_relpaths, roots}`. A
  digest of the sorted *relative path* list, not of file contents: it must be
  cheap enough to compute on every run, and it answers exactly the question
  that matters ("same population of files?"). A file whose *contents* changed
  under a stable name is **not** caught — stated here rather than pretended
  away.
- `comparability_config` — only the settings that change the numbers: `dpi`,
  `pages_per_file_cap`, `annots`, `band`, `band_pct`,
  `pixel_delta_threshold`. `--emit-diffs`, `--out` and the timeouts do not
  change a measured value and are excluded.
- `baseline_comparison` — whether the report already in this output dir is
  comparable, and if not, **every** reason.

**The behaviour.** The check runs **before any rendering** — a mismatch costs
a second, not an hour. On mismatch the harness prints a `!!!!` banner naming
each difference, **refuses to overwrite the baseline**, and exits **3**
(distinct from 1 = gate FAIL and 2 = setup error, so a script can tell "your
baseline is stale" from "pdfce regressed"). Two ways forward, both explicit:

```sh
# keep the baseline intact, write elsewhere
python render_parity.py --out tools/render-parity/out-<label>

# deliberately re-record the baseline (ARCHIVES the old one first)
python render_parity.py --rebaseline
```

A missing fingerprint is treated as **incomparable**, not as "probably fine":
a report written before fingerprinting existed cannot prove what it measured,
and the failure this guard exists to prevent is a false equivalence.

**Cost of regenerating the baseline** (the operator's call, not the
harness's): one full sweep at `--pages-per-file 1 --dpi 125` over 4,023 files,
**measured at 641 s ≈ 10.7 minutes** wall-clock on this machine
(2026-08-08, `out-corpus-4023/summary.json` → `run.elapsed_s`), one command,
no engineering work:

```sh
cp target/release/pdfce-cli.exe /tmp/pinned.exe
python tools/render-parity/render_parity.py \
    --pages-per-file 1 --dpi 125 --cli /tmp/pinned.exe --rebaseline
```

The only judgement needed is *whether* the 4,023-file numbers should become
the reference the gate defends — which also re-bases the band (§11.1), and so
re-partitions every page. That is a decision, not a chore.

## 12. Dependencies, licensing, invariants

- **No new pdfce runtime dependency.** `pypdfium2` is dev/tooling only,
  invoked out-of-tree exactly like the other corpus harnesses. It does
  **not** enter pdfce's shipped dependency set or `THIRD_PARTY_LICENSES.md`.
  pdfce depends on it **at no point** — the harness shells out to the already-
  built `pdfce-cli` binary and imports pypdfium2 only in this Python script.
  (`pypdfium2` is Apache-2.0/BSD-3-Clause-licensed and bundles the
  BSD-3-Clause PDFium binary; relevant only to whoever *runs the harness*,
  never to a pdfce build or release — LEGAL §6.)
- **GUI-core separation** is untouched — this is tooling, imports nothing
  from the GUI shell, and drives `pdfce-cli` (itself GUI-free) as a subprocess.
- **Determinism / locale-invariance** — files are sorted; DPI is fixed; no
  timestamps or clocks enter the report; both renderers are deterministic.

## 13. Honest scope (decision 010 non-goals — binding)

- **Measurement only.** No new render capability: Type3, `sh`, `/SMask`,
  `/OC`, and DeviceCMYK stay their own filed items — the harness *buckets*
  them, it does not implement them (beyond any cheap, clearly-a-bug fix it
  surfaces). No editing capability of any kind.
- **Benign noise is characterized, not eliminated.** Two independent
  renderers never agree pixel-for-pixel; that is not a defect to chase.
- **Tooling-only.** No GUI visual-diff surface (a natural later addition).
- **Not a "pixel-perfect" claim.** The deliverable is a measured, bucketed
  report with the residual named (R20/R27). Whether the Pass 1.1 remainder
  is reported "closed" depends on the harness genuinely running at full-page
  corpus scale — stated exactly, never overclaimed (the Pass 6.0 caveat).
