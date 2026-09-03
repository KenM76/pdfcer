---
name: project-pass-12-0-canvas-substrate-spec
description: Pass 12.0 canvas-interaction substrate spec (decision 010 R60 / decision 011 beta foundation slice) — key design calls and findings
metadata:
  type: project
---

Wrote `docs/ui_specs/pass-12.0-canvas-substrate.md` (2026-08-01 session,
decision 011's beta foundation slice; decision 010's Pass 12 first slice).
This is the ONE canvas substrate that `pass-6.1-markup-tools.md`,
`pass-7-form-fill.md`, and `pass-8-redaction.md` (all unshipped GUIs at the
time, only minimal-affordance versions shipped) all layer onto — R60 "built
once." Ships literally zero tools that mutate the document; every acceptance
criterion is written to be checkable with no real tool existing.

**Why:** decision 010/011 identified that three prior UI specs each
independently designed the same primitives (focusable canvas, screen↔page
transform, tool-mode dispatch, hit-test/selection, live-preview overlay) with
no single Pass ever building them. Pass 12.0 factors these into one substrate.

**Key design calls, in case a future session needs to re-derive or defend
them:**

1. **Renamed `MarkupTool`→`CanvasTool`, `Action::SelectMarkupTool`→
   `Action::SelectCanvasTool`.** Pass 8's own spec already had to bolt
   `Redact` onto `MarkupTool`; the beta's dimension/vector-edit tools need
   the same enum too. One shared dispatch enum, honestly named, not
   "markup" scope-creeping into everything. When Pass 6.1/8 are actually
   implemented, every `MarkupTool` reference in those spec files should be
   read as `CanvasTool` — flagged as an open item for the librarian, not
   silently reconciled.

2. **`CanvasTool` is a deliberately UNINHABITED enum this Pass** (`enum
   CanvasTool {}`). This is the type-level proof "ships no tool" — the
   field `active_tool: Option<CanvasTool>` can only ever be `None` until a
   future Pass adds a real variant. This is a genuinely elegant pattern
   worth reusing: it makes "zero behavior change to existing viewer" a
   structural fact, not just a passing test.

3. **The uninhabited-enum choice creates a testability gap that must be
   closed a specific way**: since no `Some(CanvasTool::X)` value can exist,
   any state-machine logic written as `match active_tool { ... }` would ship
   with entire branches (pan-suppression's `true` branch, two-stage-Escape's
   cancel branch) completely untested until a real variant lands — exactly
   the wrong moment to discover a substrate bug. **Fix: every state-machine
   decision function takes plain `bool`/`Option<T>`-shaped inputs (e.g.
   `tool_active: bool` derived from `.is_some()`), never matches on
   `CanvasTool`'s variants directly.** This is what makes 100% of the
   substrate's logic unit-testable today with zero real tools. This is the
   single most reusable insight from this Pass — applies to ANY "build the
   uninhabited/empty version of an extensible enum first" situation.

4. **New correctness finding, not present in decision 011's literal
   deliverables list**: none of the three prior specs (6.1/7/8) noticed that
   `screen_to_page`/`page_to_screen` (pass-6.1 §2.1) produce coordinates in
   DEVICE/canvas space (Y-down, top-left origin, `/Rotate`-resolved via
   `page_extent_pts`'s swapped W/H) — a DIFFERENT space from true PDF
   user-space (Y-up, un-rotated MediaBox-relative) that `pdfce-core`'s
   authoring APIs (`Rect`, annotation `/Rect`) actually consume (confirmed
   by reading `add_markup_shape`'s current code, which builds rects
   directly from `page.media_box` — genuine PDF space). This gap was
   invisible because none of the three specs has a shipped commit path that
   exercises it yet (Pass 6.1's shipped GUI is a minimal default-rect
   affordance that never calls `screen_to_page` at all). **The spec adds a
   second bridge** (`canvas_to_pdf_space`/`pdf_space_to_canvas`) built by
   INVERTING `pdfce_render::page_device_geometry(page, 1.0).2` — a
   `tiny_skia::Transform` already re-exported as `pdfce_render::tiny_skia`
   (confirmed via Grep: `pub use tiny_skia;` in `pdfce-render/src/lib.rs`),
   so this reuses the SAME transform the renderer already computes (R49/R60
   "one pipeline," applied to coordinate geometry) rather than four future
   Passes (6.1 commit, 8 redact-mark commit, 12.M2 dimension pick, 9c-min
   node-drag) each hand-deriving rotation-undo math independently — a real
   divergence risk if left unbuilt. Flagged to the librarian as a scope
   refinement to decision 011, not a deviation.

5. **Escape's four-way precedence chain decided ONCE here**, since
   pass-6.1/pass-8 each independently specified a two-stage Escape and
   pass-7 correctly needed a different single-stage rule for its draft-commit
   model. The chain: (1) discardable in-progress gesture → cancel, stay in
   tool; (2) tool active, no gesture → exit tool; (3) no tool, canvas
   selection non-empty → clear selection; (4) fall through to existing
   `ClearSelection` (rail). Written as a pure `resolve_escape(bool, bool,
   bool) -> EscapeOutcome` function, same testability discipline as #3.

6. **`CanvasTargetProvider` trait lives in `pdfce-gui`, NOT `pdfce-core`**
   — deliberate GUI-core-separation call: hit-testing-for-selection is a GUI
   concept; Pass 9a's real implementation is a thin `pdfce-gui`-side adapter
   that calls into `pdfce-core`'s read-only object model, which stays
   GUI-free. Worth defending if a future engineer is tempted to "just put
   the trait where the object model is."

7. **Marquee-vs-pan disambiguation deliberately NOT resolved here** — named
   as Pass 9a's decision (a real UX judgment call: modifier key? explicit
   Inkscape-style "Select" tool vs. always-on default?). **Global-vs-focused
   keyboard dispatch reconciliation deliberately NOT resolved here** — named
   as Pass 7's problem (`collect_keyboard_actions` today reads `ctx.input()`
   unconditionally every frame; a real focused `TextEdit` overlay needs
   focused-widget key consumption). Both are the "flag, don't assume, don't
   solve prematurely" discipline these specs consistently use — a future
   session dispatched to design Pass 9a or Pass 7 should look here first
   for what was already decided vs. explicitly deferred to them.

8. **This Pass adds ZERO new placement-taxonomy instances and ZERO new
   `ui_text.rs` entries** — stated explicitly in the spec rather than left
   implicit, matching the project's own "state the absence explicitly"
   convention (pass-7 §7's "no chord needed" precedent). A pure-substrate
   Pass having nothing to place and nothing to say is a correct, notable
   outcome worth confirming explicitly, not an oversight to flag.

**Read `docs/decisions/010`/`011` first if asked to design any of the
follow-on slices** (9a object model, 12.M1 snapping, 12.M2 dimensioning,
9c-min vector editing, or the eventual real Pass 6.1/7/8 GUI builds) — they
all build on this substrate's exact shape (`CanvasTool`, `TargetId`/
`CanvasTargetProvider`, the two geometry bridges, `resolve_gesture_interrupt`/
`resolve_escape`) and must not invent a parallel one (R60).
