---
name: project-acrobat-object-model-outruns-own-gui
description: recurring finding across sessions — Acrobat's SDK/spec-level object model is broader than what its own interactive GUI exposes; each instance is a parity-plus opportunity, not a gap to fill by inventing GUI it doesn't have
metadata:
  type: project
---

Across the 2026-09-27 layers/OCG-authoring bucket session, the SAME
pattern surfaced independently three times, in three unrelated corners
of Acrobat's object model:

1. **Folders/`/Order` groups** — Acrobat can nest a layer INTO an
   existing group (Import as Layer's "Add To Group") but has no GUI
   command that originates a new, empty group/folder from scratch.
   Consumer, never originator.
2. **Annotation/form-field `/OC`** — the spec lets any annotation or
   field widget reference an OCG for visibility, and Acrobat JavaScript/
   the PDF Library SDK can set it (`PDOCG`/`PD_Layer`), but NO Acrobat
   GUI surface (Layers panel, annotation Properties, Prepare Form field
   properties) exposes this at all. SDK-only, zero UI exposure,
   confirmed independently by two differently-worded community sources.
3. **`/RBGroups` and `/Configs`** — no Acrobat GUI authoring surface
   found for either (grouping layers into a mutually-exclusive radio
   set, or switching between multiple named configurations) — reasoned-
   absent, not yet independently confirmed against a live Acrobat
   session, but the pattern fits.

**Why this matters for pdfcer scoping:** when `pdfce-acrobat-librarian`
reports "Acrobat has no GUI for X," that is NOT evidence X is low-value
or should be `out_of_scope` — it just means there is no Acrobat
interaction model to copy, diverge from, or accidentally clone (the
trade-dress risk this whole RAG exists to avoid disappears by
construction). Several of these were flagged `should_have`/parity-plus
specifically BECAUSE Acrobat's own GUI falls short of its own object
model — e.g. a first-class single-named-layer delete (Acrobat buries
this inside Preflight, a much heavier tool than the action warrants),
or a genuine annotation/field-to-layer assignment control.

**How to apply:** when a dispatch asks "does Acrobat do X," and the
answer comes back "the object model supports it but the GUI doesn't
expose it," don't stop at recording the absence — explicitly flag it to
`pdfce-engineer`/`pdfce-ui-specialist` as a **fresh design surface**
(no Acrobat behavior to match) rather than folding it into ordinary
`out_of_scope`/`nice_to_have` triage the way an actually-unsupported
capability would be. This is now the THIRD Acrobat-Pro-parity session
to surface an instance of this exact shape (folders, annotation/field
`/OC`, RBGroups/Configs) — treat it as an expected texture of this
product, not a one-off surprise, when scoping future buckets.

See [[feedback_helpx_fetch_reliability]] for the sourcing-difficulty
side of the same session; see `layers__order_folder_reordering.md`,
`layers__annotation_form_field_oc_assignment.md`, and
`layers__ocmd_rbgroups_configs_nested_xobjects.md` in the Acrobat
Features RAG for the full sourced detail behind each instance.
