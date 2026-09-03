# pdfcer — Project Instructions

An open-source, non-monetized, feature-for-feature replacement for
Adobe Acrobat Pro. Native desktop GUI first (no web server/browser
runtime), single-folder portable, Rust + egui/eframe, plus a
first-class CLI (`pdfcer`) for scriptable batch operations. See
`README.md` and `docs/ARCHITECTURE.md` for the full picture.

These instructions are read **at the start of every Claude session**
in this project. Everything below is binding. The global rules in
`C:\Users\Ken\.claude\CLAUDE.md` also apply (documentation-first,
claim-bearing-copy verification, personal_rag lesson-writing
discipline, etc.) — this file adds pdfcer-specific rules on top.

## Project agents

This project has six agents under `.claude/agents/`:

| Agent | Role | When to dispatch |
|---|---|---|
| `pdfcer-engineer.md` | Single-session lead engineer | The default role for any engineering work in this project. If you're the orchestrator, **be this agent** — read its file at session start, follow its discipline. |
| `pdfcer-librarian.md` | Institutional memory: `ROADMAP.md` / `FEATURES.md` / `SESSION_LOG.md` / `ARCHITECTURE.md` decision-log keeper | Dispatched by the engineer for every new request (→ roadmap entry), every Pass completion (→ Shipped row), pre-compaction captures, and generalizable findings that graduate to `D:\dev\rag\rust\` / `D:\dev\rag\egui\` (ecosystem-wide) or `C:\personal_rag\pdf\` (PDF-domain). |
| `pdfcer-spec-librarian.md` | Builds/maintains the PDF-standard reference RAG at `D:\Dev\Rag-Specialized\PDF_Spec\` | Dispatched whenever a spec question needs canonical sourcing (object model, filters, fonts, crypto, PAdES, PDF/A, PDF/UA), and self-directed for corpus-building sessions. |
| `pdfcer-acrobat-librarian.md` | Builds/maintains the Acrobat Pro feature-parity RAG at `D:\Dev\Rag-Specialized\Acrobat_Features\` | Dispatched when scoping a `ROADMAP.md` Backlog bucket into a real Pass, so acceptance criteria match actual Acrobat behavior. Catalogs capabilities only — never Acrobat's GUI mechanics. |
| `pdfcer-inkscape-librarian.md` | Builds/maintains the Inkscape feature-parity RAG at `D:\Dev\Rag-Specialized\Inkscape_Features\` | Dispatched when scoping the vector-editing Passes (Pass 9), so acceptance criteria match actual Inkscape behavior. Catalogs capability/behavior/limits only — never Inkscape's GUI mechanics; Inkscape is a behavioral reference only (GPL-2.0-or-later, never a dependency or code source — standing rule R61). |
| `pdfcer-ui-specialist.md` | egui/eframe UX design + review | Dispatched by the engineer for non-trivial UI changes (new panel, new tool, an accessibility/discoverability judgment call). Returns critique + a change list; does not write code. |

The engineer agent file is the single source of truth for *how* work
happens in this project day-to-day. Read it before doing anything
substantive.

## Read first

- **`docs/ARCHITECTURE.md`** — crate layout, core data model, the two
  load-bearing invariants (GUI-core separation, round-trip/minimal-diff
  editing), packaging strategy. Read every session.
- **`docs/ROADMAP.md`** — the contract. Read every session.
- **`docs/FEATURES.md`** — the capability-shaped view: what pdfcer can do
  today and what is planned, in predicted order, with **core / cli / gui**
  checkboxes per row. Created 2026-08-05 at the operator's request because
  `ROADMAP.md` is organised by Pass and structurally cannot answer "what
  can it do?". Maintained by `pdfcer-librarian` **in the same filing as
  every `ROADMAP.md` update** — it is not a separate chore and must not be
  allowed to drift. `ROADMAP.md` stays authoritative if the two disagree.
  Deliberately terse; do not expand it.
- **`docs/SESSION_LOG.md`** — most recent entry, for what the prior
  session left in flight.
- **`docs/LEGAL.md`** — license status (**MIT**, decided 2026-08-01 —
  but publishing still needs a go-ahead, rule 8), PDF-spec
  sourcing/copyright rules, test-corpus rules.

## Project-specific rules (binding)

### 1. Spec-fidelity discipline

Never implement spec-governed behavior (object-model byte layout,
filter algorithms, xref structure, font encoding, crypto handshakes)
from training-data memory. Check `D:\Dev\Rag-Specialized\PDF_Spec\`
first; dispatch `pdfcer-spec-librarian` if the RAG doesn't yet cover
the question. Cite the ISO/ITU-T/ETSI clause in code doc comments.

### 2. GUI-core separation (load-bearing invariant)

`pdfcer-core` and `pdfcer-render` must never gain a GUI/windowing
dependency. Verify with `cargo tree -p pdfcer-core` /
`cargo tree -p pdfcer-render` on any Pass touching their `Cargo.toml`.
This is what keeps the eventual web/WASM fork a shell-crate swap
instead of a rewrite. See `ARCHITECTURE.md` §3.

### 3. Round-trip / minimal-diff editing

Objects pdfcer didn't logically touch are re-emitted byte-identical
(full rewrite) or simply omitted (incremental save — the default save
mode). Redaction is the one deliberate, explicit exception: it must
truly remove covered content, not just visually mask it. See
`ARCHITECTURE.md` §5.

### 4. Fuzzy, never sneaky

Anything pdfcer **inferred** — a value, a boundary, a classification, a
correction the operator did not directly specify (OCR text,
auto-detected form fields, recognised text blocks, snapped points,
best-fit geometry, derived centrelines, reflow results, suggested Bates
ranges, substituted or synthesised fonts) — is **disclosed, never
silent**.

**The commit point is SAVE** (`ARCHITECTURE.md` §11.1): Undo rejects,
Save commits, and **nothing in an open edit session is document state**.
So the inference **renders exactly as saved content will render** —
normally, live, reflowing normally, **with no badge, tint, red flag,
dashed outline or "provisional" layer drawn into the page view** — and
the disclosure of *what pdfcer guessed* lives **off-canvas**: a status
line, a results panel, a post-command report, a properties field. It
never blocks, never requires acknowledgement, and is never positioned
relative to the document. **There is no accept/reject gate in front of
anything.**

In `pdfcer` the invocation **is** the commit — no session, no undo —
so the CLI **prints** what it inferred on the way past instead. **What
this rule forbids is SILENCE**, in both shells; only the vehicle
differs.

This is a requirement on **disclosure**, not on any particular widget,
and it bites hardest on inferences the operator **cannot see by
definition** — OCR text at render mode 3 (invisible, under the image), a
font substitution that renders plausibly, a best-fit residual, an
over-eager snap, a near-parallel classification. **Render normally;
report separately. Both.**

Where an inference is *inherently* uncertain (a best-fit residual, a
font-trust downgrade, a reflow that overflows), the uncertainty is
**stated in the disclosure** rather than implied by the presence of a
confirm button — there is no longer a confirm button to imply it.

Inherited from the user's MatExtractor project; same principle, new
domain.

**Narrowed 2026-08-05** (decision 024 §4.4). The original wording said
every algorithmic suggestion "is a reviewable hint the operator accepts
or overrides", and that was being read as *every gesture needs an Accept
button*. Two things went wrong with that reading. It put a confirm step
in front of direct manipulations the operator had just performed and
could see — placing a dimension, typing a replacement — where undo is
the honest escape hatch and a second click is friction. And because the
confirm controls were positioned relative to the PAGE, they moved on
every zoom, scroll and page change, which is what the operator actually
reported: *"there is a separate accept / reject box somewhere on the
screen to click — I've never seen any other software operate that way."*
The complaint was placement, not the confirm step. The narrowing keeps
the obligation exactly where it was meant to be — on things pdfcer
GUESSED — and takes it off things the operator did.

**★★ NARROWED AGAIN 2026-08-13** (decision **059**, `ARCHITECTURE.md`
§12). The first narrowing took the burden off things the operator *did*;
this one takes it off the **rendering** of things pdfcer *guessed*. The
prior wording is kept legible rather than silently rewritten — **the head
of this rule said**:

> ~~"…is **visible before it becomes document state**, and the operator
> can reject it without undoing anything else."~~ and ~~"It is satisfied
> by the inferred value being on screen and the commit being a deliberate
> act — a key press, or a click on a control at a fixed, predictable
> position."~~

**The operator rejected that phrase by name**, having read it restated as
non-negotiable #1 in the outbound briefing at
`D:\Dev\FeatureRequests\pdfce_FeatureRequests\README.md`. Verbatim and in
full:

> *"I had you write a handoff to D:\Dev\FeatureRequests\pdfce_FeatureRequests
> and noticed under the non-negotiables, item 1 - must be visible before
> it becomes a document state: I think this has consequenses for
> usability. As a user I just want to type in an existing gui text box
> and have it look normal and reflow normal. I want OCRed stuff to look
> normal when the command is executed too. I only expect things to be
> committed when I hit save, and not commited if I hit undo. The nagging
> and red flagging in the original GUI made for a lot of extra bugs in
> the visibility when editing."*

**Four clauses**, all folded into the rule's text above: (1) **the commit
point is SAVE** — the session *is* the preview, so an inference that
lands in it has not "become document state"; (2) **inferred content
renders exactly as saved content will render**, with no provisional
marking on the page; (3) **disclosure moves off-canvas and stays there**;
(4) **no accept/reject gate in front of anything** — already true for
direct manipulations since 024 §4.4, now true for inferences too.

**★ This is a CORRECTNESS rule, not only a usability preference**, and
that is the half a future session will be tempted to trade away. His last
sentence is the argument: **every provisional-state marking is a SECOND
RENDERING PATH for the same content, and two paths drift.** Content drawn
as *pending* and the same content drawn as *committed* are different
code, exercised at different times — the divergence surfaces as content
that looks wrong, vanishes, double-draws or fails to reflow. **Deleting
the marking machinery removes a BUG CLASS.** A shell that re-introduces a
"helpful" highlight re-introduces the class.

**Unchanged, so this is not over-read:** invisible inferences still owe
their off-canvas disclosure (that is *why* the rule exists); `pdfcer`
still prints (rule 11 untouched); §11.2's **redaction confirmation
survives** — it warns about a destructive *save*, not about an inference;
and **R167's editor chrome survives** — a dashed outline for a widget
with no paintable `/AP`, a selection highlight, a resize handle disclose
**editability**, not pdfcer's uncertainty about content.

**The one-line test:** *would a screenshot of the editing canvas differ
from a screenshot of the same document saved and reopened?* If yes, **and
the difference is pdfcer marking its own uncertainty**, that is the
defect. The second half of that sentence is load-bearing.

**Note the shape, because it has now happened twice:** two narrowings
against this one rule, both prompted by the operator noticing shipped
friction rather than by review. **Rule 4 has been consistently read as a
mandate for VISIBLE MACHINERY when it was only ever a mandate for
NON-SILENCE.**

### 5. Roadmap discipline

Every new operator request → engineer parses into Pass entry/entries
→ dispatches `pdfcer-librarian` to file under *Backlog*/*Next up* →
reports assigned Pass IDs back. Every Pass completion → dispatch the
librarian to move it to *Shipped* + append a `SESSION_LOG.md` entry.

### 6. Documentation-first

Per the global rule: every module gets a thorough file-level
docstring (purpose, contracts, spec citations); every function gets a
doc comment explaining WHY; the docs are the logic, the code is the
syntax. If a competent engineer couldn't rebuild the module from the
docs alone, the docs are incomplete.

### 7. Test-corpus sourcing

Fixture PDFs are synthetic or clearly rights-cleared only — never a
downloaded real-world PDF of unknown provenance. See `LEGAL.md` §5.

### 8. License is MIT; pushing `main` is standing-authorized, releasing still needs a go-ahead

**Corrected 2026-08-05.** This rule said the license was undecided and
that the project must not be described as "open source". That has been
wrong since 2026-08-01, when the operator chose **MIT** (`LEGAL.md` §1,
`ARCHITECTURE.md` §12). `LICENSE` exists at the repo root,
`license = "MIT"` is set on all four crates, and every dependency is
permissive — zero copyleft, verified against the generated
`THIRD_PARTY_LICENSES.md`.

**★★ AMENDED 2026-08-27 (decision 090).** The bullet immediately below
this note used to read, unqualified:

> ~~"**Do not push or cut a release** without an explicit, current
> go-ahead. The license does not block it; publishing is the operator's
> act, not the agent's, and each one is its own decision."~~

**Ken's ruling, verbatim, given after being asked three times in one
session whether to push while `main` sat 24 commits ahead of
`origin`: "always push."** That grants the **push** half of the bullet
above as a **standing** authority — no more asking, this session or any
future one, before an ordinary fast-forward push of `main` to
`origin/main`. It does **not** grant the **release** half; the operator
answered about one act, and the rule named two. See decision 090 for the
full reasoning and the precedent (decision 061) for narrowing a broad
operator word rather than reading it generously.

What holds now, replacing the struck bullet:

- **Pushing `main` (ordinary fast-forward) no longer needs a go-ahead —
  it is standing-authorized as of 2026-08-27 ("always push").** Still
  gated, exactly as before, and still needing an **explicit, current**
  go-ahead each time:
  - ~~**cutting a tag or a release** — a release is a claim that a state is
    fit to use, a different act from making commits visible;~~
    **★★ SUPERSEDED 2026-09-02 — decision 121. Releasing is now
    STANDING-AUTHORIZED.** The operator, verbatim, given directly after
    being told that `main` was pushed but fifteen commits sat unreleased
    and OneDrive still carried the previous version: **"always go ahead
    and push the latest one."**
    So: cut the tag, package, smoke-test, deploy to OneDrive, no
    per-release go-ahead.
    ★ **Read narrowly, and the narrowing is part of the ruling** — the
    same discipline decision 090 applied to "always push". It covers the
    RELEASE act. It does **not** cover `--force`, rewriting published
    history, non-`main` branches, or remote tags other than the release
    tag; he was answering about shipping the latest work, not about
    those. And it authorises the act, **not** skipping the gates: a
    release still owes a green `tools/run-gates.sh`, a fresh-folder
    smoke test, and `verify-release.py`.
    The struck wording above is kept legible because a reader who
    remembers the old rule needs to see that it moved, not wonder
    whether they misremembered it;
  - **`git push --force`, or any push that rewrites published history** —
    destructive and unrecoverable for anyone who already cloned, and this
    project has direct evidence of the failure mode: rewriting a commit
    breaks every document that cites its hash
    (`tools/check-cited-commits-exist.py`, `0d9f4df`, found fourteen
    pre-existing casualties from exactly this cause);
  - **pushing any branch other than `main`, or creating remote branches or
    tags** — the ruling was given about `main`, not about a different act
    it was never asked about.
  Scrub the public-facing gate (`check-suite-name-absent.py`) green
  **before** pushing regardless — the repository is public, so a push
  publishes (`LEGAL.md` §1.1) — and read CI's colour from GitHub itself
  rather than carry forward a sentence that assumed one. That habit
  matters *more*, not less, now that pushing no longer stops to ask.
- **★ THE PROJECT IS ALREADY PUBLIC. This bullet said "there is still no
  git remote configured" and that was FALSE** —
  `github.com/KenM76/pdfce` (the pre-release repository, now archived;
  the project continues at `github.com/KenM76/pdfcer`, created 2026-09-03)
  is public, created 2026-08-09 04:56Z, `main` pushed 10:18Z. Corrected
  2026-08-10; repository name re-dated 2026-09-03 after the rename. Two consequences worth carrying:
  a repository with a remote is one where a careless `git push` reaches
  the world, and **anything committed here is published by default**, so
  the temp-folder convention for test files is now load-bearing rather
  than tidy. The confidential material already in history was reviewed
  by the operator and **accepted** — see `LEGAL.md` §1.1; that is
  settled, not an invitation to re-raise it.
- Note the shape of that error, because it is the expensive kind: a
  document asserted a fact about the environment that nobody had
  measured, and it read as reassurance for a day. `git remote -v` costs
  nothing.
- **GPL/AGPL prior art is categorically out** as a dependency or code
  source — MuPDF, Poppler, Ghostscript, Inkscape. An MIT project cannot
  link them (`LEGAL.md` §6.1). Inkscape stays a *behavioural* reference
  only (R61).
- `THIRD_PARTY_LICENSES.md` is generated by `cargo-about`, never
  hand-edited; regenerate it whenever the dependency set changes.

### 9. Cross-project knowledge bases

- `D:\Dev\Rag-Specialized\PDF_Spec\` — canonical PDF-standard reference
  (spec text/summaries with citations). Read-heavy; written by
  `pdfcer-spec-librarian`.
- `C:\personal_rag\pdf\` — empirical, project-internal findings about
  how real-world PDFs (from Word, LibreOffice, Chrome's "print to PDF",
  scanners, etc.) diverge from the spec in practice. Distinct from the
  spec RAG the same way `personal_rag/solidworks` is distinct from
  `sw_api_docs` for the user's SolidWorks work. **Exists and is in
  active use** (created 2026-08-04; grep it before re-deriving anything
  about producer behaviour). Written by `pdfcer-librarian`, following
  `C:\personal_rag\README.md`'s template.
- `D:\dev\rag\rust\` — Rust toolchain/Cargo/packaging quirks that
  generalize to **any** Rust project, not just pdfcer. **Already
  exists** (part of the existing Cross-project Tool RAG, registered in
  `C:\Users\Ken\.claude\CLAUDE.md`) — also holds the canonical
  `rust-style-guide-and-api-guidelines.md` reference (rule 10 below).
- `D:\dev\rag\egui\` — egui/eframe/wgpu findings, same cross-project
  scope as above. Already exists.
- `C:\personal_rag\claude_code\` — Claude Code tooling patterns.

### 10. Rust Style Guide + API Guidelines compliance

`cargo fmt --check` and `cargo clippy -- -D warnings` clean before any
Pass ships, workspace-wide. Any `pub` item added to `pdfcer-core`
(or `pdfcer`'s argument/output surface) is checked against
`D:\dev\rag\rust\rust-style-guide-and-api-guidelines.md` — naming
conventions, trait derives, error-type design (`thiserror`, not
stringly-typed errors), documentation with runnable examples. See
`ARCHITECTURE.md` §8.

### 11. CLI capabilities (`pdfcer`)

pdfcer ships a real, scriptable command-line interface alongside the
GUI — not a debug tool, a genuine parity-plus feature (Acrobat Pro has
no equivalent first-class CLI). Same crate-separation, round-trip, and
fuzzy-never-sneaky discipline as the GUI applies to every subcommand.
Default: each feature Pass ships its `pdfcer` subcommand alongside
the GUI flow, same session. See `ARCHITECTURE.md` §7 and
`ROADMAP.md`'s "CLI batch operations" backlog entry.

### 12. Acrobat feature-parity RAG (`D:\Dev\Rag-Specialized\Acrobat_Features\`)

Before scoping a Backlog bucket into a real Pass, dispatch
`pdfcer-acrobat-librarian` so acceptance criteria reflect what Acrobat
Pro actually does. This RAG catalogs capability/behavior/edge-cases/
limits **only** — it must never describe or inform copying Acrobat's
GUI structure (menu paths, panels, dialogs); pdfcer's UI is designed
independently by `pdfcer-ui-specialist`. See `ROADMAP.md`'s "Feature RAG"
glossary entry and "Feature-fidelity discipline" standing rule.

### 13. Open-source dependency licensing & attribution

Before adding any Cargo dependency, classify its license (permissive /
weak-copyleft / strong-copyleft — see `LEGAL.md` §6.1) and check
`docs/PRIOR_ART.md`. Copyleft dependencies are always flagged to the
user, never decided solo. pdfcer is **MIT** (rule 8), so GPL/AGPL is not
merely discouraged — it cannot be linked at all, and weak-copyleft
(LGPL/MPL) needs the operator's call on the linking terms. Attribution is
**generated** via `cargo-about` into `THIRD_PARTY_LICENSES.md`, never
hand-maintained; regenerate it whenever the dependency set changes and
before any packaging pass. See `LEGAL.md` §6 and `ARCHITECTURE.md` §9.

### 14. RAG format philosophy: LLM-optimized, not human-readable

Every RAG this project builds or writes to (`PDF_Spec`,
`Acrobat_Features`, `D:\dev\rag\rust`, `D:\dev\rag\egui`,
`personal_rag/pdf` once it exists) is written for **LLM consumption
only** — dense, schema-consistent, grep-first. No narrative
scene-setting, no restating context an LLM already has, no prose
padding "for the reader." If a sentence doesn't add a fact a future
lookup needs, cut it. This is a standing instruction from the user
(2026-07-23), binding on every agent that writes to any of these RAGs.

### 15. Dimension terminology: "pdf dimensions" vs "ce dimensions" — never bare "dimensions"

Two entirely different things share the word *dimension* in this project,
and they have **opposite properties**. Always qualify which:

- **pdf dimensions** — dimensions already present in the PDF, exported by
  CAD or another authoring tool. Existing page content (or foreign
  annotations). pdfcer reads them, measures against them, and must not
  silently alter them. The `55 5/8"` printed on a drawing is a *pdf
  dimension*.
- **ce dimensions** — the dimension objects **pdfcer authors**: `/Line` +
  `/IT /LineDimension` annotations with a baked `/AP`, their groups, scale,
  `/Measure` dict and `/PieceInfo` sidecar. Everything under
  `crates/pdfcer-core/src/dimension/`. Authored, editable, deletable,
  re-measurable — pdfcer's own.

**Binding on every agent**, and on every reply, commit message, doc comment,
decision record, RAG entry and **subagent dispatch**. Dispatches especially:
a subagent handed the ambiguity writes an entire analysis in it, which is
exactly how it reached the operator.

**Why (operator, 2026-08-04):** he could not decode analysis that used
"dimension" throughout without ever saying which kind — and he named the
failure in *both* directions: ambiguous output is hard for him to act on,
and an ambiguous report *from* him can send troubleshooting down the wrong
path. This is a mutual-intelligibility rule, not a style preference.

When the operator says "dimension" unqualified, infer from context and
**echo back the qualified term**, so a mismatch surfaces before the work
rather than after it.

The distinction is **provenance**, not representation: a ce dimension is
still a ce dimension after save-and-reopen, and a pdf dimension does not
become a ce dimension because pdfcer can see it.

## How a typical Claude session goes

1. **Read `docs/ROADMAP.md`** for current state.
2. **Read the most recent `docs/SESSION_LOG.md` entry** for prior context.
3. **Receive the operator's request.** Parse into Pass entries;
   dispatch `pdfcer-acrobat-librarian` if scoping a new Backlog bucket
   (so acceptance criteria are grounded in real Acrobat behavior),
   then dispatch `pdfcer-librarian` to add the entries to `ROADMAP.md`.
4. **Work the in-progress Pass**, consulting the spec RAG for any
   spec-governed behavior and dispatching `pdfcer-ui-specialist` for
   non-trivial UI decisions.
5. **Ship the Pass**: tests green, `cargo tree` invariant verified,
   packaging smoke test run if packaging changed.
6. **Dispatch `pdfcer-librarian`** to move the entry to Shipped and
   append a `SESSION_LOG.md` entry.
7. **Brief the operator** on what changed, what to try, what's next.

## Outstanding open items (surface these proactively when relevant)

- **`oxidize-pdf` (MIT) foundation-vs-scratch decision** — may already
  cover most of `pdfcer-core`'s scope; needs a dedicated audit before
  Pass 1. See `docs/PRIOR_ART.md`'s "OPEN QUESTION" and `ROADMAP.md`
  Pass 0. Not yet decided — surface this before any from-scratch
  `pdfcer-core` scaffolding begins.
- egui vs iced — not yet confirmed with the user (default: egui/eframe).
- OSS license — **DECIDED: MIT** (operator, 2026-08-01; `LEGAL.md` §1,
  `ARCHITECTURE.md` §12). Consequence: AGPL/GPL prior art (MuPDF, Poppler,
  Ghostscript) is now categorically **off the table** as a dependency — an
  MIT project cannot link GPL/AGPL (`LEGAL.md` §6.1). Publishing/pushing is
  now unblocked by the license but still awaits an explicit operator go-ahead.
- XFA scope — **NARROWED 2026-08-03, still open; do not treat as
  closed.** The original item read "verify Adobe's current XFA
  support/deprecation status before committing engineering time to
  it." Three things have since happened, and this bullet is kept
  accurate rather than retired because retiring it is Ken's call
  (`ROADMAP.md` Open operator question **(p)**), not the engineer's:
  - **Demand is measured** (decision 008 census): `/XFA` in 2 of 2,500
    organic files (0.08%) and 4 of 2,914 conformance files. Negligible.
  - **Both authoring branches are now decided** (decision 020): dynamic
    XFA is `out_of_scope` — as of Acrobat 8.1+ it carries no AcroForm
    at all, so there is no Acrobat behaviour to match. Static-XFA
    *hybrid* field creation is **refused by name**, decided from
    pdfcer's own capability boundary rather than from Acrobat's: pdfcer
    can write the AcroForm half of a hybrid but not the XFA half, and a
    one-sided add would make an XFA-aware viewer and a plain viewer
    show different field counts for the same document.
  - **★ ANSWERED 2026-08-11, and by a stronger fact than the one being
    sought.** This bullet read "what is still genuinely unverified:
    Acrobat's exact version-level deprecation date (only third-party
    approximate timing found, no Adobe-primary source)". The spec
    corpus records **XFA as deprecated in ISO 32000-2 itself**,
    including the `NeedsRendering` catalog entry, and notes explicitly
    that the deprecation is "at the ISO level, not just Adobe's
    product" (`iso32000__delta__pdf20_pass1.md`). A vendor's
    product-version timeline is a weaker fact than the standard
    dropping the feature.
    Note the shape of the error rather than just the correction: **the
    answer was already sourced in one document while another still
    asked the question.** Nothing was wrong, nothing contradicted
    anything — the finding simply never propagated to the place that
    needed it. Grep the corpus before recording something as
    unverified.
  Net: the verification is **no longer a prerequisite for form
  *authoring*** — both branches were decided without needing it. It
  would still be a prerequisite for any XFA **read/fill** work.
  Decision 020 recommends re-scoping the item to exactly that; whether
  to accept that narrowing, or retire the item outright, is question
  (p) for Ken. See `ROADMAP.md`'s XFA backlog entry for the full
  amendment chain.
- OCR engine binding — **★ THE ENGINE HALF IS ANSWERED 2026-08-12; THE
  LICENCE HALF IS NOT, AND THE TWO MUST NOT BE COLLAPSED.** This bullet
  read *"not yet decided"* and that is now half wrong, which is the more
  dangerous state for an item like this — a reader who sees "undecided"
  re-opens a settled question, and a reader who sees "decided" ships a
  file nobody agreed to redistribute.
  - **ANSWERED (operator, 2026-08-12, verbatim):** *"use whichever one is
    best for everyone including other languages, or heck, just build for
    both."* → **BOTH engines, behind Cargo features**, with
    **multi-language coverage** as the stated ranking criterion. The
    feature mechanism that makes "both" affordable shipped as `Pass 70.0`
    (`fbcb946`) **one commit before** the decision that needs it.
  - **STILL OPEN — `ROADMAP.md` open operator question `(bl)`:** whether
    a **CC-BY-SA-4.0 model file** may ship inside pdfcer's **MIT** portable
    folder. The pure-Rust engine (`ocrs`/`rten`) is **the only OCR route
    that passes pdfcer's wasm32 CI gate** — every alternative makes OCR the
    first feature that cannot cross into the web fork — and its **weights
    are copyleft**. The Apache-2.0 alternative (PaddleOCR via `ocr-rs`)
    covers **50+ languages** but has **no WASM**. This is a legal reading,
    therefore Ken's. *Default if unanswered: ship neither model set.*
  - **Shipped meanwhile:** the engine-**independent** substrate
    (`9f2af1d`, `pdfcer-core::ocr`, ISO 32000-1 §9.3.6 Table 106 mode 3),
    and `tools/check-shipped-assets.py` (`e3fb7e0`), which **enforces**
    that every redistributed asset states its licence — **enforcement is
    not acceptance.**
  - **Sourcing record:** `docs/ocr-engine-survey.md` (2026-08-12).
    **Surya is recorded there as a trap** — Apache-2.0 code, **modified
    Open RAIL-M weights with a $5 M revenue cap**; field-of-use
    restrictions cannot be bundled in an MIT app, so do not re-evaluate it
    on its accuracy numbers. **Tesseract's default Windows build ships
    LGPL binaries**, so the KillerPDF precedent below is not the free
    default it looks like. `PRIOR_ART.md` notes KillerPDF
  bundles Tesseract natively as a working precedent; OCRmyPDF's
  "sandwich" text-layer approach is the behavioral reference.
- Poppler's exact license (GPL vs LGPL) — unresolved in `PRIOR_ART.md`,
  re-verify before it matters to any decision.
- `pdfcer`'s exact subcommand surface — scoped incrementally,
  feature by feature (see `ROADMAP.md`); Pass 0 only needs the `clap`
  scaffold + a minimal `inspect` subcommand.
