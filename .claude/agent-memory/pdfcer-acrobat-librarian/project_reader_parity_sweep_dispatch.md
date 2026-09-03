---
name: project_reader_parity_sweep_dispatch
description: A 5th dispatch shape distinct from bucket-building/cross-cutting-comparison/GUI-interaction-model/second-hand-claim-verification — cataloging free Acrobat READER's baseline as its own explicit target, not Pro
metadata:
  type: project
---

2026-08-10 session ("Reader-parity sweep"): the operator dispatched a
catalog of seven consumption-side gaps (printing, Find, bookmarks,
attachments, layers panel, full-screen, signature validation) after an
audit found pdfce ahead of free Acrobat Reader on EDITING but behind on
plain CONSUMPTION. Before this session, the RAG had ZERO mentions of
"Reader" anywhere (confirmed by Grep) — every prior bucket-building
session had implicitly treated Acrobat Pro as the default reference,
even for capabilities (viewing, printing, basic navigation) that are
actually present at the free-Reader tier and don't need Pro at all.

**Recognize this shape by**: the dispatch explicitly names "Reader" (not
"Acrobat" generically) as the target, frames parity as "what the FREE
tier does," and lists specific gaps verified absent in pdfce's own
source (grep evidence supplied), not vague capability areas.

**Why this shape is genuinely different from bucket-building**: it can
surface a Reader-vs-Pro DEFAULT divergence that a Pro-first researching
habit would silently paper over. Concrete example this session: Reader's
print-comments default is "Document" (markups EXCLUDED); paid Acrobat
DC's default is "Document and Markups" (INCLUDED) — the opposite. A
bucket-building session researching "Acrobat's print options" from
general habit would very plausibly have picked up Pro's default as THE
Acrobat default and missed that Reader disagrees with it — exactly the
failure this campaign exists to prevent. **When the dispatch names
Reader specifically, actively search for "Reader" in the query, not just
"Acrobat," and flag any found Reader-vs-Pro default divergence as a
load-bearing finding, not a footnote.**

**Frontmatter handling**: the existing `acrobat_tier: standard|pro|
pro_exclusive` enum has no "reader" value (Reader sits below Standard).
Don't invent a new frontmatter key — set `acrobat_tier: standard` (since
Reader-tier implies Standard/Pro also have it) and state the
Reader-specific framing explicitly in the Capability/Behavior prose
instead. This keeps the schema consistent across the RAG rather than
introducing a one-off field only this campaign's files would carry.

**Bucket-naming consequence**: several of the seven gaps (printing, find,
bookmarks, attachments, layers, full-screen) had no existing
`roadmap_bucket` — same situation as the pre-existing "Measuring tools"
precedent (built ahead of its ROADMAP.md entry). Picked plain, obvious
bucket names (`Printing`, `Find / search`, `Bookmarks & navigation`,
`Attachments`, `Document layers (OCG)`, `Full-screen / read mode`) and
flagged all six for `pdfce-librarian` to register in `ROADMAP.md` per
rule 7, rather than trying to guess the roadmap's eventual exact
wording. The seventh gap (signature validation) landed in the
already-existing, previously-empty "Digital signatures" bucket — no new
bucket needed there.

**Sourcing texture, worth setting expectations on going in**: this
campaign's subject matter (printing options, Find semantics, panel
toggle behavior) is less documented by Adobe in long-form help articles
than a feature bucket like Forms or Redaction — most load-bearing facts
came from WebSearch AI-summaries synthesizing across several
community.adobe.com threads, not single directly-fetched pages. Flag
every fact resting on synthesis-across-snippets (vs. one directly
confirmable source) inline, same discipline as every other session, but
expect a HIGHER proportion of that sourcing tier for this shape of
dispatch than for a document-feature bucket.

Related: [project_bucket_building_pattern](project_bucket_building_pattern.md),
[project_cross_cutting_comparison_dispatch](project_cross_cutting_comparison_dispatch.md),
[project_gui_interaction_model_dispatch](project_gui_interaction_model_dispatch.md),
[project_secondhand_claim_verification_dispatch](project_secondhand_claim_verification_dispatch.md)
