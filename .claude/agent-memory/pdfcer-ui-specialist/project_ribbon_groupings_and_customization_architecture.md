---
name: project_ribbon_groupings_and_customization_architecture
description: Ribbon command-grouping audit + customization-architecture spec (docs/ui_specs/ribbon-groupings-and-customization-architecture.md), dispatched off the operator's 2026-08-05 ruling that dissolved the (ax) rule-12 conflict and asked for SolidWorks/Office-style future customizability.
metadata:
  type: project
---

Spec delivered: `D:\Dev\pdfce\docs\ui_specs\ribbon-groupings-and-
customization-architecture.md` (2026-08-05).

**Load-bearing findings for future UI work in this project:**

1. **This request was ~80% already answered by `docs/decisions/024-
   ribbon-command-surface-and-the-accept-reject-problem.md` (1,629
   lines, 2026-08-04, status DECIDED — NOT STARTED) before this session
   started.** §3.2/§3.3 already contain a complete, reasoned six-tab
   taxonomy (File menu · Home · Insert · Edit · Measure · Protect ·
   View) with per-command placement, rejected alternatives, and R123
   ("command surface structure derives from what pdfce can do, never
   from another product's menus" — the ribbon's own expression of
   CLAUDE.md rule 12 / R61, ALREADY a standing rule at draft time).
   **Always grep `docs/decisions/` for an existing decision record
   before re-deriving a grouping/taxonomy question from scratch** — this
   session's real job was audit + delta + the one genuinely new piece
   (customization architecture), not a fresh design.

2. **A real architectural fork opened ONE DAY after decision 024 was
   written, and it changes what the Measure/Edit tabs actually contain.**
   Decision 024 §3.3 Family A proposed floating tool property bars
   (TextEdit/AddText/Measure) become CONTEXTUAL RIBBON TABS (Pass 24.2).
   Decision 031 / Pass 34.0–34.1 (2026-08-05, shipped `b84fd53` +
   `e15f55b`) instead put them in a LEFT-HAND DOCK (`DockPanel::
   ToolOptions`, `dock.rs` L334-382/L211-219), driven by a MORE SPECIFIC
   later operator instruction ("side bar tab docked with page
   navigation"). These are not the same mechanism, and Family A's
   auto-activate/restore-prior-tab job is already done, more simply, by
   the dock's arm-raises/disarm-does-not-lower behavior. **Recommended:
   Family A (ribbon contextual TOOL tabs) is SUPERSEDED for tool-options
   content; Family B (selection contextual tabs, Pass 24.3, Object/
   Dimension(pdfce)/Annotation) is UNAFFECTED and still correctly
   blocked on Passes 22.0/23.2.** Flagged as a decision-log correction
   the engineer should dispatch the librarian to file (same shape as the
   Pass 27.2 tick-2 reversal) — not yet done as of this spec. **Anyone
   touching Pass 24.1/24.2 must check whether this correction has been
   filed before building Family A as originally scoped.**

3. **Naming collision caught before it shipped: do NOT name a ribbon-UI
   group identifier `GroupId`.** `pdfce_core::dimension::GroupId`
   already exists (a ce-dimension group's identity, confirmed by grep,
   `main.rs:2019`). The customization-architecture registry design uses
   `RibbonGroupId` instead, named explicitly in the spec as a rule-15-
   adjacent collision-avoidance finding. **Grep the full identifier
   before proposing any new type name in this codebase** — the project
   has enough parallel taxonomies (dock panels, canvas tools, dimension
   groups, ribbon groups) that short generic names collide silently.

4. **The customization architecture's central move: extract today's
   ~600-line inline `toolbar_controls` into a static registry
   (`RibbonCommandId` enum + `RibbonCommand`/`RibbonGroupDefault`
   tables) keyed on STABLE COMMAND IDENTITY, not source position —
   and recommend building it AS Pass 24.1 itself, not as separate
   speculative infrastructure.** Reasoning worth reusing: decision
   024's own Pass 24.1 acceptance criterion B1 ("every command reachable
   from exactly one ribbon location") is exactly what an exhaustive
   match over a `RibbonCommandId` enum proves BY CONSTRUCTION, where a
   hand-written relocation proves it only by manual enumeration —
   building the registry is the CHEAPEST way to satisfy a criterion that
   already existed, not an added cost. This is the general pattern to
   reuse: when a customization/extensibility ask arrives for a surface
   that is currently inline widget calls, look for whether the
   surface's OWN existing acceptance criteria are already best satisfied
   by a registry, rather than treating "build the registry" and "satisfy
   today's criteria" as two separate line items.

5. **Persistence for a customizable ribbon needs R15 (same as the QAT
   customization decision 024 §3.5 already refused for the same
   reason), but the STRUCTURAL change (the registry) does not — this
   split is the reusable answer to "customizable in the future" asks
   generally.** Build the identity-keyed data model now (cheap,
   independent of persistence); do NOT build the reorder/hide/reset UI
   or any persistence until the storage layer exists — a customization
   UI that forgets on restart is worse than none (decision 024's own
   framing, reused here). Recommended persistence shape once R15 lands:
   a FLAT override (`Vec<(RibbonGroupId, Vec<RibbonCommandId>)>` +
   a hidden-set), NOT a serialized tree like `egui_tiles::Tree` needs —
   a ribbon's tab set is fixed; only CONTENTS are reorderable, which is
   a much smaller serialization surface than the dock's arbitrary split
   tree. Fail-soft contract mirrors `dock.rs`'s existing one (unknown id
   dropped + disclosed; missing-from-override id re-appended at its
   default position, since unlike a dock panel a command has no
   reasonable "just don't show it" fallback).

6. **Icon-coverage audits in this project should be scoped by R124
   ("no icon needed until its command is built"), not by the full
   future taxonomy.** Checked every group in decision 024's taxonomy and
   found icon gaps ONLY where a command (a) already exists today AND
   (b) is being relocated by the CONFIRMED taxonomy — page-ops Delete/
   Extract/Clear (conditional on a still-open Home>Pages promotion
   question), Measure sub-tools Linear/Circular/Set-Scale (currently
   plain-text menu rows), Redact sub-verbs, and the Objects/Pages/Tool-
   Options dock panels (the latter two GENUINELY CURRENT gaps — Pass
   34.1 shipped 2026-08-05 with no icon assigned at all, PROVENANCE.md
   needs a follow-up entry independent of any ribbon work). Explicitly
   did NOT flag Encrypt/Sign/Certify/OCR/Crop/Optimise/Format-surface
   icons — those are backlog features, R124 exempts them by name.

7. **Ken's ruling on the ribbon-groupings question, verbatim, closes
   TWO open ROADMAP questions at once, worth recognizing this pattern
   generally:** "make the groupings make sense... they might be similar
   to acrobat's but if it makes more sense to differ, do so" answers
   both (ax) (rule-12-conflict — he did NOT grant the requested
   amendment; he answered the underlying organizing question instead,
   which R123 already covered) and (ay) (ribbon specificity — his
   answer names a DECISION PROCEDURE, "organizational sense governs,"
   rather than a target resemblance level, which is a complete answer
   even though it doesn't name a specific closeness). **When an operator
   answers the question BEHIND an open item rather than the item's
   literal phrasing, check whether that also closes an adjacent open
   item** — it did here, and both should be recommended-closed together
   rather than just the one literally addressed.

Full taxonomy-delta table, the Home>Pages hidden-vs-disabled open
question, the two-dock Reset-panel-layout problem, the full
`RibbonCommand`/`RibbonCommandKind` pseudocode, and the icon-gap table
live in the spec file itself — this memory records the reusable,
generalizable findings, not the row-by-row content.
