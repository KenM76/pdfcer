//! # §12.5.2 `/CA` — an annotation's constant opacity must actually apply
//!
//! Reported by the `pdfcer-gui` session (2026-08-14). `pdfcer-render` read no
//! annotation `/CA` at all, so **every** annotation carrying one rendered
//! solid.
//!
//! ## Why this was a shipped fidelity defect, not a missing feature
//!
//! Their framing, which is what moved it up the queue and is worth preserving:
//!
//! > *"pdfcer-gui is a viewer before it is an editor… the stated audience is
//! > drawing review, where incoming markup arrives from Bluebeam and Acrobat.
//! > Reduced-opacity markup is the house style for a shaded area or a fill over
//! > a drawing, precisely because the drawing underneath has to stay readable.
//! > Every one of those renders solid in pdfcer right now — which does not look
//! > like an opacity bug, it looks like the markup **covered the drawing**, and
//! > the drawing is the thing the operator opened the file to read."*
//!
//! The authoring control it also unblocks is secondary to that.
//!
//! ## What is asserted, and why it is differential
//!
//! A red square annotation over a white page, rendered at `/CA` 1.0 and 0.5.
//! The half-opacity render must produce **lighter** ink — compared against the
//! opaque render rather than against expected pixel values, because the exact
//! composite depends on the blend and the point is the *relationship*.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_render::render_page;

/// A one-page document with a single `/Square` annotation whose `/AP` fills
/// the whole `/Rect` red, and whose `/CA` is `ca` (omitted when `None`).
fn doc_with_square(ca: Option<f64>) -> Document {
    let ap = "1 0 0 rg 0 0 100 100 re f";
    let ca_entry = ca.map_or(String::new(), |v| format!(" /CA {v}"));
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] /Resources << >> >>"
            .to_string(),
        "<< /Type /Page /Parent 2 0 R /Annots [4 0 R] >>".to_string(),
        format!(
            "<< /Type /Annot /Subtype /Square /Rect [0 0 100 100] /F 4{ca_entry} \
             /AP << /N 5 0 R >> >>"
        ),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Length {} >>\nstream\n{ap}\nendstream",
            ap.len()
        ),
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
    Document::from_bytes(buf).expect("synthetic doc parses")
}

/// The mean green channel over the page. Red ink on white: fully opaque red is
/// green 0, white is 255, so a half-opacity red sits between — a single number
/// that moves monotonically with opacity.
fn mean_green(doc: &Document) -> f64 {
    let pages = page_tree::pages(doc).expect("page tree");
    let out = render_page(doc, &pages[0], 1.0).expect("rasterises");
    let data = out.pixmap.data();
    let total: u64 = data.chunks_exact(4).map(|px| u64::from(px[1])).sum();
    let count = data.len() / 4;
    total as f64 / count as f64
}

/// ★ Half opacity must render lighter than full opacity.
///
/// The defect: both rendered identically, because `/CA` was never read.
#[test]
fn a_half_opacity_annotation_renders_lighter_than_an_opaque_one() {
    let opaque = mean_green(&doc_with_square(Some(1.0)));
    let half = mean_green(&doc_with_square(Some(0.5)));

    assert!(
        half > opaque + 20.0,
        "★ /CA 0.5 must render visibly lighter than /CA 1.0. mean green: \
         opaque {opaque:.1}, half {half:.1}. Equal values mean /CA is being \
         ignored — the defect where every imported reduced-opacity markup \
         renders solid over the drawing it was meant to annotate."
    );
    assert!(
        half < 250.0,
        "…but it must still be painted, not skipped: mean green {half:.1} is \
         indistinguishable from a blank page"
    );
}

/// An absent `/CA` is opaque (§12.5.2), not transparent.
///
/// The failure this blocks is the mirror image: defaulting a missing key to 0
/// would make every ordinary annotation invisible.
#[test]
fn an_absent_ca_renders_identically_to_an_explicit_one() {
    let absent = mean_green(&doc_with_square(None));
    let explicit = mean_green(&doc_with_square(Some(1.0)));
    assert!(
        (absent - explicit).abs() < 0.01,
        "absent /CA must mean fully opaque: {absent:.3} vs {explicit:.3}"
    );
}

/// `/CA 0` paints nothing, and does so without erroring.
#[test]
fn a_fully_transparent_annotation_paints_nothing() {
    let blank = mean_green(&doc_with_square(Some(0.0)));
    assert!(
        blank > 254.9,
        "/CA 0 must leave the page untouched, got mean green {blank:.2}"
    );
}

/// Out-of-range `/CA` is clamped rather than refused.
///
/// A producer writing `1.5` means "opaque". Refusing to place the annotation
/// over a range check would lose content to defend a number.
#[test]
fn an_out_of_range_ca_is_clamped_not_refused() {
    let over = mean_green(&doc_with_square(Some(1.5)));
    let opaque = mean_green(&doc_with_square(Some(1.0)));
    assert!(
        (over - opaque).abs() < 0.01,
        "/CA 1.5 must clamp to opaque, not vanish: {over:.3} vs {opaque:.3}"
    );
}
