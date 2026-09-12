# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-11, after `Pass 296.8` and the 512th filing.

---

## ★★★ READ THIS PARAGRAPH FIRST — RUN THE GATE SWEEP BEFORE YOU PUSH

`tools/run-gates.sh` was treated as a **release** gate. It is not; it is a
**push** gate, and this session paid for the difference.

`main` had been **red on GitHub since 20:18Z**, from a commit pushed earlier
the same day by a session that did not read CI's colour afterwards. The cause
was a string literal with a baked-in run of spaces — a lost line-continuation
backslash. Nothing about it could fail a test. Running the sweep before pushing
found it, plus two more:

| gate | what it found | how long it had been live |
|---|---|---|
| `check-public-fns-documented.py` | `preview_style_resolution` had **no doc comment** — a splice welded its 38-line doc block onto the function inserted above it | 1 day |
| `check-string-gaps.sh` | two literals with a lost backslash | 1 day / same day |
| `check-ci-job-names.py` | the `audits` job said `(20 checks)` and ran 21 | same day |

**None would ever have failed a test, been caught by clippy, or looked wrong in
a diff** — a doc block welded to the wrong function reads as correct, because
both functions have docs.

So: **sweep, then push, then read CI's colour from GitHub.** Rule 8 already
says to read the colour; it does not yet say to sweep, and that is the gap this
paragraph exists to close.

★★ **AND SWEEP AFTER YOUR LAST EDIT, not merely before pushing** — added
2026-09-12, after the distinction cost a red CI run. The first two string gaps
were found by a sweep, fixed, and pushed. Then more code was written, the sweep
was not re-run, and the third gap reached `origin` and turned CI red.
**A gate run before your last edit is a gate that did not run.** The sweep is
seconds; the discipline is running it against the tree you are actually
pushing.

### How to run it on this machine, because the obvious way gets killed

★★ **`run-gates.sh` and `cargo test --workspace --all-features` are both
OOM-killed here**, repeatedly, including per-crate. Three watchers and two
sweeps died this session. The working procedure:

1. Run the **23 non-cargo gates in one loop** — they are seconds each. Get the
   list from `python tools/check-ci-parity.py --list`.
2. Run the **cargo gates one at a time, in the background, serially**:
   `fmt --check`, `clippy --workspace --all-targets`, `clippy --all-features`,
   `test --workspace`, `test -p pdfcer-core --no-default-features`,
   `check --target wasm32-unknown-unknown`, `cd fuzz && cargo check --bins`.
3. **Do not hold `gh run watch` open** — it is what died most often. Poll
   `gh run list --branch main --limit 1` on a wakeup instead.

★ `run-gates.sh` **buffers**, so a redirected log sits empty until it finishes.
An empty output file is not a hung run.

★★ **THE SENTENCE THAT WAS HERE WAS FALSE, AND CORRECTING IT IS THE POINT.**
It said:

> ~~"its final line reports failures **while exiting 0** — read the
> `run-gates: FAILED — N of 31` line, never the exit code."~~

`tools/run-gates.sh` ends `exit 1` on any failure (line 246) and `exit 0` only
on a clean sweep. It has always been correct. What reported 0 was **my own
pipeline** — `bash tools/run-gates.sh 2>&1 | tail -40` exits with `tail`'s
status, not the script's.

⇒ **A wrapped command's exit code is the WRAPPER's.** Run a check alone and
read its own status, or redirect to a file and grep the file — never both pipe
it and trust the code. The same mistake pushed a lint failure to `origin` an
hour later (`4608f7e`), from `cargo clippy … | grep … | head`.

★ Note the shape, because this project has met it before and it is the
expensive kind: **I attributed my own error to a defect in a tool, and wrote
the false attribution into the document a session reads FIRST.** `CLAUDE.md`
rule 8 records the same thing about "there is still no git remote configured" —
a fact about the environment that nobody had measured, reading as reassurance
for a day. `grep -n 'exit' tools/run-gates.sh` costs nothing.

---

## STATE

Workspace version `0.53.0`; the last release is **`v0.53.0`**. ★ Verify with
`gh release list` before repeating it — a previous handoff carried a release
number four versions stale for a day, and nothing in this file checks itself.

**`main` is pushed through the 512th filing (`d2465f5`) and CI is GREEN**
(run `34657680461`, 17m21s). Working tree clean, nothing unpushed.

### What shipped: one inbound batch, seven Passes, in one evening

Every one answers a request from `pdfcer-gui`.

| Pass | commit | what |
|---|---|---|
| `296.0` | `69d4d67` | **a deep-zoom region render REFUSES instead of killing the worker** — `RenderError::RasterizerLimit`, the crate's only `catch_unwind` |
| `296.1` | `141c989` | **a coverage refusal carries its remedy faces as DATA** — `Refusal::remedy_faces`, page-verified |
| `296.2` | `90576a8` | **`Display for Object` and `for Name`** — scalars exact, containers named not expanded |
| `296.3` | `5943beb` | **a PATTERN redaction reports the text it could not read** — and pdfcer's own CLI `--pattern` was silent too |
| `296.4` | `8d2f6bb` | **`page_composites_in_ink`** — ask before rendering, not after |
| `296.5` | `4f6f5a5` | the rasteriser's panic text out of the error MESSAGE |
| `296.8` | `f392b19` | `BlendSpaceFrom::token()` public |

Plus `f16e266` + `5917ece`, the pre-push gate fixes above (filed as fixes, not
Passes — `ROADMAP.md` has a commit-hash-heading precedent for that).

Filings 508–512. Decisions **151**, **152**, **153**; rules **R252**, **R253**,
**R254** minted. `R245`'s 8th dated instance.

---

## ★★★ THE INBOUND QUEUE IS EMPTY — AND THAT SENTENCE HAS BEEN WRONG TWICE

**Every `pdfcer-gui` request is answered, shipped and confirmed consumed**, and
each consumption note is in the channel. Five `request_*` files remain in
`open/` only because **archiving is the GUI side's step** — they close their own
exchanges within minutes and write the `INDEX.md` rows themselves. Do not
archive on their behalf; you will duplicate work in flight.

★★ **Two previous handoffs said "the queue is empty" and were stale within
hours.** `ls -lt` the inbound directory before believing any sentence in this
file — including this one.

`D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\`

### ★★ The channel now has a TOPIC KEY — use it

Adopted 2026-09-11, recorded in that folder's `README.md`. Every file in one
exchange carries the same key as a filename prefix: `reply_G042_…`,
`done_G042_CONSUMED.md`. **`G###` is minted by pdfcer-gui, `E###` by this
side** — two counters so a collision is impossible without coordination, which
nothing in a folder outside git can provide. The key names the **exchange**,
not the file: one defect filed twice gets one key.

It exists so *a `reply_G*` with no `done_G*` is an answer nobody acted on* is
checkable in four lines of shell. Their audit found five shipped fixes still
described as broken — one **in the operator's manual** — for two to three days
each.

---

## ★★ WHAT THIS SESSION ESTABLISHED THAT OUTLIVES ITS PASSES

### R254 / decision 153 — a value the crate already computes does not earn `pub` by DEMAND

It earned it by existing. Keeping it `pub(crate)` "until someone asks" hands
the discovery cost to **the one party who structurally cannot see the gap**.

★ I read `R151` ("an uncalled API is a cost") as licensing that, and it does
not: R151 audits whether a *published* capability gets *called*. The librarian
declined both homes I proposed and minted a new rule; it also cut my claimed
five instances to **three** on mechanism. **A shared symptom is not a shared
mechanism** — that scepticism was right three times running this session, and
it is worth asking for explicitly in a dispatch.

### R253 / decision 152 — the SAFE rendering must be the DEFAULT one

`Pass 296.0` put a third-party panic string in an error's `Display` and told
callers not to match on it. A consuming shell routes `Display` onto the page on
purpose — so the default path would have painted
`range start index 442613758592 out of range for slice of length 1088737`
across a site plan. **A variant safe only for a consumer who writes a named arm
is unsafe for every consumer who has not read its doc comment.**

### A measurement that refused to become a constant

`Pass 296.0`'s requester preferred a published max scale "because it names the
number". `examples/region_panic_ceiling.rs` bisected six page geometries and
got **three values ordering with nothing** — the largest sheet the most
fragile, an A1 and a business card sharing a boundary A4 never reaches.

⇒ **When the measurement does not support a constant, publishing one anyway is
an invented number wearing a measurement's clothes.** The guarantee became the
refusal; the constant is published only as a FLOOR below the lowest row,
checked against the table at compile time (`const _: () = assert!(…)`).

### A consumer's WORKAROUND is a defect report

Twice in one evening, both under decision 058: the `search_text` double
extraction (`296.3`) and the named arm hiding the panic text (`296.5`). **The
shell filed rather than worked around six times in two days.** That frequency
is evidence about where the boundary is drawn, not about them.

### A diagnostic's EXCERPT is not its finding

`5917ece` exists because I fixed the gap `check-string-gaps.sh` quoted and not
the second one on the same line, past its ~100-character truncation.
**Re-run the check; do not act on the printed excerpt.** (Already a
cross-project lesson at `C:\personal_rag\claude_code\lesson_20260807_truncated_read_of_wrapped_sentence.md`.)

---

## HABITS

- **`tools/edit-source.py` for every multi-line source edit.** I used ad-hoc
  python heredocs instead and it cost two of the three gate defects above —
  eaten backslashes and a doc-block splice. The machinery existed and was not
  reached for.
- **`git commit -F <file>`, never `-m`.** Unbroken.
- **Sabotage every new test.** Every test this session was falsified before
  being believed; two sabotages found that a single break turned *two* tests
  red, which is what a contract pinned in two places should do.
- **Never chain a reverting git verb.** A hook blocks it, correctly — run
  `git checkout --`, `git reset`, `git restore` **alone**.

---

## OWED (carried forward, plus this session's)

- ~~no `docs/core-api/` entry for the `offpage` module~~ — **CLOSED
  2026-09-11**, `bfa981b`, as §13 of `03-capabilities.md`. Struck rather than
  deleted so a reader who remembers it owed can see it moved.
- **`redact-offpage` residuals: ~~17 files / 23 objects~~ → 7 files / 12
  objects**, all of the `partial` kind. `Pass 297.0` (`536ef3b`) closed the
  fully-off half by making `wholly_covered` test the UNION of the bands, not
  one band — measured before and after on the operator's own 17 affected
  drawings. What remains is objects **crossing** the page edge whose cut leaves
  a sliver: on `TS-0396` page 9 the drawn extent exceeds the page box by ~1 pt
  against a 0.25 pt tolerance. Different cause, untouched.
- **NEW — 16 tests silently SKIP and report as passed**, down from 26.
  `Pass 298.0`'s guard was sabotaged to prove its test could fail and the test
  **stayed green**. ★★ **The only fix is a synthetic fixture, and that is a
  constraint rather than a preference**: 13 of the 16 need
  `fixtures/external/pdfbox`, which `fixtures/README.md` marks *"NOT
  blanket-safe … never bulk-import"*, and `fetch-corpora.sh` deliberately omits
  it; 3 need `qpdf`, never fetched either. **"Fetch the corpus in CI" is
  REFUSED, not un-chosen** — it is the obvious two-line idea and it would bulk
  import a corpus `LEGAL.md` §5 rules out. `tools/check-skippable-tests-declared.py`
  keeps the count honest; `f0d1dc7` shows the pattern
  (`synthetic_orphaned_session()` in `widget_adoption.rs`, 14 skips → 4).
  ★ Four of that file's remaining skips are deliberate: they assert the REAL
  AcroForm's composition, and converting them would measure an invented
  fixture rather than the verb. Do not "finish the job" on those.
- **NEW — above ~1e8 scale a region render succeeds again** with an underflowed
  page-space span. Nothing panics; whether those pixels mean anything is its
  own measurement. Told the shell rather than letting them discover it.
- **NEW — `check-reexport-closure.py` checks FIELDS, not method return types.**
  A verb returning an un-re-exported type is the same class. Widen it against a
  measurement, not a guess.
- **`R221`'s recorded instance count is wrong** and a commit message made it
  worse. **Do not copy an ordinal from a commit message.**
- **`tools/check-requests-scoped.py`** — owed by `R242`, still unbuilt.
- ~~**`check-public-fns-documented.py`'s denominator is `pub`** … staged fix~~
  — **MEASURED AND DECLINED 2026-09-11.** Widening it to private functions
  would mean a **1,837-row** baseline outside test modules, which is an
  instrument nobody reads. ★ My first measurement said 381 and was wrong —
  an artifact of cutting each file at its first `#[cfg(test)]` line — and I
  nearly shipped the widening on it. The answer instead is
  `tools/check-doc-block-spliced.py`, which detects the splice directly (one
  doc block containing the same heading twice) and needs no denominator at
  all. It found **four live splices** and **five baseline rows that were
  misfiled text rather than missing text**.
- **21 of 38 files in `fixtures/synthetic/text/PROVENANCE.md` are unrecorded.**
  `LEGAL.md` §5 makes this a licensing statement.
- **Backup bundle is well over 150 commits behind `HEAD`.**
- **143 of 187 standing rules are unenforced** — the operator's own next piece
  of work: *"script it or bin it."*
- **`personal_rag/pdf` entry on the operator's stamp file** (black-background
  `/DCTDecode` with no `/SMask`) — verify it landed from the 496th filing.

### The operator's own ordered plan, still the front of the queue

`Pass 142.0` (embedded-donor `format-text --set-font`), resize-page-contents
(dispatch `pdfcer-acrobat-librarian` first, rule 12), `Pass 259.0` (the
`docs/core-api/` line-citation class), `Pass 10.11` (B-T timestamps).

---

## BUILD ENVIRONMENT

★★ **This machine runs out of memory on whole-workspace cargo work.** See the
procedure at the top; it is the single most time-costly thing about this
session.

★★ **`target/debug/deps` grows without bound** — cargo never garbage-collects
it. `du -sh target/debug/deps` every session; 36 GB at one recent measurement.
Before any delete, both checks: `git ls-files target` returns 0 and
`git check-ignore -q target` passes.

★ **A stale `types.py` in the job temp directory shadowed the standard
library** and broke every `python` invocation whose script lived there, with a
traceback naming `enum`, not the shadowing file. If `python` starts failing on
`import pathlib`, look for a stdlib name in the working directory first.
