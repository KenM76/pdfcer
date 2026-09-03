---
name: project_forms_panel_spec
description: Forms (AcroForm) fill/flatten/data-interchange GUI spec at docs/ui_specs/forms-panel.md (2026-08-05) — supersedes pass-7-form-fill.md's placement/interaction-shape; list-driven PaneSubject::Forms, not canvas overlay
metadata:
  type: project
---

Spec delivered: `D:\Dev\pdfce\docs\ui_specs\forms-panel.md`, dispatched
because the engineer audited every `EditSession` verb against GUI call
sites and found forms — despite Pass 7.0/7.1 being CORE-COMPLETE since
2026-08-01 — had ZERO GUI surface. `docs/ui_specs/pass-7-form-fill.md`
(authored 2026-07-31/08-01) already existed and was never implemented; it
predates four shell changes that make its central call (mode-less canvas
overlay: real widgets projected onto per-widget screen rects, hit-tested
each frame) no longer the best fit. This is the first time in this
project's UI-spec history that a fully-detailed prior spec had to be
**formally superseded on placement/architecture** rather than extended —
worth remembering as a pattern: **always check whether a named spec
already exists for the exact feature before treating a request as new**
(the engineer's own dispatch note already knew this and asked for it), but
also **check whether the shell has moved out from under an old spec**
before treating "spec exists" as "spec still fits."

**Core-vs-GUI split, confirmed accurate at time of writing:** every fill/
flatten/choice/import/export verb (`fill_text_field`, `set_button_state`,
`set_choice_value`, `flatten_fields`, `import_form_data`,
`export_form_data`, `regenerate_appearances`) is shipped, tested, and
`/P`-certification-gated correctly in `pdfce-core` (Pass 7.0/7.1) — this
was purely a GUI-placement/interaction-design task, not a core-design one,
unlike most prior UI specs in this project which had at least one required
core change. The one required core change from the OLD spec (§6.2's `/P
2`/`/P 3` fill-permission fix) **already shipped** in Pass 7.0 —
confirmed by reading `check_certification_for_fill` in `edit.rs` directly
rather than trusting the old spec's "required" framing at face value.

**Placement decision and its reasoning, worth citing for any future
PaneSubject addition:** new `PaneSubject::Forms`, sibling of
`Properties`/`BatchTools`/`Redact`, NOT folded into `ActiveTool` (which is
structurally conditioned on `doc.active_tool: Option<CanvasTool>` being
`Some` — a sustained non-tool activity cannot live there) and NOT folded
into the Pass-34.2-widened `Properties` pane (Forms shares no state with
"the currently selected thing," unlike the ce-dimension/group
relationship that justified Properties' three-way widening). Ribbon home:
new `RibbonGroup::Forms` on `RibbonTab::Edit` ("What am I CHANGING about
what is already there?"), explicitly NOT `Tools` > `Protect` where Redact
lives — Redact's placement is reasoned entirely from its DANGER
("grouped apart for that reason"), and filling a form is rule-7 reversible/
low-stakes, so copying Redact's neighborhood would misrepresent its risk
level. **General lesson for future `PaneSubject` additions: check whether
a candidate tab's placement reasoning actually transfers, not just
whether the activity "feels similar" to something already placed.**

**The core architectural argument (list-driven vs. canvas-driven),
useful for any future canvas-adjacent panel decision:** the deciding fact
was NOT primarily cost — it was that the current shell (`PaneSubject` +
`redact_panel`'s state→action→detail list-with-per-row-navigation
precedent) already has an established, working home for exactly this
activity SHAPE (sustained, document-wide, many small commits), while the
old spec's canvas-overlay design was reasoned against a shell that had
neither `PaneSubject` nor a shipped commit-on-interaction-end precedent
to build on when it was written. A list of real `egui::TextEdit`/
`Checkbox`/`ComboBox` rows in an ordinary dock panel gets Tab-order,
`accesskit` exposure, and native Space/Enter handling MORE directly than
the same widgets projected onto a raster and hit-tested per frame — no
geometry bridge, no per-frame projection, no "which overlapping
`interact` wins" uncertainty. Full click-on-canvas-to-edit is named as a
genuine, schedulable **P2, its own future Pass** — flagged per the task's
explicit request for "this needs a core/GUI change first" items — not
silently dropped and not force-fit into this Pass's P0/P1.

**A cheap P1 middle ground worth remembering for future canvas+list
hybrid designs:** a passive on-canvas highlight rect (drawn every frame
from `Option<(ObjId, Rect)>` view state set by a list-row click, read via
the EXISTING `viewer::page_to_screen`) gives "see it on the page" WITHOUT
needing any hit-testing or `CanvasTool` variant at all — it's the same
shape as how redaction marks already draw every frame regardless of any
armed tool. Distinguish this explicitly from true interactive click-to-
edit (which DOES need hit-testing) — they are very different costs and
were conflated in the old spec's single "canvas-driven" bucket.

**Two genuinely new findings this session surfaced by reading `pdfce-core`
source directly (not just the old spec), both real correctness/safety
gaps worth flagging to any future forms-adjacent review:**

1. **Rich-text field silent-downgrade risk.** `fill_text_field` in
   `edit.rs` does NOT special-case the `RichText` field flag (bit 26,
   value `33554432`) — nothing refuses or warns before overwriting a
   rich-text field's `/V` with plain decoded text and regenerating a
   PLAIN (non-rich) appearance via `vartext.rs`, discarding whatever rich
   formatting was there. This is a real rule-4 (fuzzy-never-sneaky)
   violation waiting to happen the first time a real-world rich-text
   field reaches a fill path. Recommended fix: GUI-side disable+disclose
   in P0 (cheap, sufficient for GUI acceptance), core-side refusal in
   `fill_text_field` flagged as a defense-in-depth follow-up so
   `pdfce-cli fill-field` gets the same protection. **Byte-collision
   caution:** `RichText` (text fields) and `RadiosInUnison` (button
   fields) share the SAME bit value — any check for this must be gated
   on `field_type == Text` first, per `forms.rs`'s own doc comment
   warning ("decode against the resolved `/FT`").
2. **R48's "full-rewrite" residual may no longer be unbuildable.**
   `pass-7-form-fill.md` (July) said Flatten's "offer a true removal"
   half was unbuildable because pdfce had no full-rewrite save mode.
   **That premise is now false**: `EditSession::to_full_bytes`/
   `writer::save_full` (`edit.rs:1797`) shipped in Pass 8, built for
   redaction's forced-full-rewrite Apply pipeline
   (`crates/pdfce-gui/src/redact_apply.rs`) — it is a general session
   method with only two stated caveats (destroys signatures; doesn't
   clear a promoted object's stale compressed copy), neither
   flatten-specific. Recommended NOT changing Flatten's confirmation
   weight on this basis alone (the DEFAULT save path is still
   incremental, so the danger being weighed is unchanged) — but flagged
   the residual's status as upgraded from "not buildable, indefinitely
   deferred" to "buildable now, a scheduling call" for the engineer.
   **Always re-check a prior spec's "not buildable yet" claims against
   what has shipped SINCE it was written before repeating them —** this
   is the second time in this project's history a stated blocker turned
   out to have been quietly resolved by unrelated later work (Pass 8
   built the exact primitive Pass 7's spec was missing, with no
   awareness of each other at write time).

**Flatten's confirmation-weight re-argument, now grounded in the ACTUAL
shipped redaction pipeline rather than a hypothetical:** re-verified
against `redact_apply.rs`'s own module doc (redaction's Apply is
UNCONDITIONALLY two forced full rewrites, "no fallback that could
introduce" an incremental path) that redaction's heavy modal+checkbox-gate
weight is earned by unconditional post-save irreversibility, while
Flatten's shipped implementation (Pass 7.1: append-only overlay content
stream, NOT in-place rewrite) leaves the pre-flatten field dict
forensically recoverable under the DEFAULT (incremental) save path —
conditional, not unconditional, irreversibility. Confirmed the old spec's
"delete-shaped not redaction-shaped" call still holds, now with a
concrete shipped mechanism to point at instead of speculation.

**`/TU` (alternate name) as the list row's PRIMARY visible label, not just
an accessibility attribute** — a new recommendation this spec makes,
grounded in the Acrobat-parity RAG (`forms__field_property_model.md`):
`/TU` is documented as the practical accessible name screen readers
actually use for forms (through the interactive-field layer, not the tag
tree), so showing it as the row's VISUAL label too means an operator
reads the exact string a screen reader announces, rather than a raw
dotted PDF field name (`fully_qualified_name`) that most operators find
meaningless. Falls back to `fully_qualified_name` when `/TU` is absent;
the FQN is always still available via tooltip for FDF-import-mismatch
diagnosis. Worth citing as a general pattern for any future
field/object-list row that has both an internal name and a `/TU`-shaped
accessible name.

**Field list order = file order (`AcroForm::fields`'s own DFS order), and
this is not a placeholder simplification — it is Acrobat's own documented
fallback.** Per `forms__tab_order.md`, Acrobat's "Unspecified" tab-order
state (before Structure/Row/Column is actively chosen) falls back to raw
`/Annots` array order, which IS the file's own order. Computing `/Tabs`
S/R/C ordering is real, separate, spec-governed work belonging with field
*authoring*, correctly deferred to P2 and named rather than silently
approximated.

Read the full spec (`docs/ui_specs/forms-panel.md`) before extending any
part of the eventual forms-panel implementation — it has the complete
P0/P1/P2 split, the widget tree (§3), the `place_draft_commit`-sibling
commit-semantics recommendation (§4, citing `main.rs:15767` directly), and
the full "items for the engineer" list (§9).
