# intent-census — how often does a real PDF name a rendering intent, and how many distinct colour sources does one document carry?

Two throwaway-cheap corpus scanners that exist to answer **one question pdfcer
could not previously answer with a number**, asked by the sibling `iccce`
project on 2026-08-25:

> *"What is the realistic worst case — how many distinct
> `(source, destination, intent, BPC)` combinations could one page actually
> produce? I have a cache design that is fine at 4 and unpleasant at 40."*

That question decides whether a colour-transform cache is built eagerly (all
intents × black-point-compensation states, costed by iccce at roughly **13 s
and 290 MiB per profile pair**, itself a linear extrapolation from four
measured builds) or lazily on the combinations a document actually uses. It is
not a question of taste; it is a population question, and until this tool ran
pdfcer had **no measurement of it at all** — `docs/NEXT_SESSION.md` §4 said so
in as many words.

This file is the **logic**. The two scripts are the syntax that enacts it.

| Script | Question it answers |
|---|---|
| `ri_census.py` | How many documents name a rendering intent (`/RI` in an `/ExtGState`, or the `ri` content-stream operator), and how many name **more than one**? |
| `icc_census.py` | How many **distinct** `/ICCBased` source-profile object references, and how many `/DestOutputProfile` destination references, does one document carry? |

Nothing here ships. pdfcer depends on none of it. Same standing as
`tools/cmyk-calibration` and `tools/render-parity` (`docs/LEGAL.md` §6): a
development instrument, not a runtime artefact.

---

## 1. Why the naive version of this scan is wrong, and by how much

The obvious implementation greps the file bytes for `/RI`. That
implementation **is in here**, deliberately, as the `raw` pass — because its
error is the most useful number the tool produces.

A rendering intent reaches a PDF by two routes (ISO 32000-1 §8.6.5.8, Table 58
`/RI`; §8.4.5 Table 58's `ri` operator):

1. `/RI /RelativeColorimetric` inside an `/ExtGState` dictionary, which in any
   modern producer's output lives **inside an object stream** (§7.5.7) and is
   therefore Flate-compressed;
2. `/Perceptual ri` inside a **content stream**, likewise Flate-compressed.

So the raw pass can only see intents in files whose object and content streams
happen to be uncompressed — which in practice means hand-written test files
and decompressed fixtures. **Measured undercount on the suite v5.0 patch set:
the raw pass found 0 files with an intent; the deep pass found 12 of 51.** Not
"an undercount" as a caveat — a *total* miss on the population that matters.

The `deep` pass therefore inflates every `stream`…`endstream` body that zlib
will accept and re-runs the same search over the inflated bytes. It does this
**without parsing the xref, the trailer, or the object graph at all**, which is
what keeps it cheap enough to run over four thousand files on a machine that
is also running SOLIDWORKS:

- a stream body that is not Flate simply fails to inflate and is skipped — and
  that is *correct*, not sloppy, because a non-Flate stream cannot be hiding a
  Flate-compressed `/RI`;
- inflation is capped at 4 MiB per stream and 64 MiB per document, so a
  decompression bomb costs a bounded amount of nothing;
- files above 40 MiB are skipped outright.

The cost of that shortcut is stated rather than hidden: the deep pass **cannot
attribute an intent to a page**, because it never builds a page tree. It
answers *"does this document name more than one intent"*, which is the
cache-sizing question, and it does **not** answer *"does this page switch
intent mid-stream"*, which would need a real content-stream walk.

## 2. What was measured, 2026-08-25

### 2.1 `fixtures/` — 4 239 files, deep sample of 350

| statistic | value |
|---|---|
| raw pass, any intent | 16 of 4 239 |
| raw pass, more than one intent | 6 of 4 239 |
| deep pass, any intent | 5 of 350 |
| deep pass, more than one intent | **0 of 350** |

Every one of the six raw-pass multi-intent files is a **veraPDF conformance
probe** under `6.2 Graphics/…/Rendering intents` or `…/Extended graphics
state` — files whose entire purpose is to name all four intents in one
document. One of them (`veraPDF test suite 6-2-8-t03-fail-a.pdf`) names an
intent called **`/Custom`**, which is legal: §8.6.5.8 permits names outside the
standard four and requires a reader to substitute a default. Any consumer of a
pdfcer-supplied intent must therefore accept a name it does not recognise.

★ This corpus is **dominated by conformance suites** (2 907 of 4 239 files are
veraPDF). It is the wrong population to generalise print-production behaviour
from, which is why the suite set below is reported separately rather than
pooled with it.

### 2.2 The print-conformance suite patch set — 51 files, all deep-scanned

Print-production files, produced by real prepress tooling.

| statistic | value |
|---|---|
| raw pass, any intent | **0 of 51** |
| deep pass, any intent | **12 of 51** (24 %) |
| deep pass, more than one intent | **1 of 51** — `PCS3_221`, `Perceptual` + `RelativeColorimetric` |
| distinct `/ICCBased` source references, max | **4** (`PCS3_130`) |
| distinct `/ICCBased` source references, mean | 0.29; 12 of 51 files carry any |
| distinct `/DestOutputProfile` references | **exactly 1 in every one of the 51 files** |

The last row is the load-bearing one for a cache design: **the destination
profile is a per-document constant in 51 of 51 print-production files.** A
transform cache keyed by destination can be built once per document and reused
across every page of it.

### 2.3 The answer the numbers give

Multiplying the observed maxima rather than the theoretical ones:
**≤ 4 ICC sources + `DeviceCMYK`/`DeviceRGB`/`DeviceGray` ≈ 6 sources × 1
destination × 1–2 intents × 2 BPC states ≈ 12–24 combinations as a worst
observed case, and 1–2 in the typical file.** Nothing in either corpus
approaches 40. An eager 4 × 2 build would be waste in **100 %** of the files
measured, because no file in either corpus used more than two intents.

★ **Object-reference identity is not profile identity.** `icc_census.py`
counts distinct `<n> <g> R` references, which is a *lower* bound on distinct
profiles if two objects hold byte-identical profiles, and an *upper* bound on
cache entries if the cache is keyed by object id. Both directions are stated
because neither is the number on its own.

## 3. Evidence class

**Corpus census, structural.** Not a rendering comparison and not ground
truth: the tool reads what producers wrote, and says nothing about whether any
consumer honours it. The `raw`-versus-`deep` split is itself the tool's
internal control, and it is the reason the `fixtures/` number and the suite
number disagree by two orders of magnitude — one population compresses its
object streams and the other is full of decompressed test files.

## 4. Running it

```
python tools/intent-census/ri_census.py  <root> <deep-sample-size> <out.tsv>
python tools/intent-census/icc_census.py <root>
```

`ri_census.py` writes one TSV row per file that named any intent, carrying
both the raw-pass and deep-pass answers so the undercount is re-derivable
rather than quoted. `icc_census.py` prints its table to stdout — it deep-scans
every file and is meant for small directories.

Neither script writes into the corpus, and neither needs a built pdfcer.
