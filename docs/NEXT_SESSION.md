# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-12, after `Pass 300.2` and the 529th filing.

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

★★ **THAT "IT BUFFERS" CLAIM WAS ALSO MINE, AND IT WAS ALSO FALSE.**
The sentence here said:

> ~~"`run-gates.sh` **buffers**, so a redirected log sits empty until it
> finishes. An empty output file is not a hung run."~~

It does not buffer. Redirected straight to a file it writes each `=== <cmd>`
banner as it goes, and a sweep OOM-killed mid-`cargo test` on 2026-09-12 left
24 lines of readable progress showing every gate that had already passed.
What sat empty was `bash tools/run-gates.sh 2>&1 | tail -40` — **`tail` cannot
emit a line until its input closes.**

⇒ Note that this is the SAME ERROR as the exit-code one below, from the same
pipeline, written into this file in the same session that corrected the other
half of it. **Redirect to a file; do not pipe.** A pipeline changes both what
you see and the status you read, and both failures look like a defect in the
tool.

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

**`main` is pushed through the 529th filing** — ★ read CI's colour from
GitHub yourself (`gh run list --branch main --limit 1`); this line records
what was pushed, never what the server thought of it.

### ★★★ THE TORONTO-MAP ARC IS CLOSED EXCEPT FOR THE MEMORY HALF

Three Passes, one evening, one request — the operator's *"Acrobat can read and
zoom in on this pdf much much faster than we are capable of … the footprint in
ram for ours is enormous by comparison"*, then *"can you fix those things
without breaking the other things that our rendering engine does well"*.

| Pass | what | worth |
|---|---|---|
| `300.0` | an image whose unit square misses the viewport is skipped before the decode (§8.9.5.2), the twin of the form cull | **nothing on this file**, and that is the finding — 1,090 of 1,182 images culled and neither time nor RAM moved |
| `300.1` | a discarded full-page scan per group, moved inside the `if` that reads it | 4% |
| `300.2` | a transparency group composites over its own `/BBox`, not the whole page | **783 s → 8 s at 4×**, 54.6 s → 2.45 s at 1×, rasters hash-identical |

★★ **READ `300.2`'s COMMIT (`6ff57ab`) BEFORE OPTIMISING ANYTHING IN THIS
CRATE.** The slow path it fixed had been correctly *located* and wrongly
*diagnosed* three times across a month, by three sessions, and fixed zero
times. Every one of them named the per-group `Pixmap::new`. Timed:

    whole render                                 54.94 s
    with the composite skipped                    2.33 s
    with the allocation pooled instead           53.18 s

The allocation is 1.8 s. The `draw_pixmap` one line below it is 52.6.
**Proximity in the source is not proximity in cost.** And the misdiagnosis is
why it survived: the allocation is the half that *would* have needed the
coordinate-system rewrite, so all three sessions correctly concluded the fix
was invasive and correctly deferred it — sound reasoning from the wrong
object. The real fix moves no coordinates at all.

The general form is in `D:\dev\rag\rust\a_plausible_explanation_that_predicts_the_right_order_of_magnitude_is_not_a_diagnosis.md`.

### ★★ WHAT IS STILL OWED: THE MEMORY HALF, AND IT IS NOT WHAT WAS FIRST SAID

The investigation blamed image decoding for the 307 MB peak. It is not.
`extract-text` rasterises nothing and peaks at the same 307 MB; a bare
`inspect` load is 38 MB. Measured:

* the file's form XObjects hold **20.0 MB** of content that parses to
  **3,962,903 `ContentToken`s × 64 B = 242 MB**; one form alone is 2,291,669
  tokens;
* `ContentToken` = 64 B (`ContentTokenKind` 48 + span 16); `Object` alone is
  40 B.

⇒ The fix is **shrinking `ContentToken`**, in `pdfcer-core`. Unscoped, no Pass
number, nobody has started it. It is the last open item from this request.

### ★ HOW TO BENCHMARK HERE, because the obvious way cannot run

**`pdfcer-cli` will not release-link on this machine** — four builds
OOM-killed, including at `-j 2`. Take timings through a throwaway release test
in `pdfcer-render` instead (`cargo test -p pdfcer-render --release --test
<name> -- --nocapture`); it builds in a couple of minutes and can call
`render_page` directly. Hash `out.pixmap.data()` in the same test and you get
the A/B and the byte-identity proof from one run. Delete the file before
committing.

### ★★ TWO METHODOLOGY LESSONS FROM THIS ARC

★ Two paragraphs that stood here were DELETED rather than struck, because they
had become false: they described the group-buffer work as "unstarted" and
named the per-group allocation as the time cost. `Pass 300.2` shipped the fix
and measured the allocation at 1.8 s of 54.9 — see the table above. A handoff
that contradicts itself is worse than one merely out of date, because the
reader cannot tell which half is current.

★★ **And the one that cost the most time in the arc:**
the regression baseline built before touching the code rendered **114 fixtures
and stopped at `fontinfo` alphabetically** — it did not contain `images`,
`transparency`, `overprint` or `shading`, the four directories the change was
most likely to break. It would have certified the change while testing none of
it. **A baseline that omits the directories your change touches certifies
nothing.** The real verification was a stash / rebuild / re-render of all 364
synthetic fixtures, byte-compared: identical.

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
  drawings.

  ★★★ **What remains is NOT a cut that leaves a sliver** — that was this file's
  wording and it was wrong, corrected 2026-09-12 (`7a22c52`). The cut is
  COMPLETE: `covered_cells` snaps **outward**, so an image overhanging by 1 pt
  has its off-page sample columns cleared and the samples out there are blank.
  What clearing cannot do is move the **placement**, so the bbox still crosses
  the edge and `scan-offpage` — which classifies by GEOMETRY — still counts it.
  **The scan is reporting its own output**, the same shape as the empty text
  husk `Pass 294.2` fixed, one type over.

  ⇒ The fix is to stop counting an image whose off-page cells carry no ink, and
  it is **not** free: it needs the samples, and decoding every image during a
  scan is what made `redact-offpage` take ten minutes on one file
  (`Pass 294.1`). **It wants a measurement — how many placements, how much
  decode — before any code.** Anyone hunting a cutting defect here will find
  nothing wrong; that is the trap this paragraph exists to spring.
- **6 tests silently SKIP and report as passed** — ~~26~~ → ~~16~~ → ~~10~~ →
  ~~8~~ → **6**, four paydowns on 2026-09-12 (`f0d1dc7`, `e41892a`, `d291a03`,
  `05b3a80`).

  ★★ **THE SIX ARE TWO DIFFERENT KINDS — do not read them as one pile:**
  - **four** in `widget_adoption.rs` (the census and three preview tests) are
    **deliberate**: they assert the REAL AcroForm's own composition, so
    converting them would measure an invented fixture rather than the verb;
  - **two** in `stamp_collection.rs` read **Adobe's own installed stamp files
    from `%APPDATA%`** — the operator's machine, not a corpus. There may be no
    other way to test "we read Adobe's real files", but it means those two
    **can never run in CI on any machine**, which is a different problem from
    the corpus one and wants its own answer.
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
  ★ Four of the remaining ten are deliberate: `widget_adoption.rs`'s census
  and preview tests assert the REAL AcroForm's composition, and converting
  them would measure an invented fixture rather than the verb. Do not "finish
  the job" on those.

  ★★ **The criterion that decides convertibility**, from doing it twice: *are
  the numbers the SUBJECT or the SETTING?* `merge_document.rs`'s "12 fields
  over 13 widgets" is a property of the fixture — a synthetic source with the
  same composition tests the same thing, so all eight converted. The preview
  tests assert the corpus's own composition, which is the subject, so they
  cannot.

  ★ **`clippy::dead_code` proves a conversion is complete**, better than a
  SKIP count: when the last test stops using it, the corpus path constant
  becomes unused and the compiler names it. `merge_document.rs`'s `ACROFORM`
  is gone for that reason.

  ★★ ~~THE NEXT THREE ARE BLOCKED ON A FIXTURE NOBODY CAN GENERATE~~ —
  **RESOLVED the same day** (`d291a03`), by the operator asking whether a
  suitable PDF could be found online. It could: `LEGAL.md` §5 already names
  **veraPDF's open corpus** as approved source (b), so it was a documented
  decision rather than a search. `fixtures/verapdf/object-streams.pdf` is the
  smallest VALID file of the 78 in that corpus carrying an `/ObjStm`.

  ★ **The analysis that said "blocked" was still right and is still worth
  keeping**: pdfcer's writer only DEcompresses (`writer/save.rs:1018`), and no
  `fixtures/synthetic/**` file contains an `/ObjStm`. What changed was not the
  facts but the question — *generate one* is blocked; *use a cleared one* was
  never blocked and was already permitted.

  ★★★ **`fixtures/verapdf/` is the first non-MIT file in this tree** — the
  corpus is **CC BY 4.0**, redistribution permitted with attribution, which
  `fixtures/verapdf/PROVENANCE.md` carries. **OWED, and it is the operator's
  call:** whether `LEGAL.md` should gain an explicit line recording that, the
  way §6.7 does for the CC-BY-SA-4.0 OCR weights. Flagged by the librarian,
  not edited — that file is operator-governed.

  ★ **And one of those three never needed a corpus at all.** It needed *an
  encrypted document*, and `fixtures/synthetic/encryption/` has held eight the
  whole time. ~~Before hunting any more fixtures, re-check the remaining 8~~ —
  **DONE the same hour** (`05b3a80`): two more converted, neither needing
  anything new.

  ★★★ **And the re-check found a defect the note itself had created.**
  `structure_inspect`'s object-stream test was **still skipping for want of the
  fixture added an hour earlier** — its path was repointed, but a SECOND guard
  clause further down (`if l.object_streams.is_empty() { SKIP }`) was the real
  gate. ⇒ **Repointing a fixture is the visible half; a decline further down is
  invisible in the diff and keeps the test dead.** When you convert a test,
  grep its whole body for the skip idiom, not just its path.
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
