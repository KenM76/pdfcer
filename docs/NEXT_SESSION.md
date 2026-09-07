# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-07, immediately after releasing `v0.45.0`.

---

## STATE

**Released: `v0.45.0`** at `654b150` — GitHub release with the `windows-x64`
zip + `.sha256`, OneDrive slot **`pdfcer2`** (`pdfcer1` keeps `0.44.1` as the
previous version), `verify-release.py v0.45.0` **nine of nine on a clean
tree**, and the published asset **downloaded back from GitHub and re-hashed**
(`69e0071d…`, matches — the link works).

Workspace version **`0.45.0`**. Ledger: filings **465**, Pass ceiling
**`258.3`**, decisions **139**, rules **R243**.

**★ ONE COMMIT IS UNPUSHED** at the time of writing — `35eb0d1`, the 465th
filing. Push it (standing-authorized, decision 090). Everything before it,
including the tag, is on `origin`.

### What shipped

`Pass 155.1` + `Pass 155.2`, together, because they interlock — and the first
is a defect **the operator reported himself**.

**`155.1`** — `rotate_annotation` was not composable. `/Rect` was derived from
the previous `/Rect`, so each turn after the first bounded an already-enlarged
box while `/Matrix` only accumulated the angle, and §12.5.5 step (c) scaled
the **artwork** up to fill the surplus. Four 15° turns drew 1.93× wider than
one 60° turn. Now derived from the artwork via three rules, each **named in
the outcome** (`RectDerivation`): `Artwork` and `Geometry` compose;
`PreviousRect` does not and says so.

**`155.2`** — the angle could be written and never read.
`Annotation::appearance_matrix` + `appearance_rotation_degrees()`,
`annot::rotation_degrees()` (the one function the reader and writer share, per
`R243`), `EditSession::set_annotation_rotation` (absolute, idempotent,
refuses rather than assuming zero), and **`pdfcer_render::annot::
appearance_placement()`** made public — which deletes four copies of §12.5.5
from `pdfcer-gui`.

CLI: `rotate-annotation --absolute`, `rect_derived=` on the report.

---

## ★ THE QUEUE — EMPTY OF REQUESTS. The operator's own ordered plan is next.

**Both channels checked by diff at session start and again at the end.**
`D:\Dev\FeatureRequests\pdfce_FeatureRequests\open\` has **no unanswered
`request_*`**; the two that were open are answered by
`reply_2026-09-07-rotation-composes-now-and-the-angle-is-readable-SHIPPED.md`.
Per `R242` they do **not** leave `open/` on being answered — do not read their
presence as outstanding work; **grep `ROADMAP.md` for the filename first**.
`D:\Dev\FeatureRequests\pdfcer-gui\` unchanged since 2026-09-03.

So the top of the next session is the operator's earlier ordered plan
(2026-09-06), untouched:

1. **`Pass 142.0`** — the embedded-donor half of FF-C: `format-text
   --set-font` to a face neither on the page nor standard-14, subsetting and
   embedding a donor supplied via `--font-dir`. Entry ~`docs/ROADMAP.md`
   line 116195. `add_text --embed-font` already does the subset+embed for NEW
   text; the missing piece is binding it from `plan_font` when
   `resolve_target_resource` and `std14_by_base_font` both miss. Coverage gate
   `accept_font_target` (`R221`) must run against the DONOR's cmap.
2. **Resize page contents** — a new Pass family, IDs not yet minted. Dispatch
   `pdfcer-acrobat-librarian` FIRST (rule 12). Operator's words: *"add a resize
   page contents with all the usual options (scale to fit without distortions,
   to fill page without distortion, to fill page with distortion, set custom
   size, etc)."*
3. **`Pass 10.11`** — finish B-T timestamps (RFC 3161 token as the
   `id-aa-timeStampToken` unsigned attribute; `SignRequest.reserve` grows; the
   TSA round trip is the CLI's under decision 061; flip
   `apply::check_seed_value`'s refusal of a required `/TimeStamp`).

---

## OWED, and small

- **`docs/core-api/03-capabilities.md:1181`** cites `StickyIcon` at
  `annot_author.rs:1022`; it is at `2837`. Whole-document line-citation drift,
  pre-existing, still not fixed.
- **`tools/check-requests-scoped.py`** — owed by `R242`. A citation-link check,
  never a content check. Still unbuilt.
- **`check-public-fns-documented.py`'s denominator is `pub`**, so it cannot see
  the doc-splice defect on PRIVATE functions. The fix is a **staged**
  denominator (it is red at baseline over private items), and that is its own
  change, not a rider on a release.
- **Backup bundle is ~144 commits behind `HEAD`.** Refresh when convenient.

---

## BUILD ENVIRONMENT — READ BEFORE ANY RELEASE BUILD

★★ **`target/` reached 128 GB on a disk at 92 % full, and that is the best
current explanation for the watchdog kills.** `rm -rf
target/debug/incremental` reclaimed **26 GB** (109 G free of 954 G,
`target/` 102 G). `target/` is gitignored and `git ls-files target` returns 0
— both were checked BEFORE the delete, and that check is not optional.

- **Foreground survives where background dies.** The low-memory watchdog killed
  three commands this session — a full `cargo test --workspace`, its waiter,
  and a **background** release build — and the release build then completed
  **in the foreground** at the *same* `CARGO_BUILD_JOBS=1`. This is now a
  two-session pattern. Do the expensive build in the foreground.
- **A full `cargo test --workspace` still cannot be completed here**, even at
  `CARGO_BUILD_JOBS=1` with `--no-run` first. What worked: `--no-run` to build
  (background, ~15 min, survived), then **per-target foreground runs** at
  `--test-threads=2`. All ~2,700 tests passed that way. **Do not record a
  chunked run as "full suite green"** — say what was run.
- Release build: **5 min warm, 9½ min after a tag** (the tag forces a rebuild
  of `pdfcer-render` + `pdfcer-cli` for the banner).
- `du -sh target/` at session start, every session.

## Release procedure (worked five times on 2026-09-06/07)

bump `Cargo.toml` (+ `cargo metadata` for the root and fuzz lockfiles;
`tools/content-identity/Cargo.lock` needs it WITHOUT `--offline`) → chore
commit → **librarian filing for every unfiled code commit** (the push hook
allows only the TIP unfiled) → push → **poll CI green from GitHub, never
assume** → `git tag -a vX -m … <sha>` → **rebuild** so the banner names the tag
→ `tools/package-portable.py --no-build --note "…"` → fresh-folder smoke test →
zip + sha256 via Python `zipfile` → `git push origin vX` → `gh release create`
→ `tools/deploy-onedrive.py` → `tools/verify-release.py vX` → **download the
published asset back and re-hash it** → librarian release filing → refresh
this file.

★ **Make the smoke test reproduce the fix, not merely launch the binary.**
This release's smoke test ran the operator's own reproduction in the shipped
exe in a fresh folder and got identical rectangles from both routes. That is
worth more than `--version` printing.

★ **The byte arithmetic closes, and checking it is cheap.** The portable
folder's files must sum to the reported total, and OneDrive's must equal that
minus `BUILD-INFO.txt` plus `VERSION.txt`. Both closed exactly here. The exe
grew 2,204,672 B over `v0.44.1`.

---

## ★★ WHAT THIS SESSION GOT WRONG

### I relayed a tool's WORDING instead of the SET it counted

`package-portable.py` prints `staged 3 model file(s)` and counts everything
under `models/` — **including `PROVENANCE.md`, which is not a model.** I put
"three OCR model files" in a librarian dispatch. There are **two** `.rten`
weights plus one documentation file, 8 files total.

**The librarian caught it inside the hour, and how it caught it is the lesson:
it did the arithmetic, found it one short against the previous release's
itemized list, and left it as an OPEN QUESTION rather than guessing "9".** Its
guess about the cause was wrong; raising it was right. Same family as *"a gate
that under-reports looks green"* — a summary line named a different set than
its noun implied.

### A sabotage survived because a DIFFERENT correct rule absorbed it

Disabling the artwork rectangle rule left the requester's A/B test **green** —
the test shape carries `/Vertices` and fell through to the geometry rule,
which composes too. The test was measuring *"some rule composes"* while its
name claimed the artwork one. **This is a fourth cause for a surviving
sabotage**, beside vacuous assertion / guarantee enforced elsewhere / null
mutation: an alternate, also-correct path supplying the same answer. Fix: pin
the ROUTE, not only the outcome. Filed to `D:\dev\rag\rust\`.

### A test expectation was wrong about the FIXTURE, not the code

I asserted a placed appearance edge would be "genuinely off-axis" under a
rotating `/Matrix`. It failed. The fixture's matrix is a **quarter** turn, and
a quarter turn is axis-aligned by definition — the code was right. Replaced
with a bearing check that is correct at any angle *and* cross-checks the two
halves of `Pass 155.2` against each other.

### Prose through the Bash tool broke twice, again

Two heredocs carrying commit messages died on `unexpected EOF`. **Write the
file with the Write tool and `git commit -F` it.** This is already a standing
memory and it still cost two cycles.

---

## Standing habits

- **Check BOTH FeatureRequests channels every session, by DIFF** — and grep
  ROADMAP for each request's filename before scoping it (`R242`).
- **Write a reply for every request you close.**
- Announce every new public TYPE/SIGNATURE on the channel by name.
- Anchor a doc-comment splice on the DOC BLOCK, not the item.
- Batch releases: build everything pending, then ONE release.
- **Sabotage every new test — and check what the sabotage FELL THROUGH TO.**
- Register any new report struct in `check-outcome-disclosed`'s
  `OUTCOME_STRUCTS` in the SAME commit; the gate is opt-in and prints "clean"
  about what it was not told.
- Update `docs/core-api/` in the same Pass that changes a `pub` item, and bump
  **every** stated count (verbs, `EditError` variants, per-file line/clause
  figures in `index.md`).
