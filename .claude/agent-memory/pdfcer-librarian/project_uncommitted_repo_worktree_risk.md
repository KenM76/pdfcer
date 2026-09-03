---
name: project-uncommitted-repo-worktree-risk
description: RESOLVED 2026-08-01 (commit d8b3903, then 79d1c6f + e13f3e6) — pdfce's crates/ tree is no longer uncommitted, and the engineer now commits in logical per-Pass/per-decision chunks. Branch pass-8-redaction at 82 commits as of continuation 78 (2026-08-04, SESSION-ENDING FILING), still no remote. Backup bundle current (pdfce-20260804-final.bundle, verify-clean, to HEAD) as of continuation 78. This librarian has no shell tool — commit counts/hashes are engineer-reported/operator-reported, not self-verified, unless noted otherwise.
metadata:
  type: project
---

**RESOLVED — 2026-08-01, SESSION_LOG same-day continuation 49.** The
operator authorized "commit all work." The engineer performed the
project's FIRST implementation commit: **`d8b3903`** on branch
**`pass-8-redaction`** (branched from bootstrap `67967b2`), 373 files
changed, 168,217 insertions. Working tree is now clean except
gitignored build/scratch/corpus artifacts. This retires the risk
described below: future autonomous-builder `git worktree` dispatches
now check out real, current content instead of the stale bootstrap
commit, so the "isolated workspace is effectively empty/stale" failure
mode this file documents should no longer occur. See
`D:\dev\rag\rust\autonomous_builder_worktree_isolation_uncommitted_substrate.md`
for the mechanics writeup (that file's finding about worktree
semantics remains generally true and does NOT need retraction — only
pdfce's own instance of the risk is resolved).

**What is NOT resolved:** the commit is **LOCAL ONLY, not pushed to
any remote.** `LEGAL.md` §1 (OSS license choice) is still undecided,
and project rule 8 forbids a public-facing commit posture (pushing to
a public repo, publishing a release) before that decision is made. So
there is a *new*, narrower open item — "push authorization, gated on
the license decision" — but it is materially smaller than the
retired risk: it no longer threatens worktree-dispatch correctness,
only public visibility. Future librarian sessions should track that
under the `LEGAL.md` §1 operator item (already tracked in
`SESSION_LOG.md`'s still-open-operator-items list), not revive this
file's "commit authorization" framing.

**UPDATE — license decided, continuation 50 (2026-08-01).**
`LEGAL.md` §1 is no longer undecided: the operator chose **MIT**, and
the engineer implemented it (`LICENSE` file, `Cargo.toml`
`license = "MIT"`, `license.workspace = true` on all four crates).
Project rule 8's license precondition is now satisfied. **Push
authorization is now the sole remaining gate on public visibility** —
it is NOT implied by the license decision and was not requested
alongside it; it is tracked as its own narrow, optional open item
(`SESSION_LOG.md` continuation 50's still-open-operator-items list).
Do not treat the license decision as also authorizing a push.

**Historical record below (kept for context, not for action):**

As of at least Pass 14.0 (2026-08-01), the entire pdfce workspace source
(`crates/`, `Cargo.toml`/`Cargo.lock`, `fixtures/synthetic/`, `tools/`,
`docs/decisions/`) sits **uncommitted** on top of a single bootstrap commit
(`67967b2`). This is deliberate — the operator hasn't given commit
authorization, partly because `LEGAL.md` §1 (OSS license choice) is still
undecided and a public-facing commit posture shouldn't get ahead of that.

**Why this matters to me (the librarian):** it has now caused a **recurring**
engineering cost, not a one-off — the autonomous "KenAgent"-style builder is
dispatched into an isolated `git worktree`, which checks out a *commit*, not
the uncommitted working-tree state. A worktree branched from `67967b2` can't
see any of Passes 1–13's uncommitted code, so the builder's isolated
workspace is effectively empty/stale relative to what the orchestrating
session actually has on disk. This has bitten multiple Pass dispatches (see
`D:\dev\rag\rust\autonomous_builder_worktree_isolation_uncommitted_substrate.md`
for the full mechanics writeup). The workaround each time is instructing the
builder that the main cwd is authoritative and to write its deliverable
there directly — a per-dispatch instruction, not a fix.

**How to apply:** every time I file a Pass-shipped or pre-compaction-capture
entry, keep surfacing "commit authorization" in the still-open operator
items list (oldest-first ordering, per the existing SESSION_LOG convention)
— don't let it quietly drop off just because it's been repeated many times.
As of SESSION_LOG continuation 39 (2026-08-01, decision 015 filed) this item
was explicitly called "now especially pointed" given the tree's continued
growth (decision 015's reflow work, Pass 15.x) — keep escalating the framing
as the uncommitted tree keeps growing, don't just repeat the same wording.
As of continuation 42 (2026-08-01, Pass 15.2 shipped — FF-A/decision 015
COMPLETE end-to-end, on top of the already-COMPLETE decision 014) the framing
escalated again: the tree now holds TWO full, complete, multi-Pass
subsystems (decision 014 in-place editing + decision 015 reflow) sitting
entirely uncommitted — "the largest uncommitted span in the project's
history to date." Keep escalating in concrete terms (what subsystems/Passes
are now at risk) rather than a generic repeat, every time this item is
re-surfaced.
When an engineer flag mentions a worktree/builder-dispatch anomaly, check
first whether it's this same recurring cause before treating it as a new
finding. If the operator ever grants commit authorization, that's the
structural fix (worktrees checkout HEAD cleanly once HEAD actually reflects
current state) — note that explicitly in whatever session log entry records
the commit actually happening, since it retroactively resolves this risk.
As of continuation 46 (2026-08-01, Pass 16.1 shipped — decision 016/FF-D
now TWO THIRDS complete, only the 16.2 canvas-UI slice remaining) the tree
holds FOUR complete/near-complete subsystems uncommitted end-to-end:
decision 014 (in-place editing), decision 015 (reflow), Pass 14.4
(GUI-polish interaction set), and decision 016/FF-D's point+boxed
new-text engine (16.0+16.1). Keep escalating in concrete, subsystem-named
terms each time this re-surfaces — the framing should track exactly which
completed subsystems are at risk, not just repeat "the tree grew again."
As of continuation 47 (2026-08-01, Pass 16.2 shipped — decision 016/FF-D
now COMPLETE end-to-end) the tree holds the ENTIRE Acrobat text-handling
parity arc uncommitted: decision 014 (in-place editing, Pass 14.0-14.4),
decision 015 (reflow, Pass 15.0-15.2), and decision 016/FF-D (add-new-text,
Pass 16.0-16.2) are all COMPLETE and all uncommitted — the largest span
yet, and now framed as "a whole completed milestone with zero of it in
version control" rather than just "another subsystem added." Use that
framing (completed-milestone-sized risk) the next time a multi-Pass
decision closes out while still uncommitted.
As of continuation 48 (2026-08-01, FF-D follow-up hardening shipped —
certification-signature guard on `add_text`/`EditSession::add_text`,
closing the last flagged gap in the text-parity arc) the risk escalated
once more, in a distinct way from prior continuations: this wasn't a new
subsystem, it was the CLOSING FIX on an already-complete arc, meaning the
uncommitted tree now holds a genuinely *finished-and-hardened* milestone
(zero known open threads) with zero of it in version control. Also newly
relevant: the autonomous `/loop` was throttled to an idle heartbeat at
this same continuation (awaiting operator steer, no longer spawning
feature work) — so the uncommitted-tree risk is no longer actively
growing on its own; the next growth trigger is an operator decision
(FF-C unblock, list-authoring, or a new steer), not autonomous
continuation. Keep noting this "growth has paused, but nothing shrank"
framing until either a commit happens or new autonomous work resumes.

**UPDATE — continuation 51 (2026-08-01, Pass 9a shipped).** A SECOND
logical commit, **`e13f3e6`**, landed on top of `79d1c6f` (the
MIT-license-artifacts commit, itself on top of `d8b3903`). The engineer
is now committing shipped work in logical per-Pass/per-decision chunks
(license artifacts, then Pass 9a) rather than repeating the single
large tree-wide commit from continuation 49 — note this cadence change
in future entries rather than assuming a return to one-giant-commit
behavior. All three commits (`d8b3903`, `79d1c6f`, `e13f3e6`) remain
**local-only** — push/publish authorization is still a separate,
not-yet-granted operator item, unaffected by either commit or by the
MIT decision.

**UPDATE — continuation 52 (Pass 12.M1) then 53 (Pass 12.M2), both
2026-08-01.** The per-Pass commit cadence held: `19ed865` (docs) and
`801a748` (Pass 12.M1) landed at continuation 52; `c7c1744` (Pass
12.M2) landed at continuation 53. Current chain, six commits deep, all
**local-only**: `d8b3903` → `79d1c6f` (MIT) → `e13f3e6` (Pass 9a) →
`19ed865` (docs) → `801a748` (Pass 12.M1) → `c7c1744` (Pass 12.M2).
Push/publish authorization remains the sole open gate, unchanged in
kind since continuation 50 — only the chain depth keeps growing. When
recording a future commit, verify the current tip with `git log
--oneline -1` rather than trusting this memory's hash list, since it
will keep growing across sessions and this file updates lag behind the
actual repo.

**UPDATE — continuation 54 (2026-08-01, Pass 12.M2b shipped).** Cadence
held again: `6150e1a` (docs) and `7c93cc3` (Pass 12.M2b) landed on top
of `c7c1744`. Chain now eight deep, all **local-only**: `d8b3903` →
`79d1c6f` (MIT) → `e13f3e6` (Pass 9a) → `19ed865` (docs) → `801a748`
(Pass 12.M1) → `c7c1744` (Pass 12.M2) → `6150e1a` (docs) → `7c93cc3`
(Pass 12.M2b). This commit closes out the decision-011 dimensioning-tool
GUI milestone (see `project_mit_license_and_priority_sequence.md`'s
continuation-54 update) — same "completed-milestone-sized" framing as
continuation 47's text-parity-arc note applies here too. Still verify
tip with `git log --oneline -1` before trusting any hash list here.

**UPDATE — continuation 55 (2026-08-01, Pass 9c-min shipped) then
continuation 56 (2026-08-02).** Continuation 55 added `2abbd75` (test
hygiene), `dd3a8b8` (docs backfill), `76485b5` (Pass 9c-min — closes
decision 011's beta entirely) and a docs commit `0569373`. Continuation
56 (a NEW calendar-day session per `feedback_session_log_continuation_style`)
added SIX more: `9a68d6f` (Pass 18.0 — note: this was WRONGLY recorded
as "uncommitted" in the librarian's own continuation-56-opening filing;
corrected same continuation once the engineer clarified it was in fact
committed — a real instance of the "verify current git state, don't
trust the last filing" discipline this file exists to enforce),
`3a56b55` (Pass 17.0, live-edit rendering), `f2d5fae` (GUI observation
harness), `c998521` (selection-outline fix), `dae0139` (Pass 18.2
`object-list` CLI), `b73604d` (`.gitattributes` repo-integrity fix).
**Chain now 18 commits deep, all still local-only:** `d8b3903` →
`79d1c6f` → `e13f3e6` → `19ed865` → `801a748` → `c7c1744` → `6150e1a`
→ `7c93cc3` → `2abbd75` → `dd3a8b8` → `76485b5` → `0569373` →
`9a68d6f` → `3a56b55` → `f2d5fae` → `c998521` → `dae0139` → `b73604d`.
Push/publish authorization is STILL the sole open gate — confirmed
still not granted as of continuation 56 despite the operator answering
two other open questions (icon pipeline, Pass-17-sequencing) the same
continuation; don't infer push authorization from unrelated operator
answers. As always, verify tip with `git log --oneline -1` before
trusting this hash list — it will keep growing across sessions.

**UPDATE — continuation 57 (2026-08-02) then continuation 58 (real date
2026-08-03).** Continuation 57 added `f9bb560` (docs) and `c59b0c4`
(Pass 18.3, icon set) — chain 20 deep. Continuation 58 added EIGHT more:
`85a6cac` (docs: glyph audit) → `437a6f7` (Pass 17.1+17.2 — decision 018
COMPLETE; the R85 oracle's first run found real silent data loss in
`flatten_fields`) → `a1badc1` (chevron fix) → `d15c360` (harness
hardening) → `eeadbcb` (docs: glyph verify) → `f963895` (Pass 18.1,
`egui_tiles` dock — decision 017's numbered engineering slices all now
complete) → `3f6f5ae` (canvas hit-test coordinate fix, third root cause
of "can't click objects") → `869d891` (chevron fix, closes the glyph
class). **Chain now 28 commits deep, all still local-only:** `d8b3903`
→ `79d1c6f` → `e13f3e6` → `19ed865` → `801a748` → `c7c1744` → `6150e1a`
→ `7c93cc3` → `2abbd75` → `dd3a8b8` → `76485b5` → `0569373` → `9a68d6f`
→ `3a56b55` → `f2d5fae` → `c998521` → `dae0139` → `b73604d` →
`f9bb560` → `c59b0c4` → `85a6cac` → `437a6f7` → `a1badc1` → `d15c360`
→ `eeadbcb` → `f963895` → `3f6f5ae` → `869d891`. Push/publish
authorization remains the sole open gate, unchanged in kind, still not
granted. Verify tip with `git log --oneline -1` before trusting this
hash list.

**UPDATE — continuation 59 (2026-08-03, Pass 18.4 + `ui-strings` gate
fix) through continuation 62 (2026-08-03, decision 019 filed).**
Continuation 59 added `be62e48` (Pass 18.4) → `a5d1d18` (`ui-strings`
gate fix), chain 30 deep. Continuation 60 added FOUR more: `25b4783`
(docs: commit-chain self-correction) → `d296666` (fix: Pass 18.4
disclosure text) → `9998a6b` (Pass 18.5) → `6a6a48f` (observation-
harness client-rect fix), chain 35 deep on the implementation side / 36
on the branch total. Continuation 61 added `1b38e34` (Pass 18.6 —
text-bbox geometry fix, the fourth and last named cause of "can't click
on objects"), branch total 37. Continuation 62 (this update, pure
docs/decision-filing, no code) added TWO more, both engineer-verified
via `git cat-file -t` per R87: `67f49bb` (ui-spec §0.2/§B.3 marked
historical) → `743e463` (decision 019's record,
`docs/decisions/019-ffh-spacing-scaling-synthetic-styles.md`). **Branch
total now 39 commits, still all local-only, still no git remote
configured at all.** Backup bundle refreshed at continuation 61:
`D:\Dev\pdfce-backups\pdfce-20260803-0830.bundle` (supersedes the
earlier same-day `...20260803.bundle`); unchanged at continuation 62
(no new build artifact to re-verify against — a docs-only session).
Push/publish authorization remains the sole open gate, unchanged in
kind since continuation 50. As always, verify tip with `git log
--oneline -1` before trusting this hash list — do not assume this
memory's chain is current without checking, especially across a
calendar-day boundary.

**UPDATE — continuation 63 through 67 (2026-08-03).** Pass 19.0/19.1/
19.2/19.3 all shipped, plus the `Tw` census (`tools/tw-census`,
commits `359d486`/`5387699`, both verified by `git cat-file -t`) and
decision 019 Amendments B/C/D/E filed. **Branch total now 54 commits**
(`git rev-list --count HEAD`, continuation-67 filing), still all
local-only, still no git remote configured. Per-commit hash chain no
longer tracked exhaustively in this file past continuation 62 — the
list was becoming a maintenance burden of its own (a stale hash list
is worse than no hash list); **verify the current count with `git
rev-list --count HEAD` and the tip with `git log --oneline -1` every
time**, treating any specific number in this file as a point-in-time
snapshot, not a running total to trust. Push/publish authorization
remains the sole open gate, unchanged in kind since continuation 50.
A newly-found, in-progress item as of continuation 67: 341 corpus
files are unopenable on a `/Contents`-resolves-to-Null defect (fail-
clean violation) — a builder is fixing it; once it ships it will be
the next commit(s) to fold into this chain.

**UPDATE — continuation 68/69 (2026-08-03).** The `/Contents` defect
fix shipped (`409a6b5`, 289 files recovered, new standing rules
R94–R95) and Pass 19.4 (`Tw`) shipped (`a1638f4`), closing decision
019/FF-H end-to-end with Amendment F (R96). Branch reached 58 commits
by continuation 69, still local-only, still no remote.

**UPDATE — continuation 70 (2026-08-03, Pass 8.1 — GUI redaction-apply
flow shipped, `9a68999`).** Two hashes this continuation, both
verified by `git cat-file -t`: `24bdbc6`, `9a68999`. **Branch total now
60 commits**, still all local-only, still no git remote configured —
the gap since continuation 69 (58) is exactly the 2 new hashes,
consistent (no missing-commit flag needed this time). Push/publish
authorization remains the sole open gate, unchanged in kind since
continuation 50. Backup bundle NOT regenerated this continuation —
still stale relative to at least continuations 63 onward; flag to the
operator, don't assume it covers current HEAD. As always, verify tip
with `git log --oneline -1` and count with `git rev-list --count HEAD`
before trusting any number in this file.

**UPDATE — continuation 71/72 (2026-08-03, Pass 18.7 shipped; checker
+ FF-C classification filed).** Branch reached 62 commits by
continuation 71 (backup bundle refreshed same continuation,
`pdfce-20260803-1936.bundle`, current to `d9960cd`). Continuation 71
added `09be28d` (Pass 18.7), `d9960cd` (decision 020 filed), `1111652`
(Pass-number correction commit) — all three verified via `git
cat-file -t`. Continuation 72 (this librarian dispatch) confirms two
further hashes named by the dispatching engineer as already
committed: `4dc8cf8` (`tools/check-ledger-numbers.py`) and `d738950`
(`PRIOR_ART.md` FF-C classification + `CLAUDE.md` XFA fix) — **this
librarian could not independently verify either with `git cat-file -t`
this dispatch, having no shell-execution tool available** (tools:
Read/Write/Edit/Glob/Grep/WebSearch/WebFetch only); treat those two
hashes as engineer-reported, not librarian-verified, until a future
session with `git` access confirms them. Push/publish authorization
remains the sole open gate, unchanged in kind since continuation 50.
Verify current tip/count with `git log --oneline -1` / `git rev-list
--count HEAD` before trusting any number in this file — likely ~64+
commits by the time this is read.

**UPDATE — continuation 73 (2026-08-03, decision 021 filed: FF-C font
subsetting/glyph embedding, DECIDED/SCOPED/NOT STARTED).** Branch
`pass-8-redaction` now **66 commits**, still no remote — both figures
**dispatching-engineer-reported**, not librarian-verified: this
librarian dispatch again had no shell-execution tool available
(tools: Read/Write/Edit/Glob/Grep/WebSearch/WebFetch only), same
constraint as continuation 72. The engineer reports six hashes spanning
the range spot-verified with `git cat-file -t` on their side (all
confirmed `commit` objects): `d30842c` (decision 021 + the
ledger-checker's mentioned-but-unheaded fix), `4dc8cf8`
(`tools/check-ledger-numbers.py` — only engineer-reported, not
librarian-verified, as of continuation 72; now engineer-confirmed),
`d738950` (`PRIOR_ART.md` FF-C classification, also now
engineer-confirmed), `1111652` (Pass-number correction commit),
`d9960cd` (decision 020), `09be28d` (Pass 18.7). Treat all six as
engineer-verified, not independently librarian-verified, until a
future librarian dispatch has `git` access itself. Per-commit hash
listing stays non-exhaustive past continuation 62's count (see that
update) — always re-run `git rev-list --count HEAD` / `git cat-file -t`
rather than trusting this file's numbers past their filing date.
Push/publish authorization remains the sole open gate, unchanged in
kind since continuation 50.

**UPDATE — continuation 74 (2026-08-03, decision 021 spec-review
amendment: FF-C P0 floor narrowed to `glyf` donors, R109 split into two
refusals — no new Pass shipped, docs-only).** One new hash this
continuation, engineer-reported and stated as `git cat-file -t`-verified
on the dispatching side: `0893191` (fix: two false operator-facing font
hints, `r_inv_1_hint()`/`format_coverage_hint()`, corrected to stop
promising a remedy the write path doesn't deliver). Carried forward from
continuation 73: `d30842c`. **Branch `pass-8-redaction` now 67 commits,
still no remote** — this librarian dispatch again had no shell-execution
tool (Read/Write/Edit/Glob/Grep/WebSearch/WebFetch only) and could not
independently verify either hash. `tools/check-ledger-numbers.py`
reported GREEN, exit 0, per the dispatching message. Push/publish
authorization remains the sole open gate, unchanged in kind since
continuation 50. Verify current tip/count with `git log --oneline -1` /
`git rev-list --count HEAD` before trusting any number in this file.

**UPDATE — continuation 75 (2026-08-04, Pass 21.0 shipped: FF-C P0
floor, pdfce can now add non-Latin text to a PDF).** Six new hashes
this continuation, `88b9487`→`0c4f490`→`d4e7355`→`5b7bed3`→`eb0bde5`
→`48c6b77` — **for the first time, verified by the OPERATOR directly
with `git cat-file -t`** (per the dispatching message), not merely
engineer-relayed. This librarian still has no shell-execution tool
(Read/Write/Edit/Glob/Grep/WebSearch/WebFetch only) and could not
independently re-verify. **Branch `pass-8-redaction` now 74 commits,
still no remote.** Backup bundle refreshed:
`D:\Dev\pdfce-backups\pdfce-20260804-0015.bundle`, `git bundle
verify`-clean, supersedes `...1936.bundle`. Push/publish authorization
remains the sole open gate, unchanged in kind since continuation 50.
Verify current tip/count with `git log --oneline -1` / `git rev-list
--count HEAD` before trusting any number in this file.

**UPDATE — continuation 76 (2026-08-04, R109 fsType-read closed,
R110's primitive shipped, R-INV-4 reachability fix).** Five hashes
`58fe3f6`/`c0ed638`/`8e08e80`/`87d3cb0`/`6b69956`, operator-verified
with `git cat-file -t` again (second time this project has had direct
operator verification, not just engineer relay). **Branch reached 79
commits by continuation 76's filing** (flagged then as "stale by two
commits" against the backup, since the bundle at that point was
`...0015.bundle`, only 74-commit-deep).

**UPDATE — continuation 77 (2026-08-04, librarian-only, no code) —
BOTH carried-forward gaps from continuation 76 discharged.** (1)
**Repo/backup state independently re-verified**, not merely
re-derived: still 79 commits (matches continuation 76's count exactly
— no drift), still no remote. Backup bundle **refreshed and
verify-clean**: `D:\Dev\pdfce-backups\pdfce-20260804-0325.bundle`,
current to `6b69956` — the "stale by two commits" flag from
continuation 76 is now discharged. (2) The `ARCHITECTURE.md` §3/§4
body-section sync for Pass 21.0's `pdfce-render::font::subset`/
`pdfce-core::font_embed` modules — owed since continuation 75, carried
through 76 — is now DONE (§3 crate-layout notes, a full §4 IMPLEMENTED
entry, and a dated §12 decision-log entry closing the gap). Push/
publish authorization remains the sole open gate, unchanged in kind
since continuation 50. As always, verify tip/count with `git log
--oneline -1` / `git rev-list --count HEAD` before trusting any number
in this file past its filing date.

**UPDATE — continuation 78 (2026-08-04, SESSION-ENDING FILING).** Three
new commits this continuation: `31d2fdc` (`ShowSlot::code` widened
`u8`→`u32` + per-slot `width`) and `b98589a` (`CompositeEncoding`
shipped) — both independently `git cat-file -t` verified by the
operator as `commit` objects — plus a third commit at HEAD (fixture
`composite-editable.pdf` + the four-item wiring survey written into the
code), which is recorded as **"HEAD at session end," not a specific
hash string** — the operator confirmed the commit COUNT (82) but did
not separately verify a hash for that one commit this filing, and this
librarian has no shell tool to self-verify. **Branch `pass-8-redaction`
now 82 commits, still no remote.** Backup bundle refreshed to
`D:\Dev\pdfce-backups\pdfce-20260804-final.bundle`, `git bundle
verify`-clean, current to HEAD — supersedes `...0325.bundle`
(continuation 77). Full test/lint state also re-confirmed this
continuation: 1806 tests passing; `cargo fmt --check`, `cargo clippy --
-D warnings`, `tools/check-ui-strings.sh`, `tools/check-ledger-
numbers.py` all clean; `cargo tree` GUI-free on both `pdfce-core` and
`pdfce-render`. Push/publish authorization remains the sole open gate,
unchanged in kind since continuation 50. **This is the session-ending
filing** — the next session should re-verify tip/count/bundle currency
before trusting any of these numbers, per the standing discipline in
every prior update in this file.
