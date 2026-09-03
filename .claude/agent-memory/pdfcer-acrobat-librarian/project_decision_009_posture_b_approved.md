---
name: project-decision-009-posture-b-approved
description: decision 009's posture B (native-Rust AF*-helper reimplementation) has operator go-ahead as of 2026-08-10 and is on the critical path — the forms__calculation_validation_javascript.md addendum sourcing it is load-bearing, not speculative
metadata:
  type: project
---

`docs/decisions/009-forms-javascript-posture.md` originally shipped
2026-07-31 as "Recommended (for engineer ratification when Pass 7 is
scoped)" — posture A (recognize+disclose+never-execute) was Pass 7's
whole first-cut scope, and posture B (an opt-in, off-by-default native
Rust reimplementation of the `AFSimple_Calculate`/`AF*_Format` whitelist)
was explicitly deferred to a later Pass 7.x, gated on Pass 7's own
recognition-histogram demand signal.

**As of 2026-08-10, the operator gave the go-ahead to build posture B**,
independent of whatever the Pass 7 histogram eventually shows — this
dispatch (sourcing the exact canonical shapes + full behavioral
semantics of the whitelist from Adobe's *JavaScript for Acrobat API
Reference*, per decision 009 §6 and §13's spec-prerequisite list) is the
direct consequence, landing as a large addendum to the existing
`forms__calculation_validation_javascript.md` in
`D:\Dev\Rag-Specialized\Acrobat_Features\`.

**Why this matters for a future session**: don't reflexively treat
posture B as "not yet needed" or "gate it on the histogram" if a future
task references decision 009 — the go-ahead already happened. The
addendum's content (field-resolution semantics, the AVG-divisor rule,
the `AFPercent_Format` ×100 resolution, the full format-string tables)
is the grounding `pdfce-core`'s native reimplementation is expected to
cite directly in its own doc comments per decision 009 §14's
documentation-first obligation — this is now implementation-track
material, not a standing research backlog item.

**How to apply**: if dispatched again on anything touching
`AFSimple_Calculate`/`AF*_Format`/posture B, start by grepping
`forms__calculation_validation_javascript.md`'s "Addendum (2026-08-10)"
section rather than re-researching from scratch — it already carries a
full confidence-tiered (`ADOBE-PRIMARY`/`PDFJS-CLONE`/`COMMUNITY`/`GAP`)
answer to nearly everything decision 009 §6/§13 asked for, with the
remaining genuine GAPs named explicitly (exact byte-level generated-script
whitespace; `sepStyle=4`'s meaning; digit-overflow behavior for
`AFSpecial_Format`; hidden/read-only field inclusion in
`AFSimple_Calculate`).
