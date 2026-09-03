---
name: project-decision-024-family-a-superseded-and-r155
description: Decision 024 §3.3 Family A (contextual ribbon tabs per armed tool) is SUPERSEDED for tool-options content by decision 031 / Pass 34.1's DockPanel::ToolOptions, on the operator's own later instruction. Family B unaffected. R155 minted (pre-dispatch search-discipline rule). Check before describing Pass 24.2's scope.
metadata:
  type: project
---

**2026-08-05 (SESSION_LOG continuation 94).** Decision 024 §3.3 (2026-08-04)
proposed **Family A — TOOL tabs keyed on `doc.active_tool`**: one contextual
ribbon tab per armed tool (Measure/Add Text/Edit Text/Edit Objects), each
ending in a fixed Finish (Accept/Reject) group, replacing the three floating
property-bar `egui::Area`s. Pass 24.2 was scoped to build it.

**That is not what shipped.** One day later the operator gave a more
specific instruction: *"all of the options should be shown in a side bar
tab docked with the page navigation tab"* / *"add text and measure tools
should [be] integrated into the context sensitive sidebar tab."* Decision
031 / Pass 34.1 built `DockPanel::ToolOptions` — a **dock panel**, not a
ribbon tab — which already does Family A's job (auto-raise on arm, no
forced return on disarm, fixed predictable location for a tool's live
controls + commit/reject) on the other side of the window, shipped in three
slices (`e15f55b`, `fae916d`, `13f3c0b`) before Pass 24.2 was ever started.

**Filed as decision 024's new, append-only §11** (original §3.3 untouched)
at `pdfce-ui-specialist`'s own recommendation
(`docs/ui_specs/ribbon-groupings-and-customization-architecture.md` §2).
Mirrored in `ARCHITECTURE.md` §12's continuation-94 entry and `ROADMAP.md`'s
★ Pass 24.0–24.5 Next-up entry (a new update banner, not a rewrite).

**Family A is SUPERSEDED for tool-options content only. Family B (SELECTION
tabs keyed on `TargetId` kind — Object / Dimension (pdfce) / Annotation) is
UNAFFECTED**, still correctly blocked on Passes 22.0/23.2.

**Consequence, load-bearing for anyone scoping Pass 24.2 later:** if/when
built, the Measure/Edit/Add-Text/Edit-Objects ribbon contextual tabs carry
**invocation only** (arm the sub-tool, manage ce-dimension groups,
document-scope commands) — **never** the armed tool's live controls, which
live in `DockPanel::ToolOptions` and must not be duplicated onto a ribbon
tab. Reading decision 024 §3.3's table alone, without this correction, would
have you build a second home for controls that already have one.

**Also this session:** three open operator questions closed from one
operator answer (*"just make the ribbon command groupings make sense... if
it makes more organizational sense to have them a different way then do
so. we might want to make these customizable in the future like you can
with solidworks and ms office."*) — **(ax)** DISSOLVED (not amended; rule 12
stands, `pdfce-acrobat-librarian` not dispatched), **(ay)** CLOSED
(organizational sense governs, no resemblance target), **(aw)** CONFIRMED
(`MeasureScale` keeps its explicit confirm — ratification appended to
decision 031 §3, a second ground alongside blast radius: a typed value is
already required, so the commit point is free).

**Future direction, not yet scoped:** ribbon groupings are meant to become
operator-customizable ("like SolidWorks and MS Office"). The new ui-spec
(`docs/ui_specs/ribbon-groupings-and-customization-architecture.md` §5)
architects this now via a static `RibbonCommandId`/`RibbonCommand`/
`RibbonGroupDefault` registry — deliberately naming the group type
`RibbonGroupId`, NOT `GroupId`, to avoid colliding with
`pdfce_core::dimension::GroupId` — without building reorder/hide/reset UI
or persistence yet (explicitly recommended against, for now).

**R155 minted** (a fifth, previously-unlisted candidate that claimed the
slot R154 had reserved for decision 030's three contingent candidates,
which move again to R156 — see
[[project_decision_031_leftpanel_correction_and_r154]] for the full
transfer chain). R155 is a **pre-dispatch search-discipline rule**: before
dispatching a fresh design/consultant task, check whether an existing
decision record already answers the question. Instance: the engineer
dispatched the ribbon-grouping design task without first checking that
decision 024 already contained a complete taxonomy + R121–R125; the
specialist's own opening section caught it ("this is mostly an audit, not
a fresh design") and audited instead of re-deriving — nothing lost, but by
the specialist's own diligence, not by a built-in dispatch check.
**Distinct from R151–R154**: those audit an artifact against reality
AFTER it ships/is written (code-against-code or prose-against-type-system);
R155 fires BEFORE a design brief exists, against the decision log itself.

**How to apply:** don't describe Pass 24.2 (or any future ribbon-tab build)
as delivering tool property-bar controls — that job belongs to
`DockPanel::ToolOptions` now. Before dispatching a design task in this
project, grep `docs/decisions/` first; this is now a named standing rule
(R155), not just a good habit.
