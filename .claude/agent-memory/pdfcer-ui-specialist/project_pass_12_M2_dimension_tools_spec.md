---
name: project-pass-12-m2-dimension-tools-spec
description: Pass 12.M2 dimension-tool UI spec (docs/ui_specs/pass-12.M2-dimension-tools.md) — three-not-four CanvasTool variants, the Measure ▾ menu (a new combination of two existing precedents), the fit-input accessor gap, the tri-state scale ask
metadata:
  type: project
---

Wrote `docs/ui_specs/pass-12.M2-dimension-tools.md` (2026-08-01 session,
decision 011's headline beta capability — scaled measurement/dimensioning).
Read the ACTUAL shipped state before designing (not just decision 011's
prose): confirmed Pass 12.0 (canvas substrate) AND Pass 9a (object/
selection model + centerline, `pdfce_core::vector::*`,
`crates/pdfce-gui/src/object_provider.rs`) have ALREADY SHIPPED — grep
found the real `vector` module, `ObjectModelProvider`, live marquee/
click-select wiring in `main.rs` — while ROADMAP.md's own prose (lines
~3620-3660) still read as if 9a were only "dispatched now," a real
documentation-lag case worth remembering: **always grep the actual crate
tree for a module before trusting ROADMAP prose about what has shipped.**
Pass 12.M1 (snapping) and 12.M2 core (dimension/group/storage) do NOT
exist yet — confirmed by grep, no `snap` module anywhere in `pdfce-core`.

**Key design decision #1, worth defending if reviewed: THREE new
`CanvasTool` variants, not four.** Decision 011 names linear/radius/
diameter/scale as four capabilities; this spec collapses radius+diameter
into ONE `CanvasTool::MeasureCircular` (a display-only Radius|Diameter
toggle over the SAME fitted circle), reasoned directly from decision
011's own value-data-model line ("displayed radius = fitted_radius ×
scale; diameter = 2×") — asserting two separate tools would contradict
the data model's own claim that it's one piece of geometry, two
displays. `MeasureScale` stays a fully separate tool (not a
`MeasureLinear` sub-mode) for the OPPOSITE reason Pass 16.2 gave for Add
Text vs. TextEdit sub-mode: a two-point pick meaning "ordinary dimension"
vs. "calibrate scale" based on invisible state is exactly the silent
mode-shift fuzzy-never-sneaky forbids.

**Key design decision #2, a genuinely NEW combination of two existing
precedents (flag first if reviewing the implementation): the "Measure ▾"
toolbar entry is Markup ▾'s WIDGET (`ui.menu_button`, closes on pick) but
Edit Text/Add Text's DISPATCH (`Action::SelectCanvasTool`, a real
`active_tool` toggle) — NOT Markup ▾'s own dispatch, which is currently
an immediate-author placeholder (`Action::AddMarkupShape`, confirmed by
reading `add_markup_shape` — Pass 6.1's real drawing-tool canvas
interaction is still the deferred, unshipped slice).** Reasoned explicitly
as the rule-3 (progressive disclosure) resolution for "4 new tool
variants would be a 4th toolbar-icon-creep addition after Markup ▾/
Text ▾/Edit Text/Add Text" — a menu keeps the primary toolbar lean while
each menu row still requires the SAME explicit two-step entry (open menu,
click item) as a dedicated toggle button would, so this is purely a
toolbar-real-estate call, not a fuzzy-never-sneaky concern. Only Linear
gets a global chord (`Ctrl+Shift+D`, unverified-unclaimed at spec-authoring
time — re-verify at implementation); Radius/Diameter and Set Scale are
menu-only.

**Genuine core/GUI accessor gap, the load-bearing finding of this spec
(§3.3, §10 ask #4): `CanvasTargetProvider` is deliberately OPAQUE
(hit_test/hit_test_rect/bounds only, no node geometry) — correct for the
substrate's own selection-outline purposes but insufficient for feeding
the Taubin best-fit circle, which needs the actual PDF-space node/sample
points of every picked object.** Re-decomposing the page a second time to
get this would be exactly the "two decompositions quietly diverge" Z2
pattern decision 011 itself names. Fix: NOT a `pdfce-core` change — a
same-crate `pdfce-gui` wiring ask: `ObjectModelProvider` should expose a
`pub(crate) fn page_objects(&self) -> &PageObjects` accessor, and
`OpenDoc` should retain a second, concretely-typed handle to the SAME
`ObjectModelProvider` alongside the opaque `Box<dyn CanvasTargetProvider>`
`target_provider` field (rather than attempting a `dyn Any` downcast) —
so the circular-fit tool reuses the ONE per-page decomposition already
built for selection.

**Positive finding (mirrors Pass 16.2's own "font enumeration needs no
new accessor" precedent): NO new node-level selection primitive is
needed for "radius/diameter from multiple selected nodes … might be
small line segments."** Decision 011's phrasing maps cleanly onto
OBJECT-level multi-pick (already shipped, Pass 9a) because each "small
line segment" IS a separate path object contributing its own anchor
points, and a single polyline object selected alone already exposes its
own full anchor list via `PathObject::page_subpaths()`/`Subpath::
anchors()` (the same accessor `centerline.rs` itself already uses).
Confirmed by reading the actual `hit.rs`/`centerline.rs`/`decompose.rs`
code before assuming a node-selection scaffold was needed — checked, not
guessed.

**The `MeasureCircular` tool owns its OWN pick-set
(`MeasureCircularState.picked_objects: BTreeSet<TargetId>`), NOT
`canvas_selection`** — same "each tool owns its own state shape" rule
Pass 14.3 established for text selection (which bypasses
`CanvasTargetProvider` entirely). A plain click while this tool is active
always means "toggle this object into my fit attempt," never ambiguously
also touching the general-purpose Properties/rail selection.

**Binding data-model ask, not just a display nicety (§4.3, §10 ask #7):
group scale must be a REAL tri-state — never-set / explicit-1:1 /
calibrated — never collapsed to `Option<f64>` where `None` and
`Some(1.0)` would be indistinguishable.** Sourced directly from the
Acrobat measure RAG's own explicit recommendation (`measure__scale_and_
calibration.md`: "show 'no scale set' rather than silently presenting a
real-world-looking number against an implicit, undisclosed 1:1
assumption") — a legitimate full-size (1:1) drawing must never look like
an operator who forgot to calibrate.

**Fuzzy-never-sneaky subtlety worth remembering: derived centerlines
(filled thin quads, `pdfce_core::vector::centerline::CenterlineCandidate`)
get a DISTINCT glyph/label from the routine "segment centerline" snap
target, plus a required extra confirm step (first click promotes to
"proposed," second click or an explicit button commits) — proportional,
NOT a Pass-8-style blocking gate, since nothing here is destructive or
post-save-irreversible** (same weight-calibration reasoning as Pass
15.2's "reflow overflow disclosed calmly, no gate" call). Two visually/
semantically DIFFERENT things share the word "centerline" in decision
011's own prose (the routine stroked-path centerline, priority-5 in the
snap list, vs. the fuzzy filled-quad-derived midline) — the spec calls
this out explicitly so the GUI never conflates them under one glyph.

**Grounding honored from the just-completed Acrobat measure RAG
(`D:\Dev\Rag-Specialized\Acrobat_Features\measure__*.md`):** both
scale-entry paths (ratio vs. calibrate-by-length) are co-equal in
Acrobat, so the spec's scale-entry sub-panel keeps both fully visible
(real-length pre-selected as the recommended default, never buried);
the paper-unit-basis caption is ALWAYS shown regardless of which path is
active; radius/diameter and named-group scoping (vs. Acrobat's
geometric-only `/Viewport` scoping) are both named EXCEED points, not
apologetic gaps; feet-inches (`4'-6"`) plus an operator-selectable
fractional-inch denominator (1/8, 1/16, 1/32) is flagged as the
clearest, best-evidenced exceed-Acrobat opportunity (a durable, still-
open Acrobat feature request); chained-dimension snapping (snapping to a
PRIOR dimension's own geometry) is explicitly named as a pdfce
ORIGINATION with no Acrobat source confirming or denying it either way
— recommended (lower-risk, more useful) but flagged as a P1, not a
parity claim, and named as a real scope item for 9a/12.M1's `PageObjects`
construction (dimension annotations are a NEW object kind, not existing
page-content-stream geometry).

**Placement-taxonomy call: the new Group panel is the SAME "edit →
window" bucket as Properties, NOT an eighth taxonomy instance** — reasoned
against Pass 8's own precedent for why the Redact review panel NEEDED a
genuinely new seventh instance (Tools dock's "files outside the one you
have open" framing would be falsified) — Groups are a live,
canvas-tool-scoped, document-internal editing surface exactly like
Properties, so no new taxonomy entry is warranted. Flagged explicitly so
a future reader doesn't invent one.

Read the full spec at `docs/ui_specs/pass-12.M2-dimension-tools.md`
before reviewing or extending any part of the eventual Pass 12.M2/12.M1
implementation. §10 of that file consolidates all eight core/GUI
accessor asks in one place for the engineer's tracking.
