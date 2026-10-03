//! `G085` inside a form XObject: the marker and its rules live in the form's
//! own stream, so a shared form draws the line wherever it is painted.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::object::ObjId;
use pdfcer_core::text_edit::decoration::{DecorationSet, page_decorations};
use pdfcer_core::text_edit::{FormatOptions, FormatRequest};
use pdfcer_core::text_extract::{ExtractOptions, extract_page};
use pdfcer_core::writer::SaveOptions;

const FORM: &str = "BT /F1 12 Tf 1 0 0 1 72 700 Tm (Hello) Tj 1 0 0 1 200 700 Tm (World) Tj ET";
/// The form object number in [`pdf`].
const FORM_ID: u32 = 6;

/// One page painting the form object 6 once per `cm` in `placements`.
fn pdf(placements: &[&str]) -> Vec<u8> {
    let widths = (0..95).map(|_| "500").collect::<Vec<_>>().join(" ");
    let page: String = placements
        .iter()
        .map(|cm| format!("q {cm} cm /Fm1 Do Q "))
        .collect();
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R \
         /Resources << /XObject << /Fm1 6 0 R >> >> >>"
            .to_owned(),
        format!(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
             /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
        ),
        format!("<< /Length {} >>\nstream\n{page}\nendstream", page.len()),
        format!(
            "<< /Type /XObject /Subtype /Form /BBox [0 0 612 792] \
             /Resources << /Font << /F1 4 0 R >> >> /Length {} >>\nstream\n{FORM}\nendstream",
            FORM.len()
        ),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in offsets {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn underlined(placements: &[&str]) -> EditSession {
    let mut s = EditSession::new(Document::from_bytes(pdf(placements)).unwrap());
    let req = FormatRequest::new(0, "World").decoration(DecorationSet::UNDERLINE);
    s.format_text(&req, &FormatOptions::default()).unwrap();
    s
}

fn saved(s: &EditSession) -> Document {
    Document::from_bytes(s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0).unwrap()
}

fn form_text(doc: &Document) -> String {
    let cs = ContentStream::from_form(&doc.view(), ObjId::new(FORM_ID, 0)).unwrap();
    String::from_utf8(cs.buf).unwrap()
}

/// The `cm` translation of every rule block.
fn rule_origins(text: &str) -> Vec<[f64; 2]> {
    text.match_indices("<</Rule")
        .map(|(at, _)| {
            let block = &text[at..];
            let q = block.find(" q ").unwrap() + 3;
            let cm = block.find(" cm").unwrap();
            let m: Vec<f64> = block[q..cm]
                .split_whitespace()
                .map(|n| n.parse().unwrap())
                .collect();
            [m[4], m[5]]
        })
        .collect()
}

#[test]
fn a_run_inside_a_form_is_underlined_in_the_form() {
    let doc = saved(&underlined(&["1 0 0 1 0 0"]));
    let text = form_text(&doc);
    assert!(text.contains("/pdfc_Deco <</Line /Underline"), "{text}");
    assert_eq!(rule_origins(&text), [[200.0, 700.0]], "{text}");
}

#[test]
fn a_form_painted_twice_gets_one_rule_set() {
    let doc = saved(&underlined(&["1 0 0 1 0 0", "1 0 0 1 0 -300"]));
    let text = form_text(&doc);
    assert_eq!(rule_origins(&text), [[200.0, 700.0]], "{text}");
}

#[test]
fn moving_the_run_in_the_form_moves_its_rule() {
    let mut s = underlined(&["1 0 0 1 0 0"]);
    s.move_text_run_in_form(0, 0, 1, 20.0, 0.0).unwrap();
    let text = form_text(&saved(&s));
    assert_eq!(rule_origins(&text), [[220.0, 700.0]], "{text}");
}

#[test]
fn the_read_reports_form_glyphs() {
    let doc = saved(&underlined(&["1 0 0 1 0 0", "1 0 0 1 0 -300"]));
    let pages = pdfcer_core::page_tree::pages(&doc).unwrap();
    let decorations = page_decorations(&doc.view(), &pages[0]).unwrap();
    let opts = ExtractOptions::default().with_provenance(true);
    let page = extract_page(&doc, &pages[0], 0, &opts).unwrap();
    let mut seen = String::new();
    for run in &page.runs {
        for g in &run.glyphs {
            let ch = run.text[g.text_start as usize..].chars().next().unwrap();
            let set = decorations.of(g.provenance.as_ref().unwrap());
            seen.push(if set.underline {
                ch.to_ascii_uppercase()
            } else {
                ch.to_ascii_lowercase()
            });
        }
    }
    assert_eq!(seen.replace(' ', ""), "helloWORLDhelloWORLD");
}
