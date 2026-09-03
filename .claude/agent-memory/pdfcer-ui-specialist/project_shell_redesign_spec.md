---
name: project_shell_redesign_spec
description: Shell redesign spec (docs/ui_specs/shell-redesign.md), dispatched off the operator's 2026-08-06 confirmation of five neutral UI properties converted from a PDF-XChange-Editor resemblance request under R123 (never opened/looked at the competitor's UI).
metadata:
  type: project
---

Spec delivered: `D:\Dev\pdfce\docs\ui_specs\shell-redesign.md` (2026-08-06).

**Load-bearing findings for future UI work in this project:**

1. **The five properties resolve to ONE real structural move, not five
   separate changes.** Property 1 (persistent panels)'s genuine complaint
   is narrowly `PaneSubject::ActiveTool` vs `::Properties` — the EXACT
   "select above, edit below" relationship decision 017 §A.3 originally
   solved for the RIGHT dock (Objects/Properties) and Pass 24.3 retired
   there because that specific pairing (Objects + document `/Info`) had
   gone stale. It recurs, correctly, one level down, inside `Tool
   Options`. Recommended fix: promote `ActiveTool`→`DockPanel::ArmedTool`
   and `Properties`→`DockPanel::Properties` out of the `PaneSubject` mux
   entirely, into standalone always-visible left-dock panes (no
   `egui_tiles::Container::Tabs` at all — a `Container::Linear` stack).
   Property 3 (Pages placement/behaviour) is satisfied by the SAME move
   (promote `Pages` too). Property 4 (adjacent not floating) is satisfied
   by the SAME move BY CONSTRUCTION (more adjacent = zero-navigation
   always-visible, not one click away). **Reading property 1 literally as
   "all five `PaneSubject` values simultaneously visible" is a named bad
   idea** — `BatchTools`/`Redact`/`Forms` are alternate WORKFLOWS, not
   selection-scoped properties, and forcing simultaneity there buys
   nothing and costs density. This narrowing-to-one-move pattern (a
   short property list often has one common root cause) is worth
   checking for on any future multi-property redesign ask.

2. **A found-not-asked-for accessibility cost: promoting panes to
   always-visible LENGTHENS the Tab focus chain, directly trading against
   R125's own stated reason for non-emitting inactive ribbon tabs ("keep
   the Tab chain short").** Mitigation: give each promoted compartment a
   collapse chevron that NON-EMITS its body when collapsed (same
   mechanism as R125, applied to a new location) — a collapsed pane both
   buys density AND shortens the tab chain, but only if the operator
   actively collapses it; the default (all expanded) does not get either
   for free. Named explicitly as a genuine density-vs-simultaneity
   structural tension with no full resolution, only a runtime lever
   handed to the operator — flagged per the dispatch's explicit request
   to surface tensions rather than split the difference silently.

3. **Property 4's exact intended reading is genuinely ambiguous from the
   neutral property wording alone, and this is the ONE place in the spec
   where I said outright "I'd design this better with a reference
   screenshot, and I'm stopping instead of getting one"** (per the
   binding no-PDF-XChange-research constraint). Narrow reading (docked,
   never floating over the canvas) is satisfied by the property-1 fix
   with zero new code. Maximal reading (a fly-out hugging the selected
   object, closer than a fixed dock column) would structurally
   reintroduce the exact floating `egui::Area` Pass 34.1 deleted and the
   "box somewhere on the screen" complaint that produced decision 024
   §4.4's rule-4 narrowing — recommended REFUSED, not silently routed
   around, and flagged for the operator to confirm which reading he
   meant. **General pattern: when a binding constraint forbids looking at
   a reference and a property's wording is genuinely under-determined
   without one, say so explicitly and pick the reading best-supported by
   the project's own prior reasoning — don't silently guess or go get
   the forbidden reference anyway.**

4. **Two genuine `pdfce-core` gaps found for the new Comments (annotation
   list) surface, same class as forms-panel's `/TU` finding and the
   tool-options-dock spec's tolerance-zero-representation finding:**
   (a) `annot::Annotation` (the Pass 6.0 read model) does not carry
   `/Contents`, `/T` (author), or `/M` (mod date) at all — confirmed by
   reading the struct definition directly, not assumed; recommended
   extending it with three `Option<...>` fields, read-only, additive,
   small enough to ship same-session as the panel. **CORRECTED
   2026-08-12 (Pass 46 spec session): this gap is CLOSED.** Re-reading
   `crates/pdfce-core/src/annot.rs` directly on 2026-08-12 found
   `contents: Option<String>` (`/Contents`, dual-purpose per §12.5.2),
   `title: Option<String>` (`/T`), and `M` all present on the struct —
   evidently added by a Pass between 2026-08-06 and 2026-08-12 with no
   memory update recorded at the time. **The gap that IS still open,
   confirmed the same session:** the struct still does NOT model
   per-subtype geometry (`/L`, `/Vertices`, `/InkList`, `/QuadPoints`)
   — only `/Rect` — a separate, larger finding, see
   [[project_pass_46_canvas_interaction_model_spec]]. Lesson: a memory
   recording "field X is missing from struct Y" is a snapshot, not a
   standing fact — re-grep the struct before citing this kind of gap
   as still-open in any future spec, the same discipline the memory
   system's own guidance already states but is easy to skip when the
   finding still "sounds right" from recall. (b) No general
   `delete_annotation` verb exists — `edit.rs` L3663's own doc comment on
   `remove_redaction_mark` explicitly names WHY it is scoped narrowly
   (Popup companion cleanup, `/IRT` reply chains, must refuse Widget) and
   is the exact checklist a future general delete verb must satisfy —
   cite this comment directly rather than re-deriving the cautions.
   **Always grep the relevant `edit.rs` method's own doc comment for an
   explicit "this is deliberately NOT general-purpose, because X" note
   before assuming a sibling verb is a small addition** — this project
   writes those cautions down in advance specifically so a future spec
   doesn't have to rediscover them.

5. **A found, not requested, honesty problem for property 5's OWN
   usefulness on day one:** Pass 6.1's markup-authoring UI never sets
   `/Contents` on any geometric shape (Ink/Square/Circle/Line/Polygon) —
   only the text-bearing family (Text/FreeText) does. So a first-release
   "comment list" will, on most real pdfce-authored documents, show
   mostly untitled rows ("Square markup, page 3") with an honest
   no-note-text caption rather than reviewer prose. Recommended stating
   this ceiling to the operator BEFORE shipping so a legitimately-scoped
   P0 doesn't read as an under-delivered feature. General pattern: when a
   new "first-class surface" is requested for content pdfce's own
   authoring tools don't yet produce richly, name the gap between "what
   this panel can show" and "what pdfce can currently put there" up
   front, not after ship.

6. **A cheap taxonomy trick worth reusing: segmented in-panel button rows
   don't inherit `egui_tiles`' ≤2-tab-group cap**, because the cap exists
   specifically because `egui_tiles` 0.16.0 answers tab-bar overflow by
   HIDING tabs behind scroll arrows (a real loss-of-access risk) — a
   plain `ui.horizontal` row of toggle buttons instead WRAPS onto a
   second line in a narrow column, so nothing is ever hidden. This is
   what let the redesign put four workflow activities (Batch/Redact/
   Forms/Comments) behind one control without violating the invariant
   `dock.rs`'s own tests enforce for real `egui_tiles::Container::Tabs`
   groups. **Any future "I need more than 2 switchable things in one
   dock compartment" problem in this project should reach for a
   segmented in-panel control, not fight the tab-group cap.**

7. **Ribbon placement for the new Comments entry point reused
   `RibbonTab::Review`'s own doc comment verbatim** ("What am I adding
   for someone else to read?" — browsing what's already been added is
   the same question asked backwards, the identical move `ribbon.rs`
   already names for why Undo/Redo sits on `Edit`) rather than inventing
   new ribbon-taxonomy reasoning — R123 compliance was cheap here because
   the existing tab's organising question already covered the new
   command with no stretch.

Full widget-tree sketch, the 4-pane `Container::Linear` left-dock proposal,
the 5-slice migration path (density first, then the structural promotion,
then the Comments panel, then P1 follow-ons), and the complete P0/P1/P2
table live in the spec file itself.
