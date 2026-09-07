# NEXT_SESSION.md — engineer handoff

**Read this FIRST on resume**, then the latest `docs/SESSION_LOG.md` entry for
detail. This file is engineer-owned (write it directly; it is NOT a librarian
doc). It is replaced each session with the current handoff.

**Written:** 2026-09-07, at the operator's instruction (*"make a handoff file so
I can continue in a new session"*), immediately after releasing `v0.44.1`.
**All work is PAUSED** at his instruction. Nothing is in flight.

---

## STATE

**Released: `v0.44.1`** at `667325b` — GitHub release with the `windows-x64`
zip + `.sha256`, OneDrive slot **`pdfcer1`** (`pdfcer2` keeps `0.44.0` as the
previous version), `verify-release.py v0.44.1` **nine of nine on a clean
tree**, and the published asset **downloaded back from GitHub and re-hashed**
(`2848e636…`, matches — the link works).

`v0.44.0` was released earlier the same session at `e1bdb6c`.

**Nothing unreleased on `main`. Working tree clean. `origin/main` == `HEAD`.**

Workspace version **`0.44.1`**. Ledger: filings **462**, Pass ceiling
**`258.3`**, decisions **139**, rules **R243**.

### What shipped in the two releases

`v0.44.0` cleared **every open `pdfcer-gui` request** at that moment:
Passes `252.0` (text file → PDF, `place_text`, plus the `blank_document`
primitive — nothing in the crate could CREATE a page before, only copy one),
`253.0`–`253.3` (comment replies, review status, sticky-note icon/colour,
pop-up `/Open`), `254.1` (hairline stroke disclosure).

`v0.44.1` is `Pass 253.5` — two defects **in `253.2`**, reported by
`pdfcer-gui` within hours of `0.44.0`.

---

## ★ THE QUEUE — THREE OPEN REQUESTS, AND THE OPERATOR REPORTED THE FIRST HIMSELF

Standing operator instruction (2026-09-06): **feature requests get top
priority, and a release once they are all cleared.** These are the top of the
next session by that rule.

### 1. `Pass 155.1` — rotate grows the artwork on repeat. **THE OPERATOR'S OWN REPORT.**

`open/request_rotate_annotation_grows_the_artwork_when_applied_twice.md`,
filed 2026-09-07 against `e1bdb6c`, with a reproduction. His words, unprompted:

> *"the rotate bug in the review objects where the object gets larger with each
> enactment of the tool."*

★ **`rotate-annotation`'s own help and two doc comments currently say the
opposite** — *"the artwork does not grow; only the rectangle around it does"*.
That sentence is **correct for ONE rotation of an unrotated annotation**; what
is disputed is the SECOND, where `/Rect` — already grown — appears to be taken
as the artwork to re-bound. In `667325b` all three copies are **flagged as
under investigation rather than deleted**, because the mechanism is not yet
measured and a disclosure that quietly disappears is worse than one that says
it is in doubt. **When you fix this, fix those three sentences in the same
commit** (`edit.rs` ×2, `main.rs` ×1 — grep `artwork does not grow`).

### 2. `Pass 155.2` — an annotation's rotation angle cannot be READ

`open/request_an_annotations_rotation_angle_cannot_be_read.md`. Prompted by the
operator: *"the angle should be editable from the properties, and the box
outlined when an object is selected should be in the same angled orientation as
the object."*

★ **The librarian minted both into Backlog under `rotate_annotation`'s origin
family (`Pass 155.0`) and flagged that they INTERLOCK** — an absolute
angle-setter has to derive `/Rect` from the artwork, which is `155.1`'s fix.
**Scope them together.**

### 3. Then the operator's earlier ordered plan (2026-09-06), untouched

1. **`Pass 142.0`** — the embedded-donor half of FF-C: `format-text --set-font`
   to a face neither on the page nor standard-14, subsetting and embedding a
   donor supplied via `--font-dir`. Entry ~`docs/ROADMAP.md` line 116195.
   `add_text --embed-font` already does the subset+embed for NEW text; the
   missing piece is binding it from `plan_font` when `resolve_target_resource`
   and `std14_by_base_font` both miss. Coverage gate `accept_font_target`
   (R221) must run against the DONOR's cmap.
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
  pre-existing.
- **`tools/check-requests-scoped.py`** — owed by `R242` (see below). A
  citation-link check, never a content check.
- **`check-public-fns-documented.py`'s denominator is `pub`**, so it cannot see
  the doc-splice defect on PRIVATE functions — which is how the fourth instance
  got through this session. The fix is a **staged** denominator (it is red at
  baseline over private items), and that is its own change, not a rider on a
  release.

---

## ★★ WHAT THIS SESSION GOT WRONG — read this before scoping anything

**Three of these cost real time and one shipped a defect to the operator.**

### `R242` — an audit that lists `open/` re-scopes work that is already scoped

I audited the request channel by **listing `open/`** and treating everything
there as outstanding. **Four of the six had already been minted as
`Pass 253.0`–`253.3` the previous day**, from the same request files, naming
the same verbs by signature. Minting new IDs would have stranded four Backlog
entries describing shipped code.

**A request does not leave `open/` when it is SCOPED — only when it is
ANSWERED.** Before scoping anything from that folder, grep `docs/ROADMAP.md`
for the request's filename. It takes seconds; it worked **28 minutes** after
the rule was minted, on `Pass 252.0`.

### `R243` — a documented obligation on a future caller is not a control

`set_text_annot_style` shipped un-wrapping text boxes because it re-baked from
a reader field documented as unsafe to bake from. **I wrote that warning
myself, the same day, in the channel reply that shipped the verb one along** —
*"ALWAYS false and you must not believe it … if you ever re-author a
`/FreeText` yourself, you have to do that too"* — and then wrote exactly that
code an hour later.

The rule was minted at n=1 because the instance **refutes every mitigation by
construction**: the warning was written down, emphatic, zero days old, and its
author wrote the violating code. When two call sites must agree about a value
neither can read from the file, **the agreement goes in one function they both
call**, not in a comment.

### The doc-comment splice — FOURTH instance, and I did it while fixing the class

Inserting a function between another function and its doc block fuses two doc
blocks onto one item and leaves the other with none. **Anchor on the DOC BLOCK,
or insert after a closing brace.** `check-clap-help` and `check-cli-help-leads`
caught the clap instance (it had silently stolen `SetMarkupStyle`'s `--help`);
nothing caught the private-function instance — see OWED above.

### Gates that are OPT-IN and say "clean" about what they were not told

`check-outcome-disclosed` has a hand-maintained `OUTCOME_STRUCTS` list. A new
report struct is **invisible** to it until registered, and the summary line
still reads `clean`. Register in the same commit. Same shape:
`check-ledger-numbers` did not read `docs/core-api/` until `e1bdb6c` widened
it, so a `Pass 259.x` citation for an ID that was never minted sat on disk
while the gate printed *"MENTIONED: up to 258"*.

---

## BUILD ENVIRONMENT — READ BEFORE ANY RELEASE BUILD

★★ **The harness's low-memory watchdog killed things repeatedly this session**
— the gate sweep four times, the release build twice — on a machine showing
5–7 GB free of 15.9 GB.

- **`CARGO_BUILD_JOBS=2` is NOT enough.** `CARGO_BUILD_JOBS=1` completed the
  release build (7–9 min); `-- --test-threads=4` (or 2) completed the suite.
  `CARGO_BUILD_JOBS` caps *compilation*; `cargo test` peaks in the **run**
  phase at `--test-threads` = core count.
- **`cargo test --workspace --no-run` first**, then run — the run phase alone
  is cheap and survives.
- **A full-parallel `cargo test --workspace` reported two `pdfcer-cli`
  dimension-group tests as failing.** They pass in isolation and under bounded
  parallelism. **That is a resource collision, not a defect** — a starved run
  looks exactly like a broken one.
- `tools/run-gates.sh` as one process could not be completed at all. Drive the
  derived list in chunks, **every command, none omitted**, and note that
  `check-ci-parity.py --list` and `run-gates.sh --list` **both print 29 and are
  different sets** (symmetric difference 4; the parity list omits
  `check-history-not-rewritten.py`).
- **`du -sh target/` at session start.** It reached **93 GB**; `/d` is down to
  **121 G free of 954 G**. `rm -rf target/debug/{deps,incremental}` reclaimed
  73 GB earlier today. The next build after a prune is COLD and slow — expected.

## Release procedure (worked four times on 2026-09-06/07)

bump `Cargo.toml` (+ `cargo metadata` for the root and fuzz lockfiles;
`tools/content-identity/Cargo.lock` needs it WITHOUT `--offline`) → chore
commit → **librarian filing for every unfiled code commit** (the push hook
allows only the TIP unfiled) → push → **poll CI green from GitHub, never
assume** → `git tag -a vX -m … <sha>` → **rebuild** so the banner names the tag
→ `tools/package-portable.py --no-build --note "…"` → fresh-folder smoke test →
zip + sha256 via Python `zipfile` → `git push origin vX` → `gh release create`
→ `tools/deploy-onedrive.py` → `tools/verify-release.py vX` → librarian release
filing → refresh this file.

## Standing habits

- **Check BOTH FeatureRequests channels every session, by DIFF** — and grep
  ROADMAP for each request's filename before scoping it (`R242`).
- **Write a reply for every request you close.** A request with no reply looks
  open forever and hands the next audit the same collision.
- Announce every new public TYPE/SIGNATURE on the channel by name.
- Anchor a splice on the DOC BLOCK, not the item.
- Batch releases: build everything pending, then ONE release.
- Sabotage every new test — and where possible against the **shipped** bug, not
  merely a mutation. Two tests this session passed with the feature disabled
  until a positive control was added beside them.
