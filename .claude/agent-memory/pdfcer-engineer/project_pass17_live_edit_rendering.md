---
name: pass17-live-edit-rendering
description: 2026-08-02 — the GUI rendered the base PDF revision, not the edit overlay, hiding every editing feature since Pass 3.1; decisions 017 (dock) and 018 (fix) resolved
metadata:
  type: project
---

**The defect (decision 018).** `OpenDoc::rasterize_current` and
`ensure_object_provider` both read `session.document()` — the **base** revision.
`edit.rs`'s own doc comment says *"This is the base revision, not the edited
state."* Every editing feature (dimensions, add-text, text edit, reflow, markup,
vector move/delete/node-drag) writes to the `EditSession` overlay, so all of them
were authored correctly and rendered **invisibly**. ONE shared read-path bug —
three method calls in `pdfce-render` plus two call sites in `pdfce-gui` — not
fourteen broken features.

`refresh_pages`'s doc comment was the fossil: *"the document is not reloaded,
because the base revision ... has not changed."* True through Pass 3.1, false
since Pass 6.1 introduced staged appearance streams.

**Why:** every Pass proved its *saved* output correct; none proved its
*displayed* output correct. That gap is what [[feedback_engineer_does_the_observing]]
and the preview-equals-saved oracle exist to close.

**Fix chosen:** generalize `pdfce-render` + `decompose_page` over the existing
`ObjectGraph` trait via `DocumentView` (already exists in `pageops/assemble.rs`),
plus a `StreamSource { Contiguous | Split{base,staged} }` for staged stream
payloads. Render's whole `Document` surface is 3 methods / 50 sites, **45 of
which compile unchanged**. Keep `render_page(&Document, …)` as a thin wrapper so
CLI, roundtrip and font-parity harnesses stay untouched.

**Rejected — worth remembering:** re-serialize-and-reparse routes *viewing*
through the *writer*, inheriting its refusals. R67 refuses incremental save on
recovered files; §5.6 refuses full rewrite on hybrid files. Some real documents
would display **nothing**. A viewer must never be less capable than the parser.

**Operator answers, 2026-08-02:**
- **egui_tiles ADOPTED** — Ken chose full flexible docking ("competes with
  Acrobat... flexible docking that works as well as Inkscape's"), firing
  decision 017's named trigger. Per 017 §6.1 this needs a dated **amendment**
  to 017, not a new decision record. Crate is pre-vetted: MIT OR Apache-2.0,
  1 new package, wasm-clean, exact MSRV/egui match. Two gotchas recorded in
  017 §6.2 (`Tree` has no `Default`; `all_panes_must_have_tabs: true` or the
  tab bar vanishes at one panel).
- **Icon pipeline** → minimal SVG-path parser feeding `tiny-skia` (already a
  dependency via `pdfce_render::tiny_skia`). The recorded pre-rasterize-to-PNG
  plan was not buildable — this machine has no Inkscape/ImageMagick and
  cairosvg's libcairo fails to load.
- **Sequencing:** Pass 17 (live-edit rendering) lands BEFORE new feature work.

**How to apply:** treat "does the GUI read the same overlay the editor writes
to?" as a standing question for any new GUI surface. `session.document()` is
almost always the wrong call — `session.view()` is the edit-aware one. A
`session.document()` audit (Pass 17.1) still has open sites, notably
`count_redaction_marks`, which means redaction marks added in-session are not
counted in the GUI.
