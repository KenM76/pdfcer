//! A region render at a magnification the rasteriser cannot serve must
//! **refuse**, not panic (`Pass 296.0`).
//!
//! # What this is guarding, and why an ordinary test could not find it
//!
//! `pdfcer-render` renders on whatever thread the caller gives it, and a
//! consuming shell gives it a worker. A panic there does **not** kill the
//! process: the window stays up, the event loop keeps running, and every
//! trace line a liveness check greps for has already been written by the time
//! the thread dies. The failure is invisible to any check that asks only
//! whether the program exited — which is every check this crate had. It took
//! a thread-panic guard in a downstream harness to see it at all.
//!
//! The reported panic, from `tiny-skia`'s pipeline:
//!
//! ```text
//! range start index 442613758592 out of range for slice of length 1088737
//! ```
//!
//! `1_088_737` is the requested pixmap — window-sized, exactly as designed.
//! `MAX_PIXMAP_EDGE` could never have caught it, because the allocation was
//! never the problem.
//!
//! # ★ Why this test asserts a REFUSAL and not a ceiling
//!
//! `examples/region_panic_ceiling.rs` bisected the first failing scale across
//! six page geometries and got three values that order with nothing — the
//! largest sheet is the most fragile. There is no number to assert. There is
//! only the contract: whatever the rasteriser does, the caller gets a
//! `Result`.
//!
//! # ★★ How this test fails if the guard is removed
//!
//! It panics, and the test harness reports the panic. That is deliberate and
//! is the reason the assertion is written against a scale *known* to cross the
//! boundary rather than against an error string: there is no way to satisfy
//! this test except by actually catching the panic.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_render::{RenderError, RenderOptions, render_page_region};

/// An E-size sheet painting across its whole area — the geometry the probe
/// measured as the FIRST to give out (at ≈ 284,964×), so a test at 1,000,000×
/// is comfortably past the boundary on the most fragile page tested.
fn e_size_sheet() -> Vec<u8> {
    let (w, h) = (3370.0_f64, 2384.0_f64);
    let content = format!("0.8 0.8 0.8 rg 0 0 {w} {h} re f 0 0 0 rg 300 500 1 200 re f");
    let mut pdf = String::from("%PDF-1.7\n");
    let mut offsets = Vec::new();
    let objects: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w} {h}] \
             /Contents 4 0 R /Resources << >> >>"
        ),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
    ];
    for (i, o) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.push_str(&format!("{} 0 obj\n{o}\nendobj\n", i + 1));
    }
    let xref = pdf.len();
    pdf.push_str(&format!(
        "xref\n0 {}\n0000000000 65535 f \n",
        objects.len() + 1
    ));
    for off in &offsets {
        pdf.push_str(&format!("{off:010} 00000 n \n"));
    }
    pdf.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    ));
    pdf.into_bytes()
}

/// What a consumer's generic `Err(e) => show(e.to_string())` arm would put in
/// front of an operator.
fn outcome_message(e: &RenderError) -> String {
    e.to_string()
}

/// The viewport-sized region a shell asks for at magnification `scale`:
/// ~1100 × 990 device pixels however deep the zoom goes.
fn viewport_region(scale: f32) -> Rect {
    let span = 1100.0 / f64::from(scale);
    Rect::from_corners(300.0, 500.0, 300.0 + span, 500.0 + span * 0.9)
}

#[test]
fn a_region_render_past_the_rasterizer_limit_returns_an_error() {
    let doc = Document::from_bytes(e_size_sheet()).expect("synthetic sheet loads");
    let pages = page_tree::pages(&doc).expect("pages");

    // If the guard is gone, this line panics and the test fails there.
    let outcome = render_page_region(
        &doc.view(),
        &pages[0],
        1_000_000.0,
        viewport_region(1_000_000.0),
        &RenderOptions::default(),
    );

    match outcome {
        Err(RenderError::RasterizerLimit {
            scale,
            ref panic_message,
        }) => {
            assert!(
                (scale - 1_000_000.0).abs() < f32::EPSILON,
                "the refusal must name the scale it was asked for, got {scale}"
            );

            // ★★ The panic text must be REACHABLE and must NOT be in the
            // message (`Pass 296.5`). A consuming shell routes an error's
            // `Display` onto the page on purpose, because a structured
            // diagnostic beats "an error occurred" -- so a third party's panic
            // text in the message is a third party's panic text painted across
            // a site plan. The safe rendering has to be the DEFAULT one, not
            // the one reserved for a consumer who read the doc comment.
            assert!(
                !panic_message.is_empty(),
                "the diagnosis must still be reachable for a log line"
            );
            let shown = outcome_message(&RenderError::RasterizerLimit {
                scale,
                panic_message: panic_message.clone(),
            });
            assert!(
                !shown.contains("range start index") && !shown.contains(panic_message.as_str()),
                "the operator-facing message must not carry the rasterizer's panic text: {shown}"
            );
            assert!(
                shown.contains("1000000"),
                "it must still name the scale, which is the actionable half: {shown}"
            );
        }
        // Not a failure to chase: a refusal is a refusal, and the geometry
        // could reach `BadRasterSize` first on some future arithmetic. What
        // must never happen is the third arm.
        Err(other) => panic!("expected RasterizerLimit, got a different refusal: {other}"),
        Ok(_) => panic!(
            "1,000,000x on an E-size sheet rendered -- the boundary moved, so \
             re-run examples/region_panic_ceiling.rs and update the recorded \
             table before weakening this test"
        ),
    }
}

#[test]
fn an_ordinary_deep_zoom_still_renders() {
    // The other half of the contract, and the reason the guard is not a
    // ceiling: 100,000x is below every boundary the probe measured and must
    // still produce pixels. A guard that refused here would have cost real
    // capability to fix a crash nobody could reach.
    let doc = Document::from_bytes(e_size_sheet()).expect("synthetic sheet loads");
    let pages = page_tree::pages(&doc).expect("pages");

    let rendered = render_page_region(
        &doc.view(),
        &pages[0],
        100_000.0,
        viewport_region(100_000.0),
        &RenderOptions::default(),
    )
    .expect("100,000x is below every measured boundary");

    assert!(
        rendered.pixmap.width() > 0 && rendered.pixmap.height() > 0,
        "a successful deep zoom must return a real pixmap"
    );
}

/// The floor-below-the-measurement claim, checked at COMPILE time.
///
/// Both sides are constants, so a runtime `assert!` on them is a lint (and
/// fairly: it can never fail at a moment anybody is watching). As a `const`
/// item the same claim fails the BUILD instead, which is strictly stronger —
/// raising `MAX_GUARANTEED_REGION_SCALE` above the lowest measured boundary
/// stops being a test failure somebody could mark `#[ignore]` and becomes a
/// compile error.
///
/// Spelled `const _` because a `const` item is evaluated whether or not
/// anything names it — referring to it from the test body would be a
/// path-statement with no effect, which is a lint, and the reference was never
/// what made it fire.
const _: () = assert!(
    pdfcer_render::MAX_GUARANTEED_REGION_SCALE < 284_964.0,
    "the published floor must be below the lowest measured boundary (E-size, 284,964x)"
);

#[test]
fn the_published_floor_is_below_every_measured_boundary() {
    // The constant is a claim about a measurement, so it is checked against
    // the measurement rather than left as prose. 284,964 is the lowest first-
    // failing scale in the recorded table (E-size); the published floor must
    // sit under it with room, or it is promising something the probe did not
    // find.
    let doc = Document::from_bytes(e_size_sheet()).expect("synthetic sheet loads");
    let pages = page_tree::pages(&doc).expect("pages");
    let s = pdfcer_render::MAX_GUARANTEED_REGION_SCALE;
    assert!(
        render_page_region(
            &doc.view(),
            &pages[0],
            s,
            viewport_region(s),
            &RenderOptions::default()
        )
        .is_ok(),
        "the floor must itself render on the most fragile geometry measured"
    );
}
