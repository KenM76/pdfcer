//! # Transparency flattened into page content (`Pass 748.1`)
//!
//! [`RecordingFlattener`] through `EditSession::flatten_transparency`. The
//! oracle is the renderer: the flattened page, saved and reopened, renders
//! like the original did, and its content no longer uses transparency.

// Tests fail by panicking.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::transparency::{
    FlattenMode, FlattenOptions, FlattenTransparencyReport, RasterReason, has_transparency,
};
use pdfcer_core::writer::SaveOptions;
use pdfcer_emf::RecordingFlattener;
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

use crate::export_emf::{build, page};

const PPI: f64 = 144.0;

fn render(doc: &Document) -> Pixmap {
    let p = page_tree::pages(doc).unwrap().remove(0);
    let options = RenderOptions::default().with_backdrop(PageBackdrop::White);
    #[allow(clippy::cast_possible_truncation)]
    let scale = (PPI / 72.0) as f32;
    render_page_with(doc, &p, scale, &options).unwrap().pixmap
}

/// Flatten page 1 of `bytes`; the report and the saved, reopened result.
fn flatten(bytes: Vec<u8>, options: &FlattenOptions) -> (FlattenTransparencyReport, Document) {
    let mut s = EditSession::new(Document::from_bytes(bytes).unwrap());
    let report = s
        .flatten_transparency(&[0], options, &RecordingFlattener::new())
        .unwrap();
    let (saved, _) = s.to_incremental_bytes(&SaveOptions::default()).unwrap();
    (report, Document::from_bytes(saved).unwrap())
}

/// The share of pixels whose largest channel difference is over 24.
fn differing(a: &Pixmap, b: &Pixmap) -> f64 {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    let n = a
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .zip(b.data().as_chunks::<4>().0)
        .filter(|(p, q)| p.iter().zip(*q).any(|(x, y)| x.abs_diff(*y) > 24))
        .count();
    #[allow(clippy::cast_precision_loss)]
    let share = n as f64 / f64::from(a.width() * a.height());
    share
}

/// Renders as before, within antialiasing, and uses no transparency.
fn assert_flattened(before: &[u8], after: &Document) {
    let original = render(&Document::from_bytes(before.to_vec()).unwrap());
    let share = differing(&original, &render(after));
    assert!(share < 0.01, "{:.2}% of pixels differ", share * 100.0);
    let left = has_transparency(&after.view(), 0).unwrap();
    assert!(!left.in_page_content(), "{left:?}");
}

#[test]
fn constant_alpha_over_flat_colour_becomes_vector_regions() {
    let bytes = page(
        "/ExtGState << /X << /ca 0.5 >> >>",
        "1 0.8 0 rg 10 10 50 40 re f /X gs 0 0.4 1 rg 30 20 40 40 re f 0 0 0 RG 2 w 5 5 m 95 75 l S",
    );
    let (report, after) = flatten(bytes.clone(), &FlattenOptions::new().with_raster_ppi(PPI));
    let p = &report.pages[0];
    assert!(report.changed);
    assert_eq!(p.mode, Some(FlattenMode::Overlay));
    assert!(p.vector_regions >= 3, "{p:?}");
    assert!(p.rasterised.is_empty(), "{p:?}");
    assert_eq!(p.strokes_outlined, 0);
    assert_flattened(&bytes, &after);
}

#[test]
fn outlined_strokes_and_a_rebuilt_page() {
    let bytes = page(
        "/ExtGState << /X << /ca 0.5 /BM /Multiply >> >>",
        "1 0.8 0 rg 10 10 50 40 re f 0 0 0 RG 2 w 5 5 m 95 75 l S /X gs 0 0.4 1 rg 30 20 40 40 re f",
    );
    let options = FlattenOptions::new()
        .with_raster_ppi(PPI)
        .with_keep_text(false)
        .with_keep_strokes(false);
    let (report, after) = flatten(bytes.clone(), &options);
    let p = &report.pages[0];
    assert_eq!(p.mode, Some(FlattenMode::Rebuilt));
    assert_eq!(p.strokes_outlined, 1, "{p:?}");
    assert_flattened(&bytes, &after);
}

#[test]
fn an_image_under_alpha_is_rasterised_and_said_so() {
    let image = "/Type /XObject /Subtype /Image /Width 2 /Height 2 /ColorSpace /DeviceRGB /BitsPerComponent 8 /Filter /ASCIIHexDecode";
    let pixels = "ff0000 00ff00 0000ff ffff00>";
    let mut objects = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_string()),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] >>".into(),
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Im 5 0 R >> /ExtGState << /X << /ca 0.5 >> >> >> >>"
                .into(),
        ),
    ];
    let content = "q 60 0 0 50 10 10 cm /Im Do Q /X gs 0 0.4 1 rg 30 20 40 40 re f\n";
    objects.push((
        4,
        format!(
            "<< /Length {} >>\nstream\n{content}endstream",
            content.len()
        ),
    ));
    objects.push((
        5,
        format!(
            "<< {image} /Length {} >>\nstream\n{pixels}\nendstream",
            pixels.len()
        ),
    ));
    let bytes = build(&objects);
    let (report, after) = flatten(bytes.clone(), &FlattenOptions::new().with_raster_ppi(PPI));
    let p = &report.pages[0];
    assert!(
        p.rasterised.iter().any(|r| r.reason == RasterReason::Image),
        "{p:?}"
    );
    let text = report.disclosures.join("\n");
    assert!(text.contains("(image)"), "{text}");
    assert_flattened(&bytes, &after);
}
