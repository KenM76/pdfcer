---
name: project_tool_options_dock_and_ce_dimension_properties
description: Tool Options left-dock + implicit-commit-everywhere + ce-dimension property-panel spec (docs/ui_specs/tool-options-dock-and-ce-dimension-properties.md), dispatched off operator feedback 2026-08-05 ("side bar tab docked with page navigation," "click out = accept," ce-dimension units/tolerance/position editing)
metadata:
  type: project
---

Spec delivered: `D:\Dev\pdfce\docs\ui_specs\tool-options-dock-and-ce-
dimension-properties.md` (2026-08-05).

**Load-bearing findings for future UI work in this project:**

1. **Half of this request was already fully specified and unshipped.** The
   2026-08-04 `gesture-commit-and-shell-conventions-audit.md`
   (`commit_active_gesture` empty stub, separate propbar/status Areas
   never merged) already answered Ken's "no separate accept/reject, click
   out = accept" ask verbatim — confirmed still true by re-reading source
   on 2026-08-05 (only that spec's P0 item 1, the fixed-height status
   panel, had shipped). **Always check whether a prior ui-spec already
   covers a new-sounding operator request before re-designing it** — this
   session's job was mostly "confirm + extend + re-prioritize," not
   invent from scratch.

2. **VectorEdit and the ce-dimension position drag (`run_dimension_drag`,
   Pass 27.0/27.1) are ALREADY working precedents for implicit commit-on-
   release, with no floating Accept/Reject box, running in production
   today.** Cite these as the model any future tool's commit-wiring should
   match, not a hypothetical design — TextEdit/AddText/Measure are the
   OUTLIERS in the current app, not the norm.

3. **New three-way classification test for case-(a)-vs-(b), beyond
   "authored vs inferred": blast radius.** MeasureScale's back-calculated
   scale is deterministic (not inferred) but recomputes the DISPLAYED
   value of every OTHER ce dimension in the group, possibly off-screen —
   recommended it keep an explicit Accept anyway, as a deliberate
   exception named on blast-radius grounds rather than the settled rule-4
   authored/inferred line. Worth checking any FUTURE tool-commit design
   against this third axis, not just the two already named in the prior
   spec.

4. **The Group Manager window's units/decimal-places/fractions/standard/
   scale controls (Pass 25.5/27.2) already fully exist** — the operator's
   request sounded like new capability but is mostly a RELOCATION ask
   (floating `egui::Window` → dock), already named as owed by R81's own
   doc comment ("Dimension Groups... remaining floating-window holdout for
   a follow-up migration into the dock"). **Grep group.rs/units.rs/the
   Group Manager code before assuming a units/format ask is new work.**

5. **Genuine core-level gaps found, both same-class-as-tolerance new
   `pdfce-core` asks:** (a) tolerance + tolerance type has ZERO existing
   representation (no field anywhere) — recommended `ToleranceType::{None,
   Symmetric, Deviation, Limit}` on `DimensionRecord` (not `DimensionKind`
   — display property, not measured geometry, mirroring how `Group`
   layers `NumberFormat` over `measured_points`), named honestly
   "SolidWorks-style, never SolidWorks-conformant" per the `DimStandard`
   doc comment's own ISO-129-1 precedent. (b) extension-line drag-to-
   extend/retract needs new `ext_a_overshoot`/`ext_b_overshoot:
   Option<f64>` fields on `DimensionKind::Linear`, mirroring `offset`/
   `text_along`'s exact None-costs-nothing-on-deserialize migration
   pattern (Pass 27.0/27.1).

6. **`doc.selected_dimension` exists but drives only two things today**
   (drag-to-reposition, Delete-key removal) — there is NO property panel
   at all for an already-placed ce dimension, which is the concrete gap
   behind "the ce dimensions I add need to be editable as well." Notably:
   the radius/diameter display toggle is currently DRAW-TIME-ONLY (only in
   the tool-armed propbar) — an operator who placed Radius and later wants
   Diameter has no way to change it without redrawing. Recommended home:
   fold a selection-driven contextual section into the EXISTING
   `DockPanel::Properties` (not Tool Options, not a new taxonomy bucket) —
   and explicitly flagged that `dock.rs`'s own doc-comment premise
   ("nothing else competed for the word Properties") is now FALSE, needs
   correcting when this ships.

7. **New left-hand `egui_tiles::Tree` (separate from the existing
   right-hand `DockTree`) recommended for "Tool Options docked with page
   navigation."** Reasoned against 2 rejected alternatives (folding into
   the existing right dock — wrong side of the window, breaks the 2-label
   invariant; a second hand-rolled tab bar — reintroduces the exact
   inconsistency decision 017 already fixed once). Reuses `DockBehavior`'s
   mechanism (WidgetInfo tab names, R84 bold-on-active) rather than
   inventing a new tab convention — engineer's call whether to genericize
   `DockBehavior<'a>` or duplicate a small sibling impl.

8. **Extension-line handles need a NEW visual vocabulary (tick/hash mark),
   distinct from Bézier/node handles (26.1, filled/hollow circles) and the
   position-drag (no dedicated glyph)** — reasoned why a circle/square
   would collide with 26.1's glyph; cited `run_dimension_drag`'s own
   "a ce dimension takes priority over object selection under the
   pointer" rule as why the two families never need to render
   simultaneously for the same object, so this is disambiguation for
   sequential encounters, not simultaneous collision.

Read the full spec before extending any part of the eventual dock-
relocation or ce-dimension-property-panel implementation — it consolidates
the P0/P1/P2 change list and the "items for the engineer, not mine to
decide" section (implementation-shape call on the new left dock;
whether tolerance/extension-line-drag ship same-session or as separate
Passes; exact pixel sizing).
