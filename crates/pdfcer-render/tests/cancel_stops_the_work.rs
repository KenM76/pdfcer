//! Cancellation must STOP a render, not merely discard its result.
//!
//! At ~58 s for a heavy page, a render whose output is thrown away is
//! nearly as bad as one that is painted: it still occupies a core and
//! still delays whatever the operator asked for next. So the assertion
//! here is about ELAPSED TIME, not about the returned value.

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::settings::CmykIntent;
use pdfcer_core::view::DocumentView;
use pdfcer_render::cancel::RenderCancel;
use pdfcer_render::font::FontEnvironment;
use pdfcer_render::gstate::GraphicsState;
use pdfcer_render::tiny_skia::{self, Pixmap};
use pdfcer_render::{RenderError, RenderOptions, interpret, render_page_with_view};

fn fixture() -> Document {
    Document::from_bytes(
        std::fs::read("../../fixtures/synthetic/vector/edit.pdf").expect("fixture readable"),
    )
    .expect("fixture parses")
}

/// A token cancelled BEFORE the render starts must produce `Cancelled`
/// and paint essentially nothing.
#[test]
fn a_pre_cancelled_render_returns_cancelled() {
    let doc = fixture();
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let pages = page_tree::pages(&doc).expect("page tree walks");
    let token = RenderCancel::new();
    token.cancel();

    let err = render_page_with_view(
        &view,
        &pages[0],
        1.0,
        &RenderOptions::default().with_cancel(token),
    )
    .expect_err("a cancelled render must not return a page");

    assert!(
        matches!(err, RenderError::Cancelled),
        "expected Cancelled, got {err:?}"
    );
}

/// The direction that keeps the test above meaningful (R162): the SAME
/// document, the SAME options minus the cancellation, must succeed.
///
/// Without this, `a_pre_cancelled_render_returns_cancelled` would pass
/// identically against a build where this fixture simply failed to
/// render for an unrelated reason.
#[test]
fn the_same_render_succeeds_without_a_cancelled_token() {
    let doc = fixture();
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let pages = page_tree::pages(&doc).expect("page tree walks");

    render_page_with_view(&view, &pages[0], 1.0, &RenderOptions::default())
        .expect("the identical render must succeed when nothing cancelled it");
}

/// An un-cancelled token must not cancel anything — the flag has to be
/// read, not merely present.
#[test]
fn an_uncancelled_token_does_not_stop_the_render() {
    let doc = fixture();
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let pages = page_tree::pages(&doc).expect("page tree walks");

    render_page_with_view(
        &view,
        &pages[0],
        1.0,
        &RenderOptions::default().with_cancel(RenderCancel::new()),
    )
    .expect("an un-cancelled token must leave the render alone");
}

/// **Cancellation must STOP the interpreter, not merely change the
/// return value** — and this is the test that can tell the difference.
///
/// # Why the error-code tests above are not enough
///
/// `render_page_with_view` checks the flag *after* the render and turns
/// it into `Cancelled`. So `a_pre_cancelled_render_returns_cancelled`
/// passes **identically** whether or not the interpreter's poll exists.
/// Measured 2026-08-07 on a 129,515-path CAD page with the `break`
/// disabled: still `Some(Cancelled)`, but **10,227 ms instead of
/// 322 ms**. The observable outcome was the same; the machine did the
/// entire render anyway.
///
/// # Why pixels rather than elapsed time
///
/// A timing assertion needs a page slow enough for the difference to
/// clear the noise, and the only such page here is an uncommitted
/// benchmark file — so the test would silently not run on a fresh
/// clone, which is the failure mode this project files under R162.
/// Painting is deterministic: if the interpreter stopped at the first
/// operator, the paper is still blank. That holds on any fixture, on
/// any machine, at any speed.
#[test]
fn a_cancelled_render_paints_nothing() {
    let doc = fixture();
    let view = DocumentView::new(&doc, doc.bytes(), doc.version());
    let pages = page_tree::pages(&doc).expect("page tree walks");
    let page = &pages[0];

    let content = ContentStream::from_page(&view, page).expect("content decodes");
    let (w, h, base_ctm) = pdfcer_render::page_device_geometry(page, 1.0);

    let token = RenderCancel::new();
    token.cancel();
    let mut cancelled_pixmap = Pixmap::new(w, h).expect("pixmap allocates");
    cancelled_pixmap.fill(tiny_skia::Color::WHITE);
    interpret::run(
        &view,
        &content,
        &page.resources,
        &FontEnvironment::bundled(),
        GraphicsState::default_with_ctm(base_ctm),
        &mut cancelled_pixmap,
        Some(&token),
        RenderOptions::default()
            .with_cmyk_intent(CmykIntent::Calibrated)
            .policy(),
    );

    let mut painted_pixmap = Pixmap::new(w, h).expect("pixmap allocates");
    painted_pixmap.fill(tiny_skia::Color::WHITE);
    interpret::run(
        &view,
        &content,
        &page.resources,
        &FontEnvironment::bundled(),
        GraphicsState::default_with_ctm(base_ctm),
        &mut painted_pixmap,
        None,
        RenderOptions::default()
            .with_cmyk_intent(CmykIntent::Calibrated)
            .policy(),
    );

    let blank = Pixmap::new(w, h).map(|mut p| {
        p.fill(tiny_skia::Color::WHITE);
        p
    });
    let blank = blank.expect("pixmap allocates");

    // The direction that gives the assertion below its teeth: this
    // fixture must actually draw something, or "cancelled paints
    // nothing" would be true of a blank page and prove nothing.
    assert_ne!(
        painted_pixmap.data(),
        blank.data(),
        "the fixture must paint something when NOT cancelled, or this test cannot distinguish a working cancellation from an empty page"
    );

    assert_eq!(
        cancelled_pixmap.data(),
        blank.data(),
        "a pre-cancelled render painted pixels — the interpreter ran operators after the flag was set, so cancellation is only changing the return value while the machine does the whole render. Measured cost of exactly this bug: 10,227 ms against 322 ms on a real CAD page."
    );
}
