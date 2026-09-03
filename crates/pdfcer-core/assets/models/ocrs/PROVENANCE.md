# PROVENANCE — `ocrs` OCR model weights

Two neural-network weight files pdfcer **redistributes** inside its portable
folder. They are **not** a Cargo dependency, so `cargo-about` is structurally
incapable of seeing them and `THIRD_PARTY_LICENSES.md` will never mention them
automatically — the attribution below is authored by hand in `about.hbs`, and
`tools/check-shipped-assets.py` is what enforces that this file exists and
states terms. See `docs/ocr-engine-survey.md` §3.3.

- **Creator:** Robert Knight (the [ocrs](https://github.com/robertknight/ocrs)
  project) — **both files, one project, one licence.**
- **Sources: the two files now come from DIFFERENT CHANNELS**, and the reason
  is a measured defect rather than a preference. See §"The detection model was
  replaced" below before changing either.
  - `text-detection.rten` — <https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten>,
    the bucket the `ocrs` crate's own `examples/download-models.sh` fetches
    from. **Retrieved 2026-08-25.**
  - `text-rec-checkpoint.rten` — <https://huggingface.co/robertknight/ocrs>,
    `main` branch. **Retrieved 2026-08-13.**
- **Licence: `CC-BY-SA-4.0`** — <https://creativecommons.org/licenses/by-sa/4.0/>
  Declared in the model card's YAML front matter (`license: cc-by-sa-4.0`),
  read from the source on the retrieval date rather than from any secondary
  description. **Note that the `ocrs-models` GitHub repository carries no
  `LICENSE` file** — the model card is the declaration.
- **Training data:** [HierText](https://github.com/google-research-datasets/hiertext)
  (itself CC-BY-SA-4.0) plus synthetic data, per the model card.
- **Changes made by pdfcer: NONE.** The files are byte-identical to the
  upstream artifacts; only their *names* were shortened (see below). CC-BY-SA
  requires an indication of changes, and the honest indication is that there
  are none.

## The files

| Shipped as | Upstream filename | Channel | Bytes | SHA-256 |
|---|---|---|---:|---|
| `text-detection.rten` | `text-detection.rten` | **S3** | 2,510,284 | `f15cfb56bd02c4bf478a20343986504a1f01e1665c2b3a0ad66340f054b1b5ca` |
| `text-rec-checkpoint.rten` | `text-rec-checkpoint-s52qdbqt.rten` | Hugging Face | 9,716,444 | `606d9a0414c6b73c99df75b707c11c70d1c8b12e1d4f900922e185fc37bfca65` |

Total 12,226,728 bytes (11.66 MiB).

## ★★ THE DETECTION MODEL WAS REPLACED ON 2026-08-25, AND OCR DID NOT WORK BEFORE IT

**Read this before "tidying" the two files back onto one channel.**

Until 2026-08-25 both files came from Hugging Face, and the detection half was
`text-detection-ssfbcj81.rten`, 2,523,564 bytes, SHA-256
`614aafab…`. **With that file, `ocrs` 0.12.2 finds essentially no text.** On a
clean 150 dpi render of a page of 12 pt Helvetica it returned sixteen
fragments — `"1"`, `"?"`, `"E"` — clustered at the right page margin, plus one
"word" whose bounding box was the entire page. Not degraded output: noise.

The recognition model was never at fault, and the two hypotheses were
separated by swapping **one file at a time** rather than both:

| detection | recognition | result on the same page |
|---|---|---|
| HF `-ssfbcj81` | HF checkpoint | garbage (the shipped state until today) |
| HF `-ssfbcj81` | S3 `text-recognition` | **still garbage** |
| S3 `text-detection` | HF checkpoint | **every word correct** |
| S3 `text-detection` | S3 `text-recognition` | every word correct |

Row 3 is the one that matters: the **Hugging Face recognition checkpoint is
fine**, so it stays, and only the detection file moves. Swapping both would
have "fixed" it while leaving which file was broken unknown.

★ **The name was the misleading clue.** `text-rec-checkpoint` reads like a
training artefact next to the crate example's `text-recognition.rten`, and the
first hypothesis was that pdfcer was running a checkpoint as its recogniser.
The isolation says the opposite. Recording the wrong hypothesis alongside the
right answer because the wrong one is the one a future reader will form too.

★★ **`docs/ocr-engine-survey.md` recorded the discrepancy and dismissed it.**
It measured both channels in the same session, noted the detection files differ
by 13,280 bytes under different filenames, and concluded: *"The totals agree to
within 0.1%, so nothing in this survey turns on it."* **Everything turned on
it.** The survey's own next sentence — *"pdfcer must pin exactly which artifact
it ships and hash it, rather than treating 'the ocrs models' as one thing"* —
was exactly right and was followed for provenance while the artefact chosen was
never actually run end to end. A size difference between two builds of one
model is not a rounding error; it is two different models.

**Hugging Face hosts no working detection model.** Its `main` branch was listed
on 2026-08-25 and contains exactly two `.rten` files, the two named above. So
this is not a case of picking the wrong file from a directory — the working
artefact exists only on the author's S3 bucket.

### Licence, and the one thing a reader should know about the S3 file

The **CC-BY-SA-4.0 declaration lives on the Hugging Face model card**, which is
the project's own licence statement for its models. **The S3 bucket carries no
licence statement of its own** — it is a bare object store, and an HTTP `GET`
of a `.rten` returns weights and nothing else.

pdfcer's position, stated rather than assumed: both files are the same author's
build of the same project's models, distributed by that author through two
channels, and the project's declaration covers them. **The operator was told
that the working file's channel carries no licence text of its own and
authorised bundling it on 2026-08-25.** Recorded here because a future reader
comparing the two rows will notice the channels differ and should find the
answer beside the question rather than have to re-derive it.

**Changes made by pdfcer: still NONE.** Both files are byte-identical to their
upstream artefacts; only the detection file's *name* is shortened (the S3 name
already matches, so today's swap shortened nothing) and the recognition file
keeps the rename described below.

### ★ Why the names differ, and why the hash is what identifies these files

Upstream filenames carry a **content-addressed suffix** (`-ssfbcj81`,
`-s52qdbqt`) which is *their* versioning scheme. pdfcer strips it so
`pdfcer_core::ocr::engine_ocrs`'s `DETECTION_MODEL` / `RECOGNITION_MODEL`
constants can name a stable path, and pins the exact artifact by **SHA-256**
instead — *our* versioning scheme.

This matters more than it looks. `docs/ocr-engine-survey.md` recorded that the
**Hugging Face and S3 copies of "the ocrs models" are not byte-identical** —
different filenames, one 13,280 bytes smaller, one 124 bytes larger. *"The
ocrs models"* is therefore not one thing, and a build that fetched "the latest"
would be running weights nobody tested. The hashes above are the identity;
the names are only convenience.

## What CC-BY-SA-4.0 obliges pdfcer to do, and what it does not

**Obliges** (satisfied by this file plus the `about.hbs` entry that ships to
end users): name the creator, name and link the licence, state whether changes
were made, and do not apply effective technological measures that restrict
what recipients may do with the files.

**Does not oblige:** anything about pdfcer's own source. CC-BY-SA is a licence
for creative works and has **no linking concept at all** — Creative Commons
recommends against using CC licences for software precisely because they
"do not contain specific terms about the distribution of source code".
Shipping these files unmodified alongside MIT code is distribution of a
verbatim work in a **collection**, not an **adaptation**, and only adaptations
must be released under BY-SA. pdfcer's MIT licence is unaffected.

## ★ THE ONE THING THAT WOULD CHANGE THAT

**Modifying the weights creates Adapted Material, and the adapted weights must
then be CC-BY-SA-4.0.** That includes fine-tuning them (for CAD drawings, say),
quantizing them for speed, retraining on any corpus, or converting them into
another runtime's format.

It would bind **the derived model**, not pdfcer's source — but it means
*"we'll fine-tune this later"* is a decision with a licence attached, and it
needs its own operator decision at the time. Recorded here rather than in a
roadmap entry because this is the file someone will be looking at when they
have the weights open and the idea occurs to them.
