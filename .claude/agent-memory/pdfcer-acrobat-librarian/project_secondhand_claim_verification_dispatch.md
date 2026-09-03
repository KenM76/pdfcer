---
name: project_secondhand_claim_verification_dispatch
description: A 4th dispatch shape distinct from bucket-building/cross-cutting-comparison/GUI-interaction-model — auditing claims that reached the requester indirectly (e.g. via a commit message) rather than researching from scratch
metadata:
  type: project
---

2026-08-10 session: `pdfce-engineer` dispatched a check on four claims
about Acrobat rich-text field behavior that had reached it via a commit
message (`b8f96b1`), explicitly flagged by the filing librarian as
"unchecked against the Acrobat RAG" — i.e. the requester was relaying
hearsay-with-a-paper-trail, not asking a fresh question.

**Recognize this shape by**: the dispatch brief lists specific claims
with "confirm, correct, or refute each" language, and states the claims
did NOT come from a direct RAG read — they arrived through an
intermediate document (commit message, another agent's summary, a
prior decision record) that itself flagged them as unverified.

**What this shape asks for, distinct from bucket-building**: the correct
first move is `Grep`/`Read` the RAG for whether the claim is ALREADY
covered before doing any fresh research — in this case all four claims
turned out to already be fully sourced in
`Acrobat_Features/forms__rich_text_fields.md` (built 4 days earlier,
2026-08-06). The value-add was not re-deriving settled facts but (a)
explicitly reporting confirmation with citation so the requester can
stop treating them as hearsay, and (b) doing a targeted deepening pass
on exactly the parts flagged as weakest (here: FDF's `/RV` support was
sourced only via a weak pdftk snippet; a fresh cross-check against a
sibling `PDF_Spec` file built the same day but never cross-referenced
turned up a spec-primary confirmation — Table 246 — sitting unused one
directory over). **Always check sibling RAG files built the same day as
the file being verified** — same-day cataloging sessions on both sides
(Acrobat_Features + PDF_Spec) frequently don't cross-reference each
other even when one would strengthen the other.

**Second value-add pattern**: re-running ONE targeted WebSearch per
flagged-weak item (not a full bucket re-research) can upgrade a vague
"reported unreliable" finding into a mechanistically-explained,
multi-source-corroborated one. Here, "auto-size persistence is
unreliable with rich text" (community-level-confidence-only) became
"auto-size is structurally impossible once a field can hold multiple
run sizes at once" (three independent, cross-year Adobe Community
threads converging on the same mechanism) — which is what let the
exceed-Acrobat opportunity get stated concretely (per-run-aware
auto-size) instead of vaguely ("do better than Acrobat somehow").

**Report-back shape for this dispatch type**: state explicitly, per
claim, whether it was (a) already RAG-covered — cite the file/section,
(b) newly confirmed via fresh sourcing, or (c) still unconfirmed/GAP.
Do not silently fold this into a normal "session findings" report — the
whole point of the dispatch was distinguishing sourced fact from
relayed hearsay, so the report must preserve that distinction visibly.

Related: [project_bucket_building_pattern](project_bucket_building_pattern.md),
[project_cross_cutting_comparison_dispatch](project_cross_cutting_comparison_dispatch.md),
[project_gui_interaction_model_dispatch](project_gui_interaction_model_dispatch.md)
