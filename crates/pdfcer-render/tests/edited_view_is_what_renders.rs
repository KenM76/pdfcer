//! # Pass 17.0 regression gate — the canvas renders the EDITED document
//!
//! This file is the test whose absence let one defect survive fourteen
//! editing Passes. `docs/decisions/018-edited-state-is-what-the-canvas-
//! renders.md` §9 states it as a proposed standing rule:
//!
//! > Every Pass 3.1–16.2 proved *saved* output correct; none proved
//! > *displayed* output correct.
//!
//! Every editing feature wrote into [`EditSession`]'s overlay correctly, and
//! every editing Pass proved that by saving, reloading and inspecting the
//! bytes. Not one of them asked the question an operator asks — *"can I see
//! it?"* — because the read path went through
//! [`EditSession::document()`], the **base revision**, which by definition
//! never carries an unsaved edit.
//!
//! ## What is asserted, and why each assertion is the right one
//!
//! The tests below are deliberately **differential**: they compare the same
//! read run over two views of the *same live session*, so a passing result
//! cannot be explained by anything except the view parameter. Three layers,
//! because Pass 17.0 fixes three consumers through one mechanism:
//!
//! | Layer | Reads | Asserted |
//! |---|---|---|
//! | Object model | `decompose_page` | the added object IS present over `session.view()` and ABSENT over `session.document().view()` |
//! | Raster | `render_page_view` | the two views produce DIFFERENT pixels |
//! | Annotations | `render_page_view`'s `Diagnostics` | an annotation authored this session is surveyed over the session view and invisible over the base |
//!
//! The annotation case is the one that specifically exercises
//! [`StreamSource::Split`](pdfcer_core::view::StreamSource::Split): a
//! dimension's baked `/AP` appearance stream lives in the session's **R45
//! staging buffer**, past the end of the base file, so it is only readable
//! through a split source. A regression that reverted `DocumentView` to a
//! single `&[u8]` would pass the object-model test (the content stream is a
//! base object) and fail this one.
//!
//! ## What is deliberately NOT asserted here
//!
//! Pixel-exact preview-equals-saved (decision 018 §9's full R85 oracle,
//! comparing the session raster against the saved-then-reloaded raster
//! operation by operation) is Pass 17.2's harness. This file proves the
//! *direction* — that editing changes what is displayed — which is the
//! regression that would have caught the original defect on day one.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::path::{Path, PathBuf};

use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::text_edit::AddTextRequest;
use pdfcer_core::vector::{AxisConstraint, Matrix, Point, VectorObject, decompose_page};
use pdfcer_render::render_page_view;

/// A committed synthetic fixture with real page content and its own
/// `/Resources` (provenance: `fixtures/synthetic/addtext/PROVENANCE.md`).
/// Used rather than a hand-built blank page so the "before" side of every
/// differential assertion is a non-trivial page, not an empty one.
fn addtext_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/addtext")
        .join("plain.pdf")
}

/// A minimal one-page PDF with no content stream: catalog(1) → pages(2) →
/// page(3).
///
/// Deliberately content-free for the annotation test, so that ANY pixel
/// difference between the two rasters must have come from the annotation
/// appearance — there is no page content that could account for it.
fn blank_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// A horizontal linear dimension across the middle of the 400×400 blank
/// page — geometry chosen to fall well inside the MediaBox so its baked
/// appearance definitely lands on painted pixels.
fn linear_dimension() -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
    }
}

// ---------------------------------------------------------------------------
// Layer 1 — the object model (hit-test, selection, snapping)
// ---------------------------------------------------------------------------

/// The regression that would have caught the Pass 17.0 defect.
///
/// `add_text` splices the page's content stream and stages the rewritten
/// bytes (R45). Decomposing the SESSION view must therefore see one more
/// object than decomposing the BASE view of the very same session — and the
/// extra one must be the added text.
///
/// Why this is the load-bearing shape: `ObjectModelProvider` (the GUI's
/// hit-test/marquee/snap provider) is built from exactly this call. Before
/// Pass 17.0 it was built from `session.document()`, so the operator could
/// not click, marquee-select or snap to anything they had authored.
#[test]
fn added_text_is_present_in_the_session_view_and_absent_in_the_base() {
    let doc = Document::load(&addtext_fixture()).expect("fixture loads");
    let mut session = EditSession::new(doc);

    // The "before" snapshot. `Page` is captured per revision (decision 018
    // §10 hazard 2), and this one belongs to the base — see
    // `a_post_edit_page_snapshot_does_not_resolve_against_the_base` below for
    // what happens if the two are crossed.
    let pages_before = session.pages().expect("page tree walks");
    let base_page = pages_before[0].clone();
    let base_objects = decompose_page(&session.document().view(), &base_page, Matrix::IDENTITY)
        .expect("base page decomposes");
    let base_text = count_text(&base_objects.objects);

    session
        .add_text(&AddTextRequest::new(0, (100.0, 650.0), "Hello world").with_size(14.0))
        .expect("add_text succeeds");

    // Re-read the page exactly as `OpenDoc::refresh_pages` does after every
    // edit: the session's `/Contents` now names a stream this session
    // created, and the stale snapshot does not know about it.
    let pages_after = session.pages().expect("page tree walks after the edit");
    let session_objects = decompose_page(&session.view(), &pages_after[0], Matrix::IDENTITY)
        .expect("session page decomposes");

    // The base, re-read through its own (unchanged) page snapshot, is still
    // exactly what it was: an unsaved edit must never reach it.
    let base_again = decompose_page(&session.document().view(), &base_page, Matrix::IDENTITY)
        .expect("base still decomposes");
    assert_eq!(
        count_text(&base_again.objects),
        base_text,
        "the BASE revision must be untouched by an unsaved edit - if this \
         fails, the edit leaked into the base and the round-trip invariant \
         (ARCHITECTURE.md §5) is broken, which is far worse than the bug \
         this test was written for"
    );
    assert_eq!(
        count_text(&session_objects.objects),
        base_text + 1,
        "the SESSION view must show the text object add_text authored; \
         seeing base_text here is decision 018's defect - the read path is \
         going through session.document() again"
    );
}

/// Hazard 2, made executable: a `Page` snapshot taken AFTER an edit does not
/// resolve against the base revision at all.
///
/// Decision 018 §10 names the danger as *"a correct view paired with a stale
/// `Page`"*. This test pins the reassuring half of that: in the direction
/// that actually occurs — a post-edit page paired with a pre-edit document —
/// `add_text`'s newly created content object is simply **not in the base**,
/// so the read fails cleanly with `NotAStream` rather than silently
/// rendering something plausible.
///
/// It is asserted rather than merely observed because it is the property
/// that makes `refresh_pages` the single funnel: if a future edit reused an
/// existing content object id instead of creating one, this test would start
/// succeeding and the failure mode would go quiet.
#[test]
fn a_post_edit_page_snapshot_does_not_resolve_against_the_base() {
    let doc = Document::load(&addtext_fixture()).expect("fixture loads");
    let mut session = EditSession::new(doc);
    session
        .add_text(&AddTextRequest::new(0, (100.0, 650.0), "Hello world").with_size(14.0))
        .expect("add_text succeeds");

    let pages_after = session.pages().expect("page tree walks after the edit");
    let crossed = decompose_page(
        &session.document().view(),
        &pages_after[0],
        Matrix::IDENTITY,
    );
    assert!(
        crossed.is_err(),
        "a post-edit page snapshot names a content object the base does not \
         have; reading it must fail cleanly, not fabricate a page"
    );
}

/// Count text objects in a decomposition (the unit `add_text` adds).
fn count_text(objects: &[VectorObject]) -> usize {
    objects
        .iter()
        .filter(|o| matches!(o, VectorObject::Text(_)))
        .count()
}

// ---------------------------------------------------------------------------
// Layer 2 — the raster (what the canvas actually shows)
// ---------------------------------------------------------------------------

/// The same edit, one layer down: the pixels must differ.
///
/// `decompose_page` agreeing is necessary but not sufficient — the raster is
/// what the operator sees, and it reaches the content stream through its own
/// `ContentStream::from_page` call. This asserts both halves of the fix are
/// wired: the graph (so the content object resolves to the session's
/// rewritten stream) AND the byte source (so that stream's staged span
/// resolves at all).
#[test]
fn the_session_raster_differs_from_the_base_raster_after_add_text() {
    let doc = Document::load(&addtext_fixture()).expect("fixture loads");
    let mut session = EditSession::new(doc);

    // Each view is rasterized with ITS OWN page snapshot, which is what the
    // GUI does (`refresh_pages` re-reads the list on every edit). Crossing
    // them is a separate, separately-tested failure — see
    // `a_post_edit_page_snapshot_does_not_resolve_against_the_base`.
    let base_page = session.pages().expect("page tree walks")[0].clone();
    let base =
        render_page_view(&session.document().view(), &base_page, 1.0).expect("base rasterizes");

    session
        .add_text(&AddTextRequest::new(0, (100.0, 650.0), "Hello world").with_size(14.0))
        .expect("add_text succeeds");

    let pages = session.pages().expect("page tree walks after the edit");
    let edited =
        render_page_view(&session.view(), &pages[0], 1.0).expect("session view rasterizes");

    assert_eq!(
        edited.pixmap.width(),
        base.pixmap.width(),
        "same page, same scale - the geometry must not have changed"
    );
    assert_ne!(
        edited.pixmap.data(),
        base.pixmap.data(),
        "the edited view must NOT rasterize to the base's pixels; identical \
         pixels here is exactly the Pass 17.0 defect (decision 018 §1)"
    );
}

// ---------------------------------------------------------------------------
// Layer 3 — staged appearance streams (the StreamSource::Split path)
// ---------------------------------------------------------------------------

/// An annotation authored this session paints, and is COUNTED, only through
/// the session view.
///
/// This is the test that specifically pins
/// [`StreamSource::Split`](pdfcer_core::view::StreamSource::Split). A
/// dimension is an annotation with a **baked `/AP` appearance stream** whose
/// payload `EditSession::stage_bytes` placed in the staging buffer at
/// `base.len() + local`. Reading it needs both:
///
/// 1. the session **graph**, so the page's `/Annots` array (patched this
///    session) is seen at all — this alone is why authored dimensions and
///    markup annotations never appeared; and
/// 2. the **split byte source**, so the appearance's `data_span` — which
///    points past the end of the base file — resolves instead of failing
///    bounds and being tolerated as an undecodable stream.
///
/// The page is deliberately blank, so a pixel difference can only be the
/// annotation.
#[test]
fn a_dimension_authored_this_session_is_surveyed_and_painted_only_in_the_view() {
    let doc = Document::from_bytes(blank_pdf()).expect("blank pdf parses");
    let mut session = EditSession::new(doc);

    session
        .add_dimension(0, DEFAULT_GROUP_ID, linear_dimension())
        .expect("add_dimension succeeds");

    let pages = session.pages().expect("page tree walks");
    let page = &pages[0];

    let edited = render_page_view(&session.view(), page, 1.0).expect("session view rasterizes");
    let base =
        render_page_view(&session.document().view(), page, 1.0).expect("base view rasterizes");

    assert_eq!(
        base.diagnostics.annotations_total, 0,
        "the base revision carries no /Annots - an unsaved annotation must \
         not be visible there"
    );
    assert!(
        edited.diagnostics.annotations_total >= 1,
        "the session view must survey the dimension annotation authored this \
         session (decision 018 §1: survey_page_annotations read the base)"
    );
    assert_eq!(
        edited.diagnostics.annotations_hidden, 0,
        "nothing about this dimension is flag- or OC-hidden"
    );

    assert_ne!(
        edited.pixmap.data(),
        base.pixmap.data(),
        "the dimension's baked /AP must PAINT. Identical pixels on an \
         otherwise blank page means the appearance stream's staged span did \
         not resolve - i.e. the view is not carrying a StreamSource::Split, \
         or something reverted DocumentView to a single buffer"
    );

    // Belt and braces on the mechanism itself: the session really is carrying
    // staged bytes, so the assertion above is testing the split path rather
    // than passing vacuously on a session that happens to have staged nothing.
    assert!(
        session.authored_source().len() > session.document().bytes().len(),
        "add_dimension must have staged an appearance stream past the base"
    );
}
