//! EMF export → EMF import round trip: a page exported by
//! `emf::export_emf`, imported by `emf_import::import` and placed with
//! `EditSession::add_emf` over a blank page of the same size, renders like
//! the original page. The importer is the reader under test; the exporter
//! is the independent writer whose records it must understand.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::emf_import;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::writer::SaveOptions;
use pdfcer_render::emf::{EmfOptions, export_emf};
use pdfcer_render::{PageBackdrop, RenderOptions, render_page_with};
use tiny_skia::Pixmap;

const DPI: f32 = 144.0;

/// A one-page PDF, 100 × 80 pt, with `resources` and `content`.
fn page(resources: &str, content: &str) -> Vec<u8> {
    let stream = format!("{content}\n");
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 80] >>".to_owned(),
        format!("<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << {resources} >> >>"),
        format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
    for o in offsets {
        buf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn render(bytes: Vec<u8>) -> Pixmap {
    let doc = Document::from_bytes(bytes).unwrap();
    let p = page_tree::pages(&doc).unwrap().remove(0);
    let opts = RenderOptions::default().with_backdrop(PageBackdrop::White);
    render_page_with(&doc, &p, DPI / 72.0, &opts)
        .unwrap()
        .pixmap
}

/// (original raster, round-tripped raster, the import's disclosure).
fn round_trip(resources: &str, content: &str) -> (Pixmap, Pixmap, String) {
    let original = page(resources, content);
    let doc = Document::from_bytes(original.clone()).unwrap();
    let p = page_tree::pages(&doc).unwrap().remove(0);
    let opts = EmfOptions::default().with_raster_dpi(DPI);
    let emf = export_emf(&doc, &p, &RenderOptions::default(), &opts).unwrap();
    let imported = emf_import::import(&emf.emf).unwrap();
    let (w, h) = imported.natural_size_pt();
    assert!(
        (w - 100.0).abs() < 0.5 && (h - 80.0).abs() < 0.5,
        "{w} x {h}"
    );
    let mut s = EditSession::new(Document::from_bytes(page("", "")).unwrap());
    let rect = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: 100.0,
        ury: 80.0,
    };
    s.add_emf(0, rect, &imported).unwrap();
    let out = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    (render(original), render(out), imported.notes().summary())
}

/// Pixels whose largest channel difference exceeds `tol`.
fn differing(a: &Pixmap, b: &Pixmap, tol: u8) -> usize {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    a.pixels()
        .iter()
        .zip(b.pixels())
        .filter(|(pa, pb)| {
            pa.red().abs_diff(pb.red()) > tol
                || pa.green().abs_diff(pb.green()) > tol
                || pa.blue().abs_diff(pb.blue()) > tol
        })
        .count()
}

fn ink(p: &Pixmap) -> usize {
    p.pixels()
        .iter()
        .filter(|px| px.red() < 250 || px.green() < 250 || px.blue() < 250)
        .count()
}

/// The EMF's 0.01 mm grid may nudge an antialiased edge pixel; nothing
/// else may move.
fn assert_close(original: &Pixmap, back: &Pixmap, notes: &str) {
    let total = original.width() as usize * original.height() as usize;
    let off = differing(original, back, 48);
    assert!(ink(original) > total / 50, "the fixture paints something");
    assert!(
        off * 200 < ink(original),
        "{off} of {} inked pixels differ; notes: {notes}",
        ink(original)
    );
}

#[test]
fn a_filled_outlined_rectangle_round_trips() {
    let (a, b, notes) = round_trip("", "1 0 0 rg 0 0 1 RG 3 w 10 10 50 40 re B");
    assert_close(&a, &b, &notes);
}

#[test]
fn curves_strokes_and_caps_round_trip() {
    let (a, b, notes) = round_trip(
        "",
        "0 0.5 0 RG 4 w 1 J 1 j 10 10 m 30 70 70 70 90 10 c S \
         0 0 1 rg 50 40 m 80 40 l 65 70 l h f",
    );
    assert_close(&a, &b, &notes);
}

#[test]
fn a_clipped_fill_round_trips() {
    let (a, b, notes) = round_trip("", "q 20 20 40 30 re W n 0 0 0 rg 0 0 100 80 re f Q");
    assert_close(&a, &b, &notes);
}

#[test]
fn an_image_round_trips_as_an_image() {
    let (a, b, notes) = round_trip(
        "",
        "q 60 0 0 40 20 20 cm BI /W 2 /H 2 /BPC 8 /CS /RGB /F /AHx ID ff000000ff000000ffffff00> EI Q",
    );
    assert_close(&a, &b, &notes);
    assert!(colours(&b) >= 4, "four distinct image colours survive");
}

/// Distinct saturated (primary or yellow) colours painted.
fn colours(p: &Pixmap) -> usize {
    let mut seen = std::collections::BTreeSet::new();
    for px in p.pixels() {
        let c = [px.red(), px.green(), px.blue()];
        if c.iter().all(|v| !(8..=247).contains(v)) && c != [255, 255, 255] {
            seen.insert(c);
        }
    }
    seen.len()
}
