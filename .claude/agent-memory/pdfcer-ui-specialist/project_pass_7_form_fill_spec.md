---
name: project-pass-7-form-fill-spec
description: Key decisions from the Pass 7 (AcroForm interactive fill) UI spec authored at docs/ui_specs/pass-7-form-fill.md, 2026-07-31/08-01
metadata:
  type: project
---

Authored the full implementable Pass 7 GUI spec (form filling: text/checkbox/
radio/list/combo/pushbutton, flatten, export/import, disclosures) at
`docs/ui_specs/pass-7-form-fill.md`, on dispatch from the engineer while Pass 7
was blocked on two prerequisites (§12.7 field-model spec sourcing;
"Forms (AcroForm)" acrobat-parity bucket). Key decisions, useful when later
reviewing the actual implementation against this spec:

**No fill-mode toggle — deliberately the opposite call from Pass 6.1's
tool-mode state machine.** Widgets are directly clickable in ordinary view
mode; no `active_tool`, no crosshair cursor, no enter/exit chord. Reasoning:
markup drawing is ambiguous without a mode (a drag could mean pan/select/
draw); clicking inside an existing widget's bounded `/Rect` is never
ambiguous, and gating fill behind a mode would hide a form document's
*primary* purpose, which is the opposite of what progressive disclosure
(rule 3) is for. **Why this matters for a future review:** if the engineer
implements a fill-mode toggle anyway, that's a deviation from this spec and
should be challenged unless a concrete new ambiguity was found that the
spec didn't anticipate.

**Load-bearing cross-cutting finding, not just a UI call: `SignatureCensus::
forbids_structural_change()` (unchanged since Pass 3.2) will hard-refuse
form-fill on any `/P 2`-certified document** — and `/P 2` is both the
DocMDP default (absent `/P` reads as `Some(2)`) and the tier whose
documented purpose IS "filling in forms... and signing" (Table 254). This is
a much bigger deal than Pass 6.1's own `/P 3`-annotation residual (which was
correctly shippable-conservative as a P1 nicety) — flagged in the spec as a
**required `pdfce-core` fix for this Pass's P0**, not a deferred refinement,
because without it the Pass's headline scenario (fill a certified form)
would be visibly broken on the single most common certification permission
level. **Check this got actually fixed, not just noted, when reviewing the
shipped Pass** — it's the single highest-value thing to verify.

**Placement decisions given (five-way taxonomy applied, no new taxonomy
bucket needed this time, unlike Pass 6.1's transient-property-bar
addition):** document-is-a-form / JS-computed-field-count / NeedAppearances-
regenerate-button all → disclosure/status bar, reusing the existing
`annotations_need_appearances` line's adjacency pattern (action button lives
right next to the fact it remedies). Flatten → a single new toolbar button
in the existing edit group (explicitly NOT a new "Form ▾" dropdown menu yet
— there's only one whole-document form action this Pass, so a menu would be
premature structure; promote to a menu only when a second such action
exists — flagged as an evolution path for the librarian). Export/Import
form data (FDF/XFDF) → Tools dock (argument is a file outside the open
document, the existing rail-vs-dock test applies cleanly, no new reasoning
needed).

**Architectural recommendation given for accessibility, worth checking was
followed:** use REAL egui widgets (`egui::TextEdit` overlay for text
fields; transparent `ui.interact(..., Sense::click())` for checkbox/radio/
pushbutton — NOT a second egui-drawn visual, since the correct look is
already in the document's own rendered `/AP`; real `egui::ComboBox`/list
overlay for combo/list) rather than Pass 6.1's hand-rolled `painter()`-only
interaction. This was framed as the single biggest accessibility win
available in this Pass (real Tab-focus + real accesskit exposure for free)
and a genuine, if partial, closure of the accesskit gap Pass 6.1 could only
name, not fix. If the shipped implementation went hand-rolled/painter-only
instead for these types, that's worth asking about specifically — it would
forfeit a real, cheap accessibility gain the spec called out as available.

**Flatten confirmation weight — deliberately delete-shaped, not
redaction-shaped**, per R48 being an explicit sibling of R35 (delete's
disclosure), not of redaction's genuine irreversibility: flatten is
reversible pre-save via Undo, and even after an ordinary incremental save
the flattened field data is still forensically recoverable in the file's
prior revision (only a full-rewrite save — which pdfce doesn't have yet —
would truly remove it). So flatten gets `selection_delete_tooltip`-style
honest tooltip treatment, no blocking modal — named explicitly as the
correctly-weighted answer to the task's own "reuse the SignatureImpact
confirmation only where a real irreversible-after-save boundary exists"
question, since that boundary doesn't unconditionally exist for flatten.
