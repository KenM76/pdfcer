---
name: project_icon_set_and_toolbar_spec
description: Icon-set + toolbar UI spec (operator priority #2) — audit of the current glyph-only toolbar, ScripTree SVG style contract, icon→feature mapping, and the flagged SVG-in-egui rendering fork.
metadata:
  type: project
---

Spec delivered: `D:\Dev\pdfce\docs\ui_specs\icon-set-and-toolbar.md` (2026-08-01),
answering the operator's "add d:/dev/scriptree style icons for all gui
features" directive (`ROADMAP.md` "★ Icon set", priority #2 of a four-item
sequence: dimensioning tool → icons → text-handling → forms).

**Why this matters for future icon/UI work in this project:**

1. **pdfce has ZERO icon images today** — every "icon" is either an emoji
   (📂💾🧰📋), a bare Unicode dingbat (◀▶−+↺↻↶↷▤🗩⌨), or plain text
   (Properties, Fit page, Markup ▾). Confirmed by reading `main.rs`'s
   toolbar in full (~3658–4069) and grepping every `ui_text::*_button()`.
   Any future icon work should treat this spec's §0/§2 audit as the
   ground truth, not assume icons already exist anywhere.

2. **`PdfceApp::icon_button` (main.rs ~3633) is real, load-bearing
   infrastructure that must be REUSED, not bypassed** — it already wraps
   every icon-only glyph with `ICON_BUTTON_SIZE=(28,24)` sizing AND an
   explicit `WidgetInfo::labeled` accessible-name override (the tooltip
   text, since egui derives accessible names from visible labels which
   for a bare glyph would be nonsense). Swapping the glyph for an image
   inside this wrapper is correct; a parallel image-button path that
   skips it would regress a real, already-shipped P1-6 accessibility fix.

3. **`Self::toggle_label` (bold-text-on-selected) is the existing rule-6
   non-colour cue for toggle state** — it has NO analogue for an
   icon-only button (nothing to bold). This spec's §5.3 names the fix
   (a selected-state outline/stroke ring around the icon) as the one
   place an icon swap creates real regression risk if done carelessly.
   Any future icon-only toggle should use this same outline convention.

4. **The five-way GUI placement taxonomy is durable and citable**
   (`ARCHITECTURE.md` §12(b), ui-specialist's own prior deliverable):
   view-state→toolbar view group; edit→toolbar/window; selection-scoped→
   rail; advanced→Tools dock; disclosure→status bar. Used to justify
   where Search/Find should eventually land (toolbar, not buried in
   Tools dock) once built.

5. **ScripTree icon style contract (reverse-engineered from the SVGs,
   useful for ANY future new-icon work in this project):** 48×48
   viewBox, `fill="none" stroke="currentColor" stroke-width="2.5"
   stroke-linecap="round" stroke-linejoin="round"`, ~6-8px inset margin,
   every file carries a one-line comment disclaiming trademark risk.
   Vendor-logo files (`icon-autocad/inventor/msoffice/revit/solidworks`)
   are EXCLUDED from pdfce's icon language — real trademarks, not generic
   glyphs. `icon-mesh.svg` is a byte-identical duplicate of
   `icon-package.svg` (verified). `icon-forest.svg` is a totally
   different asset family (1024 viewBox, filled circles, no
   currentColor) — ScripTree's own category-tree decoration, not part
   of the flat tool-icon set; do not use as a style reference.

6. **One real ❌-grade collision found:** a future Redaction icon must
   NOT reuse `icon-scissors.svg` (already claimed by Split, a routine
   low-stakes structural op) — redaction is the highest-stakes feature
   in the app (R35/R52/R58) and needs visual separation from an
   unrelated "cut" glyph. Spec gives Redaction a deliberately
   SOLID-FILLED icon (the ONE rule-based exception to the set's
   outline-only convention — a solid black bar is what redaction
   actually leaves, so an outline-only icon would understate the
   feature). Flag this to whoever eventually builds the Pass 8.0 GUI
   follow-up (canvas mark/apply tool, still deferred per Pass 8.0's own
   ship notes) — do not let them reach for scissors out of habit.

7. **Two things explicitly flagged, NOT decided (per my scope
   boundary):** (a) SVG-in-egui rendering pipeline — build-time
   pre-rasterize (no new dep, fixed DPI) vs. runtime `resvg`/`usvg`
   (crisp at any DPI, MPL-2.0 potential new dependency needing rule-13
   operator sign-off); (b) ScripTree icon provenance — likely fine since
   Ken owns both projects, but confirm original-vs-derived-from-a-
   third-party-icon-font before bundling, same LEGAL.md §5 discipline
   already applied to test-corpus PDFs, now applied to icon art.

8. **Recommended, non-forced exception:** leave "100%"/actual-size as
   plain text, not an icon — a numeral reads clearer than any glyph
   substitute for exact zoom level. Named explicitly as a
   discoverability-checklist call the engineer can override, not a
   silent scope-drop of "icons for all features."

9. **Theming approach specified regardless of which rendering pipeline
   wins:** rasterize each icon ONCE as a white-on-transparent alpha
   MASK, tint at draw time via `.tint(ui.visuals().text_color())` (read
   inside the disabled `Ui` scope for the disabled state, so it
   auto-matches egui's existing fade behavior with zero new logic) —
   one asset serves light/dark/disabled/normal, no baked colour
   variants to keep in sync.

Full mapping table (27 shipped-control rows + 16 backlog reservation
rows) lives in the spec file itself — this memory records the
reusable, generalizable findings, not the row-by-row assignments.
