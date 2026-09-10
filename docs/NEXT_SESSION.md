# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-10, after `Pass 293.0`.

---

## STATE

Workspace version `0.50.0`; the last release is **`v0.50.0`** (2026-09-10,
morning). ★ Verify that with `gh release list` before repeating it — the
previous handoff carried a release number that was four versions stale for a
day, and nothing in this file checks itself.

**`main` is pushed through the `Pass 293.0` filing.** `tools/run-gates.sh`
green (the only red this session was `cargo fmt --check` and a first
`clippy --all-features` pass, both fixed before their commits).
`cargo test --workspace` **5,309 pass**.

### Six Passes shipped, all from one morning's inbound batch

| Pass | commit | what |
|---|---|---|
| `290.0` | `556878e` | **a page with no `/Resources` opens** — one blank spacer page was costing the whole document |
| `290.1` | `bce4703` | a page-tree failure stops being reported as *"every stamp points at nothing"* |
| `291.0` | `0173a95` | a shrunk or clipped stamp label **says so** — two of three fit policies were unofferable |
| `292.0` | `c11c1aa` | a placed stamp's label size can be **read and written** — and a restyle stops eating the stamp's own words |
| `293.0` | `56c5e55` | **a custom stamp can be PLACED** — one page's artwork onto another, as vector |

Filings: 494th … 496th. Decision **150** minted (494th).

---

## ★★★ THE QUEUE IS EMPTY — AND THAT SENTENCE WAS WRONG LAST TIME

**Every one of the five requests `pdfcer-gui` filed on 2026-09-10 is closed**,
with a reply written for each. Nothing of theirs is pending here.

★★ **The previous handoff said the same thing and was stale within hours.**
Five requests landed at 06:53–07:00 while it still read *"the queue is empty of
inbound work"*, and the first act of this session was discovering that by
looking. **`ls -lt` the inbound directory before believing any sentence in this
file** — including this one.

Both channels: `D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\` (the live
one) and `D:\Dev\FeatureRequests\pdfcer-gui\`.

### The operator's own ordered plan, still the front of the queue

`Pass 142.0` (embedded-donor `format-text --set-font`), resize-page-contents
(dispatch `pdfcer-acrobat-librarian` first, rule 12), `Pass 259.0` (the
`docs/core-api/` line-citation class), `Pass 10.11` (B-T timestamps).

### One artifact would close a GAP that has been open since `Pass 288.0`

★★ **Acrobat READER can place an existing custom stamp.** It cannot author new
stamp *categories* — that is what the "Pro is not installed" note has always
been about — but placement is available, and the operator's own collection is
on disk. **One stamp placed in Reader and saved settles whether a placed stamp
records which stamp it came from** (`/Name`? a private key? nothing?), which is
the last unanswered question about Acrobat's stamp model. It needs a live GUI
step, so it is the operator's minute, not an agent's.

This corrected a premise the project had been carrying in memory
(`acrobat-reader-is-available-pro-is-not` was being read as "no placement
artifact is obtainable"). Note the shape: **a constraint nobody had re-tested
became a reason not to look.**

---

## ★★ WHAT THIS SESSION ESTABLISHED THAT OUTLIVES ITS PASSES

### Decision 150 — a required attribute defaults to the value the STANDARD names

Absent `/Resources` now resolves to the **empty dictionary** — because Table 30's
own `/Resources` row says *"If the page requires no resources, the value of this
entry shall be an empty dictionary."* The standard supplies the value; pdfcer
invents nothing. `/MediaBox` deliberately did **not** move: no clause anywhere
names a default media box, so any value would be invented. **The line is
invention, not strictness**, and a test asserts the `/MediaBox` half by name.

★ The file is **certainly non-conforming** — ISO considered conditioning
`/Resources` on `/Contents` and decided AGAINST it (`pdf-issues` #81, ISO
approved) — and that *strengthened* the case: §2.2 scopes a reader's rendering
duty to *conforming* files and §1 puts conformance validation outside the
standard's scope, so **nothing in ISO 32000 ever asked a reader to refuse.**

### Two APIs that are unbuildable alone ship together

`Pass 292.0`: a **read with no write** is a number nobody can act on; a **write
with no read** is a control that opens on a guess and overwrites what was
there. The consuming shell made that argument and declined to build either
half. Where a property is inspectable *and* settable, ship the pair.

### `R245` keeps arriving as "a capability present on one route of two"

Three instances this session, and the worst **destroyed operator data
silently**: `set_text_annot_style` re-baked a stamp without the label recovery
that `resize_annotation` already called, so **changing a stamp's COLOUR
replaced `APPROVED FOR CONSTRUCTION` with `DRAFT`.** Measured on a real file,
from a control captioned "colour".

⇒ **When you add a recovery, grep for every route that re-bakes the same
object family.** The other two: `Pass 290.0`'s refusal that `plan_paste_at` had
grown a hand-written bypass around, and `Pass 290.1`'s discarded error.

### A test can reach a correct assertion THROUGH a defect

`R225`'s 18th instance and a new sub-shape. Two tests asserted *"an unwalkable
page tree is reported, not rendered as empty"* using
`fixtures/synthetic/minimal.pdf` — which was unwalkable **only because of the
bug `Pass 290.0` fixed**. Fixing it turned two unrelated, correct tests red.

★ **The tell is an unrelated test going red when you fix a bug.** Both now use
`fixtures/synthetic/xref-recover/page-tree-cycle.pdf`: a cycle is damage with
no second reading, which is what a fixture for "unwalkable" has to be.

---

## HABITS THAT PAID THIS SESSION

- **`tools/edit-source.py`** (promoted from a temp directory last session) was
  used for every multi-line source edit and refused a bad pattern once, out
  loud, instead of matching zero times in silence. Use it.
- **`git commit -F <file>`, never `-m`.** Unbroken this session.
- **Dispatch the spec librarian BEFORE reasoning from a clause.** It corrected
  a sentence in `Pass 290.0`'s doc comment *before it shipped*: "a page with no
  `/Contents` can never name a resource" is FALSE — §7.8.3 lets a form XObject,
  including an annotation `/AP` stream, inherit the page's resources, which is
  exactly the stamp-page shape that motivated the Pass.
- **Sabotage every new test.** Eleven sabotages this session, each turning
  exactly the expected test red; one (`R47`'s byte comparison) gained an
  explicit *"the fixture must not be empty"* assertion so it cannot pass
  vacuously.
- **Verify against the operator's real file**, not only fixtures. Every Pass
  this session was checked against
  `%APPDATA%\Adobe\Acrobat\DC\Stamps\YTV_yyfVN1TzJ0_6oei-GB.pdf`.

---

## OWED (carried forward, plus one new)

- **`R221`'s recorded instance count is wrong** and a commit message made it
  worse (`Pass 279.0` says "third"; the Standing Rules entry is past three).
  Reconcile in a session with budget. **Do not copy an ordinal from a commit
  message.**
- **`tools/check-requests-scoped.py`** — owed by `R242`, still unbuilt.
- **`check-public-fns-documented.py`'s denominator is `pub`**, so it cannot see
  the doc-splice defect on private functions. Staged fix, its own change.
- **21 of 38 files in `fixtures/synthetic/text/PROVENANCE.md` are unrecorded**
  (55.3 %). `LEGAL.md` §5 makes this a licensing statement, not tidiness.
- **Backup bundle is well over 150 commits behind `HEAD`.**
- **NEW — a `personal_rag/pdf` entry is owed** on the operator's own stamp
  file: its "Savy" page is a single `/DCTDecode` RGB image with a **black
  background and no `/SMask`**, so a faithful placement puts a black box on the
  page. pdfcer reproduces it pixel-identically. A future session will otherwise
  spend an hour deciding whether that is a rendering bug. (Handed to the
  librarian in the 496th filing; verify it landed.)

---

## BUILD ENVIRONMENT (unchanged, and all of it still true)

★★ **`target/debug/deps` grows without bound** — cargo never garbage-collects
it. `du -sh target/debug/deps` every session; it was 36 GB at the start of this
one. Before any delete, both checks: `git ls-files target` returns 0 and
`git check-ignore -q target` passes.

★ **`run-gates.sh` buffers**, so a redirected log can sit unchanged for
minutes and look hung when it is not. Poll for the final `run-gates:` line
rather than watching the tail.

★ **Foreground survives where background dies.** `cargo test --workspace
-- --test-threads=2` takes ~10 minutes; run it and wait rather than polling.

★ **A stale `types.py` in the job temp directory shadowed the standard
library** and broke every `python` invocation whose script lived there, with an
import traceback that names `enum`, not the shadowing file. If `python` starts
failing on `import pathlib`, look for a stdlib name in the working directory
before believing anything else.
