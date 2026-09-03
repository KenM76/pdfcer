# pdfcer — OCR engine survey and binding decision material

**Status:** research complete, **decision not made**. This document exists to
close the `docs/PRIOR_ART.md` open item *"OCR engine binding — not yet
decided"* by putting the actual evidence in front of whoever makes the call.
It ends with a recommendation, the strongest argument against that
recommendation, and **one licensing question that is the operator's to answer,
not the engineer's** (project rule 13, `LEGAL.md` §6.2 step 4).

**Written:** 2026-08-12.
**Scope:** which OCR engine `pdfcer-core` binds to, what shipping it costs, and
what the recognised text must become once it exists.
**Not in scope:** the OCR Pass's own UI layout (that is a
`pdfcer-ui-specialist` dispatch when the Pass is scoped), and the
`Editable Text and Images` reconstruction mode (a separate, later capability —
see §7.3).

**Files this document deliberately does not touch:** `docs/PRIOR_ART.md`,
`docs/ROADMAP.md`, `docs/FEATURES.md`, `docs/SESSION_LOG.md`. Those are
`pdfcer-librarian`'s. When a decision is taken, the librarian files the
`PRIOR_ART.md` row and the ROADMAP movement; this file is the research record
that the row will point at, the same relationship `PRIOR_ART.md` has to
`THIRD_PARTY_LICENSES.md`.

---

## 1. Why this is not an ordinary dependency pick

Most crate choices in this project are settled by `LEGAL.md` §6.2 step 3:
the licence is permissive, so proceed and log it. OCR is not that, for four
reasons that compound.

**It is the first dependency whose *data* is licensed separately from its
code, and more restrictively.** Every prior pdfcer dependency has been a crate
whose licence `cargo-about` can see. A neural OCR engine is a small program
plus a large trained model, and the model is a copyrightable work in its own
right that inherits the licence of the corpus it was trained on. §3.3 below
records the finding that decides most of this survey: the leading pure-Rust
candidate's models are **CC-BY-SA-4.0**, not permissive, and
`THIRD_PARTY_LICENSES.md` is structurally incapable of noticing.

**It is the largest single thing pdfcer would ship.** A portable-folder product
measures its dependencies in megabytes on disk, not just in packages. The
candidates in this survey range from ~0 MB (a Windows-supplied API) through
~12 MB (a pure-Rust model pair) to ~50–100 MB+ (Tesseract with a useful set of
language files). This is why the operator's 2026-08-12 strippability request
matters here more than anywhere else in the codebase.

**It is the first dependency that puts substantial `unsafe` into
`pdfcer-core`'s graph for performance reasons.** Decision 005's **R24** and
CI's *"assert codec crates have SIMD/unsafe features OFF"* job encode a
deliberate posture: pdfcer buys `forbid(unsafe_code)` at the cost of decode
speed, on a parser fed adversarial input. Every neural runtime inverts that
trade. §3.6 measures how much.

**Its output is an inference, and pdfcer has a binding rule about
inferences.** Project rule 4 (*fuzzy, never sneaky*) is not satisfiable by an
engine that cannot say how sure it is. §7.2 shows that the two leading
candidates differ on exactly this point, and that the difference cuts the
opposite way from every other criterion in this survey.

---

## 2. The constraint set, stated as things that can be checked

These are not preferences. Each one is either a written project invariant or a
CI job that fails the build, and each is cited so a later reader can re-check
it rather than trust this paragraph.

### 2.1 MIT, so GPL/AGPL cannot be linked at all

`LEGAL.md` §1 (MIT, decided 2026-08-01) and §6.1: strong copyleft is
*"categorically, permanently off the table as a real dependency."* Weak
copyleft (LGPL, MPL-2.0) is **not** auto-disqualified but must be **flagged to
the operator before adding**, never chosen by an agent — §6.2 step 4. The
veraPDF precedent (§6.5) shows what "flagged" looks like in practice, and also
shows the one shape that is *not* a dependency question at all: a
separate-process tool pdfcer runs but never ships.

### 2.2 `pdfcer-core` and `pdfcer-render` must type-check for `wasm32-unknown-unknown`

This is enforced, not aspirational. `.github/workflows/ci.yml`, job
`cross-target compile check (macOS / wasm32)`:

```
cargo check -p pdfcer-core -p pdfcer-render --target wasm32-unknown-unknown
```

`pdfce-gui` and `pdfcer` are deliberately excluded — they are native
shells. The job's own comment states what it proves and what it does not:
*"the workspace type-checks for these targets… NOT that it links, runs, or
opens a window."* An OCR engine living in `pdfcer-core` must survive that
command, or must be feature-gated off in the configuration the job builds.

### 2.3 No network client may enter any pdfcer crate

`ARCHITECTURE.md` §1.1, decision 003 **R12**, enforced by the `no-network` CI
job — a fail-closed `cargo tree` denylist of
`reqwest|ureq|hyper|isahc|attohttpc|curl|surf|native-tls|openssl|rustls|webpki|tokio|socket2|hickory|trust-dns`
across all four crates at the shipped Windows target. Adding one *"requires a
NEW decision record."*

This is worth reading carefully, because it forecloses more than the obvious
case. It rules out cloud OCR (§6), yes — but it equally rules out the common
convenience pattern of **downloading the model on first use**. Whatever model
pdfcer needs must be present as a file before pdfcer runs, because pdfcer cannot
contain the code to fetch it.

### 2.4 Single folder, no installer, no registry, no system runtime

`ARCHITECTURE.md` §1. Anything requiring a system-wide install, a `PATH` entry
pointing outside the folder, or a separately-installed runtime (a JRE, a
Python, a Visual C++ redistributable the user must obtain) is a serious mark
against a candidate, and each candidate below answers it explicitly.

### 2.5 OCR must be strippable at build time

The operator asked on 2026-08-12 whether modules could be stripped for lighter
builds, and the answer became a written convention in
`crates/pdfcer-core/Cargo.toml`'s `[features]` header:

1. **Default ON.** *"A capability that silently disappears from a default
   build is a regression wearing a feature flag."*
2. **The gated-out path refuses by name** (**R27**) — never a silent wrong
   answer, never a grey box.
3. **The dependency is `optional = true` and named by the feature**, so
   disabling it removes the code *and* its `THIRD_PARTY_LICENSES.md`
   attribution.
4. **CI builds `--no-default-features`**, which is what stops a `cfg` from
   rotting.

The sole existing instance is `default = ["jpx"]` / `jpx =
["dep:hayro-jpeg2000"]`. OCR is the second, and it is the case the convention
was really written for: it is the heaviest capability pdfcer will ever gate.

Note rule 1's tension with OCR specifically, and resolve it deliberately when
the Pass is scoped: *default ON* is right for a codec that costs a megabyte,
and is a genuine question for a capability that costs twelve. §8.2 proposes an
answer.

### 2.6 `pdfcer-core`/`pdfcer-render` may never gain a GUI/windowing dependency

`ARCHITECTURE.md` §3, project rule 2. No OCR candidate in this survey
threatens it directly, but one class does so indirectly — a platform OCR API
reached through a large OS-bindings crate (§5.1) — and that is called out
where it applies.

---

## 3. Candidate: `ocrs` + `rten` (pure Rust)

**`ocrs`** is Robert Knight's OCR engine; **`rten`** ("Rust Tensor engine") is
the neural runtime underneath it, by the same author. They are the only
credible pure-Rust OCR stack in the ecosystem.

### 3.1 Summary table

| Dimension | Finding |
|---|---|
| **Licence (code)** | **`MIT OR Apache-2.0`** — permissive. Verified from the crates.io API for `ocrs` 0.12.2 and `rten` 0.25.0, and from `ocrs/Cargo.toml`'s `license` field; both `LICENSE-APACHE.txt` and `LICENSE-MIT.txt` exist at the repo root. **Do not cite `api.github.com`'s `spdx_id: Apache-2.0`** — that is GitHub's single-file detector picking one arm of a dual licence. |
| **Licence (models)** | **`CC-BY-SA-4.0` — NOT permissive.** See §3.3. This is the survey's decision-driving finding. |
| **Purity** | Pure Rust, end to end. rten's README: *"End-to-end Rust. This project and all of its required dependencies are written in Rust."* Confirmed against the resolved graph — no `*-sys` crate, no `bindgen`, no ONNX Runtime linkage, **no C/C++ toolchain at build time**. |
| **Single-folder** | Yes, trivially. Code compiles into pdfcer's own binary; the only additional artifacts are two model files. |
| **Model files** | 2 files, **12,226,852 bytes ≈ 12.23 MB** total (§3.4). |
| **WASM** | **Yes — measured, not claimed.** See §3.5. |
| **Accuracy** | **No published benchmark exists.** Anecdotal, and the project says so itself. See §3.7. |
| **Languages** | **Latin alphabet only.** Adding a script is an ML training project, not configuration. See §3.8. |
| **Maintenance** | Very active, **bus factor 1**. See §3.9. |
| **Feature-gating** | Clean — a single optional dependency. See §3.10. |
| **Confidence scores** | **None, at any level.** See §7.2. This is the one criterion where `ocrs` loses outright. |

### 3.2 The dependency graph, measured

Run on a scratch crate carrying only `ocrs = "0.12.2"`, with pdfcer's own
pinned toolchain, rather than read off crates.io pages:

```
cargo metadata --format-version 1 --all-features
```

**42 packages** besides the probe crate. Every one is permissive:

| Licence expression | Count |
|---|---|
| `MIT OR Apache-2.0` | 38 |
| `MIT/Apache-2.0` (`bitflags` 1.3.2, legacy slash syntax, same meaning) | 1 |
| `Apache-2.0 OR MIT` (`rustc-hash`) | 1 |
| `Apache-2.0` (`flatbuffers` 24.12.23) | 1 |
| `(MIT OR Apache-2.0) AND Unicode-3.0` (`unicode-ident`) | 1 |

**Zero copyleft in the code graph.** Two rows deserve a note rather than a
tick:

- **`flatbuffers` is Apache-2.0 *only*** — no MIT arm. Still permissive, still
  fine under §6.1, but it is the one crate here that cannot be taken under
  MIT if that ever matters for uniformity, and Apache-2.0's §4(b)
  modification-notice and NOTICE-file obligations are slightly heavier than
  MIT's. It arrives via `rten`'s `rten_format` feature (the `.rten` FlatBuffers
  model container).
- **`unicode-ident`'s `AND Unicode-3.0`** is the same row `PRIOR_ART.md`
  already dispositioned for `subsetter`: `AND` means both terms apply, so it
  cannot be satisfied by picking the friendlier arm — but it is **already in
  pdfcer's graph today** via `syn`/`proc-macro2`, so it adds no new attribution
  obligation.

**The `no-network` denylist passes.** Run exactly as CI runs it, against the
probe crate's graph at the shipped Windows target:

```
cargo tree -p <probe> --target x86_64-pc-windows-msvc | grep -Ei '(^|[[:space:]])(reqwest|ureq|…)[[:space:]]'
→ no match   (PASS: no network-client crate in the ocrs graph)
```

This is not an accident of the current version — it is structural, and §3.11
explains why it is likely to stay true.

### 3.3 ★ THE MODELS ARE `CC-BY-SA-4.0`, AND `cargo-about` CANNOT SEE THEM

This is the finding that makes OCR an operator decision rather than an
engineer's log entry.

**The fact, measured.** The Hugging Face API for `robertknight/ocrs` returns
`cardData: {"license": "cc-by-sa-4.0"}`, and the repository carries the tag
`license:cc-by-sa-4.0`. The cause is traceable and unsurprising: the
`ocrs-models` README states the models are trained on datasets that are
*"a) open and b) have non-restrictive licenses. This currently includes:
HierText (CC-BY-SA 4.0)"* — and the share-alike condition propagated from the
training corpus into the weights.

**Why this is not a copyleft-contamination scare.** CC-BY-SA is a licence on a
*creative work*, not a software licence, and it has no linking concept at all.
Creative Commons' own FAQ says two things that matter here: that it
*"recommend[s] against using Creative Commons licenses for software"* because
CC licences *"do not contain specific terms about the distribution of source
code"*, and that a **collection** which includes BY-SA material alongside other
material is treated differently from an **adaptation** that modifies the
underlying work — the collection may carry its own licence; only adaptations
must be released under BY-SA. On that reading, shipping the unmodified `.rten`
files next to MIT code is distribution of a verbatim work in a collection, and
**there is no propagation path to pdfcer's own MIT licence**. That is the same
shape of reasoning `LEGAL.md` §6.5.2 already applied to MPL-2.0 for veraPDF.

**Why it is nevertheless the operator's call, and not mine.** Three things
resist being cleared by an agent:

1. **It is a judgement call the licence text does not name.** "Mere
   aggregation into a single install folder is a collection, not an
   adaptation" is a *reading*. It is a well-supported reading, and it is a
   reading. This project has an explicit written lesson about the cost of an
   agent asserting an unmeasured fact about its own environment (`LEGAL.md`
   §1.1); a legal conclusion asserted the same way would be worse.
2. **`LEGAL.md` §6.2 step 4 says to stop and ask for anything that is not
   permissive**, *"even if pdfcer's current license would technically allow
   it — this is a case where getting it wrong is expensive to unwind later."*
   CC-BY-SA-4.0 is not permissive. The rule fires on its own terms.
3. **Any *adaptation* clearly does propagate.** Fine-tuning the weights,
   quantizing them, retraining on pdfcer's own corpus, or converting them to a
   different runtime's format plausibly creates Adapted Material, which must
   then be released under CC-BY-SA-4.0 or a compatible licence. That would
   bind **the derived model**, not pdfcer's source — but it means "we'll just
   fine-tune it later for CAD drawings" is a decision with a licence
   attached, and it should be known now rather than discovered then.

**The attribution gap is concrete and must be closed by hand.** `LEGAL.md`
§6.3 makes `THIRD_PARTY_LICENSES.md` the compliance artifact, and it is
generated by `cargo-about` *from the Cargo dependency graph*. A model file is
not a Cargo dependency. **`cargo-about` will not see it, will not attribute it,
and nothing will fail.** CC-BY-SA-4.0 requires attribution, a licence
notice/link, and an indication of changes — so a bundled model needs its own
attribution entry, authored deliberately, in a file that is *otherwise*
mechanically generated and explicitly *"never hand-edited"*. That tension
needs resolving when the Pass is scoped (§8.4 proposes how), and it is exactly
the "correctly absent, don't fix it" hazard §6.5.4 rule 5 warns about, running
in reverse: here the omission would be **incorrect** and would look identical.

### 3.4 Model files: what ships, and how big

Measured from the Hugging Face tree API (`robertknight/ocrs`, `main`):

| File | Bytes | MB | Role |
|---|---|---|---|
| `text-detection-ssfbcj81.rten` | 2,523,564 | 2.52 | text detection (semantic segmentation) |
| `text-rec-checkpoint-s52qdbqt.rten` | 9,716,444 | 9.72 | text recognition (CRNN) |
| **Total shipped** | **12,240,008** | **≈ 12.24 MB** | |

The repository also holds `.pt` PyTorch checkpoints and `.onnx` exports; pdfcer
would ship only the two `.rten` files.

A small discrepancy worth recording rather than smoothing over, because it
shows the two distribution channels are not byte-identical: the author's S3
objects, measured by `Content-Length` in the same session, are
`text-detection.rten` **2,510,284 B** and `text-recognition.rten`
**9,716,568 B** — 13,280 bytes less and 124 bytes more than their
Hugging Face counterparts respectively, and under different filenames. The
totals agree to within 0.1% (12.23 MB vs 12.24 MB), so nothing in this survey
turns on it, but **pdfcer must pin exactly which artifact it ships and hash
it**, rather than treating "the ocrs models" as one thing.

> ### ★★★ AMENDED 2026-08-25 — EVERYTHING TURNED ON IT (`Pass 129.0`, `181d9bd`, two-hundred-and-sixty-second filing)
>
> **The paragraph above is left exactly as written** (`R215` (d)) because
> the wording is the finding.
>
> **"So nothing in this survey turns on it" was wrong.** The Hugging Face
> detection build, `text-detection-ssfbcj81.rten` (2,523,564 B), **does not
> work with `ocrs` 0.12.2.** pdfcer shipped it on 2026-08-13 and **every OCR
> run pdfcer made until 2026-08-25 produced garbage on any page** — sixteen
> fragments at the right page margin plus one "word" whose bounding box was
> the whole page, on a clean 150 dpi render of 12 pt Helvetica. Not degraded
> output. Noise.
>
> **This paragraph's own NEXT SENTENCE was exactly right, and was followed —
> for provenance only.** The artefact was pinned and hashed; **it was never
> run end to end.** Pinning the wrong file precisely is still shipping the
> wrong file.
>
> **★ Where the reasoning went, in one line, because it is reusable:** the
> discrepancy was **measured on the detection file** (13,280 B, **0.53 %**
> of that file) and then **evaluated against the COMBINED two-file total**
> (0.11 %). **The denominator swap is what made a different model look like
> a rounding error.** Two builds of one network, under two filenames, in two
> channels, are **two models** — no percentage of anything is evidence that
> they behave the same. The only evidence for that is running them.
>
> **The isolation** (`Pass 129.0`) swapped **one file at a time**, 4 runs
> over 2 files × 2 channels: `S3 detection + HF recognition` is **perfect**,
> so the recognition model was never at fault and only the detection file
> was replaced. **The first hypothesis was wrong and is recorded beside the
> right answer**: the recognition file is named `text-rec-checkpoint`, so
> the obvious theory was that pdfcer ran a *training checkpoint* as its
> recogniser. It did not. **A plausible filename is not evidence either.**
>
> **What ships now:** S3 detection (2,510,284 B) + HF recognition
> (9,716,444 B) = **12,226,728 B over 2 files**. See
> `crates/pdfcer-core/assets/models/ocrs/PROVENANCE.md` (four-row table +
> the wrong hypothesis), `LEGAL.md` §6.7.4, `ARCHITECTURE.md`'s weights
> section, and `ROADMAP.md`'s `181d9bd` entry.
>
> **§3.5's "12 MB is small" comparison and every other figure in this
> section are unaffected** — the correction is 13,280 B on a 12 MB payload.
> **The conclusion that moved is not a size; it is whether the file worked.**

**12 MB is small** — for comparison,
KillerPDF's entire portable EXE is ~15.6 MB (`PRIOR_ART.md`), and a single
Tesseract `tessdata_best` language file is roughly a third of this on its own
(§4.5).

**Provisioning, and why it is a design constraint rather than a detail.** The
`ocrs` **CLI** downloads these from an author-controlled S3 bucket
(`https://ocrs-models.s3-accelerate.amazonaws.com/…`) via `ureq`, caching to
`~/.cache/ocrs`. pdfcer **cannot do that** — `ureq` is on the `no-network`
denylist by name (§2.3). The good news is that the split is already clean
upstream: **the download code lives entirely in `ocrs-cli`, not in the `ocrs`
library.** The library takes already-loaded `rten::Model` values through
`OcrEngineParams`. pdfcer depends on `ocrs` alone, ships the two files in its
own folder, and loads them from disk. No network code enters any pdfcer crate,
and no CI job needs an exception.

**A staleness flag, recorded because it is easy to miss under all the activity
on the engine.** The S3 objects carry `Last-Modified: Mon, 01 Jan 2024`; the
Hugging Face repo's `lastModified` is 2024-01-30; `ocrs-models` last saw a push
on 2024-08-20 and has **zero commits in the last six months**. **The weights
have not been retrained in roughly two and a half years**, even though the
runtime around them is developed daily (§3.9). Whatever accuracy `ocrs` has
today is the accuracy of a 2024 model, and improvements to `rten` do not change
it. Note also that `ocrs-models` has **no LICENSE file** — the CC-BY-SA
declaration exists only on the Hugging Face card, which is a thinner
provenance record than one would want for the one non-permissive artifact in
the build.

### 3.5 WASM: measured, with the command quoted

The README claims portability *"including WebAssembly"*, and `rten` documents
`make wasm` targets and WebAssembly SIMD. Claims are not measurements, and
this project has a written lesson about the difference. So the CI gate was run
directly, on a scratch crate carrying `ocrs = "0.12.2"`, using **pdfcer's own
pinned toolchain**:

```
$ cargo +1.97.1 check --target wasm32-unknown-unknown
    Checking rten-imageproc v0.24.0
    Checking rten v0.24.0
    Checking ocrs v0.12.2
    Checking ocrsprobe v0.1.0
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 21.19s
```

**Clean.** The probe was not a bare `use` — it constructs an `OcrEngine` from
`OcrEngineParams` through the public API, so the engine's own generic code is
instantiated rather than merely parsed.

Two things this does and does not prove, stated in the same terms the CI job
uses about itself. It **does** prove the stack type-checks for the target that
the web-fork invariant is defined against, which is precisely what job
`cross-target compile check` asserts for `pdfcer-core` and `pdfcer-render`. It
does **not** prove it runs usefully in a browser: `rayon` is a non-optional
dependency of both `ocrs` and `rten`, threads on `wasm32-unknown-unknown`
require a threaded build that pdfcer does not have today, and rten's own README
warns the non-SIMD WASM builds are *"significantly slower."* For the web fork
that is a performance question to solve later; for **today's invariant it is a
pass**, and it is the only OCR candidate in this survey that passes at all.

One incidental finding, recorded because it will bite the next person who
tries this: the check fails with `can't find crate for 'core'` if run from a
directory outside `D:\Dev\pdfcer\`, because the `rust-toolchain.toml` override
does not apply there and the default `stable` toolchain has no `wasm32` target
installed. The failure looks like an `ocrs` incompatibility and is not one.
Use `cargo +1.97.1`, or run inside the repo.

### 3.6 The `unsafe` surface — the real engineering cost, quantified

pdfcer has a deliberate, written posture on this. Decision 005's **R24** keeps
SIMD features **off** on every image codec specifically to preserve
`zune-jpeg`'s compiler-enforced `forbid(unsafe_code)`, and a dedicated CI job
fails the build if any of them is switched on, because *"enabling any codec
SIMD feature requires a NEW decision record, a measurement showing it matters,
and fuzz coverage of the enabled path — not a CI edit."*

A neural runtime is the opposite bargain by construction. Counted over the
vendored sources of the resolved graph:

| Crate | `unsafe` tokens | `forbid(unsafe_code)` |
|---|---|---|
| `rten-simd` 0.24.0 | 535 | none |
| `rten-gemm` 0.24.0 | 236 | none |
| `rten-tensor` 0.24.0 | 153 | none |
| `rten` 0.24.0 | 83 | none |
| `rten-base` 0.24.0 | 13 | none |
| `rten-vecmath` 0.24.0 | 3 | none |
| **`ocrs` 0.12.2** | **0** | none |
| **`rten-imageproc` 0.24.0** | **0** | none |
| **Total** | **≈ 1,023** | — |

Two observations, and they point in opposite directions.

**It cannot be switched off.** `rten-simd` is a non-optional dependency of
`rten`. There is no feature that yields a scalar-only build. So unlike every
codec R24 governs, this is not a knob — taking `ocrs` means taking the whole
of it. That is a genuine widening of `pdfcer-core`'s unsafe surface and it
should be recorded as an accepted exception with reasons, in the shape
decision 039 used for `aes` and `sha2` (where the unsafe was cfg-selected and
therefore also un-removable), rather than passing unremarked.

**But the threat model is materially different from a codec's.** R24's
reasoning is about a parser *fed adversarial input from the public internet*
— a malformed JPEG in an untrusted PDF reaches `zune-jpeg` directly. The OCR
path is not that shape. Its input is a raster **pdfcer itself produced** by
rendering a page, or a decoded image that has already been through the
hardened codec path; the model file is a **local asset pdfcer ships**, not
attacker-controlled content. An attacker who can choose `rten`'s input has
already gotten through the codecs. That does not make 1,023 `unsafe` sites
free, and it does not excuse skipping fuzz coverage of the raster→OCR
boundary, but it does mean this is not simply R24 being abandoned.

The existing R24 CI job is scoped to named codec crates and would **not** fire
on `rten`, so the gap would be silent. If `ocrs` is adopted, extending that job
— or writing a sibling that asserts the rten stack's feature set — is part of
the Pass, not a follow-up.

### 3.7 Accuracy: anecdotal, and the project says so itself

**There is no published benchmark**, and this is verifiable rather than merely
unfound: `ocrs` issue **#43, "Add evaluation benchmarks"**, is still **open**
(filed 2024-03-30), and the roadmap issue #14 carries *"Add benchmarks so the
accuracy can be tracked over time"* as an **unchecked** box.

The only comparison to Tesseract in the project's own words is about
preprocessing effort, not accuracy — *"works well on a wide variety of images…
with zero or much less preprocessing effort compared to earlier engines like
Tesseract"* — and the README's own caveat is the more decision-relevant line:

> **"ocrs is currently in an early preview. Expect more errors than commercial
> OCR engines."**

A general web search for a third-party `ocrs`-vs-Tesseract comparison returns
2026 OCR benchmark round-ups that cover Tesseract, PaddleOCR, EasyOCR and the
VLM-based engines, and **do not mention `ocrs` at all** — it is below the
threshold of the comparison literature.

**What follows for pdfcer.** Any accuracy claim about pdfcer's OCR would have to
be **self-measured** against pdfcer's own fixture corpus (`LEGAL.md` §5 rules
apply to what goes in it), because there is no external number to cite. Under
the global claim-bearing-copy rule that is a hard constraint on the README and
on any release note: *"pdfcer OCR is comparable to Tesseract"* would be an
invented claim today, in either direction.

It also argues for a specific sequencing choice: **measure before committing
the UI**. §8.1 makes that the first slice.

### 3.8 Languages: Latin only, and the ceiling is hard

README, verbatim: **"ocrs currently recognizes the Latin alphabet only (eg.
English). Support for more languages is planned."**

The cost of a second script is not a config change. Issue #145 (Japanese, open
since 2024-12) is a good probe of what it takes: a recognition model retrained
from scratch with the CRNN alphabet growing from ~100 Latin characters to
thousands of kanji, vertical-text layout support, ruby/furigana handling that
depends on the **unfinished** layout model, and an openly-licensed dataset
(the obvious candidate, Manga109-s, cannot be redistributed). The engine also
lacks the plumbing: *"Add the infrastructure to support multiple languages and
model updates"* is an **unchecked** roadmap item.

For pdfcer's actual likely corpus — CAD drawings, Word and Chrome print-to-PDF
output, English-language scans, the material `personal_rag/pdf` was built
from — Latin-only may be entirely acceptable. It should be recorded as a
**hard ceiling with a named consequence**, not a temporary gap: it is the one
axis on which Tesseract's 100+ languages are not merely more, but
categorically available where `ocrs` has nothing.

### 3.9 Maintenance and bus factor

| | `ocrs` | `rten` |
|---|---|---|
| Latest release | 0.12.2, 2026-03-27 | 0.25.0, 2026-08-01 |
| Downloads (all-time) | 217,344 | 1,183,809 |
| Commits, last 6 months | 53 | 518 |
| Last push | 2026-08-02 | **2026-08-12 (today)** |
| Contributors | 13 (robertknight 443, dependabot 65, 11 others at 1–4) | 7 (robertknight 4,759, next 21) |
| Stars / open issues | 1,867 / 22 | — |

**Activity: excellent. Bus factor: 1.** Excluding the bot, one person has
written ~97% of `ocrs` and effectively all of `rten`. `PRIOR_ART.md` has
already used bus factor 1 as a disqualifying consideration once — it is among
the reasons `oxidize-pdf` was rejected as a foundation (decision 001). The
distinction that matters here is **blast radius**: `oxidize-pdf` was proposed
as the whole of `pdfcer-core`, where abandonment would have been existential.
`ocrs` sits behind a feature flag at the edge of the system, its models are
static files, and the fallback if it is abandoned is that OCR stops improving
— not that pdfcer stops working. That is a survivable dependency risk, but it
is a real one, and it is a reason to keep pdfcer's OCR API defined in pdfcer's
own vocabulary (§8.3) rather than re-exporting `ocrs` types.

Two smaller notes. **MSRV is moving fast**: `rten` 0.25.0 declares
`rust-version = "1.94.0"` and `edition = "2024"`, up from 1.89 less than a
year ago. pdfcer pins 1.97.1, so there is headroom today, but this dependency
will pull the pin forward more often than the rest of the graph, and
`ARCHITECTURE.md` §2.1a requires a dated decision-log entry for every bump.
And **release-mode is mandatory** — the `ocrs` README warns in bold that debug
builds of `ocrs` and its `rten*` dependencies are *"extremely slow"*, so
pdfcer's dev profile needs a `[profile.dev.package]` `opt-level` override or
debugging anything near OCR becomes impractical.

### 3.10 Feature-gating: clean, and it fits the existing convention exactly

`ocrs` is an ordinary optional Cargo dependency. It slots into the
`crates/pdfcer-core/Cargo.toml` convention with no special handling:

```toml
[features]
default = ["jpx"]          # ← see §8.2 on whether "ocr" belongs here
ocr = ["dep:ocrs", "dep:rten", "dep:rten-imageproc"]
```

All four convention rules are satisfiable literally: the dependency is
`optional`, disabling it removes the code *and* drops those 42 packages from
`THIRD_PARTY_LICENSES.md`, and the gated-out path can refuse by name in the
`ImageCodecError::FeatureUnsupported` idiom (**R27**) — *"OCR is not available
in this build"*, never a blank text layer.

`rten-imageproc` is listed explicitly because `RotatedRect` — the type
`ocrs`'s public API returns for word geometry — is defined there. It is a
transitive public type, so pdfcer takes a direct dependency to name it, or
converts at the boundary (§8.3 recommends converting).

The pure-Rust nature is what makes this trivial. There is no build script to
make conditional, no native library to locate, no `cfg` fork between a
"with-native-lib" and "without" build. Contrast §4.6.

### 3.11 One API-hygiene note against project rule 10

`OcrEngine` returns **`anyhow::Result`** throughout its public surface. That is
an application error type in a library API, contrary to the Rust API
Guidelines' `thiserror` preference that project rule 10 binds pdfcer to. It is
not a blocker — it is a reason `pdfcer-core` must wrap `ocrs` behind its own
`thiserror` enum rather than letting `anyhow` into its public API. That
wrapping is wanted anyway for the bus-factor reason in §3.9, so the cost is
zero and the requirement should simply be written into the Pass.

---
## 4. Candidate: Tesseract via Rust bindings

Tesseract is the incumbent. It is what `ROADMAP.md`'s OCR bucket names as the
default candidate, what KillerPDF bundles (`PRIOR_ART.md`), and what OCRmyPDF
drives. It is also C++, and that single fact propagates into every row of the
table below.

### 4.1 Summary table

| Dimension | Finding |
|---|---|
| **Licence (engine)** | **Apache-2.0** — CONFIRMED from the repository's own `LICENSE` file, verbatim *"Apache License / Version 2.0, January 2004"*. Permissive, and it carries an express patent grant. Latest release **5.5.3**, 2026-07-24. |
| **Licence (Leptonica)** | **BSD-2-Clause** in substance — see §4.2 for a classifier trap. Permissive. Mandatory dependency. Latest **1.87.0**, 2025-12-24. |
| **Licence (traineddata)** | **Apache-2.0** for all three variants (`tessdata`, `tessdata_best`, `tessdata_fast`). Freely redistributable. |
| **Licence (shipped DLL closure)** | **⚠ LGPL contamination in the default build** — see §4.3. Removable, but only if you know to remove it. |
| **Purity** | C++ engine + C API. Every Rust crate is a binding. Build-time toolchain depends on which family (§4.6). |
| **Single-folder** | Yes, measured — but 19–33 DLLs and ~28–54 MB. See §4.4. |
| **Model/data files** | `eng` is 3.92 / 14.69 / 22.38 MB depending on variant; `osd` is a further 10.07 MB. See §4.5. |
| **WASM** | **No. Hard wall.** See §4.7. |
| **Accuracy** | Better sourced than `ocrs`, but weaker than the folklore. See §4.8. |
| **Languages** | **120 languages + 37 script models.** The decisive advantage. §4.5. |
| **Maintenance** | Engine: excellent. Bindings: fragmented, and the best-maintained one has the worst build story. §4.6. |
| **Feature-gating** | Possible but genuinely awkward — a native library is not a `cfg`. §4.9. |
| **Confidence scores** | **Yes, per word, with bounding boxes, at every hierarchy level.** §7.2. |

### 4.2 A licence-classifier trap worth recording

Leptonica's licence text is a standard BSD-2-Clause — the two redistribution
conditions plus the standard AS-IS disclaimer, `Copyright (C) 2001-2020
Leptonica` — but it is wrapped in a `/*====…*/` C comment block with `-` line
prefixes. **GitHub's licence classifier therefore reports `NOASSERTION` /
"Other"**, not BSD-2-Clause.

Any automated licence scan will flag Leptonica as *unknown licence*, which
under `LEGAL.md` §6.2 looks like a stop-and-ask. It is not one: the text was
read, and it is BSD-2-Clause. **Record the manual classification with the
reason**, or this will be re-escalated every time somebody runs a scanner.
This is the same shape as the `font` (pdf-rs) row already in `PRIOR_ART.md` —
a licence fact a machine gets wrong — except that here the machine's answer is
wrong in the direction of false alarm rather than false comfort.

### 4.3 ⚠ The default Windows build ships LGPL binaries

This is the finding that would have been missed by classifying "Tesseract" and
"Leptonica" and stopping there, and it is exactly what `LEGAL.md` §6.2 step 5
exists to catch (*"If a dependency is FFI to a non-Rust library… the same
license check applies to that underlying library"*).

Tesseract's default CMake configuration links **libcurl** (to fetch
traineddata over HTTP) and **libarchive** (to read `.zip`/`.tar` traineddata
bundles). The libcurl branch transitively pulls:

- **`libunistring-5.dll`** — dual **LGPLv3+ / GPLv2+**
- **`libiconv-2.dll`**, **`libintl-8.dll`** — GNU libiconv / gettext, **LGPL**

Those are weak-copyleft binaries that would sit in pdfcer's shipped folder.
Under §6.1 that is a *"flag any LGPL/MPL dependency to the user before adding
it"* situation — and it would be an absurd one to have, because **pdfcer needs
neither library**. It cannot use libcurl (the no-network posture, §2.3), and
it would ship traineddata as plain files rather than as archives.

The fix is two CMake options, both present in Tesseract 5.5.3's
`CMakeLists.txt`:

```cmake
option(DISABLE_ARCHIVE "Disable build with libarchive (if available)" OFF)
option(DISABLE_CURL    "Disable build with libcurl (if available)"    OFF)
```

Building with both `ON` removes every LGPL binary from the closure and leaves
Apache-2.0 + BSD/MIT/zlib-class licences only (plus the compiler runtime,
below). **It also happens to be the smaller build.** So the licence-clean
choice and the size-efficient choice are the same choice — but only if
Tesseract is **built from source with those flags**, which means the
off-the-shelf UB-Mannheim and official-installer binaries are *not* directly
shippable, and "just vendor the DLLs from the installer" is the wrong
instinct.

**A second, unresolved licence question in the same closure.** The MinGW-built
binaries carry `libstdc++-6.dll` and `libgcc_s_seh-1.dll`, which are GPLv3
**with the GCC Runtime Library Exception** — an exception designed to permit
exactly this redistribution. The exception's precise wording could not be
fetched during this research (gnu.org returned HTTP 429), so it is recorded
here as **UNVERIFIED** rather than cleared. An **MSVC/vcpkg build sidesteps
the question entirely** by replacing the MinGW runtime with the MSVC CRT, and
is also much smaller (§4.4). If Tesseract is chosen, resolving this is a
prerequisite, not a footnote.

### 4.4 Single-folder on Windows: measured

There is **no official portable ZIP**. The project documents no Windows
installer for versions after the legacy 3.02, and the UB-Mannheim archive
publishes only NSIS `.exe` installers across its entire history. The official
GitHub release 5.5.3 **does** attach
`tesseract-ocr-w64-setup-5.5.3.20260724.exe` (26,573,224 bytes), and that NSIS
installer **extracts cleanly with `7z x`** — which is the portable-folder
path: extract once, vendor the subset, ship the folder. No installer ever runs
on the user's machine, so §2.4 is not violated.

Measured by extracting that installer and walking the PE import tables
transitively from `tesseract.exe` + `libtesseract-5.dll`:

| Configuration | DLLs | Size |
|---|---|---|
| Full extraction (everything in the installer) | 71 | 98 MB |
| Transitive closure actually required, **default build** | 33 | **44.66 MB** |
| Transitive closure, **`DISABLE_CURL` + `DISABLE_ARCHIVE`** | 19 | **37.74 MB** |
| Same, projected for an **MSVC/vcpkg** build | ~19 | **~12–13 MB — UNVERIFIED** |

Plus `tesseract.exe` itself at 1,867,784 bytes (1.78 MB) if the subprocess
route is used.

The 19-DLL licence-clean set is: `libtesseract-5` (3.34 MB), `libleptonica-6`
(2.72), `libstdc++-6` (25.17), `libgcc_s_seh-1` (0.91), `libwinpthread-1`
(0.06), and the image codecs Leptonica needs — `libjpeg-8` (0.92),
`libtiff-6` (0.60), `libwebp-7` (0.71), `libwebpmux-3` (0.08),
`libsharpyuv-0` (0.05), `libopenjp2-7` (0.50), `libpng16-16` (0.25),
`libgif-7` (0.04), `libjbig-0` (0.06), `liblerc` (0.77), `libdeflate` (0.09),
`liblzma-5` (0.18), `libzstd` (1.16), `zlib1` (0.12).

Three observations that change the shape of the decision:

- **`libstdc++-6.dll` alone is 25.17 MB — 67% of the minimal MinGW folder** —
  and it is an *unstripped* MinGW build. This is why the MSVC projection is so
  much smaller: MSVC replaces `libstdc++-6` + `libgcc_s_seh-1` +
  `libwinpthread-1` (26.14 MB together) with a CRT that is either already on
  the machine or ~1 MB of redistributables. **The ~12–13 MB figure is a
  projection, not a measurement** — nobody built it — and it must be measured
  before being relied on, because it is precisely the number that would make
  Tesseract's footprint competitive with `ocrs`'s.
- **ICU is not required.** `libicudt78.dll` (33.1 MB) and the whole
  pango/cairo/harfbuzz/freetype/fontconfig set in the installer serve
  `text2image` and the training tools only. Dropping them saves ~45 MB, and
  noticing that they are droppable is the difference between a 98 MB answer
  and a 38 MB one.
- Every system DLL referenced but not bundled (`advapi32`, `bcrypt`,
  `crypt32`, `gdi32`, `iphlpapi`, `kernel32`, `msvcrt`, `secur32`, `user32`,
  `wldap32`, `ws2_32`) ships with Windows, so nothing else is needed on the
  target machine and §2.4's "no system-wide runtime" holds.

**Verdict on the single-folder constraint: Tesseract passes**, by measurement
rather than by hope. It is not disqualified here. It is simply expensive —
call it **~28 MB (MSVC, projected) to ~54 MB (MinGW, measured)** added to
pdfcer's folder including `eng` + `osd`, against `ocrs`'s 12.24 MB — and it
requires a from-source build to be licence-clean.

### 4.5 Language data: the decisive advantage

**Licence: Apache-2.0**, confirmed from the `LICENSE` file in all three
`tessdata*` repositories. Freely redistributable, no attribution complication
beyond Apache-2.0's ordinary NOTICE handling.

`eng.traineddata`, exact bytes:

| Variant | `eng` | Engines | Character |
|---|---|---|---|
| `tessdata_fast` | 4,113,088 B = **3.92 MB** | LSTM, integer-quantised | fastest, least accurate |
| `tessdata_best` | 15,400,601 B = **14.69 MB** | LSTM, float | slowest, most accurate |
| `tessdata` | 23,466,654 B = **22.38 MB** | LSTM **+ legacy** | middle; the only variant with the legacy recognizer |

**Do not overlook `osd.traineddata`** — orientation and script detection,
which is what auto-rotates a sideways scan. It is **10,562,727 B = 10.07 MB in
every variant**, roughly 2.5× the size of `eng` in the fast variant. If pdfcer
auto-detects page rotation (and it should — a scan fed in upside down is the
single most common OCR failure a user will hit), that 10 MB is mandatory.

A sensible pdfcer baseline is therefore `tessdata_fast/eng` + `osd` ≈ **14 MB**,
with further languages as optional operator-supplied files.

**Breadth:** 120 language/script models plus a `script/` directory with 37
script-level models, in each variant. Per-language cost tracks the script's
character-set size rather than being flat: `tessdata_fast` runs roughly **1–4
MB per language** (`ceb` 0.68 MB, `cat` 1.09, `ara` 1.37, `chi_sim` 2.35,
`ces` 3.62, outliers to ~6 MB); `tessdata_best` runs **4–15 MB** (`fra` 3.79,
`deu` 8.23, `chi_sim` 12.47, `jpn` 13.67, `rus` 14.59). Full corpora are
339 MB (fast), 1,015 MB (`tessdata`) and 1,088 MB (best) — clearly not
shippable, and clearly not needing to be, since language data is exactly the
kind of thing an operator adds on demand.

**This is the row where Tesseract wins outright and `ocrs` has nothing.** Not
"more languages" — *any* language beyond Latin. Adding Japanese to a Tesseract
build is dropping a 13.67 MB file into a folder; adding it to `ocrs` is
training a model (§3.8).

### 4.6 The Rust bindings: three families, none of them comfortable

| Crate | Latest | Last commit | Licence | Mechanism |
|---|---|---|---|---|
| `tesseract-sys` → `-plumbing` → `tesseract` / `leptess` | `tesseract` 0.15.2 (2025-04-19); `leptess` 0.14.0 (2023-02-21) | 2025-04-19 / **2023-09-13** | MIT | FFI to a **prebuilt** libtesseract; bindings generated by **bindgen** |
| `tesseract-rs` (cafercangundogdu) | **0.4.0, 2026-07-31** | **2026-07-31** | MIT | **Compiles Tesseract + Leptonica from C++ source** via CMake/NMake; hand-written FFI, no bindgen |
| `rusty-tesseract` | 1.1.10 (2024-03-25) | 2024-03-25 | MIT | **Subprocess** — `Command::new("tesseract.exe")` |

**Every binding is MIT.** There is no licence problem anywhere in this column;
the problem is entirely operational, and the three options fail in three
different places.

**The `tesseract-sys` family: avoid on Windows.** Its Windows build needs MSVC
**plus** vcpkg with user-wide integration (`vcpkg install tesseract:x64-windows`)
**plus** LLVM/libclang for bindgen with `LIBCLANG_PATH` set — the heaviest
prerequisite set of the three, for no functional gain. Both `-sys` crates
carry a *"Windows and Mac maintainers wanted"* banner in their READMEs,
`leptess` has been dormant since 2023-09-13 and still pins the old
`tesseract-plumbing ~0.8` line, and docs.rs failed to build
`tesseract-plumbing` 0.11.1. This family has the best name recognition of the
three, which is exactly why it is worth writing down that it is the wrong
choice.

**`tesseract-rs` 0.4.0: the best API, the worst build.** It is the only
actively maintained FFI binding (a commit twelve days before this survey) and
has by far the richest surface — a real `ResultIterator`, `PageIterator` and
`ChoiceIterator`, plus TSV/hOCR/ALTO/box outputs (§7.2). It needs no bindgen
and therefore no LLVM. But it **compiles Tesseract's C++ from source via CMake
+ NMake**, taking minutes, and — the disqualifying detail — **its build script
downloads third-party sources and traineddata over the network using
`reqwest`**. `cargo build` fails offline.

For a project whose CI carries a fail-closed denylist naming `reqwest` by name
(§2.3), a build-time network fetch is at minimum a new decision record, and
arguably worse than the thing the denylist forbids: it makes pdfcer's build
non-hermetic and non-reproducible, which cuts against the same reasoning
`rust-toolchain.toml` pins an exact patch version for. Its `embed-tessdata`
feature (traineddata compiled into the binary, languages selected by
`TESSERACT_EMBED_LANGUAGES` at build time) is genuinely attractive for
single-file deployment and does not rescue the rest.

**`rusty-tesseract`: the cheapest build, the shallowest integration.** It
spawns `tesseract.exe`. Build-time requirements on Windows are **none beyond
stock Rust** — no MSVC C++, no vcpkg, no bindgen, no LLVM — which fits pdfcer's
"anyone can clone this and build it" posture better than anything else in this
survey. It sets `CREATE_NO_WINDOW` (0x08000000) on Windows so no console
flashes, a real detail for a GUI app. `image_to_data()` yields per-word
confidence and `image_to_boxes()` yields boxes, both parsed from the
subprocess's stdout.

Its two problems are both fixable and both need naming. It resolves the
executable **through `PATH` only** (`Command::new("tesseract.exe")`), which is
wrong for a portable folder — but `Command::new` accepts an absolute path, so
this is a small vendored change to one function, not a redesign. And it has
had **no commit since 2024-03-25**.

The subprocess model also carries an architectural cost worth weighing
honestly rather than discovering: it puts a process spawn, a temp file and a
text-parsing step in the middle of the OCR path. That is slower, harder to
cancel mid-page, and it converts "the engine returned a malformed box" from a
type error into a parse error — in a codebase whose whole discipline is to
make wrong states unrepresentable.

### 4.7 WASM: a hard wall, and the first real breach of the web-fork invariant

**Tesseract cannot be reached from a `wasm32-unknown-unknown` Rust build by
any of the three routes**, and all three fail for the same underlying reason.
The rustc platform-support documentation states it plainly:

> "This target currently has no equivalent in C/C++. There is no C/C++
> toolchain for this target."

So: linking a prebuilt `libtesseract` fails (no such artifact can exist);
compiling the C++ with `cc`/`cmake` fails (no compiler emits
`wasm32-unknown-unknown` objects); and spawning `tesseract.exe` fails
(`std::process::Command` is unimplemented on wasm).

**`tesseract.js` is not a counterexample; it is a separate toolchain.**
`tesseract.js-core` (Apache-2.0) compiles a **forked** Tesseract to WASM with
**Emscripten**, and the fork is substantial — a modified `CMakeLists.txt`, an
`src/arch_sse/` directory that hard-codes SSE because *"Tesseract feature
detection does not work in WebAssembly"*, `EM_ASM_ARGS` progress hooks patched
into `src/ccmain/control.cpp`, a rewritten `tprintf`. None of that is
reachable from `cargo build --target wasm32-unknown-unknown`, and pdfcer would
be adopting a fork of a C++ project as a JavaScript dependency.

**This is the consequential finding of §4, and it is bigger than OCR.**
`ARCHITECTURE.md` §1/§3 hold that the web fork should be a shell-crate swap,
and the wasm32 CI job (§2.2) exists to keep that true. **Choosing Tesseract
makes OCR the first pdfcer feature that cannot cross that boundary at all.**
The available responses are: call `tesseract.js` from JavaScript on the web
fork and treat the Rust side as an interface boundary; retarget the whole
application at `wasm32-unknown-emscripten` or `wasm32-wasip1`; or run a
*different* engine on the web and accept that the two products disagree about
what a page says. Each is a real decision, and each is larger than the Pass
that would trigger it. If Tesseract is chosen, that decision should be taken
and recorded **before** the Pass, not discovered during it.

`ocrs`, by contrast, simply compiles (§3.5).

### 4.8 Accuracy: better sourced than `ocrs`, weaker than the folklore

The best verifiable Tesseract-5-specific figure found is from *PreP-OCR*
(arXiv 2505.20429), which uses **Tesseract 5.5.0** as its baseline on degraded
historical scans:

| Dataset | Pages | Tesseract 5.5.0 CER, raw images |
|---|---|---|
| English | 9,606 | **5.91%** |
| French | 1,821 | **5.16%** |
| Spanish | 2,404 | **7.12%** |

**The paper's more useful finding for pdfcer is the second-order one:** its
restoration pipeline cuts that CER by 63.9–70.3%. **Preprocessing dominates
engine choice.** Deskew, binarisation and denoise before the engine ever sees
pixels are worth more than the difference between any two engines in this
survey — which is an argument for putting effort into the raster path pdfcer
already owns in `pdfcer-render`, and for not over-weighting the engine
decision itself.

Two things must **not** be repeated in pdfcer's docs or user-facing copy:

- **The ubiquitous "98–99% accuracy on clean 300 DPI printed documents" is
  UNVERIFIED.** It recurs across 2026 blog posts and could not be traced to a
  primary measurement; the sources repeating it are commercial OCR vendors
  comparing against their own products. Under the global claim-bearing-copy
  rule this is exactly the kind of plausible number that must not be lifted.
- **Tesseract's own official accuracy documentation is stale.** The tessdoc
  "4.0 Accuracy and Performance" page compares 3.04-era engines on Hindi and
  contains no `tessdata`/`_best`/`_fast` breakdown at all; the UNLV testing
  page reports 1995 baselines for Tesseract 2.00/3.02/3.04 only, nothing for
  4.x or 5.x. The absence of a current official number is itself worth
  knowing, because it is why the blog figures fill the vacuum.

The one published *relative* ordering safe to state: `tessdata_best` is the
most accurate but can be roughly 4× slower than `tessdata`; `tessdata_fast` is
the fastest and least accurate.

**Net:** Tesseract is very probably more accurate than `ocrs` today — a mature
LSTM engine with two more years of model updates, against a preview-stage
model last retrained in early 2024 whose own author says to expect more errors
than commercial engines. But **"very probably" is the honest strength of that
claim**, because the two have never been measured against each other and
neither has a benchmark pdfcer could cite. §8.1 turns that into the first task
rather than an assumption.

### 4.9 Feature-gating a native library: possible, awkward

`crates/pdfcer-core/Cargo.toml`'s convention (§2.5) assumes its rule 3 — *"the
dependency is `optional = true` and named by the feature, so disabling it
actually removes the code."* For a pure-Rust crate that is one line. For
Tesseract it depends on the binding:

- **Subprocess (`rusty-tesseract`)** gates cleanly, and is in fact the easiest
  thing in this survey to strip: the Rust dependency is small and pure, and
  "OCR off" means not copying a DLL folder. It also degrades at *runtime*
  rather than at compile time, which is a genuinely different and sometimes
  more useful property — §8.3 uses it.
- **FFI (`tesseract-rs`, `tesseract-sys`)** gates fine at the *Cargo* level,
  but the build-script conditionals do not disappear: a build with OCR off
  must not invoke CMake, must not require an MSVC C++ toolchain, and must not
  attempt a network fetch. That is achievable, but it means the answer to
  *"can someone build pdfcer without a C++ toolchain?"* becomes **"yes, if they
  remember `--no-default-features`"**, and CI must then build both
  configurations to keep it true. With `ocrs` the question does not arise.

There is also a licensing consequence that only bites the native route.
`THIRD_PARTY_LICENSES.md` is generated from the Cargo graph, so switching the
feature off correctly drops the *binding's* attribution — but Tesseract's,
Leptonica's and the traineddata's notices were never in that file to begin
with, because none of them is a Cargo dependency. They must be carried as
hand-authored notices beside the DLLs, and removed by hand when OCR is
stripped. That is the same hand-attribution problem as `ocrs`'s models (§3.3),
arriving from a different direction, and §8.4 solves it once for both.

---
## 5. The other realistic candidates

The brief asked for `ocrs` and Tesseract and invited additions. Four more are
worth recording — one because it is a genuinely serious third option that
neither the brief nor `ROADMAP.md` anticipated, one because it looks free and
is not, one because it is the natural hedge, and one because it is a licence
trap that a later reader would otherwise walk into.

### 5.1 Windows-native OCR (`Windows.Media.Ocr`) — free, and disqualified three times over

Windows 10/11 ship an OCR engine. The models live in `C:\Windows\OCR`, there
is nothing to bundle, nothing to license, and nothing to download. Reachable
from Rust through Microsoft's own **`windows` crate** (**MIT OR Apache-2.0**,
v0.62.2, 2025-10-06), which fully generates `OcrEngine`, `OcrResult`,
`OcrLine` and `OcrWord`. A working unpackaged reference implementation exists
(`winocr`, MIT). On paper this is the perfect fit for a portable app: **0 MB**.

It fails on three independent grounds, and each alone is enough.

**It exposes no confidence at all.** `OcrWord` has exactly two properties —
`BoundingRect` and `Text`. There is no confidence value at word, line, or
result level anywhere in the API. `OcrLine` does not even carry its own
bounding box (you union the word rects yourself). §7.2 explains why that is
fatal rather than inconvenient.

**Microsoft documents it as requiring package identity.** The *"WinRT APIs not
supported in desktop apps"* reference (updated 2026-07-22) lists all four OCR
types plus `SoftwareBitmap`/`BitmapDecoder` under *"APIs that require package
identity… supported only in desktop apps that are packaged"*, and a Microsoft
engineer confirmed it directly on Q&A: *"OCR API indeed requires package
identity… it's better to package your win32 application into msix mode."*
There is counter-evidence — `winocr` is a shipping unpackaged `.exe` that
works — so this is a **documented-support risk rather than a known failure**.
That is arguably worse for pdfcer than a hard failure would be: it means the
capability works until a Windows update decides it should not, in a product
whose entire distribution premise (§2.4) is *no installer, no MSIX, no
package identity*.

**And there is no guaranteed recogniser** — which undercuts the one thing that
made this candidate attractive. Microsoft's note on
`OcrEngine.AvailableRecognizerLanguages` is explicit: *"A language pack must be
installed on the device to be used. A user can install new OCR language packs
through Windows Settings."* Recogniser availability is a property of the
**user's machine configuration**, not of the OS. On a stock en-US machine
English OCR is present in practice — but only as a consequence of en-US being
the system language, not as a contract, and on a machine whose system language
is something else `TryCreateFromLanguage` returns **null rather than an
error**. `AvailableRecognizerLanguages` must be probed at runtime and the UI
must degrade honestly. For a portable app handed to an arbitrary Windows
machine that is a real failure mode, and it is one pdfcer **cannot compensate
for**, because having no bundled models is the entire reason for choosing this
path in the first place.

One further geometric constraint, recorded so nobody re-derives it:
`OcrResult.TextAngle` means the word rectangles are only valid in image
coordinates when the angle is zero, which the sandwich geometry (§7.1.4) would
have to compensate for.

**Where this candidate does earn a place: as an opportunistic fast path, never
as the engine.** Use it when a suitable recogniser happens to be installed and
fall back to the bundled engine otherwise. That captures the genuine
zero-install, zero-model benefit where it exists without betting the feature
on a machine configuration pdfcer does not control. It does **not** change the
primary decision, because the fallback has to be able to do the whole job
anyway — and it would mean two engines producing different text for the same
page, which is its own disclosure problem under rule 4. Recorded as a possible
later optimisation, explicitly out of scope for the first Pass.

**A near relative, also rejected.** `oneocr-rs` (MIT) binds *OneOCR*, the
newer engine behind the Windows 11 Snipping Tool, and it **does** provide word
confidence, image angle, and printed-vs-handwritten classification —
everything `Windows.Media.Ocr` lacks. It is disqualified for a different and
simpler reason: it requires copying `oneocr.dll`, `oneocr.onemodel` and
`onnxruntime.dll` out of `C:\Program Files\WindowsApps`. That is
redistributing proprietary Microsoft binaries, which `LEGAL.md` §6 does not
permit and no amount of engineering convenience makes acceptable.

**Apple Vision, for completeness.** On macOS, `objc2-vision` (**Zlib OR
Apache-2.0 OR MIT**) reaches a framework that is better than the Windows one
on every axis that matters here: per-observation bounding boxes, **confidence
scores**, and ranked alternative candidates via `topCandidates()`. Recorded
only to make the point that the platform-OCR *idea* is not unsound — it is the
Windows implementation specifically that cannot meet rule 4's bar, and macOS
is not a supported pdfcer platform anyway (**R9**).

### 5.2 `ocr-rs` (rust-paddle-ocr) — the serious third option

This one was not on the brief and should have been. It is a Rust binding to
**PaddleOCR** models running on **MNN**, Alibaba's inference engine.

| Dimension | Finding |
|---|---|
| Licence (code) | **Apache-2.0**, LICENSE file verified |
| Licence (runtime) | MNN (Alibaba) — **Apache-2.0**, verified at the repository |
| Licence (**models**) | PaddleOCR — repository is **Apache-2.0**, verified. **The released weights are PROBABLE, NOT CONFIRMED — see the caveat below.** |
| Purity | Not pure Rust — MNN is C++ — **but statically linked** |
| Single-folder | **Zero DLLs on Windows MSVC.** Its `build.rs` emits `cargo:rustc-link-lib=static:+whole-archive=MNN` and *deliberately deletes* `.dll`/`.so`/`.dylib` from the prebuilt library directory to force static linking |
| Models | shipped in-repo: **PP-OCRv6 tiny 3.2 MB**, PP-OCRv5 mobile fp16 10.8 MB, PP-OCRv6 small 15.6 MB |
| Languages | **50+** |
| WASM | **No** — MNN is C++ (§4.7's wall, same reason) |
| Maintenance | v2.4.1, 2026-08-05; 304 stars; recent commits are Windows-linking fixes |

**Why it matters: it dissolves the two problems that make this decision hard.**
`ocrs`'s blocking issues are its CC-BY-SA-4.0 weights (§3.3) and Latin-only
recognition (§3.8). `ocr-rs` appears to have **permissive weights and 50+
languages**, at **3.2 MB** for the smallest model pair — a quarter of `ocrs`'s
footprint. And unlike Tesseract it needs no DLL folder, no from-source CMake
build to be licence-clean, and no `PATH` or subprocess.

**⚠ THE WEIGHTS LICENCE IS PROBABLE, NOT CONFIRMED, AND THIS SURVEY'S OWN
EVIDENCE SAYS WHY THAT DISTINCTION BITES.** What was verified is that the
**`PaddlePaddle/PaddleOCR` repository** is Apache-2.0. **A repository licence
is not proof that the released model weights carry the same terms** — and the
cautionary case is in this very document: **`surya` ships Apache-2.0 code
alongside revenue-capped Open RAIL-M weights** (§5.4). No separate restrictive
weights licence was found for PaddleOCR, but **no per-model licence statement
was located on the ModelScope or Hugging Face artifacts either.** Before this
candidate is adopted, somebody must **read the model card for the specific
detection/recognition pair being bundled**, exactly as §3.3's finding for
`ocrs` was established. The bundled size of a det+rec pair is likewise
**UNVERIFIED** beyond the repository's own stated figures.

That check is cheap and it is the difference between this being a clean
fallback and being a second instance of the same trap. It is listed in §10.

**What it costs: the WASM story, and a prebuilt binary blob.** MNN is C++, so
`cargo check --target wasm32-unknown-unknown` cannot succeed — this candidate
hits exactly the wall §4.7 describes, and the same web-fork consequence
follows. It also links a **prebuilt** MNN static library rather than building
from source, which means pdfcer would be shipping a binary artifact it did not
compile and cannot easily audit — a different flavour of the supply-chain
question `LEGAL.md` §6.2 step 5 raises, and one that deserves its own look
(what exactly is in that `.lib`, who built it, is it reproducible) before
adoption.

**Recorded as a live alternative, not a recommendation**, because the licence
and language advantages are real and because the decision below turns on
exactly the axes where it is strong. If the operator's answer to §9's question
is *"no CC-BY-SA in the shipped folder"*, this is the candidate that answer
points at — **not** Tesseract.

### 5.3 `tract` (Sonos) — the hedge, not an engine

`tract` (**MIT OR Apache-2.0**, v0.23.4, 2026-07-08, in Sonos production) is a
pure-Rust inference runtime, WASM-documented, with no `-sys` dependency. It is
**not an OCR engine** — it is the layer `rten` is an alternative to. It is
worth naming for two reasons: it can load PaddleOCR ONNX models directly,
which means the *permissive* PaddleOCR weights could in principle be run on a
*pure-Rust, WASM-clean* runtime — the combination that would satisfy every
constraint in §2 at once; and it is a second home for pdfcer's OCR should
`rten`'s bus factor of 1 (§3.9) ever become a live problem.

**Nobody has built that combination.** There is no maintained crate wiring
PaddleOCR detection + recognition onto `tract` with the pre/post-processing
those models need. `sceptre` (MIT, v0.6.0, 2026-08-08) is the closest — it
abstracts over `ort`/`tract`/`candle` — and its repository is **twelve days
old with 10 stars**, which is not a dependency. Building the wiring in-house
is a real project, not a Pass. Recorded as the shape of the ideal answer and
as an escape hatch, not as something available today.

### 5.4 Rejected outright, with reasons

**`ort` (ONNX Runtime bindings) and the `oar-ocr` family built on it** — the
licences are fine throughout: `ort` is MIT OR Apache-2.0, ONNX Runtime itself
is MIT, and **`oar-ocr`** (the most credible OCR crate in this tier —
Apache-2.0, v0.9.1 2026-08-07, PP-OCRv4/v5/v6 models, **15 scripts** including
CJK, Arabic, Cyrillic and Devanagari, actively pushed) is Apache-2.0 with
Apache-2.0 PaddleOCR weights. `oar-ocr` even solves the provisioning problem
properly: it downloads from ModelScope by default, but its builders **accept
raw ONNX bytes such as `include_bytes!`**, so the models can be embedded and
nothing phones home — which is exactly what §2.3 requires.

**They fail on the DLL, and the failure is easy to miss because it is a
default rather than a documented requirement.** Reading `ort`'s own
`Cargo.toml`: `default = ["std", "ndarray", "tracing", "download-binaries",
"tls-native", "copy-dylibs", "api-27"]`. **`download-binaries` and
`copy-dylibs` are both on by default**, so a default build fetches a prebuilt
ONNX Runtime *at build time* and copies the dynamic library next to the
binary. `onnxruntime.dll` ships beside `pdfcer.exe`, tens of megabytes of it,
and the build is no longer hermetic. On Windows specifically there is **no
DLL-free x86_64 row** in pyke's distribution table, so the resolver picks the
DirectML build and `DirectML.dll` comes too. A genuinely zero-DLL build means
compiling ONNX Runtime from C++ source (over an hour). `ort` is also still at
`2.0.0-rc.13` with no 2.0 final after years, and its prebuilts require
**x86-64-v3** (Haswell, 2013+) — a silent minimum-CPU requirement pdfcer has
not otherwise taken on.

Recorded rather than dismissed, because `oar-ocr`'s 15 scripts and permissive
weights are genuinely attractive, and because if the DLL ever becomes
acceptable this is the candidate that argument favours. It is the same
trade-off `ocr-rs` (§5.2) resolves in pdfcer's favour by static-linking MNN
instead.

**`surya`** — and this is the trap worth writing down, because the licences
split in the direction that catches people. **The code is Apache-2.0**, not
GPL as is sometimes assumed. **The weights are a modified AI-Pubs Open RAIL-M
licence, "free for research, personal use, and startups under $5M
funding/revenue."** Open RAIL-M carries field-of-use restrictions and a
revenue trigger, so those weights **cannot be bundled in an MIT application**:
pdfcer would be redistributing restricted artifacts inside a package whose
licence promises recipients unrestricted use. The Rust port `surya-rs` is
abandoned (last release 2024-02-01) in any case. **docTR** is Apache-2.0
throughout but has no Rust port.

**`candle` + TrOCR** — `candle` is MIT OR Apache-2.0 and WASM-capable, and a
TrOCR example exists (no Donut example). It fails on size and on licence
provenance: `trocr-base-printed` is **1.33 GB** and `trocr-large-printed`
**2.43 GB**, against a 12 MB budget; and **the TrOCR Hugging Face cards carry
no `license` field at all**. The upstream `microsoft/unilm` repository is MIT,
but the weights have no explicit grant — under §6.2 that is unusable
regardless of capability, the same disposition `PRIOR_ART.md` already gave the
`font` (pdf-rs) crate.

**Crates that assert a licence with no LICENSE file in the repository** —
`rapidocr-core`, `pure-onnx-ocr`, `pure-onnx-ocr-sync`, `ffai-carmenta`,
`ddddocr`. A `Cargo.toml` string is not a grant, and `cargo-about` would
happily emit attribution that cannot be substantiated. Same rule as
`PRIOR_ART.md` applied to the `font` crate; recorded as a class so the next
survey does not re-evaluate them one at a time. (`ffai-carmenta` additionally
contradicts its own "pure Rust" claim — `tokenizers`/`onig` pulls in the
Oniguruma C regex engine.) `uni-ocr` had a single release in June 2025 and its
repository now 404s. `wonnx` was archived 2024-07-21; `luminal` has been stuck
at 0.2.0 since 2024-03-01.

---
## 6. Cloud / API OCR — rejected explicitly, for two independent reasons

Named here only so the rejection is on the record rather than implied by
omission. Azure AI Vision (Read), Google Cloud Vision, AWS Textract, Mistral
OCR and the VLM-based engines that dominate the 2026 accuracy round-ups are
all substantially more accurate than anything in this survey. **None of them
is available to pdfcer.**

**Reason one — the privacy posture, which is a promise, not a deployment
detail.** `ARCHITECTURE.md` §1.1: *"pdfcer makes no network calls of any kind
by default… Every document a user opens is processed entirely locally,
in-process, with no data ever leaving the machine unless the user explicitly
initiates it themselves."* An OCR feature that uploads the page is the exact
inverse of that sentence, and it is the inverse in the worst possible place:
the documents people OCR are scans — contracts, medical records, invoices,
drawings — and the whole page image goes over the wire, not a hash or a
telemetry counter. §1.1 says the posture is *"a load-bearing part of the
project's value proposition ('not a web app' is a promise about data handling,
not just deployment topology)"* and is *"treated with the same weight as the
GUI-core-separation and round-trip invariants."*

**Reason two — it is mechanically impossible without a new decision record,
and that is deliberate.** The `no-network` CI job (decision 003 **R12**) is a
fail-closed denylist over `pdfcer-core`, `pdfcer-render`, `pdfcer` and
`pdfce-gui`. Every HTTP client and TLS stack anyone would reach for —
`reqwest`, `ureq`, `hyper`, `curl`, `rustls`, `native-tls`, `openssl` — is on
it by name, and so is `tokio`. There is no way to write a cloud-OCR client
that compiles. The job's failure message is explicit: *"Adding one requires a
NEW decision record."*

**What §1.1 does leave open, stated precisely so nobody over-reads this
rejection.** §1.1 does contemplate a future network feature that is *"off by
default and explicitly opted into, disclosed plainly in the UI and in
`README.md`, never silently enabled"* — the example given is an update
checker. A cloud-OCR *provider plugin*, opt-in, disclosed, with the document
never leaving the machine unless the operator picks that provider for that
document, is therefore not forbidden by the letter of the posture. It is
nevertheless **out of scope for this survey and should stay out of scope for
the OCR Pass**, because it would require reversing R12 for the whole workspace
to serve an optional accuracy upgrade, and because the thing R12 protects is
the ability to say the sentence in §1.1 without qualification. If it is ever
wanted, it is its own decision record with its own operator sign-off — never a
line item inside an OCR Pass.

**One adjacent pattern that is NOT rejected, because it is a different shape.**
`LEGAL.md` §6.5 already establishes that pdfcer may *run a separate process* it
neither ships nor links (veraPDF). By the same logic, a future *"OCR with an
external tool the operator installed themselves"* escape hatch — pdfcer shells
out to a `tesseract.exe` already on the machine, consumes hOCR, ships nothing —
raises no licensing question and no network question at all. It is a
legitimate power-user feature and it is **not a substitute for a bundled
engine**, because it fails the single-folder promise for everyone who has not
installed anything. Noted as an option for §8, not as a candidate.

---

## 7. Design questions

### 7.1 (A) The sandwich: confirmed, and what it means in PDF operators

**Confirmed. pdfcer should produce OCRmyPDF-style "sandwich" output**: the
original page content is left completely untouched, and an invisible,
position-aligned text layer is appended over it. The page looks
byte-for-byte identical when rendered, and becomes searchable, selectable and
copyable.

This is not merely the popular choice — it is the only approach compatible
with pdfcer's own round-trip invariant. `ARCHITECTURE.md` §5 / project rule 3:
objects pdfcer did not logically touch are re-emitted byte-identical, or simply
omitted under incremental save. An OCR mode that *replaced* the scanned image
with reconstructed text and vector art would rewrite the page's entire visual
content on the strength of an inference — the exact thing rule 3 exists to
prevent, and (per §7.3) a separate, later capability rather than the default.

It is also the mode the spec corpus already anticipates. `PDF_Spec`'s
`iso32000__s__9.3.md`, in its own "Why it matters for pdfcer" section, states:

> **"Text rendering mode 3 is how OCR text layers are made invisible** — this
> is the 'sandwich' approach `PRIOR_ART.md` cites for OCRmyPDF. A Pass 1
> renderer MUST honour mode 3 or scanned-page OCR layers will render as
> visible garbage over the image."

#### 7.1.1 The rendering mode

ISO 32000-1 **§9.3.6, Table 106** (`iso32000__s__9.3.md`):

| Mode | Meaning |
|---|---|
| 0 | Fill text (the default, `Tmode = 0`) |
| 1 | Stroke text |
| 2 | Fill, then stroke |
| **3** | **Neither fill nor stroke text (invisible)** |
| 4–7 | Clipping variants |

**Mode 3 is the one.** Set with the integer operand `3 Tr` (§9.3, the `render`
operator). Not mode 7 — mode 7 paints nothing but *accumulates a clipping
path* which is applied at `ET` and intersects the current clip, so an OCR layer
emitted with `7 Tr` would silently clip everything drawn afterwards on that
page. That is a real trap: both modes "paint nothing", and only one of them is
inert.

Two spec details that matter for correctness:

- **Text state is not reset by `BT`.** `iso32000__s__9.3.md`: text state
  persists across text objects within a content stream and is reset only per
  page. So the OCR layer must emit `3 Tr` explicitly inside its own run — and
  must not leak it. Wrapping in `q … Q` handles this (below), because `Tr` is
  part of the graphics state that `Q` restores.
- **§9.3.6: "Only a value of 3 for text rendering mode shall have any effect
  on text displayed in a Type 3 font."** Irrelevant to authoring (pdfcer writes
  a Type 1 Standard-14 face) but relevant to pdfcer's *renderer*, which must
  honour mode 3 for OCR layers other tools produced.

#### 7.1.2 What it does to the existing content stream: nothing

The sandwich is an **append**, and pdfcer already has the machinery, built for
Pass 16.0 / FF-D (add-text). `PDF_Spec`'s
`iso32000__ref__page_content_append.md` is the recipe, and its guarantee is
exactly what is wanted here:

| # | Object | Action | Original bytes? |
|---|---|---|---|
| 1 | page dict | `/Contents` single→array, or append to the existing array | re-emitted (incremental update) |
| 2 | NEW content stream | created — holds `q BT … ET Q` | new |
| 3 | page `/Resources` | `/Font` subdict gains one entry | re-emitted or new |
| 4 | `/Font` subdict | one new name→fontdict entry | re-emitted or new |
| 5 | NEW font dict | Standard-14, no embedding | new |

**The original content stream object is never in that list.** Only the page
dict's `/Contents` *reference* changes; the stream it points to is untouched,
so the scanned image is byte-identical (**R32/R46**). The default incremental
save (**R34/R36**) writes the modified page dict plus the new objects in a new
update section. One undoable command.

Four constraints from that recipe carry over verbatim, and each is a real bug
if skipped:

1. **`q … Q` is normative, not hygiene.** §8.4.2: `q`/`Q` *"shall be balanced
   within a given content stream (or within the sequence of streams specified
   in a page dictionary's `Contents` array)."* The original stream is already
   balanced, so the appended one must be self-balanced too.
2. **`q` does not reset to defaults.** The appended stream inherits whatever
   state the previous streams left. `Tf` has **no default at all** (§9.3.1) and
   must be emitted; so must the rendering mode; the inherited fill colour is
   *not* guaranteed black (irrelevant under `3 Tr`, but do not rely on that
   accidentally).
3. **Append at the END of the `/Contents` array**, so the text layer is last
   in the painters' order. Under mode 3 nothing is painted, so z-order does not
   affect appearance — but it does affect the order a text-extractor walks the
   content, and putting the OCR layer last keeps "the image, then what we think
   it says" as the reading order.
4. **The `/Resources` inheritance trap.** If the page has no `/Resources` of
   its own it inherits from an ancestor `/Pages` node **shared with its
   siblings**. Mutating that would add the OCR font to unrelated pages and
   break their minimal diff. Give the page its own `/Resources` that
   *references the same indirect subdictionary objects*, with a freshly merged
   `/Font`.

#### 7.1.3 The font

A bundled **Standard-14 face with no embedding** — `/Type /Font /Subtype
/Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding`, per §9.6.2.2 and the
append recipe's §4. No glyph is ever painted, so the face is a pure
metrics-and-encoding carrier; what it must get right is that text extraction
and copy-paste recover the correct Unicode.

`iso32000__ref__page_content_append.md` §4 recommends emitting the **full form**
(`/FirstChar` + `/LastChar` + `/Widths` from `fontdata::std14_*`) rather than
the 3-key minimal form, because PDF 1.5 deprecates the Standard-14 special
treatment (*"conforming writers should represent all fonts using a complete
font descriptor"*) and self-contained metrics lay out correctly in a reader
without built-in AFMs. That reasoning applies unchanged here, and slightly
more strongly: §7.1.4's positioning maths *depends* on the widths, so a reader
disagreeing about them is a visible defect in selection geometry.

**A correction to the received wisdom about how OCRmyPDF does this, because it
changed recently and the old answer is still what everyone repeats.** The
familiar description — *"OCRmyPDF embeds a glyphless font whose every glyph is
blank"* — was true of the implementation through v16 and is **no longer how
v17 works**. OCRmyPDF **17.0.0 (2026-01-30) replaced the renderer wholesale**:
`hocrtransform/_hocr.py` no longer exists, `hocrtransform/` is parsing-only,
and the content-stream generator now lives in
`src/ocrmypdf/fpdf_renderer/renderer.py`. That renderer runs a **four-phase,
per-word font selection** — language-preferred Noto, then a fallback chain,
then a glyph-coverage scan of every installed system font — and only reaches a
glyphless face as a last resort. **The invisible layer normally uses real Noto
faces with real glyph outlines, drawn at `3 Tr`.** The old `pdf.ttf` was
deleted on 2026-01-06; the modern last-resort face is **`Occulta.ttf`**
(Latin for *hidden*), 12,512 bytes, **OCRmyPDF's own work, Apache-2.0,
© 2026 James R. Barlow** — not a rename of Tesseract's font. It carries six
contourless glyphs, a `cmap` **format 13** (≈12 KB rather than ≈780 KB with
format 12), and — the detail that earns its existence — **three advance widths**
(0 for combining marks, 500 normal, 1000 for East-Asian wide/fullwidth)
against Tesseract's single 500. That is what makes CJK and combining marks
select correctly.

**pdfcer's Standard-14 route is therefore closer to modern OCRmyPDF than to the
folklore, and the divergence is narrower than it looks:** both use a real
font with real metrics and rely on explicit operators for placement. pdfcer's
version needs **no embedded font program at all** — smaller output, nothing to
subset, nothing to attribute — because it can lean on the reader's built-in
Standard-14 metrics and on `fontdata::std14_width` for its own arithmetic.
**Record this as a deliberate divergence with its reason**, so a later reader
does not "fix" pdfcer toward an embedded glyphless font without knowing that
the choice was made and why.

Two consequences of that choice to accept knowingly. Standard-14 + WinAnsi
cannot represent CJK at all, so if pdfcer ever gains a non-Latin engine the
sandwich emitter needs a second path (a `Type0`/`Identity-H` CID font with a
`ToUnicode` CMap — the shape `ocrs-cjk-cli` uses, and the shape pdfcer's own
Pass 21.0 composite-authoring work already built machinery for). And
`ZapfDingbats`/`Symbol` are irrelevant here; the OCR face is always one of the
twelve Latin ones, `/Helvetica` being the obvious pick since its widths are
the least eccentric.

**★ If that second path is ever needed, `Occulta.ttf` can simply be embedded.**
Per OCRmyPDF's `REUSE.toml`, `src/ocrmypdf/data/Occulta.ttf` is **Apache-2.0**
(© 2026 James R. Barlow) — *not* MPL-2.0 like the renderer beside it, and
therefore **directly embeddable in an MIT project with attribution**. It is
12,512 bytes, covers the full BMP (U+0000–U+FFFF) via a `cmap` format 13
many-to-one mapping, and carries the three-class advance widths (0 for
combining marks, 500 for Latin/Greek/Cyrillic/Arabic/Hebrew, 1000 for CJK and
fullwidth). That is a solved, tested, permissively licensed answer to a
problem pdfcer would otherwise be building a CID font to solve — the most
immediately actionable finding in this section, and worth knowing *now* even
though the first Pass does not need it.

One consequence to accept explicitly: WinAnsi covers Latin-1 plus the usual
CP1252 additions. Since the candidate engines are Latin-only anyway (§3.8),
the encoder's coverage is not currently the binding constraint — but a
character the engine emits and WinAnsi cannot encode must be **refused and
disclosed**, never silently dropped or transliterated, which is what
`InverseEncoding::encode_str`'s existing `Refusal` path already does.

#### 7.1.4 Positioning: one text run per recognised word

The naive approach — one `Tj` per line — is wrong, because the recognised
glyph widths have nothing to do with Helvetica's. Select a line of such text
and the highlight rectangle drifts progressively away from the pixels it is
supposed to be over.

The correct construction places **each recognised word independently**, and
scales it horizontally so its *advance* matches the width of the box the
engine reported. The grouping that follows is not arbitrary — see §7.1.5 for
why it is one text object per **line** rather than per word:

```
q
BT
  3 Tr                                  % invisible (§9.3.6) — once per text object
  /pdfceOCR <size> Tf                   % font + size (from the line box height)
  <x> <y> Td                            % first word's baseline origin
  <Th*100> Tz                           % this word's horizontal scale
  (codes) Tj                            % WinAnsi codes, not UTF-8
  <dx> <dy> Td                          % relative move to the next word
  <Th*100> Tz                           % next word's scale
  (codes) Tj
  …
ET
Q
```

**The scale.** ISO 32000-1 §9.4.4 gives the glyph advance as

```
tx = ( (w0 − Tj/1000) · Tfs + Tc + Tw ) · Th
```

so with `Tc = Tw = 0`, the natural width of the word at size `Tfs` is
`Σw0 · Tfs`, and the horizontal scaling needed to make it fill a box of width
`W` is

```
Th = W / ( Σw0 · Tfs )
```

where `Σw0` comes from `fontdata::std14_width` for each code. **`Tz` takes a
percentage, not a ratio** (`iso32000__s__9.3.md`: `100 Tz` → `Th = 1.0`) — the
spec RAG flags storing the raw operand as `Th` as a known 100× bug. Emit
`Th · 100`.

**This formula is not pdfcer's invention, and it is worth knowing that two
independent implementations converged on it.** OCRmyPDF v17's renderer
computes, per word, `word_tz = (word_width_pt / natural_width) * 100` where
`natural_width` is a real font measurement, not an estimate. Tesseract's own
built-in PDF renderer (`src/api/pdfrenderer.cpp`) computes
`h_stretch = kCharWidth * 100.0 * word_length / (fontsize * pdf_word_len)`,
which — since its glyphless font has `DW = 500` — reduces algebraically to the
same `100 × w_box / w_natural`. Agreement between two engines that share no
code is the strongest evidence available that this is simply the right
arithmetic.

**The vertical placement and the size.** `Tfs` comes from the **line** box
height, not the word's. OCRmyPDF's rule is worth copying because it is
battle-tested: `font_size = line_box_height + baseline_intercept`, falling
back to `line_box_height * 0.8` if that yields anything below 1.0 pt. The
translation places the word's **baseline origin** — not the top-left of the
box — and the baseline sits above the box bottom by the font's descender
fraction.

**Rotation.** For a rotated word, the six-operand `Tm` form carries the
rotation in `a b c d` rather than forcing the run upright. OCRmyPDF instead
emits **no `Tm` at all** — pure `Td`, with rotation carried by an outer
`q … cm … Q`, which is equivalent and slightly cheaper. Either is fine; the
point is that the mechanism exists, and it is where `ocrs`'s `rotated_rect()`
(§3.1) is a genuinely better primitive than an axis-aligned box. On a page
scanned a few degrees off-square, axis-aligned boxes overlap and their
selection rectangles are visibly wrong, and there is nothing left to
reconstruct the true angle from. `Windows.Media.Ocr` has the same problem in a
sharper form: its word rectangles are only valid in image coordinates when
`OcrResult.TextAngle` is zero (§5.1).

**Word, not character.** Per-word is the granularity that makes selection,
search-hit highlighting and copy-paste behave: it keeps a word's characters in
one show operation, so an extractor sees a word, while pinning each word to
its own measured box. Per-character would be more precise, and would produce
enormous content streams with no word boundaries for extraction to recover.
`ocrs` exposes per-character rects, so the option remains open if measurement
ever shows it is needed.

**Spaces.** OCRmyPDF appends the space to the *word's own string* rather than
emitting a synthetic space word — a change made in v17, where v16 had emitted
separate space words each with its own stretched `Tz`. That is the better
answer and pdfcer should copy it. What must **not** happen is emitting literal
spaces *and* driving position from the boxes, which double-counts and yields
doubled spaces on copy.

**One garbage filter worth stealing.** OCRmyPDF's
`_check_aspect_ratio_plausible` **suppresses an entire line** when the ratio of
its recognised text length to its box dimensions falls below 0.1 — which
catches the characteristic junk an OCR engine produces from an undetected
rotation. That is a cheap, high-value guard, and under rule 4 a *suppressed*
line must be **disclosed as suppressed** rather than silently dropped (§7.2).

#### 7.1.5 ★ One `BT…ET` per LINE, not per word — a real-world reader bug, not a preference

The obvious structure is one text object per word: each word gets its own
`BT … ET`, sets its own `Tr`, `Tf`, `Tz` and position, and cannot be affected
by anything else. It is clean, it is trivially correct against the spec, and
**it is the wrong choice**, for a reason no reading of ISO 32000-1 will
surface.

OCRmyPDF's renderer groups words into one text object per line, and says why
in a source comment:

> *"This avoids a poppler bug where `Tz` (horizontal scaling) is not carried
> across `BT`/`ET` boundaries, affecting all poppler-based tools and viewers
> (Evince, `pdftotext`, etc.)."*

Per §9.3, text state — including `Tz` — is **not** reset by `BT` and persists
across text objects within a content stream; the spec RAG states this
explicitly. Poppler disagrees. And poppler is not a fringe consumer: it is the
engine behind Evince, Okular, `pdftotext` and `pdftoppm` (`PRIOR_ART.md`), so
"selection is wrong in `pdftotext`" is a bug a real user will report.

**The safe construction satisfies both readings at once**, and costs nothing:
group per line, and **emit `Tz` before every word** rather than relying on it
persisting. Then a reader that carries `Tz` across `BT` and a reader that does
not both get the same answer, because the value is never inherited in the
first place. The same defensiveness applies to `3 Tr` — set it inside every
text object rather than once per content stream.

**This is precisely the class of finding `C:\personal_rag\pdf\` exists for** —
an empirical divergence between what the standard permits and what real
consumers do, discovered from another implementation's scar tissue rather than
from the spec. It should be written there when the Pass ships, whichever
engine is chosen, because it is a property of PDF readers and not of pdfcer.

#### 7.1.6 What pdfcer must build, versus what it already has

Already present and reusable, verified in the tree:

- `text_edit::encoding::InverseEncoding` — Unicode → WinAnsi codes, with an
  explicit `Refusal` for uncoverable characters.
- `fontdata::std14_width`, `std14_descriptor`, `std14_base_font_name` — the
  metrics `Σw0` needs, already bundled.
- The append plumbing behind `Document::add_text` /
  `CommandKind::AddText` — `/Contents` single→array, the `/Resources`
  inheritance-safe merge, incremental save, undo.
- `pdfcer-render::render_page*` — page → raster, the OCR input path.
- `image_codec::*` — DCT/CCITT/JBIG2/JPX decode, for reading a scanned page's
  image XObject directly instead of re-rendering it.

**Not present, and this is the real work:** `AddTextRequest` has no
`render_mode` field, no `Tz` field, and no per-word placement — its emitter
writes one `Tm` + `Tj` per *line* at rendering mode 0. So the sandwich is a
**sibling emitter** that reuses the encoder, the metrics and the append
plumbing, not a flag on `add_text`. That is the right shape anyway: the two
have different inputs (an operator's string versus a list of measured boxes)
and different failure modes.

---
### 7.2 (B) Rule 4 applied to OCR: what the operator must see and be able to reject

Project rule 4 was **narrowed on 2026-08-05** (decision 024 §4.4), and the
narrowing matters here, because a careless reading of the old wording would
produce exactly the interface the operator complained about. The current text:

> Anything pdfcer **inferred** — a value, a boundary, a classification, a
> correction the operator did not directly specify (OCR text, auto-detected
> form fields, recognised text blocks, …) — is **visible before it becomes
> document state**, and the operator can reject it without undoing anything
> else.
>
> This is a requirement on **disclosure**, not on any particular widget. It is
> satisfied by the inferred value being on screen and the commit being a
> deliberate act… It is **not** satisfied by a control whose position is
> derived from the document.
>
> Where an inference is *inherently* uncertain (a best-fit residual, a
> font-trust downgrade, a reflow that overflows), the uncertainty is **stated
> in the disclosure**, not merely implied by the presence of a confirm button.

OCR is named in that rule's own list. Three obligations follow, and they are
not equally easy to satisfy.

#### 7.2.1 What must be visible before commit

**The recognised text, in place, over the page.** Not a summary, not a count,
not "OCR complete — 1,204 words found." The failure mode of OCR is that it
produces *plausible* wrong text, and a count cannot expose that. The operator
has to be able to see `RECEIVED` where the page says `RECEIVED` and `RECElVED`
where the engine guessed wrong. This is the same reasoning `export/dxf.rs`
gives for making `DxfScaleSuggestion` a three-case enum rather than an
`Option<f64>`: *"the case that actually hurts is the third one, and it
collapses into `None` where nobody can see it."*

**The commit control must not be anchored to the document.** This is the
narrowing's specific complaint, in the operator's own words: *"there is a
separate accept / reject box somewhere on the screen to click — I've never
seen any other software operate that way."* An accept control positioned
relative to a *page* moves on every zoom, scroll and page change. The OCR
disclosure belongs in a panel or a status bar at a fixed, predictable
position — one control for the whole run, not one per word floating over the
canvas.

**Rejection must be granular and must not undo anything else.** At minimum:
reject the whole run. Better, and what the rule's *"without undoing anything
else"* actually asks for: reject a page, or a line, and keep the rest. Since
the sandwich is a single appended content stream per page (§7.1.2), per-page
rejection is nearly free — it is simply not emitting that page's stream.

**What must NOT be required**, per the narrowing: a confirm click on every
word. The operator correcting a misrecognised word is performing a direct
manipulation whose result is fully visible and reversible in one undo; the
rule explicitly does not demand a second click for that.

#### 7.2.2 What must be *stated*, because OCR is inherently uncertain

Rule 4's last clause is the one with teeth here. OCR is not a
*sometimes*-uncertain inference like a snapped point; it is uncertain by
construction, on every word. So the disclosure must **say so with numbers**,
not merely offer a button.

Concretely, the disclosure should carry:

- **Which engine and which model produced this**, by name and version. This is
  the same three-part promise `form_script/disclose.rs` centralises — *what
  computes the value, whether pdfcer ran it, whether the shown value may be
  stale* — and it is centralised there for a stated reason worth repeating:
  scattering it across CLI, GUI and report *"would let one of them drift, and
  the one that drifts is the one that stops saying 'may be stale'."*
- **Per-word confidence**, where the engine provides it — surfaced as
  low-confidence words shaded or outlined **at their true positions on the
  canvas**, which is the whole point of having boxes at all. An operator
  scanning a page for three highlighted words is doing something possible; an
  operator proof-reading 1,204 words against the image is not.
- **The suppressions.** Lines dropped by the aspect-ratio guard (§7.1.4), and
  words the WinAnsi encoder refused (§7.1.3), are pdfcer *deciding not to
  record something it recognised*. Silence there is the sneaky case.
- **The honest ceiling.** If the engine is Latin-only and the page contains
  non-Latin script, say so. A blank result for a Japanese page must not look
  like "there was no text."

#### 7.2.3 ★ Confidence: where the candidates diverge, and it cuts against `ocrs`

This is the one criterion on which the pure-Rust recommendation loses, and it
loses to the rule the project cares most about.

| Engine | Per-word confidence | Geometry | Notes |
|---|---|---|---|
| **Tesseract** | **✅ Yes** — `ResultIterator::Confidence(level)`, a float 0–100, at **every** hierarchy level (block / paragraph / line / word / symbol) | `BoundingBox(level, …)`, axis-aligned | Also via TSV (`conf` column) and hOCR (`x_wconf`). Three independent delivery paths |
| **`ocrs`** | **❌ None, anywhere** | `bounding_rect()` **and `rotated_rect()`**, per word *and per character* | Verified at source, not from docs |
| **`Windows.Media.Ocr`** | **❌ None** — `OcrWord` has exactly `BoundingRect` and `Text` | axis-aligned words only; `OcrLine` has no box at all | §5.1 |
| **Apple Vision** | ✅ Yes, plus ranked alternative candidates | per-observation boxes | not a pdfcer platform (**R9**) |
| **`oneocr-rs`** | ✅ Yes, plus angle and handwriting classification | — | disqualified: redistributes Microsoft binaries (§5.1) |

**`ocrs` exposes no confidence at any public API level.** This was checked at
source rather than inferred from documentation. `TextChar` is the complete
per-character record:

```rust
pub struct TextChar {
    pub char: char,   // Character that was recognized.
    pub rect: Rect,   // Approximate bounding rectangle of character in input image.
}
```

`TextWord` and `TextLine` reach their data through a `TextItem` trait offering
exactly `chars()`, `bounding_rect()` and `rotated_rect()`. The recognition
model's log-probabilities are consumed by the CTC/greedy decoder and
**discarded** before anything public sees them.

The only probability-adjacent public surface is detection-side —
`OcrEngine::detect_text_pixels()` returns a per-pixel text-probability map,
and `detection_threshold()` reports the cutoff. **That answers "is this region
text", not "is this character right"**, and only the second question is the
one rule 4 needs answered.

**Which Rust Tesseract bindings actually surface it**, since the C++ API
having a feature is not the same as it being reachable:

| Binding | Per-word confidence | Mechanism |
|---|---|---|
| `tesseract-rs` 0.4.0 | ✅ best coverage | real `ResultIterator`/`PageIterator`/`ChoiceIterator`, plus `get_tsv_text`, `get_hocr_text`, `get_alto_text` |
| `rusty-tesseract` | ✅ | `image_to_data()` → per-line `.conf`, parsed from subprocess stdout |
| `leptess` | ✅ but no iterator | `mean_text_conf()`, TSV/hOCR/ALTO — per-word only by parsing |
| `tesseract` 0.15.2 | ⚠ aggregate only | `mean_text_conf() -> i32`; per-word requires parsing TSV |

**The honest resolution, and it is not "so choose Tesseract."** Rule 4 requires
the uncertainty to be *stated*, and an engine that has no confidence value can
still satisfy it — by stating that it has none. *"This engine does not report
per-word confidence; every word is unverified"* is a true, useful, and
rule-4-compliant disclosure. It is worse than shading the doubtful 3% of
words, and it is not sneaky.

What would be sneaky is presenting `ocrs` output in an interface designed
around confidence shading, with nothing shaded, so the absence of highlights
reads as *"the engine is confident"* when it means *"the engine cannot say."*
That distinction should be designed in from the start, which is a
`pdfcer-ui-specialist` dispatch when the Pass is scoped, not an engineering
afterthought.

Three ways to close the gap properly, in increasing cost: **(a)** disclose the
absence, as above, and use the detection probability map to shade
*low-detection-confidence regions*, which is a weaker but real signal pdfcer
already has access to; **(b)** patch `ocrs`'s `recognition.rs` to plumb the
decoder's existing log-probabilities into `TextChar` and offer it upstream —
the values exist and are thrown away, so this is a small, well-motivated
contribution rather than a fork; **(c)** carry a fork. **(b) is the right
answer** and should be attempted early, because if it lands upstream the
single strongest argument against the recommendation disappears.

### 7.3 What this design deliberately does not do: "Editable Text and Images"

Acrobat offers two OCR output modes, and the distinction is recorded in the
parity RAG (`text_edit__ocr_prerequisite_for_scanned_text.md`): **Searchable
Image** keeps the page image and adds an invisible text layer for search and
copy; **Editable Text and Images** reconstructs the page as real text and
image objects the edit tool can manipulate. Acrobat Standard has the former
only; the latter is Pro-gated — the one clear, sourced tier distinction in
that entire bucket.

**§7.1 describes the Searchable Image mode, and that is the correct default
and the correct first Pass.** Reconstruction is a much larger capability: it
must decide which pixels were text and erase them from the image, choose a
substitute font per run, and re-lay-out the page — every step an inference, on
top of an inference, rewriting content the round-trip invariant (rule 3)
otherwise protects.

Two things worth carrying forward rather than losing. The parity RAG already
notes that pdfcer is **not a tiered product**, so shipping both modes without a
tier gate is *"a straightforward parity-plus position once the OCR engine
decision lands"* — a real differentiator, later. And Acrobat's own
font-matching algorithm for reconstructed text is **undocumented** (a recorded
GAP), which means there is no behaviour to match and pdfcer would be designing
that part from first principles anyway.

`ROADMAP.md` already carries the dependency in the other direction: **FF-G**,
OCR-gated scanned-text editing, edits the OCR layer and never the raster. That
is the right sequencing — searchable first, editable later, and "editable"
meaning *the text layer*, not the pixels.

### 7.4 (C) Is there a Rust crate that does the whole sandwich? Effectively, no

**Three crates genuinely emit `3 Tr` invisible text over a preserved page
raster. All three are 2026 arrivals, single-author, with ≤8 GitHub stars, and
around 2,000 downloads between them.** None is a dependency pdfcer could take
under rule 13 with a straight face.

| Crate | Licence | Version | Assessment |
|---|---|---|---|
| **`deepocr` / `deepocr-core`** | **MIT OR Apache-2.0** — *both* LICENSE files present, the cleanest of the three | 0.1.0, 2026-07-26 | `lopdf`-based, reproduces the glyphless-font trick (`add_glyphless_font`), emits `BT\n3 Tr\n`, test asserts `"3 Tr"`. OCR via `ocrs`. **0 stars, 17 downloads, and all 13 commits landed on the same day.** Architecturally right, operationally unproven |
| **`harumi`** | claims `MIT OR Apache-2.0` — **⚠ no LICENSE file anywhere in the repo**; GitHub's detector returns `null` | 1.19.0, 2026-06-26, 8★ | `lopdf` + `ttf-parser`, pure Rust, no C deps, WASM-ready. Documents *"render_mode 3 — invisible (selectable/searchable, no paint)"*, exposes `invisible_text_stream()`. **⚠ Emits no `Tz` at all** — grepped and confirmed — so it places text at a point with a font size but never fits it to the recognised box width. Materially less than the mechanism requires |
| **`ocrs-cjk-cli`** | MIT OR Apache-2.0, both files present | 0.1.0, 3★ | Independent `lopdf` implementation by `harumi`'s author. Builds a real `Type0`/`CIDFontType2` font with a `ToUnicode` CMap — the CJK-capable shape §7.1.3 notes pdfcer would eventually need |

**Two that look like candidates and are not.** `pdf-ocr` (PDFluent) is
**proprietary** — its crates.io licence field is literally `"non-standard"`,
its README requires a commercial licence for production plus an OEM add-on for
redistribution, and there is no public source. `ocrmypdf-rs` is a
`std::process::Command` shell-out wrapper **with no LICENSE file**.

**⚠ A flag for `PRIOR_ART.md`, and it belongs in the librarian's filing rather
than being lost here.** `oxidize-pdf`'s `PdfOcrConverter::convert_to_searchable_pdf()`
is **advertised but stubbed** at HEAD of v4.3.0: the invisible-text half is
real, but `add_image_to_page` is `// TODO: Implement image embedding` and
draws a grey placeholder rectangle, while `copy_page_content` writes the
literal string `"This page already contains text content"`. **The "searchable
PDF" it produces discards the scan.** This is a fourth entry in decision 001's
list of capability claims that did not survive inspection, in the same
project, found the same way. Do not count it as existing capability.

**So pdfcer composes engine + text-layer authoring itself.** That is not a
hardship — §7.1.6 shows most of the pieces already exist — and it is what the
project would do anyway under decision 001's from-scratch posture.

**And the reference implementations are unusually friendly.** The `3 Tr`
primitive is exposed by every Rust PDF writer (`lopdf` MIT; `pdf-writer` and
`oxidize-pdf` MIT/Apache-2.0; `printpdf` MIT, the last three with a typed
`TextRenderingMode::Invisible`), and — more useful — **there are permissively
licensed implementations pdfcer may legally read *and* copy from**:

- **Tesseract's `src/api/pdfrenderer.cpp` — Apache-2.0.** Readable and
  copyable with attribution. Per `PRIOR_ART.md`, Apache-2.0 sources are the
  lowest-risk class in this whole project alongside PDF.js.
- **OCRmyPDF's *legacy* v16 `hocrtransform/_hocr.py` — MIT-headered.**
  Directly copyable with attribution. This is the more interesting half of the
  licence split below.
- `deepocr`'s `searchable/font.rs` and `ocrs-cjk-cli`'s `pdf.rs` — MIT OR
  Apache-2.0.

**The OCRmyPDF licence split is per-file and matters more than the repository
default.** OCRmyPDF is MPL-2.0 overall (relicensed from GPL-3.0 on
2020-08-05), but the SPDX headers differ by file:

| File | SPDX |
|---|---|
| `fpdf_renderer/renderer.py` — **the current v17 generator** | **MPL-2.0** |
| `hocrtransform/__init__.py`, `hocr_parser.py` | **MIT** |
| v16.13.0 `hocrtransform/_hocr.py` — the **legacy** generator | **MIT** |
| `data/Occulta.ttf` | Apache-2.0 |

For an MIT project that resolves cleanly:

- **The algorithm is free to reimplement regardless.** `3 Tr`,
  `font_size = box_height + intercept`, `Tz = 100 · w_box / w_natural` are
  ideas and arithmetic, not expression. Independent Rust code is unencumbered,
  and this is the same read-vs-copy line `LEGAL.md` §6.1 already draws.
- **Copying `renderer.py` would be possible but it would not become MIT.**
  MPL-2.0 is file-level weak copyleft: the file stays MPL and its source must
  be disclosed. §3.3 of MPL-2.0 permits the Larger Work under other terms — so
  it is **not** the categorical bar GPL/AGPL is — but it is an operator call
  under rule 13 and it would put an MPL entry in `THIRD_PARTY_LICENSES.md`.
  Note also that **fpdf2 itself is LGPL-3.0**, a second reason the current
  renderer is reference-only.
- **The MIT-headered files are directly copyable with attribution**, including
  the entire legacy v16 renderer. If code lineage is ever wanted instead of a
  clean-room, take it from there and not from `renderer.py`.

**Recommendation: clean-room it.** The algorithm is a page of arithmetic;
transliteration buys nothing and costs an attribution obligation. Note that
§6.6's pdf.js precedent (R61's pattern) already covers *behavioural* reference
to an MPL-2.0 project, so reading OCRmyPDF for behaviour is settled practice
here — it is copying that raises the question.

---
## 8. Recommendation

### 8.0 The comparison, on the axes that actually decide it

| | `ocrs` + `rten` | Tesseract (subprocess) | `ocr-rs` (PaddleOCR/MNN) | `Windows.Media.Ocr` |
|---|---|---|---|---|
| Code licence | MIT OR Apache-2.0 | Apache-2.0 + BSD-2 + MIT | Apache-2.0 | MIT OR Apache-2.0 |
| **Model licence** | **⚠ CC-BY-SA-4.0** | Apache-2.0 | Apache-2.0 repo, **weights unconfirmed** | n/a (OS) |
| Shipped size | **12.24 MB** | ~28 MB (MSVC, projected) – ~54 MB (MinGW, measured) | **3.2–21 MB** | **0 MB** |
| Files added to folder | 2 models | 20 binaries + 2 traineddata | 2 models, **0 DLLs** | 0 |
| Build toolchain | **none — cargo only** | none (subprocess) / CMake+MSVC (FFI) | MSVC (static link) | none |
| **wasm32 CI gate (§2.2)** | **✅ PASSES — measured** | ❌ impossible | ❌ impossible | ❌ impossible |
| **Per-word confidence** | **❌ none** | **✅ yes** | ✅ yes | ❌ none |
| Word geometry | **rotated + per-char** | axis-aligned | axis-aligned | axis-aligned, angle-dependent |
| Languages | **Latin only** | **120 + 37 scripts** | **50+** | **not guaranteed at all** — user's installed language packs |
| Feature-gating | trivial | easy (subprocess) / awkward (FFI) | moderate | easy |
| Bus factor | 1 | large | small | Microsoft |
| Accuracy evidence | none published | CER 5.9% on degraded scans | none checked | none checked |

### 8.1 Recommendation: `ocrs` + `rten`, **conditional on a measurement and an operator answer**

**Adopt `ocrs` + `rten` as pdfcer's OCR engine**, behind a Cargo feature, with
models shipped as two files in the portable folder — **subject to the operator
resolving §9's licence question**, and **subject to a measurement that has to
happen first**.

The reasoning is not "pure Rust is nicer." It is that **`ocrs` is the only
candidate that does not force a structural concession**, and the concessions
the others force are permanent while `ocrs`'s weaknesses are contingent.

- **It is the only candidate that clears the wasm32 CI gate** (§2.2), measured
  rather than claimed (§3.5). Every other option — Tesseract by any of three
  routes, `ocr-rs`, the Windows API — makes OCR the first pdfcer feature that
  cannot cross into the web fork at all (§4.7). That is not an OCR decision;
  it is a decision about `ARCHITECTURE.md` §1/§3, taken sideways, inside a
  feature Pass.
- **It adds nothing to the build.** No C++ toolchain, no vcpkg, no CMake, no
  LLVM/libclang, no build-time network fetch, no prebuilt binary blob. `cargo
  build` continues to be the whole story for anyone cloning the repository —
  which is a property this project has and would be spending permanently.
- **It is the smallest real option that ships something** — 12.24 MB, two
  files, no DLL folder, no `PATH`, no subprocess, no MSIX.
- **It gates perfectly** against the convention written on 2026-08-12 (§3.10),
  which is the operator's own recent, explicit requirement.
- **Its dependency graph is permissive throughout** and passes the `no-network`
  denylist unmodified (§3.2).
- **Its weaknesses are fixable in a way the alternatives' are not.** The
  missing confidence is *discarded data*, recoverable by an upstream patch
  (§7.2.3(b)). Latin-only is a model problem, and models are swappable files.
  Whereas "C++ cannot target `wasm32-unknown-unknown`" is not going to change.

**Two conditions, both real:**

1. **§9's licence question is the operator's, and this recommendation is void
   without an answer.** If the answer is "no CC-BY-SA in the shipped folder,"
   the recommendation changes — and it changes to **`ocr-rs`** (§5.2), **not**
   to Tesseract: 50+ languages, 3.2 MB models, zero DLLs, Apache-2.0 code and
   an Apache-2.0 MNN runtime, at the cost of the WASM story and a prebuilt
   binary blob. **That fallback is itself conditional on one unmade check** —
   the PaddleOCR *repository* is Apache-2.0 but the *released weights* have not
   been confirmed, and `surya` (§5.4) is this document's own proof that the two
   can differ. Read the model card for the exact det/rec pair first. If it
   comes back restricted, the fallback becomes Tesseract-subprocess after all,
   with §9.1's two licence items to clear.
2. **Measure before building the UI.** Neither `ocrs` nor Tesseract has a
   benchmark pdfcer can cite (§3.7, §4.8), so pdfcer must generate its own
   number on its own corpus. That is the first slice, below.

### 8.2 Suggested Pass shape

**Slice 0 — the bake-off, before any pdfcer code is written.** Assemble a
rights-cleared fixture set (`LEGAL.md` §5) that reflects what pdfcer's users
actually scan, and run `ocrs`, Tesseract 5.5.3 and `ocr-rs` over it
out-of-tree, measuring character error rate. Two reasons this comes first: it
is the only way any accuracy claim in `README.md` can be made honestly under
the claim-bearing-copy rule, and the *PreP-OCR* finding (§4.8) says
preprocessing cuts CER by ~65%, so the bake-off should also measure
**deskew/binarise/denoise before the engine** — which may turn out to matter
more than the engine choice and is work in `pdfcer-render`, which pdfcer owns.
Out-of-tree, like `tools/difftest/`, so nothing is adopted by measuring it.

**Slice 1 — the sandwich emitter, engine-independent.** Build
`pdfcer-core::ocr::sandwich` against a *plain data* input type — a list of
`(text, rect_or_rotated_rect, confidence: Option<f32>)` per line — with no
`ocrs` type anywhere in its signature. This slice is testable with
hand-written fixtures, needs no engine at all, and is the part §7.1.6 shows is
mostly assembled already. It also means slice 0's outcome cannot invalidate
it.

**Slice 2 — the engine binding**, behind `feature = "ocr"`, wrapped in a
pdfcer `thiserror` type (§3.11), converting `RotatedRect` at the boundary so no
`ocrs` type is public. Plus the `pdfcer ocr` subcommand (project rule 11 —
same session, not later).

**Slice 3 — the GUI review flow**, after a `pdfcer-ui-specialist` dispatch that
is given §7.2 as its brief, and given the confidence-absence problem (§7.2.3)
explicitly rather than being left to discover it.

**On the `default = [...]` question raised in §2.5:** the convention says
default ON, and the reason given is sound — *"a capability that silently
disappears from a default build is a regression wearing a feature flag."*
Recommend **`ocr` default ON** so pdfcer's ordinary build is the full-featured
one, with `--no-default-features` the deliberate lighter build, exactly as
`jpx` works. The 12 MB is in the *model files*, not the code, and those are
folder contents the packaging step controls independently — so a "lite"
package can omit the models and let the code refuse by name (**R27**) without
needing a different binary at all. That is a better lever than the feature
flag, and it is available only because `ocrs` is small.

### 8.3 Design constraints to write into the Pass

- **No `ocrs` type in any public `pdfcer-core` signature.** Bus factor 1
  (§3.9) plus a live possibility of switching to `ocr-rs` (§8.1 condition 1)
  make the boundary load-bearing, not stylistic.
- **No `anyhow` in `pdfcer-core`'s public API** — project rule 10 (§3.11).
- **Emit `Tz` and `3 Tr` inside every text object**, never relying on
  persistence across `BT`/`ET` (§7.1.5).
- **A page's OCR layer is one appended content stream, rejectable
  independently** (§7.1.2, §7.2.1).
- **Extend the R24-style CI assertion** to cover the rten stack's features, or
  write a sibling job. The existing one is scoped to named codec crates and
  would not fire (§3.6).
- **Record the `unsafe` widening as an accepted exception with reasons**, in
  the shape decision 039 used for `aes`/`sha2` (§3.6).
- **Consider a subprocess escape hatch as a *separate*, later feature** —
  pdfcer shells out to a `tesseract.exe` the operator installed themselves,
  ships nothing, consumes hOCR. `LEGAL.md` §6.5's veraPDF precedent makes the
  pattern already-settled, it raises no licence and no network question, and
  it gives power users 120 languages without pdfcer carrying 54 MB. It is
  **not** a substitute for a bundled engine, because it fails the
  single-folder promise for everyone who has installed nothing.

### 8.4 The attribution problem, solved once for whichever engine wins

Every candidate has non-Cargo artifacts that `cargo-about` cannot see:
`ocrs`'s CC-BY-SA-4.0 models, Tesseract's Apache-2.0 DLLs and traineddata,
`ocr-rs`'s prebuilt MNN library. `LEGAL.md` §6.3 says
`THIRD_PARTY_LICENSES.md` is generated and *"never hand-edited"*, and that
rule should stay exactly as it is.

**The answer is a second, sibling file — not an exception to the first.**
Something like `THIRD_PARTY_DATA_LICENSES.md`, hand-authored, covering the
shipped artifacts that are not Cargo dependencies, generated-file discipline
untouched. Two things make this more than bookkeeping: it is the only place a
CC-BY-SA attribution obligation could actually be discharged, and it is the
natural home for the §6.5.4-rule-5-style note explaining **why these entries
are correctly absent from the generated file**, so nobody "fixes" the
apparent omission in either direction.

---

## 9. ★ The question that is the operator's, not the engineer's

**May pdfcer ship a `CC-BY-SA-4.0`-licensed model file inside its MIT-licensed
portable folder?**

The facts, so the question can be answered rather than researched again:

- **The `ocrs` models are CC-BY-SA-4.0.** Declared on the Hugging Face model
  card and in the API's `cardData`; inherited from the HierText training
  corpus. The `ocrs-models` repository itself has **no LICENSE file** — the
  declaration exists only on the model card, which is thinner provenance than
  one would want for the single non-permissive artifact in the build.
- **The code is unaffected either way.** `ocrs`, `rten` and all 42 transitive
  packages are permissive; CC-BY-SA has no linking concept and cannot reach
  pdfcer's source (§3.3).
- **The supporting reading:** Creative Commons' own FAQ distinguishes a
  **collection** (which may carry its own licence; ShareAlike does not reach
  it) from an **adaptation** (which must be BY-SA). Shipping the unmodified
  `.rten` files beside MIT code is, on that reading, a collection. This is the
  same structure of argument `LEGAL.md` §6.5.2 already accepted for MPL-2.0.
- **Why it is not cleared by an agent anyway.** `LEGAL.md` §6.2 step 4 fires on
  anything not permissive, *"even if pdfcer's current license would technically
  allow it — this is a case where getting it wrong is expensive to unwind
  later."* And "mere aggregation is a collection" is a **reading**, not a
  measured fact. This project has an expensive written lesson (§1.1) about an
  agent asserting an unmeasured environmental fact in the very document meant
  to warn about it; a legal conclusion asserted the same way would be worse.
- **What clearly *does* propagate:** fine-tuning, quantizing, retraining or
  format-converting the weights plausibly creates Adapted Material and binds
  **the derived model** to CC-BY-SA-4.0. So "we'll fine-tune it for CAD
  drawings later" is a decision with a licence attached, and it is better
  known now.
- **Attribution is owed regardless of the answer** and `cargo-about` will not
  provide it — §8.4.
- **There is a likely-permissive alternative if the answer is no**, and it is
  **not** Tesseract: **`ocr-rs`** (§5.2) — Apache-2.0 code, Apache-2.0 MNN
  runtime, 3.2 MB models, 50+ languages, zero DLLs on Windows, at the cost of
  the WASM story and a prebuilt binary blob. **Its weights licence is probable
  rather than confirmed** and must be read off the specific model card before
  it is relied on; §5.2 explains why that distinction is not pedantry.

**Three things this question is not**, so it is not answered against the wrong
worry:

1. It is **not** a copyleft-contamination risk to pdfcer's own MIT licence.
   Nothing here resembles the MuPDF/Ghostscript situation §6.1 forecloses.
2. It is **not** blocking the research or the design. §7 stands whichever
   engine wins; slices 0 and 1 of §8.2 can both start today.
3. It is **not** urgent in the sense of needing an answer this week — but it
   **is** blocking *slice 2*, and answering it late means having built the
   binding against the wrong engine.

### 9.1 A second item, smaller, also the operator's if Tesseract is ever revisited

If Tesseract is chosen despite §8.1, two licence items must be resolved first
and neither is an engineering call:

- The default Windows build **ships LGPL binaries** (`libunistring`,
  `libiconv`, `libintl`) via the libcurl branch — §4.3. Removable with
  `DISABLE_CURL` + `DISABLE_ARCHIVE`, but only by **building from source**, so
  the convenient prebuilt installers are not directly shippable.
- The MinGW runtime's **GCC Runtime Library Exception** wording is
  **UNVERIFIED** (gnu.org returned HTTP 429 during research). An MSVC/vcpkg
  build avoids the question and is also ~25 MB smaller — but that ~12–13 MB
  figure is a **projection, not a measurement**, and would need building
  before being relied on.

---

## 10. Verification gaps — re-check before treating as settled

In the style of `PRIOR_ART.md`'s own section, because this document will be
read months from now.

- **The MSVC Tesseract folder size (~12–13 MB)** is a projection from the
  MinGW measurement, not a build. Everything else in §4.4 was measured.
- **The GCC Runtime Library Exception** text — not fetched (HTTP 429).
- **`ocrs` vs Tesseract vs `ocr-rs` accuracy** — no comparison exists
  anywhere. §8.2 slice 0 is the only route to a number.
- **★ The PaddleOCR *released weights* licence** — the repository is
  Apache-2.0 (verified), but no per-model licence statement was located on the
  ModelScope or Hugging Face artifacts. **This is the highest-priority open
  item in this document after §9**, because §8.1's fallback rests on it, and
  because `surya` (§5.4) is a live example of Apache-2.0 code shipping with
  restricted weights. Read the model card for the exact det/rec pair.
- **The bundled size of a PP-OCR det+rec pair** — the 3.2/10.8/15.6 MB figures
  come from `ocr-rs`'s own repository, not from a measurement here.
- **`ocr-rs`'s prebuilt MNN static library** — not audited. Who built it, what
  is in it, is it reproducible? Answer before adopting (§5.2).
- **`Windows.Media.Ocr`'s package-identity requirement** — Microsoft's docs and
  a shipping unpackaged binary disagree (§5.1). Only matters if that candidate
  is revisited.
- **`Windows.Media.Ocr` minimum image dimension** — no Microsoft page
  documents one; a single Q&A reports ~40×50 empirically.
- **`ocrs`'s wasm32 *runtime* behaviour** — §3.5 measured `cargo check`, which
  is exactly what CI asserts and no more. `rayon` threading in a browser is
  unaddressed and will need work whenever the web fork is real.
- **The `ocrs` models' provenance** rests on a Hugging Face card, since
  `ocrs-models` carries no LICENSE file. Re-verify before shipping.
- **`harumi`'s licence** — an SPDX string in `Cargo.toml` with no LICENSE file
  in the repository (§7.4). Only matters if it is ever reconsidered.
