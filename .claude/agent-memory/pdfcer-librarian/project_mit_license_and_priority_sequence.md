---
name: project-mit-license-and-priority-sequence
description: pdfce's OSS license is DECIDED (MIT, 2026-08-01); operator's four-item priority sequence — dimensioning (DONE) → icons (DONE) → text-handling (item #3: FF-H DONE, FF-C Pass 21.0 SHIPPED (glyf/TrueType donors, add-only) + R109 fsType-read + R110 primitive + R-INV-4 reachability fix all SHIPPED — Pass 21.1 (composite-run editability) substrate SHIPPED continuation 78 (ShowSlot::code widened, CompositeEncoding), WIRING itself deliberately NOT started, surveyed as 4 coupled changes; do NOT call FF-C or Pass 21.1 done; FF-B open) → forms (item #4, still undispatched). Pass 8.1 (GUI redaction-apply) SHIPPED. Decision 021 (FF-C) filed 2026-08-03, add-only re-scope, ceiling still R110. Continuation 78 (2026-08-04, SESSION-ENDING): 82 commits, backup bundle final for the session; a new RAG finding filed (regression test disarmed by an incidental-property change); four operator decisions consolidated into one list in SESSION_LOG for Ken's return.
metadata:
  type: project
---

**2026-08-01, SESSION_LOG continuation 50.** Two facts landed together
in one operator instruction:

1. **`LEGAL.md` §1 license decision: MIT.** Implemented same session —
   repo-root `LICENSE` (standard text, "Copyright (c) 2026 Ken
   Mantle"), `license = "MIT"` in `Cargo.toml` `[workspace.package]`,
   `license.workspace = true` on all four member crates. Dependency
   audit: 100% permissive, zero copyleft — MIT requires no dependency
   rework. Consequence: GPL/AGPL prior art (MuPDF, Poppler,
   Ghostscript) is now categorically, permanently excluded as a real
   dependency (was already the practical posture, now locked in).
   Project rule 8's license precondition is satisfied — but **this
   does NOT authorize pushing** the existing local commit (`d8b3903`)
   or publishing; that's a separate, still-open operator item.

2. **Four-item priority sequence** (verbatim: *"get the dimensioning
   tool completely functional in the gui interface. add
   d:/dev/scriptree style icons for all gui features. finish off all
   the text handling stuff. work on form building tools after if that
   makes sense."*):
   1. Dimensioning tool → completely functional in GUI. Promoted to
      **ACTIVE**. State at the time: only Pass 12.0 (canvas substrate,
      uninhabited) shipped; decision 011's remaining slices — 9a, 12.M1,
      12.M2, 9c-min — not built. Pass 9a dispatched to build.
      **UPDATE (continuation 51, 2026-08-01):** Pass 9a (object/
      selection model + centerline) SHIPPED and committed (`e13f3e6`);
      Pass 12.M1 (snapping) now in progress. 12.M2/9c-min remain. A
      marquee-vs-pan UX flag from 9a is owed a `pdfce-ui-specialist`
      review at the 12.M1/12.M2 stage.
      **UPDATE (continuation 52, 2026-08-01):** Pass 12.M1 (snapping
      engine + fuzzy snap indicator) SHIPPED and committed (`801a748`,
      on top of `19ed865` docs / `e13f3e6` 9a / `79d1c6f` MIT /
      `d8b3903` impl — 3 of 5 beta slices done). The marquee-vs-pan
      flag is now RESOLVED (kept — dimension tools use click-A-then-
      click-B, no conflict with marquee-drag). **Pass 12.M2**
      (dimensioning + scale/group + hybrid storage + OCG layer) is
      now in progress, dispatched same continuation; **9c-min**
      remains after it as the beta's last slice.
      **UPDATE (continuation 53, 2026-08-01):** Pass 12.M2 (dimension-
      ing + scale/group + hybrid storage + OCG layer, the headline
      capability) SHIPPED and committed (`c7c1744`, chain now
      `d8b3903`/`79d1c6f`/`e13f3e6`/`19ed865`/`801a748`/`c7c1744`, six
      deep, all local-only). Dimensions fully authorable via CLI,
      disclosed in GUI, but on-canvas click-to-author was deliberately
      DEFERRED to a new engineer-assigned slice, "Pass 12.M2b"
      (on-canvas dimension authoring), now building — that's the slice
      that actually delivers "completely functional in the GUI." 9c-min
      still last, after 12.M2b. Also this continuation: the ScripTree
      icon DESIGN (priority #2) is now complete
      (docs/ui_specs/icon-set-and-toolbar.md, 27 controls, redaction
      solid-fill exception) though its BUILD hasn't started — two
      decisions named as operator/KenAgent-gated before that build is
      scoped: (a) SVG-in-egui pipeline (pre-rasterize PNG, no dep, vs.
      resvg/usvg, MPL-2.0, rule-13 sign-off needed), (b) ScripTree icon
      provenance/licensing confirmation before bundling into MIT pdfce
      (likely fine, Ken owns both, but must be confirmed not assumed).
      Neither decision made yet — check current state before assuming
      either is resolved.
      **UPDATE (continuation 54, 2026-08-01):** Pass 12.M2b (on-canvas
      dimension authoring gesture) SHIPPED and committed (`7c93cc3`,
      chain now `d8b3903`/`79d1c6f`/`e13f3e6`/`19ed865`/`801a748`/
      `c7c1744`/`6150e1a`/`7c93cc3`, eight deep, all local-only).
      **MILESTONE: priority #1 ("dimensioning tool completely functional
      in the GUI") is now SUBSTANTIALLY MET** — dimensions fully
      authorable both via CLI and on-canvas (click-A/click-B linear,
      pick-set+fit circular, reference-line+dialog scale, dimension-
      groups panel). **Only 9c-min (basic vector editing) remains** of
      decision 011's five originally-named slices, now IN PROGRESS.
      **Both icon-build gated decisions RESOLVED this continuation** by
      direct operator answer: (a) pre-rasterize SVGs to PNG at build
      time, zero new dep, `resvg`/`usvg` rejected; (b) use ScripTree's
      own SVGs where they fit, draw new ones in-style otherwise,
      resemble Inkscape/Adobe visual CONVENTIONS (not their artwork) for
      new icons — Ken's own confirmation of ownership/intent, verbatim
      captured in `ROADMAP.md`'s Icon-set entry and `SESSION_LOG.md`
      continuation 54. Icon BUILD is now unblocked but still queued
      behind 9c-min. Also this continuation: the pre-existing
      integration-test temp-path-collision flake (Backlog, filed at
      Pass 9a) was independently observed a second time during 12.M2b —
      a bounded fix (thread-unique temp paths) is now dispatched,
      Backlog entry amended "FIX IN PROGRESS."
   2. ScripTree-style SVG icons for all GUI features (styled after
      `D:\Dev\ScripTree\icons\*.svg`) — new, unscoped Backlog item,
      queued behind #1.
   3. Finish text-handling: FF-B, FF-H, FF-C all now schedulable
      (FF-C's license/rule-8 gate specifically lifted by item 1 above).
      **List-authoring is a SEPARATE, still-unanswered scope question**
      — this instruction does NOT resolve it; don't conflate the two.
   4. Form-building tools (field CREATION/authoring — distinct from the
      shipped Pass 7.0/7.1 fill/flatten subsystem) — queued last,
      operator's own hedge ("if that makes sense") noted verbatim.

**Why this matters:** this is the current top-level work-order for the
whole project as of 2026-08-01 — any future librarian dispatch
("what's next", "roadmap update", "pre-compaction capture") should
check `ROADMAP.md`'s "★★★ Operator priority sequence" block (top of
"Next up") for the live, authoritative version of this before assuming
anything from an older session's framing (e.g. the pre-2026-08-01
"text-parity arc awaits an operator decision" framing is now
superseded — the decision arrived).

**How to apply:** when asked to add a new Backlog/Pass entry or judge
sequencing, respect this four-item order unless the operator gives a
new explicit steer. Don't let a lower-priority item (icons, text-
handling, forms) get scheduled ahead of the dimensioning tool without
a fresh operator instruction to reorder. See also
[[project-loop-throttled-awaiting-steer]] (the steer this sequence
represents) and [[project-uncommitted-repo-worktree-risk]] (the
license-vs-push distinction this decision sharpened).

**UPDATE — 2026-08-02, SESSION_LOG continuation 55 then 56: priority
#1 (dimensioning) is now COMPLETE, and the operator has since INSERTED
a new item ahead of #2 (icons).** Continuation 55: `9c-min` shipped
(`76485b5`) — all six of decision 011's beta slices done, priority #1
fully met, not just "substantially met." Same continuation: subagent
budget (200) EXHAUSTED — no further work delegable to builder/
librarian/spec agents; remaining work (icons, text-handling, forms)
would need to happen directly in-context or the operator raises
`CLAUDE_CODE_MAX_SUBAGENTS_PER_SESSION`. **Continuation 56 (new
calendar-day session): a GUI usability report from the operator
("can't click objects," no docking, dimensioning tool "didn't seem to
have a way to set dimensions") led to decisions 017/018 and a new Pass
17.x (live-edit rendering) — the engineer proposed sequencing Pass 17.x
BEFORE the rest of the four-item list (icons/text/forms), and the
operator CONFIRMED that reordering the same continuation, along with
confirming the icon SVG pipeline (tiny-skia SVG-path parser, no new
dependency — NOT the previously-recorded pre-rasterize-to-PNG plan,
which turned out non-executable on this machine).** Net effect: the
four-item list from continuation 50 is still the operator's standing
priority order for icons→text→forms, but **Pass 17.x (not itself one
of the original four items) now sits ahead of all three remaining
items**, confirmed rather than assumed. Pass 17.0 (of 17.0/17.1/17.2)
shipped same continuation (`3a56b55`); 17.1/17.2 remain and gate
starting the icon build. Check `ROADMAP.md`'s ★★★★★ reordering entry
(now CONFIRMED, not proposed) and Open operator questions (a)/(f) (both
RESOLVED) before assuming the plain four-item order from continuation
50 is still the immediate dispatch order — it isn't, until 17.1/17.2
ship.

**UPDATE — 2026-08-03, SESSION_LOG continuation 58 (real date; header
stays "2026-08-02" per [[feedback-session-log-continuation-style]]):
Pass 17.1, Pass 17.2, AND Pass 18.1 have all now SHIPPED — the
Pass-17-gate this file's last update said to wait for is now CLEARED
for real.** Decision 018 (live-edit rendering) is COMPLETE end-to-end
(`437a6f7`); decision 017's four numbered engineering slices
(18.0/18.1/18.2/18.3) are ALL shipped (`f963895` for 18.1). Items #3
(finish text-handling: FF-B/FF-H/FF-C) and #4 (form-building) from THIS
memory's four-item sequence are now genuinely unblocked and are the
next concrete dispatch per the operator's continuation-50 order, absent
a new steer — the icon build (#2) already shipped early at continuation
57, so all of items #1–#3's prerequisites are now satisfied and only #3
itself (and then #4) remain undispatched. **Two new items surfaced this
continuation that may compete for priority attention, not yet weighed
against the four-item order by the operator:** the GUI has NO
redaction-apply flow at all (found to be a structural R85-oracle gap,
not an oversight — see `ROADMAP.md` Backlog); and ui-spec §B.4/§C
follow-ons (`TextObject`/`ImageObject` core additions, full selection-
legibility asks) were flagged as a deviation from Pass 18.1's own
stated scope, also filed to Backlog. Check `ROADMAP.md`'s ★★★ operator
priority sequence + ★★★★★ REORDERING entries for current status before
assuming either the plain four-item order or the Pass-17-gate framing
from continuation 56 still describes what's next — it doesn't; the gate
is gone and items #3/#4 are the live dispatch target.

**UPDATE — 2026-08-03, SESSION_LOG continuation 62: item #3 (finish
text-handling) now has a concrete scoping for FF-H specifically —
decision 019, filed as `ROADMAP.md`'s ★ Pass 19.x.** FF-H (the third of
the three named text-parity fast-follows, alongside FF-B/FF-C) is
DECIDED: `Tc`/`Tz` + super/subscript ship as parity, free-form `Ts` +
synthetic bold/italic ship as a deliberate exceed, `Tw` is
evidence-gated behind a corpus census (not a direct control unless
≥60% of sampled documents show it in use), and the minimal
StructTree/`/ActualText` piece named in FF-H's original bundle is CUT
entirely and re-filed as a separate, ungated Backlog item (FF-I) — a
scoping call worth flagging to Ken since he may have counted it inside
"finish off all the text handling stuff." Build order established:
**FF-H → FF-C → FF-B** (not on FF-H's own value — judged the least of
the three — but because Pass 19.0, FF-H's first slice, is a shared
text-state-tracking correctness prerequisite both FF-C and FF-B
inherit). **Pass 19.0 (text-state consolidation) is IN PROGRESS as of
this update**, being built by a separate dispatch. Five new open
questions filed for the operator (`ROADMAP.md` Open operator questions
(g)–(k)): the `Tw` census middle band, FF-C's rule-13 dependency
classification (the MIT decision lifted rule 8, it did NOT pre-approve
any specific crate — don't conflate the two when FF-C's turn comes),
the FF-I StructTree cut, list-authoring (re-surfaced, still
unanswered), and a newly-found parity gap this decision did NOT scope —
kerning (Acrobat retains it per the same Dov-Isaacs source that
established `Tw`/`Ts` were dropped; pdfce has no kerning surface
distinct from `Tc`). Item #4 (forms) remains untouched and still queued
behind #3. Commit chain: two docs commits this session, `67f49bb`
(ui-spec historical marking) → `743e463` (decision 019 record), chain
now **39 commits**, still local-only, still no git remote configured.

**UPDATE — 2026-08-03, SESSION_LOG continuation 63–66: Pass 19.0
through 19.3 ALL SHIPPED — FF-H's formatting-slice family is complete
except the conditional `Tw` slice.** `Tc`/`Tz`/super-subscript (19.1),
free-form `Ts` + synthetic bold/italic (19.2), and the GUI property
surface (19.3) all shipped 2026-08-03. Pass 19.3 also fixed a
project-wide defect: every property-bar Apply in the shipped GUI had
silently refused since Pass 14.3 (span-convention mismatch,
`pin_names_operator`) — new standing rule R93. Decision 019 grew
Amendments A/B/C/D along the way (design corrections + the pinned-span
defect record). Branch at 51 commits by continuation 66.

**UPDATE — 2026-08-03, SESSION_LOG continuation 67: the `Tw` census
(Pass 19.4's gate) has been RUN — BUILD band cleared (91.6% of show
operators / 97.4% of glyphs) — but Pass 19.4 has NOT started.** New
tool `tools/tw-census`. Decision 019 §3.2's own "large and growing"
composite-font-default premise is FALSIFIED on this corpus (81.2% of
text-bearing docs have no composite run at all); filed as decision 019
Amendment E. **The census sweep also found a real pdfce defect: 341
corpus files (8.5%) refuse to open at all** on a `/Contents` array
element resolving to Null (a fail-clean violation — a single bad
array element should degrade that page, not condemn the whole
document; hand-verified as a legal file wrongly refused). **The
engineer prioritized fixing this defect above starting Pass 19.4** — a
control reaching 91% of text matters less than 341 unopenable real
files. Open operator question (g) (the 25–60% middle band) is CLOSED
AS MOOT — the loose metric, which the decision bands are written
against, landed cleanly in BUILD, so the middle-band judgement call
never became live. Branch at 54 commits, still no remote. **Next
concrete step, once the defect fix ships: dispatch Pass 19.4** per
Amendment E's cleared verdict — this is still item #3 of the
four-item sequence; item #4 (forms) remains undispatched behind it.
(Continuation 68, same real date: the `/Contents` defect fix SHIPPED,
`409a6b5`, 289 files recovered — Pass 19.4 then started.)

**UPDATE — 2026-08-03, SESSION_LOG continuation 69: Pass 19.4 (`Tw`)
SHIPPED (`a1638f4`) — decision 019 / FF-H is COMPLETE end-to-end, all
five slices 19.0–19.4 shipped. Item #3 of the four-item sequence is
DONE as far as FF-H's own scope goes** (FF-C and FF-B remain
unscheduled, per decision 019's own Q3 build order FF-H → FF-C → FF-B —
do not read "item #3 done" as "all text-handling done"). **Decision 019
Amendment F filed**, recording three findings the build surfaced: (1)
the composite-run refusal (R91) was UNREACHABLE as originally
implemented — a `match_run` text-decode/filter stage silently consumed
every composite run before the font-aware gate could run, so R91 would
have shipped as referenced-but-never-executed dead code; fixed, new
standing rule R96 filed on the general shape (a guard clause behind an
unreachable filter). (2) A named limit: the fix is reachable via the
GUI's pinned-span path but not via CLI `--find` on composite runs
(closing needs FF-E). (3) `Tw` is multiplied by `Th` (§9.4.4) — the
disclosure now quotes the effective value. **The engineer's next
dispatch, same continuation, is the GUI redaction-apply flow** (Backlog
→ In progress, no Pass ID yet) — a sequencing call flagged for the
operator (jumped ahead of item #4/forms on security-completeness
grounds, not itself required by any standing instruction; new Open
operator question (l)). Branch `pass-8-redaction`, 58 commits
(`77bc58e`/`a1638f4` verified via `git cat-file -t`), still no remote.
Check `ROADMAP.md`'s ★★★ priority sequence item 3 and the new "GUI
redaction-apply flow" In-progress entry before assuming the
continuation-67/68 framing ("item #3 next, item #4 undispatched") still
describes current dispatch — FF-H's slot in item #3 is now closed and a
NEW item (redaction-apply, not itself one of the four) is what's
actively building.

**UPDATE — 2026-08-03, SESSION_LOG continuation 70: the GUI
redaction-apply flow SHIPPED as Pass 8.1 (`9a68999`) — nothing is now
in progress, and item #4 (form-building tools) is next per the
standing order.** `redact_apply.rs` runs the same absence proof at
GUI runtime, before the confirmation dialog opens; two new defects
found by direct observation (marks-list-pushes-Apply-below-the-fold;
a mislabeled overlap count) both fixed same commit; three new standing
rules filed (R97 free-function-for-testable-security-proofs, R98
apply-before-confirm-for-pure-operations, R99 state-before-detail-in-
short-dock-panes; ceiling now R99). **Separately, and concurrently:
`pdfce-acrobat-librarian`'s form-building/authoring research is DONE**
(5 new `forms__*.md` files + 3 addenda) and **a KenAgent decision
agent is actively scoping item #4 in `docs/decisions/` as of this
filing** — the next dispatch after this is very likely a Pass 8.1-style
build against that decision, not a fresh research round. Headline
research finding to carry forward: field-name collision is
type-branched (same-type merges into `/Kids`, different-type refuses
by name) — `pdfce-core`'s field model should be a `/Kids` object graph
from day one. Branch `pass-8-redaction`, 60 commits
(`24bdbc6`/`9a68999` verified via `git cat-file -t`), still no remote.
Open operator question (l) (the redaction-apply sequencing call) is
now "outcome done, ratification still open" — don't read it as fully
closed.

**UPDATE — 2026-08-03, SESSION_LOG continuation 72: FF-C's rule-13
licensing sub-question is CLEARED, no operator decision needed —
FF-C now blocks only on scope/sequencing (Q3 build order FF-H → FF-C →
FF-B).** The engineer worked the classification ahead of the Pass
(`subsetter 0.2.6`, MIT OR Apache-2.0, all-permissive transitive
graph, `cargo metadata`-verified) instead of waiting on an operator
check-in — `LEGAL.md` §6.2 step 3 applies (proceed and log). Full
record: `PRIOR_ART.md` §Fonts "FF-C dependency classification (rule
13) — COMPLETE, 2026-08-03"; `ROADMAP.md`'s FF-D-fast-follow-FF-C
Backlog bullet and Open operator question (h) both amended same
continuation. Don't describe FF-C as licence-gated in any future
session — only "when does it get scoped" remains open. Item #4
(forms) is still queued behind item #3's remaining FF-C/FF-B slices,
per the standing order; a KenAgent decision agent is concurrently
scoping FF-C (will land as decision 021).

**UPDATE — 2026-08-03, SESSION_LOG continuation 73: decision 021 filed
— FF-C is DECIDED and SCOPED as ★ Pass 21.x, status NOT STARTED.**
Item #3's FF-C slot now has a concrete build plan, same as FF-H got at
continuation 62 (decision 019). **Headline correction worth carrying
forward accurately: FF-C as previously described everywhere in the
project (R71, decision 014 §5.3, the spec-RAG stub) was NOT
implementable** — it described extending the document's own embedded
font in place, but a subset font by definition doesn't contain the
missing glyph; there's no operation on `FontFile2` alone that produces
it. FF-C is re-scoped **add-only**: it adds a new, subsetted font
resource from a donor face (decision 012's `--font-dir`) and never
touches an existing font program/dictionary — new standing rule R107.
Do not describe FF-C as "extends the embedded subset" in any future
session; that framing is now wrong. Slices: 21.0 (core+CLI, P0 floor,
lifts pdfce's widest text-authoring wall — no non-Latin text at all
today) → 21.1 (composite-run edit, makes 21.0 editable — **do not
report FF-C as done if only 21.0 has shipped**, that would be a
capability regression against the shipped Std-14 add-text path,
invisible to the existing R85 raster oracle) → 21.2 (`set-font` to an
embedded face) → 21.3 (GUI, `pdfce-ui-specialist` first). New standing
rules R107–R110 (ceiling now R110, was R106). Net dependency cost
refined from 2 packages to 1 (`subsetter`, `default-features = false`
— `PRIOR_ART.md` amended). Two new operator questions filed, both
Ken's per `docs/decisions/README.md`, neither blocking 21.0/21.1: (r)
font-EULA policy for a donor whose `OS/2` `fsType` forbids/is
unparseable; (s) whether Pass 21.0 refuses complex scripts (Arabic/
Devanagari/Thai) by name, since R17 (no shaping, ever) means they'd
embed but render wrong (recommendation: refuse by name). Item #4
(forms) remains queued behind item #3's now-single remaining open
slice, FF-B (FF-H done, FF-C scoped-not-started). Also this
continuation: `tools/check-ledger-numbers.py`'s own blind spot
(ceiling scanned only headings, missed Pass 20.x claimed-in-prose)
fixed same day at `d30842c` — see
[[project-uncommitted-repo-worktree-risk]] for the commit/hash update
and `D:\dev\rag\rust\ci_gate_red_at_baseline_enforces_nothing.md` for
the generalized finding.

**UPDATE — 2026-08-03, SESSION_LOG continuation 74: `pdfce-spec-librarian`'s
decision-021 dispatch returned — eight findings, two change the work.
Decision 021 AMENDED (§10), status unchanged (DECIDED/SCOPED/NOT
STARTED).** **The one fact most likely to bite a future session that
skips straight to "21.0, glyf donors, go build": Pass 21.0's P0 floor is
now `glyf` (TrueType-outline) donors ONLY — CFF donors are refused by
name (`DonorUnsupported`) until a later slice**, because `subsetter`
wraps CFF donors in an `OTTO` sfnt that ISO 32000-1 §9.9 Table 126
requires a `cmap` for, and `subsetter` strips `cmap` unconditionally
(verified at source, `lib.rs:492`). L1 (the non-Latin headline) still
holds — Noto Sans JP/CJK, DejaVu, most Google Fonts are TrueType `glyf`
— but don't assume a CFF/OpenType-CFF donor "just works" at 21.0; it's a
named, tested refusal, not an oversight. **R109 also split**: fsType is
TWO distinct refusals now, not one `EmbeddingNotPermitted` —
`SubsettingNotPermitted` (bit 8, `0x0100`, forbids the one thing FF-C
does) and `EmbeddingNotPermitted` (bit 9, `0x0200`, the spec's own
"unembeddable" case). Open operator question (r) is narrowed to just
absent/unparseable `OS/2` (and the spec-silent `fsType == 1`) — the
forbids-embedding/forbids-subsetting cases are no longer Ken's call,
they're spec-sourced and R109 refuses them by name automatically.
**Separately this continuation, unrelated to the spec review:** two
shipped operator-facing hints (`r_inv_1_hint()`, `format_coverage_hint()`)
were found FALSE — both told the operator that supplying a font would
fix a coverage/subset refusal, and neither does, in any shipped build —
fixed at `0893191`. A synthetic embedded-subset-font fixture is now
explicitly owed against Pass 21.0 (`fixtures/synthetic` has none
suitable) — both to test 21.0 itself and to finally observe the
corrected hints on screen. Full record:
`docs/decisions/021-ffc-font-subsetting-and-glyph-embedding.md` §10 and
`ARCHITECTURE.md` §12's continuation-74 dated entry. Branch now 67
commits — see [[project-uncommitted-repo-worktree-risk]].

**UPDATE — 2026-08-04, SESSION_LOG continuation 75: Pass 21.0 (FF-C P0
floor) SHIPPED (`48c6b77`) — pdfce can now add non-Latin (`glyf`/
TrueType-donor) text via `add-text --embed-font`. Item #3's FF-C slot
is PARTIALLY closed — do NOT describe FF-C or item #3 as "done."**
Six-commit chain `88b9487`→`0c4f490`→`d4e7355`→`5b7bed3`→`eb0bde5`
→`48c6b77`, all six verified by the OPERATOR directly with `git
cat-file -t` (a first — previously always engineer-relayed). **Two
things explicitly NOT done, named so a future session doesn't assume
FF-C is finished:** (1) **Pass 21.1** (composite-run editability under
verified-injective `/ToUnicode`, R110) — promoted to `ROADMAP.md` In
progress; decision 021 is explicit that 21.0 alone is a capability
REGRESSION (pdfce can add non-Latin text it can never edit) against
the shipped Std-14 add-text path. (2) **R109's `fsType` donor-
permission read** — named in 21.0's original scope but did NOT ship;
`add-text --embed-font` currently embeds a donor face without checking
whether its own OpenType permission bits forbid subsetting/embedding.
Flagged as a real gap against rule 4 (fuzzy-never-sneaky), not mere
polish — recorded in three places in `ROADMAP.md` (the Pass 21.0
Shipped entry, a dated amendment on R109's Standing-rules bullet, and
the new Pass 21.1 In-progress entry). Three RAG findings from this
Pass's bug hunt escalated to `D:\dev\rag\rust\` (a rule-shaped
"assert termination, don't guard the unreachable" pattern; two testing-
discipline findings — stale disclosure text, exit-code `_ =>`
catch-all — deliberately NOT promoted to new `ROADMAP.md` standing-rule
numbers, consistent with the continuation-74 precedent that solo rule
adoption isn't this librarian's call). One PDF-domain empirical finding
filed to `C:\personal_rag\pdf\` (embedded-font-size census ≠ donor-face
ceiling — different populations). Branch now **74 commits** — see
[[project-uncommitted-repo-worktree-risk]]. Item #4 (forms) remains
queued behind item #3's still-open FF-C follow-ons (21.1, fsType read,
21.2, 21.3) and FF-B, unchanged in kind since continuation 73.

**UPDATE — 2026-08-04, SESSION_LOG continuation 76: R109's fsType read
SHIPPED (`58fe3f6`), R110's primitive SHIPPED (`c0ed638`), AND a
shipped-but-unreachable R-INV-4 refusal was found and fixed
(`8e08e80`+`87d3cb0`+`6b69956`) — item #3's FF-C follow-ons are
shrinking but Pass 21.1 (actual composite-run editability) is STILL
unbuilt.** Headline finding: `edit.rs` carried a comment claiming
composite runs are refused later by R-INV-4 — false, because
`match_run` silently filtered every composite run to `NoMatch` before
the R-INV-4 gate could ever run, so the correct font-limitation
refusal had NEVER once fired on any input, from Pass 21.0's ship
through this fix. Fix was ordering (classify font before matching
text), not new machinery — same dead-guard-behind-a-filter shape as
the Pass 19.4 `Tw` finding, filed as a second occurrence. Composite
runs are now correctly located and refused for the right, disclosed
reason — **still not rewritable**: `ShowSlot::code` (currently `u8`)
must widen to hold multi-byte CIDs before R110's conditional edit-lift
has anything to attach to. Branch reached 79 commits. Two items
carried forward as still-owed at continuation 76's own filing: the
`ARCHITECTURE.md` §3/§4 body-section sync for Pass 21.0's new modules,
and a judgement call on whether the four-instance "confident comment
asserts untrue behavior" pattern warrants a new standing rule.

**UPDATE — 2026-08-04, SESSION_LOG continuation 77 (librarian-only, no
code): both of continuation 76's carried-forward items resolved.**
(1) **No new standing rule** — the pattern is judged to already be R93
(now filed as its fourth occurrence) plus R96 (now filed with a second
occurrence recording the generalized "precondition-after-search" shape
this instance demonstrates); R86 (still PENDING, awaiting item (e))
gets a queued scope note that "observed working" also covers refusal
paths, not just successes, since that habit — not a new rule — is what
actually caught the `edit.rs` defect. (2) **`ARCHITECTURE.md` §3/§4
sync for Pass 21.0's `pdfce-render::font::subset`/`pdfce-core::
font_embed` modules is now DONE**, plus a dated §12 entry closing the
gap. Repo/backup re-verified: still 79 commits, backup bundle
refreshed to `...0325.bundle`, verify-clean, current to `6b69956`. Item
#3's remaining live work is unchanged in kind: `ShowSlot::code`
widening + multi-byte operand writer to actually close Pass 21.1; item
#4 (forms) still queued behind it.

**UPDATE — 2026-08-04, SESSION_LOG continuation 78 (SESSION-ENDING
FILING): Pass 21.1's SUBSTRATE shipped, WIRING deliberately not
started.** `ShowSlot::code` widened `u8`→`u32` + per-slot `width`
(`31d2fdc`) and `CompositeEncoding` (character→CID via
`injective_inverse()`, `b98589a`) both landed, plus a new
`composite-editable.pdf` fixture. **Composite runs remain
LOCATABLE-BUT-REFUSED, not editable** — the wiring itself surveyed as
FOUR coupled changes (composite branch ahead of the `Unsupported` bail
in `glyph_names()`; `/W`/`/DW` advance lookup per §9.7.4.3, not
`/Widths`; hex-string operand emission in `emit_edited_operator`
instead of the literal-string path; width-aware `carried_codes`
subset-floor accounting) — deliberately not attempted this
continuation because a half-applied version risks a silent wrong-output
edit, worse than leaving the Pass open. **Notable near-miss, escalated
as a new RAG finding
(`D:\dev\rag\rust\regression_test_guard_via_incidental_property_disarms_silently.md`):**
the type widening alone would have made it POSSIBLE to silently disarm
`tests/composite_refusal_reachable.rs` (continuation 76's regression
test) had slot-pushing been added carelessly — that test currently
passes because composite runs produce zero slots today, an incidental
property, not because it directly asserts the ordering it exists to
guard; the fix for whoever wires this in is to rewrite the test to
search for text known ABSENT from the page, a discriminator immune to
slot count. **Branch now 82 commits**, backup bundle refreshed to
`...final.bundle` — see
[[project-uncommitted-repo-worktree-risk]]. **Four operator decisions
outstanding, consolidated into one list in this filing's SESSION_LOG
entry** (font-EULA policy / complex-script refusal posture / forms
sequencing status-check / R86 ratification) — Ken has been away the
entire session; check that consolidated list first the next time he is
present, rather than re-grepping continuations 73–78 individually. Item
#3's remaining live work is the four-item wiring survey above,
unchanged in scope from continuation 77's framing but now precisely
enumerated; item #4 (forms) still queued behind it.
