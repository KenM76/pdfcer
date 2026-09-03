//! # R85 — the preview-equals-saved oracle (Pass 17.2)
//!
//! > **R85 — Preview-equals-saved.** For every editing operation, the raster
//! > of the edited session view must be pixel-identical to the raster of the
//! > saved-then-reloaded document. What the operator sees before saving is
//! > what they get after saving.
//! >
//! > — `docs/ROADMAP.md`, Standing rules; stated in
//! > `docs/decisions/018-edited-state-is-what-the-canvas-renders.md` §9.
//!
//! ## Why this file exists
//!
//! Between Pass 3.1 and Pass 16.2, **fourteen** editing features shipped
//! green. Each one saved its output, reloaded it, and proved the bytes were
//! right. Not one of them proved the operator could *see* the edit — and
//! none of them could, because the GUI's read path passed
//! `EditSession::document()`, the base revision, which by construction never
//! carries an unsaved edit. Every gate passed; every feature was invisible.
//!
//! That is a **gate** defect. The missing gate is this file. Its shape
//! follows from the diagnosis: the failure was a *divergence between two
//! renderings of the same logical document* — the one on screen and the one
//! in the saved file — so the test compares exactly those two rasters and
//! demands they be identical. Under the original defect every case below
//! fails, because the preview side would render the base while the saved
//! side rendered the edit.
//!
//! It inverts cleanly against the project's two existing content oracles:
//! R46 proves an authored object is *present*; `ARCHITECTURE.md` §5.9's
//! absence test proves a redacted one is *gone*; R85 proves the *picture*
//! agrees with the file.
//!
//! ## What it caught on its first run
//!
//! Worth recording, because it is the argument for the whole rule.
//! `EditSession::flatten_fields` was **burning nothing into the page**. It
//! produced three separate whole-dictionary writes to the same page object
//! in one command — `/Contents`, `/Resources /XObject` and `/Annots` — each
//! computed from the pre-command state, so applying them overwrote rather
//! than composed and only the last survived. Flatten therefore deleted the
//! fields and the widgets exactly as designed, created the burn stream and
//! the appearance XObjects exactly as designed, and left the page
//! referencing neither: every flattened form lost its visible values.
//!
//! Every existing flatten test passed throughout, because they asserted the
//! outcome counts (`fields_flattened`, `widgets_burned`, `pages_touched`) —
//! all of which were correct — and none of them rendered the result. That is
//! the same blind spot, in miniature, that decision 018 was written about:
//! the operation was *performed* correctly and the *picture* was wrong, and
//! only a test that looks at pixels can tell the difference. Fixed in
//! `edit.rs`; see the note at the `flatten_fields` write-batch loop.
//!
//! ## Why it lives here (`crates/pdfcer-render/tests/`) and not in `tools/`
//!
//! Decision 018 §8 leaves the placement open — "a `tools/` harness or hidden
//! test-only subcommand" — and explicitly notes that rule 11 (CLI parity)
//! does **not** require a new public `pdfcer` subcommand for it, because
//! the one-shot `parse → edit → save` CLI model never renders an unsaved
//! session. Three reasons put it here instead:
//!
//! 1. **It must run by default.** Every `tools/*` crate in this repo is a
//!    detached single-crate workspace listed in the root `Cargo.toml`'s
//!    `exclude`, so `cargo test --workspace` does not build or run any of
//!    them. A gate that only runs when somebody remembers to run it is the
//!    same class of thing as the gate that was missing — this one is wired
//!    into the standing `cargo test --workspace` gate and cannot be
//!    forgotten.
//! 2. **No new public surface is needed.** `render_page_view` already exists
//!    (Pass 17.0), so a live `EditSession` can be rasterized headlessly with
//!    the shipped API. Nothing here is a test-only back door, which means
//!    the oracle exercises the *same* entry point the GUI canvas calls —
//!    not a parallel path that could quietly diverge from it.
//! 3. **This crate has both halves.** `pdfcer-render` depends on
//!    `pdfcer-core`, so `EditSession` (the preview side) and the rasterizer
//!    (both sides) are available without a new dependency, and this is
//!    already where the Pass 17.0 direction test
//!    (`edited_view_is_what_renders.rs`) lives — which names this file as
//!    its follow-on.
//!
//! ### On "reuse the raster oracle `tools/roundtrip` already has"
//!
//! Decision 018 §9 says to reuse it. Reuse turned out to be impossible in
//! the literal sense and unnecessary in the substantive one:
//! `tools/roundtrip`'s comparison is **two private functions in a
//! binary-only, workspace-excluded crate** (`render_first_page` and
//! `check_raster` in `tools/roundtrip/src/main.rs`) — there is no library
//! target to import from. What matters is that the *comparison semantics*
//! do not fork, and they do not: roundtrip asserts raw
//! `Pixmap::data()` byte equality with **no tolerance**, and so does
//! [`assert_pixmaps_identical`] below. The one deliberate difference is the
//! failure message — roundtrip can only report differing buffer lengths,
//! while this reports the differing-pixel count and the first differing
//! coordinate, because "which pixels" is the first question anyone debugging
//! a preview mismatch asks.
//!
//! Exact equality (not a tolerance) is right **here specifically** because
//! both sides are the *same rasterizer at the same scale with the same
//! options*; the only variable is which revision it read. Any tolerance
//! would be a place for a real divergence to hide. (`tools/render-parity`'s
//! `PIXEL_DELTA_T = 32` model is for the genuinely different problem of
//! cross-renderer comparison against pdfium.)
//!
//! ## The shape of every case
//!
//! [`check`] is the whole oracle; each `#[test]` is a few lines of setup.
//!
//! ```text
//!   fixture  ──►  EditSession  ──►  apply ONE editing operation
//!                      │
//!         ┌────────────┴────────────┐
//!         │                         │
//!    session.view()          to_incremental_bytes()
//!         │                    → Document::from_bytes
//!         │                         │
//!   render_page_with_view    render_page_with_view
//!         │                         │
//!         └──────► IDENTICAL? ◄─────┘
//! ```
//!
//! Both sides go through `render_page_with_view` with **the same**
//! `RenderOptions::default()` (annotations on, `FontEnvironment::bundled()`
//! — never an injected face, per decision 012 R63's determinism invariant),
//! at the same [`SCALE`]. The page comes from the matching revision on each
//! side (`EditSession::pages()` for the preview, `page_tree::pages()` for the
//! reloaded document); crossing them is decision 018 §10 hazard 2 and is
//! precisely what this must not do.
//!
//! ### The second assertion: the edit has to have DONE something
//!
//! A preview-equals-saved test would pass trivially if the operation were a
//! no-op — two identical rasters of an unedited page. So [`check`] also
//! renders the **pristine base** and takes a [`Visible`] verdict from each
//! case:
//!
//! - [`Visible::Yes`] — the operation must change the page's pixels. This is
//!   the R46-style presence half, and it is what makes the equality
//!   assertion mean something.
//! - [`Visible::No`] — the operation is visually neutral **by design**, with
//!   the reason recorded at the call site. Exactly one case uses it
//!   (`flatten`: burning a widget's `/AP` into page content is supposed to
//!   look identical, which is the property that makes flattening safe), and
//!   there `Visible::No` is itself the meaningful assertion.
//!
//! ## Coverage
//!
//! Decision 018 §9 / R85 name **twelve** operations. **Eleven of the twelve
//! are covered**; the twelfth (`redact-apply`) cannot be, for a structural
//! reason spelled out immediately below. The table has more than eleven rows
//! because `annotate` is exercised in three distinct shapes and redaction
//! *marking* earns a row of its own:
//!
//! | Operation | Test | Fixture |
//! |---|---|---|
//! | `add-text` | [`add_text_preview_equals_saved`] | `addtext/plain.pdf` |
//! | `annotate` (markup) | [`markup_annotation_preview_equals_saved`] | `addtext/plain.pdf` |
//! | `annotate` (text markup) | [`highlight_annotation_preview_equals_saved`] | `addtext/plain.pdf` |
//! | `annotate` (free text) | [`free_text_annotation_preview_equals_saved`] | `addtext/plain.pdf` |
//! | `dimension-add` | [`dimension_preview_equals_saved`] | `dimension/plain-base.pdf` |
//! | `object-move` | [`object_move_preview_equals_saved`] | `vector/edit.pdf` |
//! | `object-delete` | [`object_delete_preview_equals_saved`] | `vector/edit.pdf` |
//! | `node-move` | [`node_move_preview_equals_saved`] | `vector/edit.pdf` |
//! | `edit-text` | [`edit_text_preview_equals_saved`] | `textedit/nonembedded.pdf` |
//! | `format-text` | [`format_text_preview_equals_saved`] | `textedit/format_color.pdf` |
//! | `reflow` | [`reflow_preview_equals_saved`] | `reflow/reflow.pdf` |
//! | `fill-field` | [`fill_field_preview_equals_saved`] | `forms/demo-form.pdf` |
//! | `fill-field` (3 widgets, 2 pages) | [`fill_field_multi_widget_preview_equals_saved`] | `forms/multi-widget-form.pdf` |
//! | `flatten` | [`flatten_preview_equals_saved`] | `forms/demo-form.pdf` |
//! | redaction *marking* | [`redaction_mark_preview_equals_saved`] | `redact/demo-secret.pdf` |
//!
//! ### `redact-apply` is NOT covered, and cannot be — a real gap, named
//!
//! R85 lists `redact-apply`. It is **structurally outside** the invariant as
//! stated, and silently omitting it is exactly the hole this rule exists to
//! close, so it is stated here instead:
//!
//! - Applying a redaction is not an `EditSession` operation. It is the free
//!   function `pdfcer_core::redact::apply_redactions(&Document, &SaveOptions)
//!   -> (Vec<u8>, RedactionReport)`, which consumes a **loaded document** and
//!   emits a **new file** (internally a full rewrite — the one deliberate
//!   exception to the §5 minimal-diff invariant, because redaction must
//!   truly remove content). There is no session overlay in which an applied
//!   redaction exists, therefore no preview raster to compare a saved one
//!   against. "Preview equals saved" has no left-hand side.
//! - Consistent with that, the GUI has **no apply-redactions flow at all**
//!   today: `pdfce-gui` only marks (`add_redaction`,
//!   `mark_redactions_by_*`) and discloses unapplied marks
//!   (`count_redaction_marks`). Apply is CLI-only
//!   (`pdfcer`'s `redact --apply`). An operator therefore cannot see a
//!   pre-apply preview in the first place.
//!
//! What *is* covered is the half that has a preview: **marking**. A
//! `/Redact` mark authored this session paints a red outline preview
//! (deliberately never a solid fill, so a mark can never be mistaken for a
//! completed redaction), and [`redaction_mark_preview_equals_saved`] proves
//! that outline survives save/reload pixel-identically.
//!
//! Closing the apply half properly needs one of two things, and both are
//! design decisions rather than test work: a session-level `apply_redactions`
//! whose result lives in the overlay, or a GUI apply flow that re-opens the
//! rewritten document (in which case the meaningful invariant is
//! *reopened-equals-written*, not preview-equals-saved). Recorded for the
//! roadmap; not papered over here.
//!
//! ## How to add an operation to this oracle
//!
//! 1. Pick or add a synthetic fixture under `fixtures/synthetic/<feature>/`
//!    (LEGAL.md §5 — synthetic or rights-cleared only, with a
//!    `PROVENANCE.md`).
//! 2. Write a `#[test]` that builds an `EditSession` over it and applies
//!    **exactly one** operation. One operation per test: a mismatch must
//!    name the operation that caused it without bisection.
//! 3. Call [`check`] with a stable operation label, the page index the edit
//!    touched, and a [`Visible`] verdict — `Visible::No` only with a written
//!    reason it is visually neutral by design.
//! 4. Add a row to the coverage table above. If the operation *cannot* be
//!    covered, say so in the gap section with the structural reason, as
//!    `redact-apply` does. A silently skipped operation is the failure mode
//!    R85 exists to prevent.
//!
//! ## If a case ever fails
//!
//! Do not add a tolerance and do not weaken the assertion. A mismatch means
//! one of exactly three things, and all three are worth knowing:
//!
//! - the session view resolves something the saved file does not (or the
//!   reverse) — a `StreamSource`/staging bug, the R45 coordinate system;
//! - the writer emits something the overlay did not describe — a §5
//!   round-trip violation;
//! - the operation genuinely cannot be previewed, in which case document
//!   *why*, like `redact-apply` above, and assert the narrower true property
//!   instead.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::annot_author::{Color, MarkupSpec, TextAnnotSpec, TextMarkupKind};
use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, NewImage, NewRadioButton, NewTextField};
use pdfcer_core::fontdata::Std14;
use pdfcer_core::image_import;
use pdfcer_core::page_tree::{self, Page, Rect};
use pdfcer_core::text_edit::{
    AddTextRequest, EditOptions, EditRequest, FormatOptions, FormatRequest, MetricSpec,
    ReflowRequest, ScriptPosition, StyleSynthesis,
};
use pdfcer_core::vartext::{Quadding, TextColor};
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::view::DocumentView;
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::tiny_skia::Pixmap;
use pdfcer_render::{RenderOptions, render_page_with_view};

/// Raster scale for both sides, in device pixels per PDF point
/// (`scale = dpi / 72`), so 1.0 is 72 DPI.
///
/// The value is not load-bearing for correctness — a divergence between the
/// two revisions shows at any scale — but it is deliberately the same 1.0
/// the sibling render tests use, and deliberately not larger: the oracle
/// runs a dozen page renders on every `cargo test --workspace`, and a gate
/// people are tempted to skip for being slow is a gate that decays.
const SCALE: f32 = 1.0;

/// Whether the operation under test is expected to change what the page
/// looks like.
///
/// This exists so that "the two rasters matched" can never be satisfied by
/// an operation that did nothing at all. See the module docs' *"the edit has
/// to have DONE something"*.
#[derive(Debug, Clone, Copy)]
enum Visible {
    /// The edit must alter the page's pixels versus the pristine base.
    Yes,
    /// The edit is visually neutral **by design**; the payload is the reason,
    /// which is printed in the failure message if the page changes anyway.
    No(&'static str),
}

/// Path to a committed synthetic fixture.
fn fixture(dir: &str, name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(dir)
        .join(name)
}

/// Load a fixture into an [`EditSession`] ready for one operation.
fn session(dir: &str, name: &str) -> EditSession {
    let path = fixture(dir, name);
    let doc =
        Document::load(&path).unwrap_or_else(|e| panic!("fixture {} loads: {e}", path.display()));
    EditSession::new(doc)
}

/// The render options both sides share.
///
/// Constructed in one place on purpose: the entire oracle rests on the two
/// renders differing in *nothing but the revision they read*, and two call
/// sites each spelling out their own options is the obvious way for that to
/// stop being true. `default()` means annotations ON (an authored markup or
/// dimension is painted, which several cases depend on) and
/// `FontEnvironment::bundled()` (no injected face — decision 012 R63).
fn options() -> RenderOptions {
    RenderOptions::default()
}

/// Rasterize one page of one revision.
fn raster(view: &DocumentView<'_>, page: &Page, label: &str) -> Pixmap {
    render_page_with_view(view, page, SCALE, &options())
        .unwrap_or_else(|e| panic!("{label}: rasterizes: {e}"))
        .pixmap
}

/// Assert two pixmaps are byte-identical, reporting *where* they differ.
///
/// Same semantics as `tools/roundtrip`'s `check_raster` — raw RGBA byte
/// equality, no tolerance (see the module docs for why exactness is right
/// here and why the tool's helper could not literally be imported). The
/// richer message is the only difference: a differing-pixel count plus the
/// first differing coordinate and its two colours, because a bare "the
/// buffers differ" sends the reader straight back to a debugger.
fn assert_pixmaps_identical(op: &str, preview: &Pixmap, saved: &Pixmap) {
    assert_eq!(
        (preview.width(), preview.height()),
        (saved.width(), saved.height()),
        "{op}: preview raster is {}x{} but the saved-then-reloaded raster is {}x{} — the two \
         revisions disagree about the page geometry itself (/MediaBox, /CropBox or /Rotate), \
         which is a preview-equals-saved failure before a single pixel is compared",
        preview.width(),
        preview.height(),
        saved.width(),
        saved.height(),
    );

    let (a, b) = (preview.data(), saved.data());
    if a == b {
        return;
    }

    let width = preview.width() as usize;
    let mut differing = 0usize;
    let mut first: Option<(usize, usize, [u8; 4], [u8; 4])> = None;
    for (i, (pa, pb)) in a.chunks_exact(4).zip(b.chunks_exact(4)).enumerate() {
        if pa != pb {
            differing += 1;
            if first.is_none() {
                first = Some((
                    i % width,
                    i / width,
                    [pa[0], pa[1], pa[2], pa[3]],
                    [pb[0], pb[1], pb[2], pb[3]],
                ));
            }
        }
    }
    let total = a.len() / 4;
    let (x, y, pv, sv) = first.expect("byte-unequal buffers contain a differing pixel");
    panic!(
        "{op}: R85 VIOLATION — preview-equals-saved.\n  \
         {differing} of {total} pixels differ ({:.3}%).\n  \
         first at (x={x}, y={y}): preview RGBA {pv:?} vs saved RGBA {sv:?}.\n  \
         The operator would see one thing before saving and get another after. Do NOT relax \
         this assertion: either the session view and the saved file genuinely disagree (a \
         staging/StreamSource or writer bug), or this operation cannot be previewed and needs \
         its own documented, narrower property — see this file's module docs.",
        (differing as f64) * 100.0 / (total as f64),
    );
}

/// **The oracle.** Assert R85 for one already-applied editing operation.
///
/// `session` must have had **exactly one** operation applied to it, and
/// `page_index` is the page that operation touched, expressed in the
/// *session's* page-index space (which is also the saved document's, since
/// saving preserves page order).
///
/// Three renders, in a deliberate order:
///
/// 1. **base** — `session.document().view()`, the pristine file as loaded.
///    Only used for the [`Visible`] check.
/// 2. **preview** — `session.view()`, the exact call the GUI canvas makes
///    (`OpenDoc::rasterize_current`). This is what the operator sees.
/// 3. **saved** — `to_incremental_bytes` → `Document::from_bytes` →
///    `page_tree::pages` → render. This is what they get.
///
/// Incremental save (not `to_full_bytes`) because it is pdfcer's **default**
/// save mode and therefore the one an operator actually exercises; it also
/// gives a free structural check, since an incremental output that failed to
/// carry an edit would still reload cleanly and would fail here loudly. A
/// `save_full` variant would be a reasonable second axis, and is deliberately
/// left out for now: it would double the runtime of the gate to re-test the
/// same read path through a different writer, and the writer's own identity
/// gates (`tools/roundtrip`, R32/R46) already cover full rewrites.
fn check(op: &str, session: &EditSession, page_index: usize, visible: Visible) {
    // --- the pristine base, for the "did anything happen?" assertion ---
    let base_doc = session.document();
    let base_pages = page_tree::pages(base_doc).expect("base page tree walks");
    let base_view = base_doc.view();
    let base = base_pages
        .get(page_index)
        .map(|page| raster(&base_view, page, &format!("{op}/base")));

    // --- preview: what the canvas draws right now ---
    let pages = session.pages().expect("session page tree walks");
    let page = pages
        .get(page_index)
        .unwrap_or_else(|| panic!("{op}: session has a page {page_index}"));
    let preview = raster(&session.view(), page, &format!("{op}/preview"));

    // --- saved: what the file contains after the default save ---
    let (bytes, _report) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap_or_else(|e| panic!("{op}: incremental save succeeds: {e}"));
    let reloaded =
        Document::from_bytes(bytes).unwrap_or_else(|e| panic!("{op}: saved output reloads: {e}"));
    let saved_pages = page_tree::pages(&reloaded).expect("saved page tree walks");
    let saved_page = saved_pages.get(page_index).unwrap_or_else(|| {
        panic!(
            "{op}: the saved document has a page {page_index} (it has {})",
            saved_pages.len()
        )
    });
    let saved = raster(&reloaded.view(), saved_page, &format!("{op}/saved"));

    // The R85 assertion itself.
    assert_pixmaps_identical(op, &preview, &saved);

    // And the assertion that keeps the first one honest.
    if let Some(base) = base {
        let changed = base.width() != preview.width()
            || base.height() != preview.height()
            || base.data() != preview.data();
        match visible {
            Visible::Yes => assert!(
                changed,
                "{op}: the edited preview is pixel-identical to the PRISTINE base, so this case \
                 proves nothing — the operation either did not apply or is invisible at scale \
                 {SCALE}. Fix the case (or, if the operation really is visually neutral by \
                 design, say so with Visible::No and the reason)."
            ),
            Visible::No(why) => assert!(
                !changed,
                "{op}: this case declares itself visually neutral by design ({why}), but the \
                 edited preview differs from the pristine base. Either the design changed or \
                 the declaration is wrong — both are worth knowing."
            ),
        }
    }
}

// =====================================================================
// Text authoring
// =====================================================================

/// `add-text` — a new `BT…ET` run appended as a fresh content stream, plus a
/// standard-14 font added to the page `/Resources` (Pass 16.0).
///
/// The added stream is a session-created object, so the preview side reaches
/// it only through the overlay and the saved side only through the appended
/// revision — two entirely different lookups that must produce one picture.
#[test]
fn add_text_preview_equals_saved() {
    let mut s = session("addtext", "plain.pdf");
    s.add_text(&AddTextRequest::new(0, (100.0, 650.0), "Preview equals saved").with_size(18.0))
        .expect("add_text applies");
    check("add-text", &s, 0, Visible::Yes);
}

/// `edit-text` — in-place content-stream surgery (Pass 14.1). Unlike
/// `add-text` this **rewrites an existing base object**, so the preview
/// resolves the overlay's replacement while the saved side resolves the
/// appended update — the classic place for a stale read to hide.
#[test]
fn edit_text_preview_equals_saved() {
    let mut s = session("textedit", "nonembedded.pdf");
    s.edit_text(
        &EditRequest::find_replace(0, "teh", "the"),
        &EditOptions::default(),
    )
    .expect("edit_text applies");
    check("edit-text", &s, 0, Visible::Yes);
}

/// `format-text` — a size change on an existing run (Pass 14.2). Size is
/// chosen over colour because it changes glyph *geometry*, so a stale read
/// cannot accidentally agree.
#[test]
fn format_text_preview_equals_saved() {
    let mut s = session("textedit", "format_color.pdf");
    s.format_text(
        &FormatRequest::new(0, "hello").size(20.0),
        &FormatOptions::default(),
    )
    .expect("format_text applies");
    check("format-text", &s, 0, Visible::Yes);
}

/// `format-text` — the Pass 19.1 direct text-state controls: character
/// spacing (`Tc`), horizontal scaling (`Tz`) and a superscript (`Ts` plus a
/// reduced `Tf` size).
///
/// A separate case from the size one above, and not redundant with it,
/// because these three exercise a **different renderer path**: `Tz`
/// reshapes the glyph outline itself (§9.3.4 — it affects "both the glyph's
/// shape and its horizontal displacement"), `Ts` translates the text
/// rendering matrix (§9.3.7), and `Tc` enters the §9.4.4 advance. If
/// `pdfcer-render` honoured any of the three differently from the way the
/// saved file is re-read, the preview would show a spacing the file does
/// not have — which is precisely the R85 failure, and precisely the class of
/// bug an operator would report as "it looked right until I saved it".
///
/// It is also the gate on the state-RESTORE emission: the fixture's run is
/// followed by other content, so a restore that this crate's interpreter
/// resolved differently from the core's walk would diverge here.
#[test]
fn format_text_spacing_preview_equals_saved() {
    let mut s = session("textedit", "format_color.pdf");
    s.format_text(
        &FormatRequest::new(0, "hello")
            .char_spacing(MetricSpec::Absolute(0.6))
            .h_scale(80.0)
            .script(ScriptPosition::Superscript),
        &FormatOptions::default(),
    )
    .expect("format_text applies the 19.1 spacing controls");
    check("format-text-spacing", &s, 0, Visible::Yes);
}

/// `format_text` with a **free-form baseline rise** (Pass 19.2) — the
/// deliberate exceed over Acrobat's coarse toggle.
///
/// Separate from the spacing case above because the operand is now the
/// operator's own number rather than one of two pdfcer-derived constants: an
/// arbitrary `Ts` exercises the same §9.3.7 translation with a value the
/// emitter did not choose, which is the case where a rounding or unit
/// mismatch between the two sides would show.
#[test]
fn format_text_free_form_rise_preview_equals_saved() {
    let mut s = session("textedit", "format_color.pdf");
    s.format_text(
        &FormatRequest::new(0, "hello").rise(MetricSpec::Absolute(4.5)),
        &FormatOptions::default(),
    )
    .expect("format_text applies a free-form rise");
    check("format-text-rise", &s, 0, Visible::Yes);
}

/// `format-text` with **word spacing** (`Tw`, Pass 19.4) — the last member
/// of the FF-H family, and a genuinely different renderer path from the
/// three cases above.
///
/// `Tc` widens every glyph's advance; `Tw` widens **only** the ones whose
/// code is the single-byte 32 (§9.3.3). That conditional is implemented
/// twice — once in `pdfcer-core`'s authoring advance
/// (`glyph_advance_with`, which zeroes the `Tw` term for any code ≠ 32)
/// and once in `pdfcer-render`'s own text state
/// (`TextState::advance_for`'s `apply_word_spacing`, fed by a
/// `word_spacing_applies: b == 32` decision in the code walker). Two
/// independent implementations of one spec rule is exactly the shape that
/// drifts, and the failure it would produce — the preview and the saved
/// file disagreeing about where the words after a space sit — is the
/// canonical "it looked right until I saved it" bug.
///
/// The fixture's run is `hello world`, so there IS a code 32 to exercise;
/// a space-free run would pass this test vacuously.
#[test]
fn format_text_word_spacing_preview_equals_saved() {
    let mut s = session("textedit", "format_color.pdf");
    s.format_text(
        &FormatRequest::new(0, "hello world").word_spacing(MetricSpec::Absolute(6.0)),
        &FormatOptions::default(),
    )
    .expect("format_text applies word spacing");
    check("format-text-word-spacing", &s, 0, Visible::Yes);
}

/// `format_text` with **synthetic bold** (Pass 19.2) — and the single most
/// likely R85 failure in this slice.
///
/// Faux bold emits a *rendering mode* (`2 Tr`), a *line width* and a
/// *stroking colour*. Every one of those is a graphics-state parameter the
/// rasterizer must reproduce on both sides, and the fixture's text is BLUE,
/// so §9.3.6's stroking-colour rule is under test here too: if the two sides
/// disagreed about which colour the outline takes, the rasters would differ
/// in exactly the pixels that make the letterform.
///
/// The sibling gate in `synthetic_style_render.rs` proves the rasterizer
/// *has* these capabilities at all; this one proves that what the operator
/// sees before saving is what the file contains.
#[test]
fn format_text_synthetic_bold_preview_equals_saved() {
    let mut s = session("textedit", "format_color.pdf");
    s.format_text(
        &FormatRequest::new(0, "hello").synthetic(StyleSynthesis::Bold),
        &FormatOptions::default(),
    )
    .expect("format_text applies synthetic bold");
    check("format-text-synthetic-bold", &s, 0, Visible::Yes);
}

/// `format_text` with **synthetic italic** (Pass 19.2) — the matrix half.
///
/// The shear is emitted as a pair of injected absolute `Tm` operators, which
/// is a *positioning* rewrite rather than a state change, and it is the only
/// thing this slice emits that the R88 restore ladder does not cover. If the
/// rasterizer resolved either injected matrix differently from the way the
/// saved file re-reads it, the run would sit or lean differently on screen
/// than on disk.
#[test]
fn format_text_synthetic_italic_preview_equals_saved() {
    let mut s = session("textedit", "format_color.pdf");
    s.format_text(
        &FormatRequest::new(0, "hello").synthetic(StyleSynthesis::Italic),
        &FormatOptions::default(),
    )
    .expect("format_text applies synthetic italic");
    check("format-text-synthetic-italic", &s, 0, Visible::Yes);
}

/// Both syntheses plus a rise, together — the combination decision 019 §3.6
/// singles out because a shear displaces a RAISED run horizontally by
/// `Trise · tan θ`. Three mechanisms interacting is where a preview/saved
/// divergence is most likely to hide, so it gets its own case rather than
/// being assumed to follow from the three above.
#[test]
fn format_text_synthetic_bold_italic_with_rise_preview_equals_saved() {
    let mut s = session("textedit", "format_color.pdf");
    s.format_text(
        &FormatRequest::new(0, "hello")
            .rise(MetricSpec::Absolute(5.0))
            .synthetic(StyleSynthesis::BoldItalic),
        &FormatOptions::default(),
    )
    .expect("format_text applies bold+italic+rise");
    check(
        "format-text-synthetic-bold-italic-rise",
        &s,
        0,
        Visible::Yes,
    );
}

/// `reflow` — re-wrapping a whole paragraph block to a new width
/// (Pass 15.1/15.2): the largest single content-stream rewrite pdfcer
/// performs, moving every line of the block.
#[test]
fn reflow_preview_equals_saved() {
    let mut s = session("reflow", "reflow.pdf");
    s.reflow_block(0, 0, &ReflowRequest::new().with_wrap_width(400.0))
        .expect("reflow_block applies");
    check("reflow", &s, 0, Visible::Yes);
}

// =====================================================================
// Annotation authoring — the R45 staging path
// =====================================================================

/// `annotate` (markup) — an authored `/Square` whose `/AP` appearance stream
/// lives in the session's **R45 staging buffer**, past the end of the base
/// file.
///
/// This is the case that specifically exercises
/// [`StreamSource::Split`](pdfcer_core::view::StreamSource::Split): on the
/// preview side the appearance bytes are resolved out of `staging` by an
/// offset comparison, and on the saved side out of the reloaded file by a
/// normal span slice. A regression that reverted `DocumentView` to a single
/// contiguous `&[u8]` would draw an empty appearance on the preview side and
/// fail here.
#[test]
fn markup_annotation_preview_equals_saved() {
    let mut s = session("addtext", "plain.pdf");
    s.add_markup(
        0,
        &MarkupSpec::Square {
            rect: Rect {
                llx: 72.0,
                lly: 600.0,
                urx: 400.0,
                ury: 700.0,
            },
            border: Some(Color::Rgb(1.0, 0.0, 0.0)),
            interior: None,
            border_width: 4.0,
            border_effect: None,
        },
    )
    .expect("add_markup applies");
    check("annotate/square", &s, 0, Visible::Yes);
}

/// `annotate` (text markup) — a `/Highlight` over a quad, i.e. an annotation
/// whose appearance is painted *over existing page content* rather than in
/// empty space. Blending against what is already there is a second, distinct
/// way for the two revisions to disagree.
#[test]
fn highlight_annotation_preview_equals_saved() {
    let mut s = session("addtext", "plain.pdf");
    s.add_markup(
        0,
        &MarkupSpec::TextMarkup {
            kind: TextMarkupKind::Highlight,
            quads: vec![pdfcer_core::annot_author::Quad::from_rect(Rect {
                llx: 70.0,
                lly: 712.0,
                urx: 300.0,
                ury: 736.0,
            })],
            color: Color::Rgb(1.0, 1.0, 0.0),
        },
    )
    .expect("add_markup applies");
    check("annotate/highlight", &s, 0, Visible::Yes);
}

/// `annotate` (free text) — a `/FreeText` annotation, whose appearance
/// stream additionally carries its own font resource, so the preview side
/// must resolve a *font dictionary created this session* as well as staged
/// stream bytes.
#[test]
fn free_text_annotation_preview_equals_saved() {
    let mut s = session("addtext", "plain.pdf");
    s.add_text_annotation(
        0,
        &TextAnnotSpec::FreeText {
            rect: Rect {
                llx: 72.0,
                lly: 500.0,
                urx: 400.0,
                ury: 560.0,
            },
            text: "Reviewed".to_owned(),
            font: Std14::Helvetica,
            font_size: 24.0,
            color: TextColor::Gray(0.0),
            quadding: Quadding::Left,
            multiline: false,
            border: None,
            border_width: 0.0,
        },
    )
    .expect("add_text_annotation applies");
    check("annotate/freetext", &s, 0, Visible::Yes);
}

/// `dimension-add` — a linear dimension (Pass 12): an annotation with a
/// baked `/AP`, an OCG membership and a measurement dictionary. The
/// heaviest authoring path, and the one whose invisibility decision 018 §1
/// used as the worked example of the original defect.
#[test]
fn dimension_preview_equals_saved() {
    let mut s = session("dimension", "plain-base.pdf");
    s.add_dimension(
        0,
        DEFAULT_GROUP_ID,
        DimensionKind::Linear {
            a: Point::new(100.0, 200.0),
            b: Point::new(300.0, 200.0),
            constraint: AxisConstraint::Horizontal,
            offset: 0.0,
            text_along: 0.0,
        },
    )
    .expect("add_dimension applies");
    check("dimension-add", &s, 0, Visible::Yes);
}

// =====================================================================
// Vector object editing (Pass 9c)
// =====================================================================

/// `object-move` — translating one decomposed path object, which rewrites
/// the page's first `/Contents` stream in place.
#[test]
fn object_move_preview_equals_saved() {
    let mut s = session("vector", "edit.pdf");
    s.move_object(0, 0, 30.0, -20.0)
        .expect("move_object applies");
    check("object-move", &s, 0, Visible::Yes);
}

/// `object-delete` — removing one path object from the content stream.
///
/// The inverse of the presence oracles: here the preview must show the
/// object *gone*, and the saved file must agree. A stale read shows a ghost
/// the operator believes they deleted, which is the single most dangerous
/// flavour of this defect short of redaction.
#[test]
fn object_delete_preview_equals_saved() {
    let mut s = session("vector", "edit.pdf");
    s.delete_object(0, 0).expect("delete_object applies");
    check("object-delete", &s, 0, Visible::Yes);
}

/// `node-move` — relocating a single path node (Pass 9c-min), the finest
/// grained content edit pdfcer performs.
#[test]
fn node_move_preview_equals_saved() {
    let mut s = session("vector", "edit.pdf");
    s.move_node(0, 0, 1, Point::new(200.0, 100.0))
        .expect("move_node applies");
    check("node-move", &s, 0, Visible::Yes);
}

// =====================================================================
// Forms
// =====================================================================

/// `fill-field` — filling a text field regenerates the widget's `/AP`
/// appearance stream into the R45 staging buffer, so what the operator sees
/// typed into the field is a staged stream on the preview side and a saved
/// object afterwards.
#[test]
fn fill_field_preview_equals_saved() {
    let mut s = session("forms", "demo-form.pdf");
    let outcome = s
        .fill_text_field("FullName", "Ada Lovelace")
        .expect("fill_text_field applies");
    assert!(
        outcome.widgets_updated >= 1,
        "the fixture's FullName field has a widget to update"
    );
    check("fill-field", &s, 0, Visible::Yes);
}

/// `fill-field` on a **multi-widget** field — one value, three appearances,
/// across two pages (decision 020's F0).
///
/// # Why the single-widget case above does not cover this
///
/// `fill_text_field` fans out over `field.widgets`, and a merged (Shape A)
/// field has exactly one — so the shipped oracle case exercises a loop that
/// runs once. Every way that loop can be wrong for N > 1 is invisible to it:
/// generating one stream and attaching it to three widgets, generating three
/// and attaching only the first, or attaching each to the wrong widget. All
/// three produce a correct `widgets_updated` count and a document that parses.
///
/// This matters now rather than in the abstract because the merge primitive
/// GENERATES this shape — a second `add-field` under an existing name
/// promotes a merged field into a `/Kids` parent with two widgets. Authoring
/// starts producing exactly the input the fill path has never been rendered
/// against.
///
/// **Page 2 is the page checked**, deliberately. Two of the three widgets are
/// on page 1 and only the third is on page 2, so a fill that painted every
/// widget onto the first page — or that skipped the last widget — leaves page
/// 2 blank in the saved file while the preview shows it filled. Page 1 would
/// hide both errors behind the widgets that *are* correct there.
#[test]
fn fill_field_multi_widget_preview_equals_saved() {
    let mut s = session("forms", "multi-widget-form.pdf");
    let outcome = s
        .fill_text_field("Reference", "R-2000")
        .expect("fill_text_field applies");
    assert_eq!(
        outcome.widgets_updated, 3,
        "the fixture's Reference field has three widgets across two pages",
    );
    check("fill-field/multi-widget", &s, 1, Visible::Yes);
}

/// **The merge itself** — a second `add-text-field` under an existing name,
/// which promotes a merged (Shape A) field into a `/Kids` parent with two
/// widgets, then filled so both appearances paint (decision 020's F1).
///
/// # Why this is a separate case from the two above
///
/// [`fill_field_multi_widget_preview_equals_saved`] renders a multi-widget
/// field that a **byte-authored fixture** already contained. It proves the
/// fill path handles the shape; it says nothing about whether pdfcer PRODUCES
/// that shape correctly, because the fixture was written by hand. This case
/// starts from a one-widget field and makes pdfcer build the second widget —
/// so the promotion, not just its result, is what gets rendered.
///
/// # The specific defect this is an oracle for
///
/// Promotion is two whole-page `/Annots` writes in one command: the original
/// entry is RETARGETED from the field dict (now a non-terminal parent, which
/// under Table 220 has no appearance of its own) to the new widget dict that
/// took over its annotation role, and the second widget is APPENDED. F1 found
/// these computed independently from the pre-command state, so the append
/// silently discarded the retarget and `/Annots` named a dictionary that had
/// stopped being a widget. It is the same double-write failure the oracle
/// already caught once in `flatten_fields`.
///
/// That class of defect is exactly what R85 sees and a parse-level assertion
/// does not: the document still parses, `list-fields` still reports one field
/// with two widgets, and only the PICTURE disagrees — and it disagrees
/// between preview and saved specifically, because the two sides resolve the
/// page through different lookups.
///
/// **Page 0 is checked and both widgets are on it**, deliberately: a lost
/// retarget silently drops the ORIGINAL widget while the appended one still
/// paints, so a page holding only the new widget would look entirely correct.
#[test]
fn field_merge_preview_equals_saved() {
    let mut s = session("forms", "demo-form.pdf");
    // `FullName` exists in the fixture as a single-widget text field with no
    // value — Shape A, the input promotion is defined over.
    s.add_text_field(
        &NewTextField::new(0, "FullName", Rect::from_corners(72.0, 600.0, 340.0, 624.0))
            .declining_tooltip(),
    )
    .expect("a same-name, same-type add merges rather than refusing");
    let outcome = s
        .fill_text_field("FullName", "Ada Lovelace")
        .expect("fill_text_field applies to the promoted field");
    assert_eq!(
        outcome.widgets_updated, 2,
        "the merge must leave ONE field with TWO widgets — a count of 1 means \
         the promotion did not happen and this case is silently testing a \
         plain fill instead",
    );

    check("field-merge", &s, 0, Visible::Yes);
}

/// **Radio selection** — a three-member group authored member by member,
/// then one member chosen (decision 020's F2).
///
/// # Why this needs its own case, given the merge case above
///
/// [`field_merge_preview_equals_saved`] renders a merge whose widgets all
/// paint the SAME thing. A radio group is the first authored shape whose
/// widgets must paint DIFFERENTLY from one another at the same instant — one
/// dot, two empty rings — and that difference is carried entirely by per-widget
/// `/AS` values (§12.5.5) written in a single command.
///
/// That is a multi-write shape, and multi-writes to related objects computed
/// from one pre-command snapshot are how this project's two worst rendering
/// defects happened: `flatten_fields` overwriting its own `/Contents`,
/// `/Resources` and `/Annots` writes so only the last survived, and F1's
/// promotion discarding its `/Annots` retarget. Both produced documents that
/// parsed, reported correct counts, and drew the wrong picture. `list-fields`
/// saying `widgets=3 value=Green` cannot distinguish a group where one dot is
/// painted from one where three are, or none.
///
/// # What a failure here would look like
///
/// If the `/AS` writes clobbered each other, the saved side would show a
/// different set of filled dots than the preview — every member off, or the
/// wrong member on. The [`Visible::Yes`] half separately guarantees the
/// authored group is not invisible: three empty rings would still differ from
/// the pristine page, so this case cannot pass by drawing nothing.
#[test]
fn radio_selection_preview_equals_saved() {
    let mut s = session("dimension", "plain-base.pdf");
    // Placed well inside a 400×400 page: a widget whose /Rect falls outside
    // the page still reports as painted, so an off-page group would make this
    // case assert on two identical blank rasters.
    for (i, value) in ["Red", "Green", "Blue"].iter().enumerate() {
        let top = 324.0 - (i as f64) * 40.0;
        s.add_radio_button(
            &NewRadioButton::new(
                0,
                "Pick",
                Rect::from_corners(40.0, top - 24.0, 64.0, top),
                *value,
            )
            .declining_tooltip(),
        )
        .expect("each call adds a member to the one group");
    }
    s.set_button_state("Pick", "Green")
        .expect("the shipped fill path selects a member");

    check("radio-selection", &s, 0, Visible::Yes);
}

/// `flatten` — burning a filled widget's appearance into page content and
/// removing the field (Pass 7).
///
/// **The one deliberately [`Visible::No`] case**, and it takes a two-stage
/// setup to be that honestly. Flatten needs a filled field to have anything
/// to burn, but filling is itself visible, so a single session that fills
/// *and* flattens would compare a flattened page against an **empty** form
/// and merely re-prove that filling shows up (which
/// [`fill_field_preview_equals_saved`] already covers). The first attempt at
/// this test did exactly that and failed its own `Visible::No` declaration —
/// correctly.
///
/// So the fill is committed to a real file first, and the flatten runs in a
/// **fresh session over the already-filled document** — which is also the
/// realistic operator story: open a completed form, flatten it. Now the
/// pristine base *is* the filled form, and the assertion says what it means:
///
/// > Flattening is pixel-neutral. The appearance the widget was painting
/// > becomes page content painting the identical marks.
///
/// That neutrality is the property that makes flattening safe to offer at
/// all — a flatten that changed the page's look would be silently rewriting
/// the operator's document — so asserting it is worth more here than
/// asserting a change.
///
/// The R85 equality assertion still carries its full weight independently: it
/// proves the *burned-in content* renders the same from the session overlay
/// as from the saved file, which is a different claim from "flatten looked
/// like a no-op".
#[test]
fn flatten_preview_equals_saved() {
    // Stage 1 — produce a genuinely filled document.
    let mut filling = session("forms", "demo-form.pdf");
    filling
        .fill_text_field("FullName", "Ada Lovelace")
        .expect("fill_text_field applies");
    let (filled_bytes, _report) = filling
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("the filled form saves");

    // Stage 2 — a fresh session whose BASE is the filled form; flatten only.
    let filled = Document::from_bytes(filled_bytes).expect("the filled form reloads");
    let mut s = EditSession::new(filled);
    let out = s.flatten_fields(None).expect("flatten_fields applies");
    assert!(
        out.widgets_burned >= 1,
        "the filled field's widget is burned into page content"
    );
    check(
        "flatten",
        &s,
        0,
        Visible::No(
            "flattening burns the widget's own /AP into page content unchanged; the marks on \
             the page are identical before and after, which is exactly what makes it safe",
        ),
    );
}

// =====================================================================
// Redaction (marking half only — see the module docs for the apply gap)
// =====================================================================

/// Redaction **marking** — `/Redact` annotations authored by a text search,
/// each painting a red-outline preview.
///
/// This is the covered half of R85's `redact-apply` entry; the applying half
/// has no session preview at all (module docs, *"`redact-apply` is NOT
/// covered"*). Marking is worth its own case regardless: the outline is the
/// operator's only evidence that a region is marked, and the pending-marks
/// disclosure in the GUI status bar is counted from the same session state
/// this renders.
#[test]
fn redaction_mark_preview_equals_saved() {
    let mut s = session("redact", "demo-secret.pdf");
    let ids = s
        .mark_redactions_by_search("SECRET", false)
        .expect("mark_redactions_by_search applies");
    assert!(
        !ids.is_empty(),
        "the fixture contains the word the search marks"
    );
    check("redact-mark", &s, 0, Visible::Yes);
}

// =====================================================================
// Image placement — the transparency round trip, end to end
// =====================================================================
//
// These two close the loop the `transparency_not_previewed` disclosure
// was opened for. `EditSession::add_image` wrote correct transparency
// from the day it shipped; `pdfcer-render` deferred it, so a transparent
// PNG looked opaque in pdfcer and right in Acrobat — the file was correct
// and the preview lied about it. The dedicated pixel proofs live in
// `image_transparency.rs`; what these add is the OPERATOR'S OWN PATH:
// import a real PNG, place it, and require the preview and the saved
// file to agree.
//
// Worth noting what R85 does and does not catch here. Before this Pass
// both sides rendered the image opaque, so both were wrong in the SAME
// way and R85 passed — a reminder that "preview equals saved" is a
// consistency oracle, not a correctness one. The correctness assertion
// is `Visible::Yes` plus `image_transparency.rs`'s pixel values; R85's
// job is to guarantee that whatever transparency the renderer does apply
// survives the staging → incremental-save → reload round trip, where the
// `/SMask` reaches the two sides through entirely different lookups (the
// session overlay's R45 staging half versus an appended revision).

/// `add-image` with an alpha channel — a `/DeviceRGB` base plus a
/// separate 8-bit `/SMask` (§8.9.5 Table 89), the shape a colour-type-6
/// PNG becomes.
///
/// The mask is a session-created stream, so on the preview side it is
/// reached through the overlay and on the saved side through the appended
/// revision. A `StreamSource` bug that served the base buffer for a
/// staged span would decode the mask as garbage on exactly one of the two
/// and show up here as a pixel difference.
#[test]
fn add_image_with_smask_preview_equals_saved() {
    let bytes = std::fs::read(fixture("images", "rgba8.png")).expect("fixture reads");
    let img = image_import::import(&bytes).expect("rgba8.png imports");
    let mut s = session("addtext", "plain.pdf");
    s.add_image(&NewImage::new(
        0,
        Rect {
            llx: 100.0,
            lly: 500.0,
            urx: 280.0,
            ury: 620.0,
        },
        &img,
    ))
    .expect("add_image applies");
    check("add-image (/SMask)", &s, 0, Visible::Yes);
}

/// `add-image` with a single transparent colour — a colour-key `/Mask`
/// array (§8.9.6.4), the shape a `tRNS` chunk on a truecolour PNG
/// becomes.
///
/// A different mechanism entirely from the case above: no second stream,
/// no alpha samples, just a range test against the base image's own
/// pre-`/Decode` values. It is included because the two share no code
/// below `image::decode` and a regression in one would not touch the
/// other.
#[test]
fn add_image_with_colour_key_mask_preview_equals_saved() {
    let bytes = std::fs::read(fixture("images", "rgb-trns.png")).expect("fixture reads");
    let img = image_import::import(&bytes).expect("rgb-trns.png imports");
    let mut s = session("addtext", "plain.pdf");
    s.add_image(&NewImage::new(
        0,
        Rect {
            llx: 100.0,
            lly: 500.0,
            urx: 280.0,
            ury: 620.0,
        },
        &img,
    ))
    .expect("add_image applies");
    check("add-image (colour-key /Mask)", &s, 0, Visible::Yes);
}

// =====================================================================
// The oracle's own guard
// =====================================================================

/// The oracle must be able to FAIL.
///
/// A comparison harness that returns "identical" for everything is worse
/// than none, because it launders untested code as tested — and this one is
/// asserting `==` on two buffers produced by the same function, which is
/// precisely the shape that can degenerate into a tautology (compare a
/// pixmap with itself, compare two empty buffers, compare through a `slice`
/// that silently yields nothing). This test pins the negative: two rasters
/// of genuinely different content are reported as differing.
///
/// It is the same reasoning as the `StreamSource` non-straddling test —
/// prove the guard fires, not just that it is present.
#[test]
fn the_oracle_reports_a_genuine_difference() {
    let s = session("addtext", "plain.pdf");
    let pages = s.pages().expect("pages walk");
    let clean = raster(&s.view(), &pages[0], "guard/clean");

    let mut edited_session = session("addtext", "plain.pdf");
    edited_session
        .add_text(&AddTextRequest::new(0, (100.0, 650.0), "different").with_size(24.0))
        .expect("add_text applies");
    let edited_pages = edited_session.pages().expect("pages walk");
    let edited = raster(&edited_session.view(), &edited_pages[0], "guard/edited");

    assert_eq!(
        (clean.width(), clean.height()),
        (edited.width(), edited.height()),
        "same fixture, same scale — the geometry must match so the comparison is about content"
    );
    assert_ne!(
        clean.data(),
        edited.data(),
        "the oracle's comparison must be able to see a real difference; if this fails, every \
         other test in this file is vacuous"
    );

    // And the failure path itself: comparing those two through the oracle's
    // own assertion must panic rather than pass.
    let result = std::panic::catch_unwind(|| {
        assert_pixmaps_identical("guard", &clean, &edited);
    });
    assert!(
        result.is_err(),
        "assert_pixmaps_identical must PANIC on differing pixmaps — if it does not, R85 is not \
         being enforced by any test in this file"
    );
}
