---
name: project_gui_capability_surface_audit_dispatch
description: A 6th dispatch shape distinct from the other five — "catalog EVERY control of one existing Acrobat dialog surface, plus what happens silently with no control at all" — requires auditing same-bucket files for completeness, not just adding new ones
metadata:
  type: project
---

2026-08-10 session ("Print dialog capability surface," same day as, and
building directly on, the Reader-parity-sweep session that first created
the `printing__*.md` files). The dispatch: ground pdfce's GUI print
flow against Acrobat Pro's own Print-dialog capability surface because
the operator's requirement was verbatim "it should have all the options
that acrobat pro has" — a parity claim needing full-surface sourcing,
not a single-feature or cross-cutting-comparison ask.

**Recognize this shape by**: the dispatch names one SPECIFIC dialog/
surface (not a whole roadmap bucket, not a whole-product comparison) and
asks for its COMPLETE control inventory — "every control," "the full
list," explicit sub-enumeration requests ("give me: what it does, its
default, any edge case"). It also explicitly separates "controls" from
"behaviors that are NOT controls" (silent defaults, hidden-state
interactions) — a two-part ask this shape should always expect even when
not stated as bluntly as this session's did.

**Why this shape is genuinely different from bucket-building or
cross-cutting-comparison**: it requires auditing ALREADY-CATALOGED files
in the same bucket for OMISSIONS, not just adding net-new files. This
session's clearest finding: `printing__scaling_modes.md`, built the SAME
DAY by a prior session, had documented three of Acrobat's four Page
Sizing & Handling modes (Size/Booklet/Multiple) but **omitted Poster
entirely** — a real gap in a file less than a day old. Same-day
provenance is not a substitute for a fresh completeness check when the
new dispatch's scope is narrower and deeper than the file's original
scope. **When this shape's dispatch names a specific surface that
overlaps an existing file, re-read that file's Capability section
against a fresh "does this look like a COMPLETE enumeration or a
representative sample" question before assuming it's already covered.**

**A genuinely new file-type this shape produces**: a "silent behaviors"
consolidation/index file (this session:
`printing__optional_content_and_silent_behaviors.md`) that doesn't
source anything new itself — it exists purely to give the requesting
engineer ONE place to check every "this happens with no dialog control
at all" fact that's otherwise scattered across several already-built
files in different buckets (this session: a printing-bucket file,
the layers bucket, and the markup bucket, tied together because they
all answer "what does Acrobat do silently at print time"). Worth
building when the requesting brief explicitly separates "controls" from
"silent behaviors," per this session's own framing — don't duplicate
sourcing into the index file, just cross-reference and state the
conclusion.

**R83 (no affordance without capability) is load-bearing for this
dispatch shape specifically**, more directly than most bucket-building
sessions: because the ask is "ground the GUI's acceptance criteria,"
every capability found needs an explicit buildability judgment against
the target project's actual engine, not just a parity fact. This
session's concrete example: Poster mode is real, sourced, and
Reader-tier (not Pro-gated) — but flagged `nice_to_have` and explicitly
NOT-a-checkbox-yet because pdfce's raster pipeline lacks the
cross-sheet tiling-transform and overlap-region-duplication machinery
Poster requires, a materially bigger lift than Multiple (close to
existing grid-composition capability) or Booklet (page-index remapping
only, same per-page render path). State this kind of buildability
delta explicitly in the RAG file's pdfce-parity-notes section, don't
leave it for the requesting engineer to re-derive from the raw Acrobat
facts alone.

**Sourcing texture**: a meaningfully higher proportion of this session's
facts trace to a single directly-fetched non-Adobe tutorial
(`acrobatusers.com/tutorials/printing-documents-acrobat-and-reader-x/`)
than to WebSearch-snippet synthesis, once that one fetch succeeded — a
reminder that for a "enumerate every control" ask specifically (as
opposed to a "what does Acrobat do" fact-lookup ask), a single
comprehensive third-party tutorial page, once you find one and it
fetches, is worth more than a dozen narrow WebSearch queries each
confirming one control in isolation. Try a broad "list every control"
WebSearch first to find a candidate comprehensive tutorial page, THEN
WebFetch that specific page, rather than only running narrow per-control
searches from the start.

Related: [project_bucket_building_pattern](project_bucket_building_pattern.md),
[project_cross_cutting_comparison_dispatch](project_cross_cutting_comparison_dispatch.md),
[project_gui_interaction_model_dispatch](project_gui_interaction_model_dispatch.md),
[project_reader_parity_sweep_dispatch](project_reader_parity_sweep_dispatch.md),
[project_secondhand_claim_verification_dispatch](project_secondhand_claim_verification_dispatch.md)
