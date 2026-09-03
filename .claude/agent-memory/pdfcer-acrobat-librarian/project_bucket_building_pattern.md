---
name: project-bucket-building-pattern
description: reusable file-decomposition pattern for cataloging a new ROADMAP.md Backlog bucket — N per-capability files + a small handful of shared cross-cutting files
metadata:
  type: project
---

Six buckets built so far all converged on the same shape without it
being specified up front: **Core document ops** (2026-07-31, 9 files),
**Comments & markup** (2026-07-31, 9 files), **Forms (AcroForm)**
(2026-07-31, 12 files + 1 companion file in the separate `XFA` bucket),
**Redaction** (2026-07-31, 7 files), **Measuring tools** (2026-07-31,
7 files), and **Text & object editing** (2026-07-31, 9 files, TEXT
capabilities only — see below). Pattern that emerged in all six:

- One file per distinct capability/feature-cluster the operator actually
  invokes (e.g. `rotate_pages`, `text_markup_quadpoints`, `text_fields`).
- A small number (2-4) of **cross-cutting files** shared across every
  per-capability file in the bucket, ALWAYS including a
  **permissions/signature-interaction** file (encryption `/P` bitmask +
  certified-document permission levels + signature-invalidation — the
  facts differ per bucket — e.g. Forms found form-fill-in is its OWN `/P`
  bit and its OWN DocMDP tier, distinct from both core_ops' structural
  gate and markup's commenting gate — but the FILE SHAPE repeats every
  time). Forms additionally needed a **field_property_model** cross-cutting
  file (the shared name/value/`/DA`/`/TU`/actions-dictionary model every
  per-type file references, avoiding restating it 5+ times) and an
  **appearance_generation_and_needappearances** cross-cutting file (forms'
  own twist on the same `/AP`-reliance question markup's
  `appearance_stream_generation.md` already covers — live regeneration
  during fill + the `/NeedAppearances` flag specifically).
- **Ask explicitly, every new bucket, "does this bucket need its own
  cross-cutting file(s) beyond permissions/signatures"** — Comments &
  markup needed appearance-stream-generation + display-visibility-
  semantics; Forms needed field-property-model +
  appearance-generation-and-needappearances. Not a fixed template, but a
  recurring question worth asking up front rather than discovering
  mid-session.
- **A bucket can spawn ONE small companion file in a DIFFERENT, related
  roadmap_bucket** when the dispatching engineer's brief explicitly asks
  for corroboration of something that properly belongs taxonomically
  elsewhere (Forms' brief asked to corroborate XFA's deprecation
  timeline — a separate `XFA` bucket in the taxonomy — so a single
  `xfa__deprecation_status_and_hybrid_forms.md` file was added to the XFA
  prefix rather than folded into `forms__*` or skipped). This is
  deliberately NOT a full build-out of that other bucket — just the one
  fact the current session was asked to chase — and should be reported
  back explicitly as a partial/companion addition, not conflated with
  "XFA bucket is now built."

**Why this matters:** per-capability files stay grep-first and dense
(single-feature focus); cross-cutting files avoid restating the same
mechanism-level facts (e.g. the encryption `/P` bitmask structure, or the
field name/value/action-dictionary model) in every single per-capability
file — each per-capability file instead cross-references the shared file
via `related_files`.

**How to apply:** when scoping the next bucket (Redaction, Bates, Digital
signatures, Encryption, etc. — see `D:\Dev\pdfce\docs\ROADMAP.md` for
current priority order), start by asking (a) what are the distinct
operator-facing capabilities, (b) what cross-cutting concerns
(permissions/signatures almost certainly; possibly others specific to
that bucket) apply across all of them, and (c) whether the dispatching
brief asks for any fact that properly belongs to a DIFFERENT bucket's
taxonomy prefix (XFA-from-Forms was the first instance of this) rather
than writing N flat files with duplicated boilerplate.

**Security-contrast pattern worth reusing**: when a bucket involves
Acrobat executing embedded, potentially-untrusted logic (forms
JavaScript was the first instance), ground pdfce's non-execution/
more-conservative posture in ADOBE'S OWN security documentation if it
exists (Acrobat's Application Security Guide sandbox/broker-process
architecture, in the Forms case) rather than only in general security
principles — it makes the parity-gap argument much stronger ("even Adobe
treats this as an attack surface requiring its own sandbox") than an
unsourced "JS execution is risky" claim would.

**Redaction (destructive-operation) bucket found a NEW pattern worth
reusing for any future destructive/trust-critical bucket (Encryption is
the obvious next candidate)**: when sources genuinely CONFLICT on a
must_have-grade removal-scope question (here: does plain Apply Redactions
remove annotations/form-fields/links intersecting a mark, or does that
require the separate Sanitize step — two classes of source disagree and
neither could be resolved by further searching), the right move is
NEITHER to pick a side by guessing NOR to just say "unclear" and move on.
Instead: (1) record the conflict explicitly as a GAP with both readings
cited, (2) recommend pdfce adopt the STRICTER/more-conservative reading
as its OWN deliberate default regardless of which one turns out to be
Acrobat's actual behavior, and (3) frame this as a parity-PLUS decision
(pdfce being safer than an ambiguous reference product) rather than an
unresolved parity-match question blocking Pass scoping. This keeps a
genuine sourcing gap from stalling `pdfce-engineer`'s acceptance-criteria
work — the Pass can proceed on pdfce's own conservative default while the
GAP stays flagged for later resolution rather than being silently
dropped or silently guessed.

**A single most-authoritative source can still be unfetchable — flag
harder than usual.** The Redaction session's best available source (an
Australian Cyber Security Centre technical report specifically examining
Acrobat Pro DC 2017's redaction internals, covering a real documented
CMap/font-remnant recoverability defect) could not be directly fetched
in EITHER its HTML or PDF form (both timed out, same failure mode as
helpx). Because it's meaningfully MORE authoritative than the community/
blog sourcing available for the same fact, this got its own explicit
"re-verify via direct fetch/download before treating as
implementation-detail-grade settled" flag in the RAG file itself, beyond
the usual per-fact confidence flagging — worth doing again whenever a
single unusually-strong source (government report, standards-body
publication) is search-snippet-only rather than directly read.

**A bucket can be built AHEAD of its own `ROADMAP.md` Backlog-bucket
registration** when the dispatching agent explicitly says so (Measuring
tools, 2026-07-31 — built to ground an in-flight decision, decision 011,
before that decision record and its `ROADMAP.md` entry had landed).
Handle this by: (1) using the bucket name the dispatching brief gave
verbatim as `roadmap_bucket` across every file for internal consistency,
(2) adding the row to THIS RAG's own `index.md` taxonomy table as usual,
but (3) explicitly flagging in `index.md` (not silently) that
`ROADMAP.md` doesn't yet have a matching Backlog entry, and that
`pdfce-librarian` (not this agent) needs to add one so rule 7's
"exactly match" invariant holds once the decision record lands. Don't
treat "the roadmap doesn't have this bucket yet" as a reason to
decline/delay building the catalog — the whole point was to ground the
decision BEFORE it's finalized.

**Text & object editing (2026-07-31) added two new wrinkles worth
reusing.** (1) **A bucket can be cataloged PARTIALLY BY DESIGN, not just
"ahead of roadmap registration"** — the dispatching brief explicitly
scoped this session to TEXT capabilities only (in-place edit, reflow,
formatting, fonts), leaving image/object/vector editing — which the
bucket's own `ROADMAP.md` description also covers — deliberately
uncataloged for a later session. Handled by: recording the taxonomy
table's file count as "9 (text-only; object/vector editing not yet
cataloged)" rather than a bare number, and stating the partial scope
explicitly in both `index.md`'s Trigger-topics entry and the report back
to the dispatching agent — so a future session doesn't mistake 9 files
for "the bucket is fully built." (2) **Not every bucket needs a NEW
cross-cutting file type beyond permissions/signatures** — this was the
first bucket where the answer to the standing "does this bucket need its
own cross-cutting file(s)" question was genuinely "no": every other
finding (formatting, font-handling, OCR-prerequisite, tag-structure
corruption, add-new-text) fit cleanly as its own PER-CAPABILITY file
rather than a shared cross-cutting concern touched by every other file.
Worth remembering that "ask the question" doesn't always mean "the answer
is yes" — a bucket can legitimately need only the standard
permissions/signature cross-cutting file. Also: this session's
permissions/signature file is the THIRD bucket in a row (after Core
document ops and Redaction) to independently conclude "no dedicated `/P`
bit, inherits the general content-modification gate" — worth citing this
convergence explicitly in future such files as evidence for treating
shared gating as a settled `pdfce-core` architecture recommendation,
while still remembering Forms is the one confirmed counter-example (its
own dedicated bit + DocMDP tier) so the pattern isn't overstated as
universal.

**Forms bucket, form-BUILDING/authoring extension (2026-08-03) added a new
wrinkle worth reusing: a bucket can need a second, LATER extension session
to catalog a DIFFERENT capability within the SAME bucket that the original
build deliberately or incidentally left uncovered** — the original Forms
session (2026-07-31) covered field types, appearance/NeedAppearances,
auto-detection, flatten, and fill/save/export (i.e. FILLING an
already-authored form), but not AUTHORING (placing a brand-new field on a
page that had none). The dispatching brief was explicit that filling is
already shipped (Pass 7.0/7.1) and must NOT be re-cataloged — handle this
the same way as the Text-editing bucket's "partial by design" pattern
above: state the split explicitly in the file count note, in the Trigger-
topics entry, and in the report back, so a future session doesn't assume
"Forms bucket, N files" means uniformly-covered ground. **A second,
reusable finding from this same session**: when a dispatching brief
explicitly flags one question as "the one I most expect to be subtle" or
similarly signals it as the highest-value target, budget search effort
asymmetrically toward THAT question first — in this case a single,
precisely-targeted search ("Acrobat form field name already exists rename
duplicate field name") surfaced a directly-quoted Acrobat error string on
the first attempt, which became the session's central, highest-value
finding (the type-checked name-collision branch: same-type merges, cross-
type refuses). Worth treating "the requester told me what they expect to
be decisive" as a genuine prioritization signal, not just framing color.
**Third finding**: this session hit TWO distinct instances of "searched
directly, targeted, and confirmed nothing exists" (radio-group member-
deletion aftermath; tab-order behavior on mid-order field insertion) —
worth explicitly labeling these gaps as "search-confirmed-unfillable from
web sourcing" rather than the more common "not yet searched" gap framing,
since it changes the recommended next step for `pdfce-engineer` from
"search again" to "test empirically against a real Acrobat install once
fixtures exist" — a materially different, more useful instruction to leave
behind for a gap that has already absorbed real search budget.

**Priority-upgrade deep-dive — a new, narrow trigger shape worth telling
apart from a full bucket/sub-bucket extension (2026-08-06, Forms bucket,
rich-text fields).** Distinct from every pattern above: this wasn't
triggered by a new Pass being scoped or a dispatching brief's own
open question, but by the OPERATOR overriding a prior session's own
`should_have`/judgment-call flag with an explicit ruling ("it should be
able to handle X if Acrobat can, or if it makes it better than Acrobat").
When a feature gets promoted `should_have`→`must_have` this way, treat
the existing 2-3-line stub bullet as no longer sufficient for
acceptance-criteria work and spin it into its OWN dedicated
`<prefix>__<feature>.md` file (don't just edit the priority tag in place)
— the operator's ruling is itself evidence the feature is about to be
BUILT, not just tracked, and acceptance criteria need the same
depth-of-sourcing every other must_have file in the bucket has. Convert
the original stub's bullet into an explicit pointer (not a duplicate
restatement) and propagate the priority-change + pointer to every
`related_files` cross-reference the stub already had (this session:
`field_property_model`, `appearance_generation_and_needappearances`,
`fill_save_and_data_export` all got one-line additions, not full
rewrites). Worth watching for this same trigger shape recurring on other
existing `should_have`/judgment-call flags in this RAG (file-select
fields, barcode fields, CSV aggregation, etc.) — any of them could get
the same operator-ruling promotion later, and the same
stub-to-dedicated-file move should apply.

**Print & prepress (PDF/X) bucket, first build (2026-08-08) — the bucket
that finally covers colour management, added a new dispatch SHAPE worth
naming: "two explicit named decision questions" rather than a general
capability sweep.** The dispatching brief for this bucket didn't just ask
"catalog X" — it named two specific, load-bearing questions up front ("is
there a defensible correct answer for untagged CMYK→screen" and "where does
colour accuracy stop being rendering and start needing real ICC machinery")
and said explicitly these were decisions, not lookups. Handled by: (1)
budgeting search effort asymmetrically toward exactly those two questions
FIRST, before the general capability sweep (same instinct as the Forms
"requester told me what they expect to be decisive" finding, but now
applied to two decision questions instead of one fact); (2) once sourced,
stating the answer explicitly, by name, in the session's `index.md` summary
— "directly answers decision question N" — rather than letting the answer
sit implicitly inside a per-file Behavior bullet where a time-pressed
dispatcher would have to re-derive it; (3) when the answer to "is there a
correct answer" turns out to be "no, even the reference product's own
answer is a configurable house default," record that AS the answer (a
negative/relativizing finding is still a resolved decision question, not an
unresolved GAP) — this is a different shape from the redaction-style "sources
conflict, pick the conservative default" pattern below: here the sources
agree, and the agreed-upon fact is itself "there is no universal ground
truth." A single well-chosen named-expert source (again Dov Isaacs, already
established as a high-trust named source from the FF-H session) directly
fetched via `community.adobe.com` was the single highest-value citation of
the session — worth trying a targeted WebSearch for "[topic] [known expert
name if one exists from a prior session]" before a generic capability
search, now confirmed useful a second time.

**Contested/conflicting-tier-or-fact resolution — a DIFFERENT pattern
from the redaction removal-scope GAP above, worth telling apart.** The
redaction pattern applies to a must_have-grade MECHANISM question with
no way to pick a side (recommend the conservative default, move on). A
DIFFERENT situation showed up cataloging Measuring tools:
`acrobat_tier` itself was contested — one third-party comparison table
called the Measure tool Pro-exclusive, while two independent first-hand
sources (an Adobe Community expert answer + a detailed third-party
walkthrough) described the same toolset working fully in free Reader.
Neither side is a "pick the safe default" situation (tier is a
descriptive fact about Acrobat, not a pdfce design choice to default
conservatively on). The right move here: **weigh the sources
explicitly** (two independent first-hand descriptions of actual usage
outweigh one vendor's static comparison table), **record the
evidence-weighted call as the field value**, but **flag the losing side
by name, with its source, as CONTESTED** in the same file rather than
silently picking one and dropping the other. This preserves the
disagreement for a future re-check instead of erasing it — a materially
different resolution shape from "recommend the conservative default,"
worth reaching for whenever the conflict is over a fact-about-the-world
rather than a design/mechanism decision.
