# pdfcer — Legal posture

This file exists because pdfcer sits at the intersection of three legal
concerns most projects don't have all at once: (1) it aims to be
open-source and eventually public, (2) its entire purpose is
implementing a copyrighted, partially-paywalled ISO standard, and (3)
it deliberately targets feature parity with a specific commercial
product. None of this blocks the project; all of it needs a documented,
consistent posture so nobody (human or LLM) makes an ad hoc call under
time pressure that creates exposure later.

## 1. Open-source license — DECIDED: MIT (2026-08-01)

**pdfcer is MIT-licensed.** The operator chose MIT explicitly on
2026-08-01. Implemented same-session: a standard-text `LICENSE` file
at the repo root (`Copyright (c) 2026 Ken Mantle`), `license = "MIT"`
in `Cargo.toml`'s `[workspace.package]`, and `license.workspace = true`
on all four member crates (`pdfcer-core`, `pdfcer-render`, `pdfce-gui`,
`pdfcer`) — `cargo metadata` confirms each resolves to MIT. Recorded
in `ARCHITECTURE.md` §12 Decision log (2026-08-01 entry) per this
section's former instruction.

**Dependency-compatibility check performed as part of the decision:**
every dependency in the workspace's `Cargo.lock` is permissive
(MIT/Apache-2.0/BSD/ISC/Zlib/Unicode) — **zero copyleft** — verified
against the generated `THIRD_PARTY_LICENSES.md` (§6.3). MIT is
therefore compatible with the dependency set as it stands; no existing
dependency needs to change or be re-flagged as a result of this
decision.

**Consequence — copyleft prior art is now categorically, permanently
off the table as a dependency.** Per §6.1 below: an MIT-licensed pdfcer
cannot link GPL/AGPL code into its own distributed binary. This
forecloses **MuPDF, Poppler, and Ghostscript** (all AGPL-3.0 or
GPL-family, see `docs/PRIOR_ART.md`) as real dependencies for good —
they remain read-only architectural/algorithmic reference only
(independently reimplemented, never copied), the same posture
`PRIOR_ART.md` already recommended on the merits, now also locked in
by the license itself. This is not a new restriction in practice (no
copyleft dependency was ever adopted), but it removes the
hypothetical "choose AGPL and unlock MuPDF/Poppler/Ghostscript" branch
this section used to describe as a live option.

**What this decision did NOT do — it did not, by itself, authorize a
push.** MIT satisfied project rule 8's license precondition for a
public-facing commit posture; publishing remained a separate act.

**★ SUPERSEDED BY EVENTS, 2026-08-09.** This paragraph used to end *"the
project's first implementation commit (`d8b3903`) remains local only"*.
It is not local. The operator created `github.com/KenM76/pdfcer`
(**public**) on 2026-08-09 04:56Z and pushed `main` at 10:18Z. Verify
rather than trust this sentence too — `git remote -v` and
`gh repo view KenM76/pdfcer --json visibility`.

A push still requires the operator's own act; what changed is that the
act happened. See §1.1 for what came with it.

**★ NARROWED 2026-08-27 (decision 090).** "A push still requires the
operator's own act" no longer holds for an ordinary fast-forward push of
`main` — Ken's ruling *"always push"* made that standing-authorized. It
still holds, unchanged, for cutting a release, force-pushing, or pushing
any branch other than `main`. See §1.1's own 2026-08-27 amendment and
`CLAUDE.md` rule 8 for the full text.

Historical note (context for why this took the shape it did):
realistic candidates presented to the operator were **MIT or
Apache-2.0** (permissive, easiest adoption/embedding) vs. **GPL-3.0 or
AGPL-3.0** (copyleft, blocks proprietary forks/hosted-service
loopholes, would have unlocked MuPDF/Poppler/Ghostscript as real
dependencies). The operator chose MIT.

### 1.1 git history carries a third party's confidential material — AND THE REPOSITORY IS ALREADY PUBLIC (2026-08-09; corrected and decided 2026-08-10)

> **★ THIS SECTION WAS WRONG FOR ONE DAY, IN THE DIRECTION THAT MATTERED.**
>
> Written 2026-08-09 as a *prospective* blocker — "publishing would
> disclose this" — on the stated basis that no git remote existed. **That
> basis was false when it was written.** `github.com/KenM76/pdfcer` was
> created 2026-08-09 04:56Z and pushed at 10:18Z; this section was written
> that evening and asserted the opposite.
>
> Found 2026-08-10 by running `git remote -v` while verifying something
> unrelated. Verified, not inferred: `gh api
> repos/KenM76/pdfcer/contents/tools/realdrawings-smoke?ref=817d518^`
> **returns the directory listing** — the harness whose own README said
> *"Nothing in this directory is to be committed at all"* is fetchable by
> anyone from the public repository.
>
> The lesson is not "check the remote." It is that **this document
> asserted an environmental fact it had not measured**, in the one section
> written to warn about that fact, and the assertion made the risk read as
> hypothetical for a day. Any claim here about the state of the world —
> what is pushed, what is configured, what is reachable — must be a
> command someone ran, quoted, or it does not belong in this file.
>
> **OPERATOR DECISION, 2026-08-10: leave the repository public and accept
> it.** Presented with the measured exposure and four options (make
> private now and decide later; rewrite history and force-push; leave
> public and correct the record; or gather a full inventory first), the
> operator chose to leave it public and correct the record. Open question
> **(bh)** is therefore **CLOSED — resolved as "accept"**, not still open.
>
> What that decision does and does not mean, so a later reader does not
> over-read it: it settles the material already in history. It is **not**
> a standing licence to publish anything else, and §1's rule that a push
> is the operator's act still holds.

> **★ AMENDED 2026-08-27 (decision 090).** The closing clause above —
> *"§1's rule that a push is the operator's act still holds"* — is now
> **half true**. Ken's ruling *"always push"* (given 2026-08-27, in answer
> to being asked three times in one session) grants an ordinary
> fast-forward push of `main` to `origin/main` **standing** authority: no
> further per-push go-ahead is needed. **What survives unchanged**: this
> section's own point — that the 2026-08-10 decision was not a standing
> licence to publish *other* material — is untouched, because that was
> never about the push mechanic. **What also survives, named explicitly so
> this paragraph is not over-read the other way:** cutting a tag or
> release, force-pushing or rewriting published history, and pushing any
> branch other than `main` all still need their own explicit, current
> go-ahead. See `CLAUDE.md` rule 8 and `ARCHITECTURE.md` §12 decision 090
> for the full ruling and its narrowings.

**The facts, measured rather than estimated.**

Commit **`817d518`** ("remove a third party's confidential material from
the tree") removed, from the *working tree*:

- `tools/realdrawings-smoke/` — **deleted**. It named a real company's
  drawings as confidential and gave the filesystem path to them. Its own
  `README` said *"Nothing in this directory is to be committed at all"*,
  and all three files had been committed anyway. The `ROADMAP.md` entry
  describing it claimed *"nothing proprietary is committed"*, which was
  false: the RESULTS were gitignored, the harness was not.
- A named drawing identifier and its path, redacted to a neutral
  placeholder across `ROADMAP.md`, `SESSION_LOG.md`, `tools/gui-drive.ps1`
  and one agent-memory file, together with a pair of strings lifted from
  that drawing's own text.
- The provenance of the corpus census in `docs/decisions/008` — the
  operator's employer and their document store — with every number kept.

**None of that removal reaches git history.** Nothing has ever been
deleted from this repository (819 paths ever, 746 tracked at that
commit, every difference a directory object), so all of the above is
still fully recoverable from the **288 commits that precede `817d518`**.
A `git clone` of a published repository fetches those commits. The
material would be as public as if the removal had never happened.

**Verify before relying on this paragraph rather than after** — the
counts move with every commit:

```bash
git log --oneline | wc -l               # total commits
git log --oneline 817d518 | wc -l       # commits up to and including the removal
git log --all --full-history -- tools/realdrawings-smoke   # the deleted harness, still reachable
```

**THE DECISION WAS THE OPERATOR'S AND HAS BEEN MADE: ACCEPT (2026-08-10).**

The options that were on the table, recorded because the reasoning
behind a closed decision is the part that stays useful:

| Option | What it would have cost |
|---|---|
| **Rewrite history** (`git filter-repo` over the affected paths and strings, then force-push) | Every commit hash changes. Every hash cited in `ROADMAP.md`, `SESSION_LOG.md`, `tools/commits-filed-baseline.txt` and the decision log goes dangling — and this project's filing gates are built on those hashes. **It is also incomplete on its own:** GitHub keeps unreachable objects fetchable by SHA until Support purges them. |
| **Squash to a fresh initial commit** | The engineering record — a substantial part of this project's documentation-first value — stops existing publicly. |
| **Make it private while deciding** | Seconds, reversible, stops further exposure. Not chosen. |
| **Accept** ← **CHOSEN** | The material stays reachable. Weighed against 0 forks and 0 stars at the time of the decision. |

**Binding on `pdfcer-engineer` going forward:**

- **Do not re-open this to be helpful.** It is decided. Raising it again
  as though it were open wastes the operator's attention on a question he
  has already answered with the facts in front of him.
- **Do not treat it as precedent.** It settles *this* material. The next
  third-party file committed here is a new decision, and `tools/` still
  has a temp-folder convention precisely so there is no next one.
- **`817d518` did what it said and no more.** It cleaned the working
  tree; it never reached history, and its own final paragraph says so in
  capitals. Do not cite it as the problem being closed.

## 2. ISO / ITU-T / ETSI standard copyright — how the spec RAG is scoped

The PDF ecosystem's normative documents have genuinely mixed licensing:

| Document | Publisher | Free to download? |
|---|---|---|
| ISO 32000-1:2008 (PDF 1.7) | ISO, but originally Adobe-authored | **Yes** — Adobe published the identical text freely; this is the practical primary source for the PDF 1.7 baseline. |
| ISO 32000-2:2020 (PDF 2.0) | ISO | **No** — paywalled (~200 CHF), not freely redistributable. |
| ISO 19005-1/2/3/4 (PDF/A) | ISO | **No** — paywalled. Free secondary sources exist (PDF Association technical notes, veraPDF validation rules/corpus, the Isartor test suite) that encode most of the same normative content in a legitimately free/open form. |
| ISO 14289 (PDF/UA) | ISO | **No** — paywalled. Free secondary sources: PDF Association technique documents, PAC (PDF Accessibility Checker) documentation. |
| ETSI EN 319 142-1/2 (PAdES) | ETSI | **Yes** — ETSI's standard practice is free publication. |
| ITU-T T.4 / T.6 (CCITT Group 3/4) | ITU-T | **Yes** — ITU-T Recommendations are freely downloadable from itu.int. |
| ITU-T T.88 (JBIG2) | ITU-T | **Yes** — same. |
| ITU-T T.81 (JPEG) / ISO 10918-1 | ITU-T (free) vs ISO (paywalled) — identical content | **Yes, via the ITU-T copy.** |
| ITU-T T.800 (JPEG2000) / ISO 15444-1 | ITU-T (free) vs ISO (paywalled) — identical content | **Yes, via the ITU-T copy.** |
| Adobe XMP Specification (Parts 1-3) | Adobe | **Yes** — Adobe publishes this directly. |
| ICC.1:2022 (ICC profile format) | International Color Consortium | **Yes** — color.org publishes it free. |
| OpenType spec | Microsoft (practical source) vs ISO/IEC 14496-22 (paywalled, same content) | **Yes, via Microsoft's free copy.** |
| Adobe Supplements / Extensions to ISO 32000, legacy XFA spec | Adobe | **Yes** (historically published freely, though URLs move — verify current location, don't trust a stale link). |

**The pattern:** wherever ISO paywalls a standard whose content
originated with (or is mirrored by) a standards body with an open-
publication norm (ITU-T, ETSI) or the original corporate author
(Adobe, Microsoft, the ICC), prefer that free source. `pdfcer-spec-
librarian` owns applying this table — see its agent file for the
full sourcing protocol.

**Redistribution rule (binding on `pdfcer-spec-librarian`):**

- RAG files may **paraphrase and summarize** normative content, and
  may include **short verbatim quotations** (a sentence, a table row)
  with a clear citation (document + clause/section/table number).
  This is standard, low-risk technical-reference practice.
- RAG files must **not** bulk-copy multi-paragraph verbatim text from
  a paywalled source (ISO 32000-2, ISO 19005, ISO 14289) into the RAG.
  For those, work from the free secondary sources (PDF Association /
  veraPDF notes, the freely-available ISO 32000-1 baseline plus public
  PDF-2.0 delta summaries) and mark paraphrased sections as such.
- The raw source documents themselves (whether freely downloaded or,
  if the user owns a purchased ISO copy, provided locally) are staged
  under `D:\Dev\Rag-Specialized\PDF_Spec\_sources\` and are **never**
  committed to the pdfcer git repository and **never** referenced from
  any pdfcer release artifact.
- The RAG directory `D:\Dev\Rag-Specialized\PDF_Spec\` itself lives
  **outside** the pdfcer repository. If it is ever put under version
  control for the user's own backup purposes, that repository must be
  **private**, never public, never a release asset — same discipline
  as the existing "SolidWorks tools are PRIVATE" rule in the user's
  global CLAUDE.md, applied here to licensed reference material
  instead of proprietary work product.

## 3. Patent posture (brief, non-exhaustive — flag to the user if a specific filter/codec raises a real question)

- **JBIG2** arithmetic coding (MQ-coder) patents have expired; treat as
  clear, but if implementing JBIG2 symbol/refinement coding raises a
  specific still-live patent question, stop and ask rather than assume.
- **CCITT Group 3/4** — decades-old ITU-T fax standards, no known live
  patent concerns.
- **JPEG (baseline DCT)** — the historical JPEG patent disputes (e.g.
  Forgent Networks) are long resolved/expired; treat as clear.
- **JPEG2000** — most core patents have expired given its age; if a
  specific optional feature (e.g. certain wavelet variants) is flagged
  by a crate's own documentation as patent-encumbered, respect that
  crate's guidance rather than re-deriving a legal opinion from scratch.
- This section is a starting orientation, not a legal opinion. If a
  genuine patent-risk question comes up for a specific feature, that's
  a "ask the user" moment, not a "the engineer decides" moment — patent
  risk is qualitatively different from the usual engineering judgment
  calls this project's agents make solo.

## 4. Trademark posture

- "Acrobat", "Adobe", the Adobe PDF logo, and Adobe's product UI/icon
  trade dress are **not** to be used in pdfcer's name, branding,
  marketing copy, icons, or about-box text. "Feature-for-feature
  replacement for Acrobat Pro" is fine as an internal engineering/
  roadmap framing (as used in `ARCHITECTURE.md` and `ROADMAP.md`); it
  needs softer, non-infringing phrasing in any public-facing copy
  ("a free, open-source alternative to commercial PDF editors" style) —
  this is a `pdfcer-librarian` / user judgment call when public-facing
  copy is actually drafted, not a concern for internal engineering docs.
- The PDF format itself and the word "PDF" are not trademark-
  restricted for describing file-format compatibility; this is a
  different question from using Adobe's product branding.

### 4.1 "pdfcer" name-collision check (2026-07-23)

Before treating "pdfcer" as the final public-facing name (not just a
dev codename): a practical collision check was run, web-search-level
(not a formal trademark-registry search).

- **crates.io**: `pdfcer` is unregistered — confirmed via a direct API
  query returning 404 "crate `pdfcer` does not exist." Clear.
- **GitHub**: no `pdfcer` user or organization exists — confirmed via
  direct query (404). A fuzzy name search turned up ~32 unrelated
  repos (`pdfcevir`, `PDFCertificateGenerator`, etc.), none with real
  prominence (single-digit stars). Clear.
- **Trademark**: no confirmed registered "PDFCER" mark found via
  general web search. **This was not a formal USPTO TMsearch-database
  query** (blocked/not attempted at that depth) — good enough to keep
  using the name now, but run an actual USPTO search before any formal
  trademark filing.
- **Confusion risk**: low. Doesn't phonetically or visually resemble
  Acrobat, Acrobat Pro, Acrobat Reader, or other known PDF tools
  (Foxit, Stirling-PDF, PDFCreator, PDFgear, pdfFiller). Reads as an
  initialism, not a merely-descriptive term like "PDFEdit" would be —
  plausibly more defensible if trademark protection is ever pursued,
  though that depends on what "CE" is understood to stand for.

**Bottom line: no blocking issue found.** Safe to keep "pdfcer" as the
working and likely-final name; revisit only if a formal trademark
filing is ever pursued (do the USPTO search then, not now).

## 5. Test corpus sourcing (binding on pdfcer-engineer)

- Fixture PDFs checked into `fixtures/` must be either: (a) synthetic,
  generated by pdfcer's own tooling or a documented script, or (b)
  drawn from a corpus with clear redistribution rights (e.g. the PDF
  Association's public test suites, veraPDF's open corpus, or files
  the user personally authored and has rights to redistribute).
- **Never** check in a real-world PDF of unknown provenance (a
  downloaded invoice, a scanned document found online, an AI-generated
  "looks like a real business PDF" test file) without confirming its
  license/rights situation first. This mirrors the SWFormat project's
  "no client IP in any artifact" discipline, applied to PDFs instead
  of SOLIDWORKS files.
- If a bug report requires a specific real-world PDF to reproduce and
  its provenance is unclear, keep it in a local, non-committed
  scratch/debug location — describe the bug and the minimal structural
  cause in the SESSION_LOG / lesson instead of committing the file
  itself.
- **★ The veraPDF CORPUS named above is NOT the veraPDF VALIDATOR.** This
  section governs the **corpus** (a rights-cleared source of fixture
  PDFs, in use since 2026-07-30). The **validator application** — a
  dual-licensed GPLv3+/MPLv2+ tool pdfcer **runs** but never ships — is
  governed by **§6.5**, which records pdfcer's **MPL-2.0 election** and six
  binding usage rules. **Two artifacts, two licence questions; do not
  answer one with the other.**

## 6. Open-source dependency licensing & attribution

pdfcer leans on the existing Rust/OSS ecosystem rather than
reinventing everything (see `docs/PRIOR_ART.md` for the actual
survey). Every dependency brings its own license, and pdfcer's own
license — ~~(§1, still undecided)~~ **MIT, decided 2026-08-01 (§1)** —
determines what's even usable; this section is the binding discipline
for that intersection.

> **⚠ CORRECTED 2026-08-07. The struck words above were WRONG for six
> days.** §1 has read *"DECIDED: MIT"* since 2026-08-01, and §6.1 below
> has carried the consequence inline the whole time — so no careful
> reader was misled, but a reader who stopped at this paragraph was.
> **This is a single-location-amendment failure**, the same defect the
> *Update protocol*'s same-filing propagation duty exists to prevent, and
> the fourth time this project has hit it. The original wording is struck
> rather than deleted so the failure stays visible. **What pdfcer's own
> licence gates has NOT changed in substance** — §6.1 has stated the MIT
> consequence (GPL/AGPL categorically out) since the decision was made.
> Every other editable location carrying the stale claim was corrected in
> the same filing; see §7's 2026-08-07 second entry for the full swept
> set, the patterns used, and the stale statements outside `docs/` that
> are reported rather than edited.

### 6.1 The permissive/copyleft split, and why it gates the §1 decision

> **⚠ HEADING IS HISTORICAL, corrected 2026-08-07 — read it in the past
> tense.** It gated the §1 decision; **§1 was decided (MIT) on
> 2026-08-01**, so this split now runs the other way — it says what a
> *dependency* may be, given a licence pdfcer has already chosen, not what
> pdfcer's licence may be. The heading is left unedited because it is a
> section anchor other documents link to. **The bullets below were already
> correct** and were amended in place on the decision date.

- **Permissive** (MIT, Apache-2.0, BSD-2/3-Clause, Zlib): safe to
  depend on regardless of what pdfcer's own license ends up being.
  Most of the Rust crate ecosystem defaults to MIT/Apache-2.0 dual.
  **This is also pdfcer's own license as of 2026-08-01 (§1) — the
  entire current dependency set is permissive, so nothing here
  changes in practice; this classification tier is simply the one
  pdfcer itself now belongs to.**
- **Weak copyleft** (LGPL, MPL-2.0): usable as a dynamically-linked
  dependency in most cases without forcing pdfcer's own license to
  match, but static linking (the Rust ecosystem's norm — everything
  compiles into one binary) can blur that line. **Flag any LGPL/MPL
  dependency to the user before adding it** rather than assuming
  static-linking is fine.
- **Strong copyleft** (GPL-2/3, AGPL-3): if pdfcer **links** GPL/AGPL
  code into its own binary (not just "reads it for inspiration" —
  actual linking/embedding), pdfcer's own distributed binary must also
  be GPL/AGPL-compatible. **DECIDED 2026-08-01: pdfcer is MIT-licensed
  (§1), so GPL/AGPL dependencies are now categorically, permanently
  off the table as real dependencies** — they can only ever be
  read-only architectural/algorithmic reference (independently
  reimplemented, not copied). This forecloses MuPDF, Poppler, and
  Ghostscript (see `docs/PRIOR_ART.md`) as real dependencies for good;
  the "choose AGPL and unlock them instead" branch this paragraph used
  to describe as a live option no longer exists — §1's decision is
  final, not a per-dependency judgment call to revisit.

### 6.2 Rule: no dependency added without a license check

Before `pdfcer-engineer` adds ANY new crate to a `Cargo.toml` (not just
at Pass 0 — every time, for the life of the project):

1. Check the crate's license (its `Cargo.toml` `license` field, or its
   repo's `LICENSE` file if ambiguous).
2. Classify it per §6.1 above.
3. If permissive: proceed, log it in `docs/PRIOR_ART.md`'s adopted-
   dependencies table.
4. If weak or strong copyleft: **stop and ask the user** before adding
   it, even if pdfcer's current license would technically allow it —
   this is a case where getting it wrong is expensive to unwind later
   (ripping out a load-bearing dependency after other code depends on
   its API is real rework), so it warrants a check-in every time, not
   just a one-time policy decision.
5. If a dependency is FFI to a non-Rust library (e.g. binding to a C
   library for JPEG2000/JBIG2 support), the same license check applies
   to that underlying library, AND it reopens the "single Rust binary,
   no heavy runtime" portability question from `ARCHITECTURE.md` §6 —
   flag both concerns together.

### 6.3 Attribution mechanism: generated, not hand-maintained

Hand-maintaining a NOTICE/THIRD_PARTY_LICENSES file is error-prone and
drifts from reality as dependencies change. pdfcer uses **`cargo-about`**
(the standard Rust-ecosystem tool for this) to generate the attribution
file from the actual `Cargo.lock` dependency graph:

- Set up at Pass 0 (or as soon as the workspace has real dependencies):
  a `about.toml` config + a `cargo about generate` invocation that
  produces `THIRD_PARTY_LICENSES.md` (or `.html`) at the repo root.
- Regenerate it as part of the packaging step (§6), not just once —
  a stale attribution file shipped alongside a newer dependency set is
  a real (if usually low-stakes) compliance gap.
- This file **is** meant to ship with releases (unlike the private
  RAGs) — it's the actual legal notice a downstream user/redistributor
  needs. `pdfcer-librarian` doesn't own it; `pdfcer-engineer` regenerates
  it mechanically as part of the release/packaging checklist.

### 6.4 `docs/PRIOR_ART.md`

The living survey of candidate/adopted open-source dependencies and
reference projects, maintained by `pdfcer-engineer` (dispatch
`pdfcer-librarian` for the actual file edits, same discipline as
`ARCHITECTURE.md`'s decision log). Distinct from the generated
`THIRD_PARTY_LICENSES.md`: `PRIOR_ART.md` is the research/decision
record (why a crate was chosen or rejected, what the license
implications were); `THIRD_PARTY_LICENSES.md` is the mechanically
generated compliance artifact. Both matter; they serve different
readers (an engineer deciding what to depend on, vs. a downstream
user checking license compliance).

### 6.5 veraPDF — the licence ELECTION, and the arms-length usage rules (2026-08-07, BINDING)

**Operator request, verbatim, 2026-08-07:**

> *"I would like you to run veraPDF's validator against our own output as
> soon as it would make sense to do so."*
> *"Install whatever you need to get veraPDF working."*
> *"use verapdf in a way that wouldn't cause us to change our license in
> order to stay conforming with it's tos."*

That third sentence is the requirement this subsection discharges. It is
recorded here rather than only in `ROADMAP.md` because it is a **licensing
constraint on the project**, and because the answer has to survive being
found by someone a year from now who has only the artifacts.

#### 6.5.0 What is installed, and where — deliberately OUT of the repo tree

| Fact | Value |
|---|---|
| Product | **veraPDF 1.30.2** (greenfield validator) |
| Install path | **`D:\tools\verapdf`** — outside `D:\Dev\pdfcer\`, deliberately |
| Installer SHA-256 | `6cc6341cb1af644044054b81f00a6590a7918abb18f762243de115258bcad838` |
| GPG | **Good signature**, RSA key `13DD102B4DD69354D12DE5A83184863278B17FE7`, *"Carl Wilson `<techlead@verapdf.org>`"* (veraPDF's tech lead) |
| Runtime | Java **17.0.7** present |

**The install path is part of the legal posture, not a convenience.** A
copy inside the repo tree is one `git add -A` away from being
**redistributed**, which rule 1 below forbids. Keeping it on a separate
path makes the arms-length relationship a property of the filesystem
rather than of anyone's memory.

#### 6.5.1 THE LOAD-BEARING FACT — every veraPDF component is DUAL-licensed GPLv3+ / MPLv2+

Verified per-repository by fetching each project's README from
`raw.githubusercontent.com`: **`veraPDF-apps`**, **`veraPDF-library`**,
**`veraPDF-model`**, **`veraPDF-parser`** — all four state dual
licensing. The `veraPDF-library` README's own words:

> *"The veraPDF PDF/A Validation Library is dual-licensed, see:*
> *- GPLv3+ (`LICENSE.GPL`, GNU General Public License, version 3)*
> *- MPLv2+ (`LICENSE.MPL`, Mozilla Public License, version 2.0)"*

**`LICENSE.MPL` was additionally confirmed to EXIST in `veraPDF-apps`**
(HTTP 200, *"Mozilla Public License, version 2.0"*), **because a README
naming a licence is weaker evidence than the licence file being there.**

**Independently re-confirmed from the INSTALLED BINARY** by the librarian,
2026-08-07 — see §6.5.5 for the exact output and for a correction to how
this banner had been characterised.

#### 6.5.2 THE ELECTION — pdfcer receives veraPDF under **MPL-2.0**, not GPL-3.0

**Under a dual licence the RECIPIENT chooses. pdfcer chooses MPL-2.0.**

**An undocumented choice is an ambiguous one**, and ambiguity is the
entire risk here: a future reader who finds only "veraPDF" and "GPL" in
the same sentence will conclude pdfcer has a licensing problem. **This
subsection exists so that reader finds the election instead.**

**Why MPL-2.0 is safe for an MIT project:** MPL-2.0 is **file-level** weak
copyleft — its obligations attach to the *MPL-licensed files themselves*,
not to everything they touch. **MPL-2.0 §3.3 expressly permits combining
Covered Software into a "Larger Work" distributed under other terms.**
**There is therefore no propagation path from veraPDF to pdfcer's MIT
licence** (§1), even in the counterfactual where pdfcer did something
closer than it does.

#### 6.5.3 The SECOND, INDEPENDENT protection — the usage pattern triggers nothing even on the GPL branch

**Recorded because either protection alone suffices, and a record with two
independent legs does not fall over if one is later disputed.**

- **GPL-3.0 §0 affirms unlimited permission to RUN the unmodified
  program.** Running a validator is not a licensed act requiring
  compliance.
- **Copyleft attaches to DISTRIBUTING a combined or derivative work.**
  pdfcer distributes neither.
- **The GPL reaches neither a program's OUTPUT nor a program that merely
  CONSUMES that output.** A validation report is data about pdfcer's file,
  not a derivative of veraPDF.

So even if the election in §6.5.2 were set aside entirely, the pattern in
§6.5.4 rule 4 would still be clean.

#### 6.5.4 THE ENFORCEABLE RULES — this is the operational answer

**Binding on `pdfcer-engineer` and every agent, for the life of the
project.**

1. **Never vendor, bundle, or redistribute veraPDF** — not in the repo,
   not in a release, not in the single-folder portable package.
2. **Never link or embed** any veraPDF jar, class, or code into a pdfcer
   binary.
3. **Never copy its source, its validation profiles, or its model
   files.** **Reimplementing from the ISO spec is fine; lifting profile
   XML is not.** (This is the same read-vs-copy line §6.1 draws for
   GPL/AGPL prior art, applied to an artifact pdfcer actually executes.)
4. **Separate process only**, invoked over the **documented CLI**, with
   pdfcer consuming the **XML report**. No in-process embedding, no JNI,
   no shading.
5. **DEV-TIME ONLY.** veraPDF is never a runtime dependency of pdfcer and
   **never appears in any `Cargo.toml`.** **It will therefore correctly
   never appear in `THIRD_PARTY_LICENSES.md`** — `cargo-about` generates
   that file from the Cargo dependency graph and can only see Cargo
   dependencies. **This is stated explicitly so nobody "fixes" the
   apparent omission by adding it**; adding it would be adding a
   dependency that does not exist, and would misdescribe pdfcer's
   distributed artifact to its downstream users.
6. **★ THE GATE MUST *SKIP*, NOT *FAIL*, WHEN veraPDF IS ABSENT.** A
   required gate would make veraPDF a **de facto build dependency** —
   muddying exactly the arms-length position §6.5.2/§6.5.3 rest on — and
   would break anyone who clones the repository without it. **A skipped
   gate must say it skipped and why**, so the skip is never mistaken for
   a pass.

#### 6.5.5 ⚠ A CORRECTION, recorded rather than quietly dropped — the CLI banner names BOTH branches

**This subsection was drafted on the understanding that veraPDF's CLI
startup banner *"says only 'Released under the GNU General Public License
v3'"* and was therefore misleading about the dual licence. **The librarian
checked the installed binary and that characterisation is NOT accurate at
1.30.2.** `./verapdf.bat --version` and `--help` both emit, verbatim:**

```
veraPDF 1.30.2
Built: Wed Jun 03 10:55:00 EDT 2026
Developed and released by the veraPDF Consortium.
Funded by the PREFORMA project.
Released under the GNU General Public License v3
and the Mozilla Public License v2 or later.
```

**The banner names BOTH branches. It is not misleading; it is
line-wrapped.**

**But the underlying concern is real, and is better stated once the
mechanism is right.** The licence sentence **spans two lines**, and the
first line — *"Released under the GNU General Public License v3"* — is a
complete, plausible, and **wrong** sentence on its own. **Anyone who
greps the banner, reads a truncated capture, or quotes the first matching
line gets GPL-only.** That is exactly how the mischaracterisation above
arose, and it is a **reproducible reading hazard**, not a one-off
mistake. **The predictable failure mode stands, with a corrected cause:**
someone finds a GPL-only fragment of this banner in a year and concludes
pdfcer has a licensing problem it never had. **§6.5.1 and §6.5.2 are the
answer to give them.**

**How the correction was established (R87):** the librarian ran the
installed `D:\tools\verapdf\verapdf.bat` with `--version` and with
`--help` and read the complete output of each, rather than grepping for a
licence keyword — the grep is the failure mode being described.

**★ AMENDED 2026-08-07 (same day, later) — THE ATTRIBUTION, which is the
part that was still missing: the hazard was the READER'S, not veraPDF's.**
The engineer who made the original claim identified the exact mechanism
and asked for it to be recorded: **the earlier read used `head -5`**,
which cut the banner at exactly line 5 and dropped line 6 — the line
carrying *"and the Mozilla Public License v2 or later."*

**Nothing above blamed veraPDF's wording, and that reading is confirmed
correct rather than changed:** this subsection already says the banner
*"names BOTH branches. It is not misleading; it is line-wrapped."* **What
is added is whose truncation it was.** The paragraph above describes the
hazard generically (*"anyone who greps the banner, reads a truncated
capture, or quotes the first matching line"*), which leaves open the
reading that the *banner* set the trap. **It did not. The trap was set by
the tool invocation, on the reader's own side of the pipe** — a
line-limited read of output the reader chose to limit. veraPDF's text
is fine, and a future reader must not "fix" the banner, file an upstream
issue about it, or cite it as a licensing ambiguity.

**Why the generic hazard is nevertheless kept, and not softened into
"someone made a mistake":** the shape is **reproducible and
self-inflicted**, which is the worst combination. *A truncated read of a
WRAPPED sentence yields a complete, plausible, and WRONG sentence — one
that carries no syntactic signal that it was cut.* A truncated word or a
dangling clause announces itself; a truncated **sentence** does not.
`head -n`, `--max-count`, a tail-limited capture, and a preview pane all
produce it, and the reader has no cue to check. **The countermeasure is
mechanical**: when a licence, a version, a warranty or any other
claim-bearing line is read from tool output, read the output **whole**,
or grep with trailing context (`-A2`), and never quote the first matching
line as the complete statement.

**Escalated as a cross-project finding to `C:\personal_rag\claude_code\`**
— it is tool-invocation methodology, neither PDF-domain nor
Rust-ecosystem, and this instance is its sharpest form because the
truncation came from the invocation rather than from the data.

#### 6.5.6 Two boundaries that must not be blurred

- **§6.2's "stop and ask the user before adding copyleft" is SATISFIED,
  from the other direction: the OPERATOR REQUESTED IT.** The rule exists
  so a copyleft artifact never enters the project on an agent's solo
  judgment. Here the operator asked for veraPDF by name and simultaneously
  set the constraint (*"in a way that wouldn't cause us to change our
  license"*). **No further sign-off is outstanding for the arms-length,
  dev-time-only use described above** — and §6.2 is **unchanged** for
  anything else, including any future proposal to treat veraPDF as a real
  dependency, which rules 1/2/5 forbid outright.
- **The veraPDF CORPUS and the veraPDF VALIDATOR are SEPARATE ARTIFACTS
  WITH SEPARATE TERMS. Do not conflate them.** The **corpus** has been in
  use since 2026-07-30 as a test-fixture source under **§5** (which names
  it as a rights-cleared corpus) and appears in ~100 places across this
  repository — every one of those pre-existing references is to the
  **corpus**, not the tool. This subsection governs the **validator
  application** only, and is the **first** mention of it anywhere in the
  repository. **Established by `git grep -i verapdf` over tracked files
  and reading every hit (2026-08-07): all prior hits are corpus
  references, the PDF/A spec-sourcing note in §2, or the `PRIOR_ART.md`
  row about KillerPDF's own corpus testing. `git grep -E '\bMPL\b'`
  confirms no prior MPL classification of veraPDF existed** (R87 — stated
  so the absence is checkable rather than asserted).

### 6.6 pdf.js — behavioural reference only (2026-08-10), the second instance of R61's pattern

**Flagged for recording by `pdfcer-engineer` while sourcing Pass 7.2's
posture-B AcroForm-JavaScript recompute (decision 009); written up here
because it is the SAME shape of question `ARCHITECTURE.md` §12's
2026-08-01 entry and standing rule **R61** already answered for Inkscape,
not a new question.**

**What happened.** Decision 009's posture B (native Rust reimplementation
of a whitelisted subset of Acrobat's `AFSimple_Calculate`/`AF*_Format`
JavaScript helpers — never executing any script, R53) needed to know real
Acrobat *behaviour* for edge cases the Adobe API reference alone does not
settle: how a blank operand participates in `SUM`/`AVG`/`PRD`/`MIN`/`MAX`,
whether a decimal comma is reinterpreted, how `AFPercent_Format` relates
its stored value to its displayed one. Every attempt to fetch Adobe's own
primary *JavaScript for Acrobat API Reference* PDF this session returned
binary structure with no extractable text. Mozilla's **pdf.js** —
**MPL-2.0**, an independent clean-room reimplementation of Acrobat's
form-scripting behaviour, no code shared with Adobe's — was consulted for
the *behaviour* those edge cases produce.

**The R61 pattern, applied.** R61 (2026-08-01, decision 010) established:
a copyleft project may be a **behavioural reference** for pdfcer — never a
dependency, never a code source, never (in Inkscape's case) a GUI-mimicry
target — provided pdfcer reimplements independently from the observed
behaviour and never links or copies. pdf.js sits on **weaker copyleft**
than Inkscape's GPL-2.0-or-later (MPL-2.0 is file-level weak copyleft,
§6.5.2 above), so the same posture is, if anything, on firmer ground here
than it was for R61's original case.

**What was and was not done, stated so it is checkable:**
- **Read**: pdf.js's publicly-documented/observable calculation and
  formatting behaviour (via its source, since it is the only available
  primary evidence of Acrobat-compatible JS behaviour when Adobe's own
  reference could not be extracted this session).
- **Never copied**: no pdf.js source, expression, or algorithm text
  appears in `pdfcer-core`. The Rust implementation
  (`crates/pdfcer-core/src/form_script/`) was written from the *observed
  behaviour*, independently.
- **Never linked**: pdf.js is not a Cargo dependency, build-time or
  otherwise, and will not appear in `THIRD_PARTY_LICENSES.md` for this
  reason — same "correctly absent, don't fix it" note as §6.5's veraPDF
  dev-tool rule.
- **Evidence-tier discipline carried into the code**: `form_script`'s doc
  comments tag each behavioural claim `ADOBE-PRIMARY` / `PDFJS-CLONE` /
  `COMMUNITY` / `GAP` rather than flattening everything to "Acrobat does
  X" — a single-source behavioural claim is marked as such at its use
  site, per this project's general claim-sourcing discipline (global
  CLAUDE.md, "Claim-bearing copy").

**No new standing rule minted.** This is R61's existing scope, applied to
a second copyleft project in a second subsystem — recorded as a §6.6
precedent entry (matching R61's own "behavioural reference only" framing)
rather than as R186, because minting a rule number for an instance of an
already-stated rule would let a future reader satisfy "check R61" and
"check R-pdf.js" separately and still miss that they are the same
constraint under two names — the same reasoning `ROADMAP.md` Standing
rules gives for treating a scope note as an amendment rather than a peer
rule (2026-08-07 R156/R87 amendments).

**Full technical record:** `docs/decisions/009-forms-javascript-posture.md`
§6 (whitelist scope) and `ARCHITECTURE.md` §12's 2026-08-10 entry
(design points + evidence-tier discipline for Pass 7.2).

### 6.7 CC-BY-SA-4.0 OCR model weights — the operator ACCEPTED them into the MIT portable folder (2026-08-13, BINDING)

**Operator's answer, verbatim and in full, 2026-08-13:**

> *"yes to the license. keep going."*

**That is the entirety of what he said, and it is recorded at exactly that
length.** He was answering `ROADMAP.md` open operator question **`(bl)`**
as this project had been carrying it:

> **May a CC-BY-SA-4.0 model file ship inside pdfcer's MIT single-folder
> portable distribution?**

**Answer: YES.**

#### 6.7.1 Why this needed §6.2 step 4 in the first place

§6.2 step 4 says **stop and ask** for anything that is not permissive,
*"even if pdfcer's current license would technically allow it."*
CC-BY-SA-4.0 is not permissive, so the rule fired on its own terms. Two
further features made it a genuine question rather than an obvious no:

- **§6.1's categorical GPL/AGPL bar is about LINKING.** CC-BY-SA-4.0
  attaches to a **data file** pdfcer would **redistribute**, not to code it
  links. The bar does not reach it, and nothing else in this document did
  either.
- **The alternative is not equivalent.** The pure-Rust route
  (`ocrs`/`rten`) is **the only OCR candidate that passes pdfcer's own
  wasm32 CI gate** — every alternative makes OCR the first feature that
  cannot cross into the web fork (`ARCHITECTURE.md` §3) — and its weights
  are the copyleft ones. PaddleOCR's weights are **Apache-2.0** and cover
  **50+ languages**, but that route has **no WASM**. The honest framing
  was *"copyleft weights and a web future"* versus *"permissive weights,
  more languages, no web future"*, **not** *"one is clean and one is
  not"*.

#### 6.7.2 The reasoning the operator accepted — attributed as the SURVEY'S READING, not as an agent's legal conclusion

`docs/ocr-engine-survey.md` §3.3 set out the analysis below. **It is
recorded here as the reading that was put in front of the operator and
that he accepted, not as a legal conclusion this project's agents
reached or are entitled to reach.** The distinction is the whole reason
the question was escalated rather than cleared.

The survey's reading, in its own terms:

- CC-BY-SA is a licence on a **creative work**, not a software licence,
  and **has no linking concept at all**. Creative Commons' own FAQ
  *"recommend[s] against using Creative Commons licenses for software"*
  because CC licences *"do not contain specific terms about the
  distribution of source code"*.
- CC distinguishes a **collection** — BY-SA material sitting alongside
  other material, where the collection may carry its own licence — from
  an **adaptation**, which modifies the underlying work and must itself
  be released under BY-SA. On that reading, **shipping the unmodified
  `.rten` files next to MIT code is distribution of a verbatim work in a
  collection, and there is no propagation path to pdfcer's own MIT
  licence.**
- **This is the same SHAPE of reasoning §6.5.2 already applied to
  MPL-2.0 for veraPDF** — a copyleft artifact whose own terms permit the
  surrounding larger work to carry other terms. §6.5.2 is the
  neighbouring precedent, and the parallel is why the survey framed the
  question as answerable at all.

**The survey itself named three things that resist being cleared by an
agent**, and they are why this subsection exists rather than a code
comment: the collection-vs-adaptation call is a **reading** and not
something the licence text names; §6.2 step 4 fires on its own terms; and
**any adaptation clearly does propagate** (see 6.7.3 item 3).

#### 6.7.3 ★ FOUR THINGS THIS ACCEPTANCE DOES NOT DECIDE

Recorded together because the gap between *"the licence is accepted"* and
*"therefore X"* is exactly where a wrong inference would live.

1. **It is not authority to publish or release anything.** §1's publish
   gate and project rule 8 are **untouched** — pushing and releasing
   remain separate, still-ungranted operator acts. **The repository is
   public** (§1.1), so a bundled weight file is **published the moment it
   is committed**. The licence answer removes a licence obstacle; it does
   not remove the publish gate, and the gate does not become weaker
   because the obstacle in front of it is gone.
2. **It does not choose the engine.** The engine question was answered
   separately on 2026-08-12 — *"use whichever one is best for everyone
   including other languages, or heck, just build for both"* → **both
   engines, behind Cargo features**, ranked on multi-language coverage.
   **`(bl)` removes the licence obstacle in front of the pure-Rust one
   only.**
3. **It does not clear an ADAPTATION.** Survey §3.3 is explicit:
   fine-tuning, quantizing, retraining, or **converting the weights to
   another runtime's format** plausibly creates **Adapted Material**,
   which must then be released under CC-BY-SA-4.0 or a compatible
   licence. That binds **the derived model**, not pdfcer's source — but
   *"we'll just fine-tune it later for CAD drawings"* **is a decision
   with a licence attached. A future Pass that touches the weights owes
   its own operator decision under §6.2 step 4; this acceptance does not
   cover it.**
4. **It does not close the attribution obligation — it CREATES it.**
   CC-BY-SA-4.0 requires **attribution, a licence notice/link, and an
   indication of changes**. §6.3 makes `THIRD_PARTY_LICENSES.md` the
   compliance artifact and it is generated by `cargo-about` **from the
   Cargo dependency graph**. **A model file is not a Cargo dependency, so
   `cargo-about` will not see it, will not attribute it, and nothing will
   fail.** The compliance artifact for the weights must therefore be
   **authored deliberately** — a `PROVENANCE.md` naming the licence, plus
   a citation in `about.hbs` — which is exactly what
   `tools/check-shipped-assets.py` (`e3fb7e0`) already **enforces** for
   any `crates/*/assets/` directory. **Enforcement is not acceptance; the
   acceptance is what arrived on 2026-08-13, and the enforcement is what
   makes it verifiable.**

   Note the direction of the §6.5.4-rule-5 hazard here. For veraPDF the
   absence from `THIRD_PARTY_LICENSES.md` is **correct** and must not be
   "fixed". For a bundled model the absence would be **incorrect** — and
   **it looks identical**.

#### 6.7.4 Provenance is thinner than one would want, and pdfcer must PIN AND HASH

Recorded because it is an obligation the acceptance creates, not a
reservation about it:

- **`ocrs-models` has no LICENSE file.** The CC-BY-SA-4.0 declaration
  exists **only on the Hugging Face model card** (`cardData:
  {"license": "cc-by-sa-4.0"}` plus the `license:cc-by-sa-4.0` tag). That
  is a thinner provenance record than one would want for the one
  non-permissive artifact in the build.
- **The two distribution channels are not byte-identical**, measured in
  survey §3.4 — **and ★ SINCE 2026-08-25 THIS TABLE ALSO RECORDS WHICH
  COLUMN pdfcer ACTUALLY SHIPS, WHICH IT PREVIOUSLY DID NOT** (`Pass
  129.0`, `181d9bd`, two-hundred-and-sixty-second filing). The prior
  version tabulated both channels and totalled them, describing a choice
  pdfcer **had not yet made in fact** while the repository had already made
  it silently:

| File role | S3 bytes | Hugging Face bytes | Delta (S3 − HF) | **BUNDLED** |
|---|---|---|---|---|
| text detection | **2,510,284** | 2,523,564 | **−13,280 B** | **★ S3** — `text-detection.rten`, sha `f15cfb56…` |
| text recognition | 9,716,568 | **9,716,444** | +124 B | **HF** — `text-rec-checkpoint.rten`, sha unchanged since 2026-08-13 |
| **total, 2 files, AS BUNDLED** | — | — | — | **12,226,728 B (≈ 12.23 MB) = 6.11 MB each** |
| *(channel totals, for reference only)* | *12,226,852* | *12,240,008* | *−13,156 B (0.11 %)* | *neither is what ships* |

  **★★ WHY THE S3 DETECTION ARTEFACT IS THE ONE BUNDLED, and this is a
  FUNCTIONAL fact, not a licensing preference.** **The Hugging Face
  detection build does not work with `ocrs` 0.12.2.** On a clean 150 dpi
  render of 12 pt Helvetica it returns sixteen fragments at the right page
  margin plus one "word" whose box is the whole page — noise, not degraded
  output. Isolated by swapping **one file at a time** (4 runs over 2 files
  × 2 channels): S3 detection + **HF** recognition is **perfect**, so the
  recognition model was never at fault and **only the detection file was
  replaced**. Between 2026-08-13 and 2026-08-25 **every OCR run pdfcer made
  produced garbage.**

  **The licence consequence, stated because it is the reason this belongs
  in this file.** The **S3 channel carries no licence text of its own** —
  the **CC-BY-SA-4.0 declaration lives only on the Hugging Face model
  card**, for the same author's same network. **The operator was told
  exactly that and authorised bundling the S3 artefact on 2026-08-25.**
  The licence conclusion below is therefore **unchanged** (unmodified
  works in a **collection**, not an **adaptation**; pdfcer's MIT licence
  unaffected), but **the provenance chain for the detection file now runs
  through a different host than the licence declaration**, and
  `PROVENANCE.md` states that mixture rather than implying one channel.

  **The filenames differ between channels as well** — which is precisely
  how two different models came to look like one. **So pdfcer must pin
  exactly which artifact it ships and hash it, rather than treating "the
  ocrs models" as one thing.** That pinning was an engineering obligation
  of `Pass 71.0` and **was honoured for provenance while the artefact
  pinned was never run end to end** — see `docs/ocr-engine-survey.md`
  §3.4's dated amendment.
- **The weights are stale by construction:** S3 objects carry
  `Last-Modified: Mon, 01 Jan 2024`; `ocrs-models` last saw a push
  2024-08-20. Not a licence fact, but it travels with the same files and
  is easy to lose.

**Mirrors:** `ROADMAP.md` *Open operator questions* → `(bl)` (answered,
ceiling stays `(bl)`, next free `(bm)`), and `Pass 71.0`'s *Next up*
entry, which is **no longer blocked on an operator decision**. Full
evidence: `docs/ocr-engine-survey.md` §3.3–§3.5.

## 7. Decision log

- **2026-07-23** — Legal posture document created at project bootstrap.
  License: **undecided, open item**. No public commit/publish until
  decided. Spec-sourcing table established. Test-corpus rule established.
  **[⚠ SUPERSEDED as to the licence by the 2026-08-01 entry below — MIT.
  Marker added 2026-08-07; the entry itself is correct AS OF ITS DATE and
  is not edited. The publish gate is NOT superseded: it survives the
  licence decision as a separate, still-ungranted operator
  authorization.]**
- **2026-07-23 (same-session amendment)** — Added §6, open-source
  dependency licensing & attribution discipline, per user request to
  survey existing OSS projects for prior art and ensure proper
  crediting. Established: permissive-vs-copyleft classification gates
  what's usable given pdfcer's own (still undecided **[⚠ as of 2026-07-23
  only — MIT since 2026-08-01; marker added 2026-08-07, entry not
  edited]**) license; no
  dependency added without a per-instance check, copyleft always
  flagged to the user; attribution via generated `cargo-about` output
  (`THIRD_PARTY_LICENSES.md`), not hand-maintained; research findings
  land in a new `docs/PRIOR_ART.md`.
- **2026-07-23 (same-session amendment 2)** — Fixed a section-numbering
  bug: this file jumped from §5 straight to a "§7" with no §6 —
  renumbered the dependency-licensing section to §6 (was mislabeled
  §7) and this decision log to §7 (was §8). Updated every cross-file
  reference to the old numbers. Also: **name-collision check on
  "pdfcer" completed, came back clean** — no existing crates.io crate
  (confirmed 404), no existing GitHub user/org (confirmed 404), no
  confirmed trademark or well-known-product conflict via web search,
  low phonetic/visual confusion risk with Acrobat or other PDF tools.
  Not a formal USPTO trademark-database search (recommended before
  any actual trademark filing, not required to keep using the name
  for the repo/crate). See §4 Trademark posture.
- **2026-08-01 — §1 license decision made: MIT.** The operator chose
  MIT (over Apache-2.0 or a GPL/AGPL copyleft option) as part of a
  combined instruction that also set the project's next work focus
  (dimensioning tool, icons, text-handling completion, form-building —
  see `ROADMAP.md` "In progress"/"Next up"). Implemented same-session:
  `LICENSE` file (repo root, standard MIT text, "Copyright (c) 2026
  Ken Mantle"), `license = "MIT"` in `Cargo.toml`
  `[workspace.package]`, `license.workspace = true` on all four member
  crates, `cargo metadata`-confirmed. Dependency-license audit: 100%
  permissive (MIT/Apache-2.0/BSD/ISC/Zlib/Unicode), zero copyleft, per
  `THIRD_PARTY_LICENSES.md` — MIT is fully compatible, nothing to
  unwind. **Consequence, recorded in §1/§6.1 above:** GPL/AGPL prior
  art (MuPDF, Poppler, Ghostscript) is now categorically and
  permanently excluded as a real dependency — reference-only, as
  `PRIOR_ART.md` already recommended on the merits. **Project rule 8's
  license precondition is now satisfied**, but this decision alone
  does NOT authorize pushing the existing local commit (`d8b3903`) or
  publishing a release — that remains a separate, still-open operator
  authorization. See `ARCHITECTURE.md` §12 (2026-08-01 entry, same
  decision) for the architectural-decision-log mirror of this record.
- **2026-08-07 — §6.5 added: the veraPDF LICENCE ELECTION and the
  arms-length usage rules.** Prompted by the operator's direct request to
  run veraPDF's validator against pdfcer's own output, with the explicit
  constraint *"use verapdf in a way that wouldn't cause us to change our
  license in order to stay conforming with it's tos."* **veraPDF 1.30.2
  (greenfield) is installed at `D:\tools\verapdf`, deliberately OUTSIDE
  the repository tree**; installer SHA-256
  `6cc6341cb1af644044054b81f00a6590a7918abb18f762243de115258bcad838`,
  GPG-verified good signature from veraPDF's tech lead (RSA key
  `13DD102B4DD69354D12DE5A83184863278B17FE7`).
  **THE DECISION: pdfcer receives veraPDF under MPL-2.0, NOT GPL-3.0.**
  Every veraPDF component is dual-licensed **GPLv3+ / MPLv2+** (verified
  per-repo across `veraPDF-apps`, `veraPDF-library`, `veraPDF-model`,
  `veraPDF-parser`, plus confirmation that `LICENSE.MPL` actually exists
  in `veraPDF-apps`, and independently re-confirmed from the installed
  binary's own banner). **Under a dual licence the recipient chooses, and
  an undocumented choice is an ambiguous one** — so the choice is made and
  written down. MPL-2.0 is file-level weak copyleft and **§3.3 expressly
  permits combination into a "Larger Work" under other terms**, so there
  is **no propagation path to pdfcer's MIT licence** (§1).
  **A SECOND, INDEPENDENT protection is recorded because either alone
  suffices:** the usage pattern triggers nothing even on the GPL branch —
  GPL §0 affirms unlimited permission to **run** an unmodified program,
  copyleft attaches to **distributing** a combined or derivative work, and
  the GPL reaches neither a program's output nor a program that merely
  consumes it.
  **Six enforceable rules** (§6.5.4): no vendoring/bundling/
  redistribution; no linking or embedding; no copying of source,
  validation profiles or model files (**reimplementing from the ISO spec
  is fine; lifting profile XML is not**); separate process over the
  documented CLI consuming XML; **dev-time only, never in any
  `Cargo.toml`** — so it will **correctly never appear in
  `THIRD_PARTY_LICENSES.md`, and nobody should "fix" that**; and **the
  gate must SKIP, not FAIL, when veraPDF is absent**, because a required
  gate would make it a de facto build dependency and muddy the very
  arms-length position this decision rests on.
  **§6.2's "stop and ask before adding copyleft" is satisfied from the
  other direction — the operator requested it**, and set the constraint
  himself in the same breath. §6.2 is unchanged for everything else.
  **⚠ ONE CORRECTION IS RECORDED RATHER THAN QUIETLY DROPPED (§6.5.5):**
  the CLI banner had been characterised as naming GPL-3.0 only and being
  misleading. **It is not — at 1.30.2 it names BOTH branches.** The
  librarian verified this by running the installed binary. **The real
  hazard is that the licence sentence is LINE-WRAPPED**, so its first line
  (*"Released under the GNU General Public License v3"*) is a complete,
  plausible, wrong sentence that any grep or truncated read will return on
  its own. **The predictable failure mode stands with a corrected cause.**
  **The veraPDF CORPUS (in use under §5 since 2026-07-30) and the veraPDF
  VALIDATOR are separate artifacts with separate terms — §6.5 governs the
  validator only, and is the first mention of the tool anywhere in this
  repository** (established by reading every `git grep -i verapdf` hit
  over tracked files). See `ARCHITECTURE.md` §12's 2026-08-07 eleventh
  entry for the architectural-decision-log mirror.
- **2026-08-07 (second entry this day) — THE "STILL UNDECIDED" LICENCE
  CLAIM IS SWEPT OUT OF EVERY EDITABLE `docs/` LOCATION, six days late;
  and §6.5.5's veraPDF correction gains its ATTRIBUTION.** No legal
  position changes in this entry. **§1 has said MIT since 2026-08-01 and
  is unchanged; the publish gate is unchanged and still ungranted.** What
  changes is that the document stops contradicting itself.

  **The defect.** §6's opening paragraph read *"pdfcer's own license (§1,
  still undecided) determines what's even usable"* — **wrong since
  2026-08-01**, and wrong in the paragraph that introduces the whole
  dependency-licensing discipline. **§6.1's bullets carried the MIT
  consequence inline from the decision date**, so a reader who continued
  past the intro got the right answer; a reader who stopped at the intro,
  or grepped it, did not. **This is a single-location amendment** — the
  failure mode the *Update protocol*'s same-filing propagation duty was
  written for (`ROADMAP.md`), and the **fourth** occurrence in this
  project.

  **★ HOW THE SET WAS ESTABLISHED (R87 — stated so the completeness claim
  is checkable, not asserted).** Four passes, **tracked files only**
  (`git grep`, so untracked scratch and build output cannot inflate or
  deflate the count):

  | # | Pattern / method | What it was for |
  |---|---|---|
  | 1 | `licen[sc]e[^.]{0,40}(still )?(undecided\|not (yet )?decided\|to be decided\|TBD)` and its mirror image | the canonical phrasing, both word orders, both spellings |
  | 2 | bare `-i undecided` over the whole repo | catches the claim when it is nowhere near the word *licence* (this is how `about.toml` and the `jbig2.rs` header were found) |
  | 3 | `not .{0,20}open.?source`, `OSS licen`, `licen[sc]e[^.]{0,30}open item` | the *"must not be described as open source"* half of the same stale posture |
  | 4 | **reading §6 and §6.1 end to end** | **this is the pass that found §6.1's heading** (*"why it gates the §1 decision"*), which **matches none of the three patterns** — it states the staleness without using any of the stale words |

  **Pass 4 is the load-bearing one and the reason no "grep came back
  clean" claim is made here.** A stale fact does not have to contain the
  word that made it stale. **The honest completeness claim is therefore:
  every location matching patterns 1–3 was enumerated and dispositioned,
  plus one found only by reading. There may be further prose-only
  instances that no pattern would surface**; that limit is stated rather
  than papered over.

  **Corrected in place (editable `docs/`, dated markers, text preserved —
  nothing deleted):** §6's opening paragraph (struck + corrected, with the
  failure named); §6.1's heading (historical-tense note beneath it — the
  heading itself left intact because other documents anchor to it);
  `docs/PRIOR_ART.md`'s egui font-licence note (*"pdfcer's own
  (still-undecided, §1/`LEGAL.md` §1) software license"*);
  `docs/ui_specs/gui-polish-current-featureset.md` **twice** — §1's
  empty-state-heading rationale and the `ui_text.rs` doc-comment it
  specifies, which is **the upstream source of the stale comment now
  living in shipped code**.

  **Deliberately NOT edited, with the reason in each case:**
  - **§7's two 2026-07-23 entries above** — a dated log entry that was
    **correct at its date** is not a defect. Both get forward-pointer
    markers; neither is edited. The first one's marker also records that
    **only the licence half is superseded** — its publish gate survives.
  - **`docs/decisions/*.md`** (six files carry the phrase) — **append-only
    by hard rule; never edited.** Per the propagation duty, the editable
    documents that mirror them (this file, `ROADMAP.md`, `ARCHITECTURE.md`)
    are canonical and now carry the correction.
  - **`docs/SESSION_LOG.md`** (~25 hits) — append-only, and every hit is a
    dated *"still undecided"* status line that was true on its own date.
    The 2026-08-01 entry records the decision.

  **★ STALE STATEMENTS OUTSIDE `docs/` — FOUND, NOT FIXED, because this
  filing was scoped to `docs/`. These are OWED and are the operator's /
  engineer's to clear:**

  > **✅ ALL SIX CLEARED 2026-08-07, same day, by commit `f51675d`** —
  > **NONE was deleted.** In every one the undecided licence was doing
  > duty as *the reason not to publish*; MIT removed the reason but **not
  > the restriction**, so each was **RE-POINTED at the still-ungranted
  > operator authorization**. **Two got STRONGER** — `jbig2.rs` and
  > `.claude/agents/pdfcer-inkscape-librarian.md`: an MIT project cannot
  > link GPL **at all**, so a risk-pending-a-decision became a categorical
  > bar. **`ui_text.rs` keeps its no-release-claim behaviour verbatim**
  > and records why, so nobody "simplifies" the comment away and takes the
  > restriction with it. The table below is left exactly as filed — it is
  > the record of what was found. See `ROADMAP.md`'s
  > `### docs/fix — six stale "licence undecided" statements` entry.

  | File | What it still says | Why it matters |
  |---|---|---|
  | **`README.md`** line 46 | *"`docs/LEGAL.md` \| License status (currently undecided — do not publish)"* | **The worst one.** The repo's front door, and the one line a new reader (human or LLM) reads about licensing before opening anything else. |
  | `about.toml` line 49 | *"…pdfcer's own (…undecided) software license…"* | inside the `cargo-about` config that generates `THIRD_PARTY_LICENSES.md` |
  | `crates/pdfce-gui/src/ui_text.rs` line 1142 | *"no tagline, no 'open source'/release claim (the project's licence is still undecided, CLAUDE.md rule 8)"* | **shipped code.** The *behaviour* is still right — no release claim in the empty state, because the **publish** gate is what forbids it — but the stated **reason** is obsolete. |
  | `crates/pdfcer-core/src/image_codec/jbig2.rs` line 14 | *"copyleft against an undecided `LEGAL.md` §1"* | the disqualification still holds and is now **stronger**, not weaker: MIT forecloses GPL FFI outright |
  | `.claude/agents/pdfcer-engineer.md` lines 145, 411 | *"License status (undecided — don't publish)"* | read at the start of every engineering session |
  | `.claude/agents/pdfcer-inkscape-librarian.md` line 43 | *"pdfcer's still-undecided license"* | |

  **One is already self-corrected and needs nothing:**
  `.claude/agent-memory/pdfcer-librarian/project_uncommitted_repo_worktree_risk.md`
  states the stale claim at line 24 and **corrects it at line 36**.

  **The through-line worth keeping, and the reason this is filed as a
  decision-log entry rather than a typo fix:** in every one of these
  locations the *undecided licence* was doing duty as the reason **not to
  publish**. The licence decision removed that reason, and the publish
  gate survived it on its own footing — **a separate, still-ungranted
  operator authorization** (§1, project rule 8). **Anything that cites the
  undecided licence as its publish gate should be re-pointed at the
  authorization, not simply deleted**, or the gate loses its stated basis
  while remaining in force.

  **Also in this filing: §6.5.5 gains the attribution it was missing** —
  the veraPDF CLI banner is fine and names both branches; **the truncated
  read was self-inflicted by a `head -5` invocation** that cut at line 5
  and dropped line 6. The generalizable shape (*a truncated read of a
  wrapped sentence yields a complete, plausible, wrong sentence*) is
  escalated to `C:\personal_rag\claude_code\`. See `ARCHITECTURE.md`
  §12's 2026-08-07 **twelfth** entry for the mirror of this record.
- **2026-08-13 — ★★ CC-BY-SA-4.0 OCR MODEL WEIGHTS ARE ACCEPTED INTO THE
  MIT PORTABLE FOLDER. Open operator question `(bl)` is ANSWERED: YES.**
  Operator, **verbatim and in full**: *"yes to the license. keep going."*
  **That is the entire answer and it is recorded at that length.** He was
  answering the question as this project had been carrying it — *may a
  **CC-BY-SA-4.0** model file ship inside pdfcer's **MIT** single-folder
  portable distribution?* Full subsection: **§6.7**.
  **§6.2 step 4 is thereby DISCHARGED for this artifact** — the
  stop-and-ask fired correctly (CC-BY-SA-4.0 is not permissive), the
  question was escalated rather than cleared by an agent, and the operator
  answered. **§6.1's categorical GPL/AGPL bar was never the governing
  rule here**: it is about **linking**, and this is a **data file pdfcer
  redistributes**.
  **The reasoning is recorded as the SURVEY'S READING THAT THE OPERATOR
  ACCEPTED, not as an agent's legal conclusion** (§6.7.2) —
  `docs/ocr-engine-survey.md` §3.3: CC-BY-SA has no linking concept, CC
  distinguishes a **collection** (own licence permitted) from an
  **adaptation** (must be BY-SA), and unmodified `.rten` files beside MIT
  code are a collection with **no propagation path to pdfcer's MIT
  licence** — **the same shape of reasoning §6.5.2 already applied to
  MPL-2.0 for veraPDF**, which is the neighbouring precedent.
  **What it unblocks:** `Pass 71.0`'s **engine half** only. Slice 1
  (`9f2af1d`, the engine-independent invisible-text-layer substrate) and
  `ocr::models` (`af5580e`, the resolver) had already shipped; the engine
  was blocked on this and **only** this. **`ocrs`/`rten` is now a
  permitted route**, including its **two bundled `.rten` weight files**.
  **★ FOUR THINGS IT DOES NOT DECIDE (§6.7.3), because the gap between
  "the licence is accepted" and "therefore X" is where a wrong inference
  would live:** (1) **not authority to publish or release** — §1's
  publish gate and project rule 8 are untouched, and since the repository
  is public (§1.1) a bundled weight file is **published the moment it is
  committed**; (2) **not an engine choice** — that was the separate
  2026-08-12 answer (*"…or heck, just build for both"* → both engines
  behind Cargo features, ranked on multi-language coverage), and `(bl)`
  clears the licence obstacle in front of **the pure-Rust one only**;
  (3) **not clearance for an ADAPTATION** — fine-tuning, quantizing,
  retraining or **format-converting** the weights plausibly creates
  **Adapted Material** binding the **derived model** to CC-BY-SA-4.0, so
  a future Pass touching the weights **owes its own §6.2 step 4
  decision**; (4) **not the end of the attribution obligation but its
  beginning** — `cargo-about` generates `THIRD_PARTY_LICENSES.md` **from
  the Cargo dependency graph**, a model file is **not a Cargo
  dependency**, so it **will not be seen, will not be attributed, and
  nothing will fail**. The artifact must be hand-authored
  (`PROVENANCE.md` + an `about.hbs` citation), which is what
  `tools/check-shipped-assets.py` (`e3fb7e0`) already **enforces**.
  **Enforcement is not acceptance** — and note the §6.5.4-rule-5 hazard
  runs **in reverse** here: veraPDF's absence from
  `THIRD_PARTY_LICENSES.md` is **correct** and must not be "fixed"; a
  bundled model's absence would be **incorrect**, and **it looks
  identical**.
  **★ PROVENANCE IS THIN AND MUST BE PINNED (§6.7.4):** `ocrs-models` has
  **no LICENSE file** — the declaration exists **only on the Hugging Face
  model card — and the S3 and Hugging Face copies are NOT byte-identical**:
  detection **2,510,284 B (S3)** vs **2,523,564 B (HF)** = **13,280 B
  smaller**, recognition **9,716,568 B** vs **9,716,444 B** = **124 B
  larger**, under **different filenames**, totals **12,226,852 vs
  12,240,008 B over 2 files** (0.11% apart). **pdfcer must pin exactly
  which artifact it ships and hash it** — an engineering obligation of
  `Pass 71.0`, not an open question.
  **★ AMENDED 2026-08-25 (`Pass 129.0`, `181d9bd`): what ships is MIXED —
  S3 detection (2,510,284 B) + HF recognition (9,716,444 B) =
  12,226,728 B over 2 files**, because **the HF detection build does not
  work with `ocrs` 0.12.2 at all** and produced noise on every page from
  2026-08-13 to 2026-08-25. **The S3 channel carries no licence text of
  its own**; the operator was told that and authorised the bundle. Full
  table and reasoning in §6.7.4 above.
  **Mirrors:** `ROADMAP.md`'s *Open operator questions* → `(bl)`
  (**answered, not retired — ceiling stays `(bl)`, next free `(bm)`**) and
  `Pass 71.0`'s *Next up* entry (**no longer blocked on an operator
  decision**); `SESSION_LOG.md`'s hundred-and-thirty-sixth filing.
  **No `ARCHITECTURE.md` §12 decision record was minted** — this is the
  operator's decision, and §12 records the project's *engineering*
  decisions; **`LEGAL.md` is where licence decisions live.** Decision
  ceiling therefore stays **057**, next free **058**.
