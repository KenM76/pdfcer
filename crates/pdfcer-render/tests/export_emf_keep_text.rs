//! EMF export with text kept as text (G033).
//!
//! The oracle is the PDF itself: a run's reference point, em height and
//! per-character `Dx` must equal the text origin, font size and glyph
//! advances the content stream and font program state, converted to the
//! EMF's 0.01 mm logical units — never the writer's own arithmetic read
//! back. Fallback runs must be counted by reason and written as outlines.
//!
//! Fixtures: hand-built pages showing Standard-14 text, and the synthetic
//! donor face from `tools/gen-subset-font-fixtures.py` embedded by pdfcer's
//! own add-text (`docs/LEGAL.md` §5).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::addtext::{self, AddTextRequest};
use pdfcer_render::RenderOptions;
use pdfcer_render::emf::{EmfExport, EmfOptions, EmfText, export_emf, walk_records};
use pdfcer_render::font::subset::plan_subset;
use skrifa::prelude::{LocationRef, Size};
use skrifa::{FontRef, MetadataProvider};

/// Points → EMF logical units (0.01 mm).
const LU_PER_PT: f64 = 2540.0 / 72.0;

fn fixture(name: &str) -> Vec<u8> {
    let path = format!(
        "{}/../../fixtures/synthetic/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("missing fixture {path}: {e}"))
}

fn build(objects: &[(u32, String)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..=max_num {
        let (_, off) = offsets.iter().find(|(n, _)| *n == num).unwrap();
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            max_num + 1
        )
        .as_bytes(),
    );
    buf
}

/// A 200 × 120 pt page showing `content` with `/F1` = `base_font`.
fn std14_page(base_font: &str, content: &str) -> Document {
    let stream = format!("{content}\n");
    Document::from_bytes(build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".into()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 120] \
                 /Resources << /Font << /F1 << /Type /Font /Subtype /Type1 \
                 /BaseFont /{base_font} >> >> >> >>"
            ),
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>".into()),
        (
            4,
            format!("<< /Length {} >>\nstream\n{stream}endstream", stream.len()),
        ),
    ]))
    .unwrap()
}

/// `hello.pdf` (200 × 120 pt) plus `text` at (20, 60) in the donor face.
fn donor_page(text: &str, render_mode: u8) -> Document {
    let plan = plan_subset(
        &fixture("text/subset-donor.ttf"),
        0,
        &['A', 'B', 'C'],
        "pdfceSubsetDemo",
        "ABCDEF",
    )
    .unwrap();
    let doc = Document::from_bytes(fixture("hello.pdf")).unwrap();
    let req = AddTextRequest::new(0, (20.0, 60.0), text)
        .with_embedded_face(plan)
        .with_size(36.0)
        .with_render_mode(render_mode);
    Document::from_bytes(addtext::add_text(&doc, &req).unwrap().bytes).unwrap()
}

fn export(doc: &Document, text: EmfText) -> EmfExport {
    let page = page_tree::pages(doc).unwrap().remove(0);
    export_emf(
        doc,
        &page,
        &RenderOptions::default(),
        &EmfOptions::default().with_raster_dpi(144.0).with_text(text),
    )
    .unwrap()
}

/// Every record of `kind`, head included.
fn records(emf: &[u8], kind: u32) -> Vec<&[u8]> {
    let mut off = 0;
    let mut out = Vec::new();
    for (t, s) in walk_records(emf).expect("well-formed") {
        if t == kind {
            out.push(&emf[off..off + s as usize]);
        }
        off += s as usize;
    }
    out
}

fn i32_at(r: &[u8], o: usize) -> i32 {
    i32::from_le_bytes(r[o..o + 4].try_into().unwrap())
}

/// One `EMR_EXTTEXTOUTW`: reference point, characters, `Dx`.
struct TextOut {
    reference: (i32, i32),
    text: String,
    dx: Vec<i32>,
}

fn text_outs(emf: &[u8]) -> Vec<TextOut> {
    records(emf, 0x54)
        .into_iter()
        .map(|r| {
            let n = i32_at(r, 44) as usize;
            let off_string = i32_at(r, 48) as usize;
            let off_dx = i32_at(r, 72) as usize;
            let units: Vec<u16> = (0..n)
                .map(|i| u16::from_le_bytes([r[off_string + 2 * i], r[off_string + 2 * i + 1]]))
                .collect();
            TextOut {
                reference: (i32_at(r, 36), i32_at(r, 40)),
                text: String::from_utf16(&units).unwrap(),
                dx: (0..n).map(|i| i32_at(r, off_dx + 4 * i)).collect(),
            }
        })
        .collect()
}

/// One `EMR_EXTCREATEFONTINDIRECTW`'s height, escapement, weight, italic
/// and face name.
fn fonts(emf: &[u8]) -> Vec<(i32, i32, i32, bool, String)> {
    records(emf, 0x52)
        .into_iter()
        .map(|r| {
            assert_eq!(r.len(), 368, "LogFontExDv with no axes");
            assert_eq!(&r[360..364], &[0x64, 0x76, 0x00, 0x08]);
            let face: Vec<u16> = r[40..104]
                .chunks(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .take_while(|&u| u != 0)
                .collect();
            (
                i32_at(r, 12),
                i32_at(r, 20),
                i32_at(r, 28),
                r[32] != 0,
                String::from_utf16(&face).unwrap(),
            )
        })
        .collect()
}

fn lu(pt: f64) -> i32 {
    (pt * LU_PER_PT).round() as i32
}

#[test]
fn a_kept_run_sits_at_the_pdf_origin_with_the_font_advances_as_dx() {
    let doc = donor_page("ABCA", 0);
    let out = export(&doc, EmfText::KeepText);
    let t = out.outcome.text;
    // hello.pdf's own Helvetica lines are kept too.
    assert_eq!(t.runs_as_outlines(), 0, "{t:?}");
    assert_eq!(t.runs_as_text, records(&out.emf, 0x54).len());

    let runs: Vec<TextOut> = text_outs(&out.emf)
        .into_iter()
        .filter(|r| r.text == "ABCA")
        .collect();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    // PDF (20, 60) on a 120 pt page is (20, 60) pt from the top-left.
    assert_eq!(run.reference, (lu(20.0), lu(60.0)));

    let donor_bytes = fixture("text/subset-donor.ttf");
    let donor = FontRef::new(&donor_bytes).unwrap();
    let metrics = donor.glyph_metrics(Size::new(36.0), LocationRef::default());
    let mut pen = 0.0f64;
    let mut want = Vec::new();
    for c in "ABCA".chars() {
        let gid = donor.charmap().map(c).unwrap();
        let next = pen + f64::from(metrics.advance_width(gid).unwrap());
        want.push(lu(next) - lu(pen));
        pen = next;
    }
    assert_eq!(run.dx, want, "Dx = the glyph advances at 36 pt");

    let face = fonts(&out.emf);
    let (height, escapement, _, _, name) = &face[face.len() - 1];
    assert_eq!(*height, -lu(36.0), "lfHeight is minus the em");
    assert_eq!(*escapement, 0);
    assert!(!name.is_empty());
}

#[test]
fn the_default_writes_no_text_records() {
    let doc = donor_page("ABCA", 0);
    let out = export(&doc, EmfText::Outlines);
    assert!(records(&out.emf, 0x54).is_empty());
    assert!(records(&out.emf, 0x52).is_empty());
    assert_eq!(out.outcome.text, Default::default());
}

#[test]
fn stroked_text_stays_outlines_and_is_counted() {
    let doc = donor_page("AB", 1);
    let out = export(&doc, EmfText::KeepText);
    assert!(text_outs(&out.emf).iter().all(|r| r.text != "AB"));
    assert_eq!(out.outcome.text.fallback_paint, 1, "{:?}", out.outcome.text);
}

#[test]
fn standard_14_names_map_to_installed_faces_with_their_style() {
    let doc = std14_page("Helvetica-BoldOblique", "BT /F1 12 Tf 10 20 Td (Hi) Tj ET");
    let out = export(&doc, EmfText::KeepText);
    assert_eq!(out.outcome.text.runs_as_text, 1, "{:?}", out.outcome.text);
    let (_, _, weight, italic, face) = fonts(&out.emf).remove(0);
    assert_eq!(face, "Arial");
    assert_eq!(weight, 700);
    assert!(italic);
    let run = text_outs(&out.emf).remove(0);
    assert_eq!(run.text, "Hi");
    assert_eq!(run.reference, (lu(10.0), lu(100.0)));
    // Helvetica-Bold widths: H 722, i 278.
    assert_eq!(
        run.dx,
        vec![lu(12.0 * 0.722), lu(12.0 * 1.0) - lu(12.0 * 0.722)]
    );
}

#[test]
fn rotated_text_records_its_counterclockwise_angle() {
    // Tm rotates the baseline 90° counterclockwise: the text reads upward.
    let doc = std14_page("Helvetica", "BT /F1 12 Tf 0 1 -1 0 50 20 Tm (Up) Tj ET");
    let out = export(&doc, EmfText::KeepText);
    assert_eq!(out.outcome.text.runs_as_text, 1, "{:?}", out.outcome.text);
    let (_, escapement, _, _, _) = fonts(&out.emf).remove(0);
    assert_eq!(escapement, 900);
}

#[test]
fn skewed_and_condensed_text_stays_outlines_and_is_counted() {
    let doc = std14_page(
        "Helvetica",
        "BT /F1 12 Tf 1 0 0.3 1 10 20 Tm (Sk) Tj ET BT /F1 12 Tf 50 Tz 10 60 Td (Tz) Tj ET",
    );
    let out = export(&doc, EmfText::KeepText);
    assert!(records(&out.emf, 0x54).is_empty());
    assert_eq!(
        out.outcome.text.fallback_geometry, 2,
        "{:?}",
        out.outcome.text
    );
    assert_eq!(out.outcome.text.runs_as_outlines(), 2);
}

#[test]
fn a_symbol_face_stays_outlines_and_is_counted() {
    let doc = std14_page("Symbol", "BT /F1 12 Tf 10 20 Td (ab) Tj ET");
    let out = export(&doc, EmfText::KeepText);
    assert!(records(&out.emf, 0x54).is_empty());
    assert_eq!(
        out.outcome.text.fallback_symbol_face, 1,
        "{:?}",
        out.outcome.text
    );
}
