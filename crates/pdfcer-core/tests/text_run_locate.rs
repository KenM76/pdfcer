//! `G037` — `vector::locate_text_run` joins an extracted glyph to the surgery
//! run a verb acts on.
//!
//! The oracle is construction: every show operator in these fixtures carries
//! its own letters, so the letter says which run it must resolve to.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree;
use pdfcer_core::text_extract::{self, ContentStreamRef, ExtractOptions, PageText};
use pdfcer_core::vector::{
    Matrix, PageObjects, TextRunRef, decompose_page, locate_text_run, locate_text_runs,
};

fn assemble(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
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

fn stream_obj(keys: &str, content: &str) -> String {
    format!(
        "<< {keys} /Length {} >>\nstream\n{content}\nendstream",
        content.len() + 1
    )
}

const HELVETICA: &str =
    "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>";

/// Page `content` plus form `/X1` (object 5) holding `form`.
fn doc(content: &str, form: &str) -> Document {
    Document::from_bytes(assemble(&[
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /Font << /F1 6 0 R >> /XObject << /X1 5 0 R >> >> >>"
            .to_owned(),
        stream_obj("", content),
        stream_obj(
            "/Type /XObject /Subtype /Form /BBox [0 0 300 300] \
             /Resources << /Font << /F1 6 0 R >> >>",
            form,
        ),
        HELVETICA.to_owned(),
    ]))
    .unwrap()
}

fn models(d: &Document) -> (PageText, PageObjects) {
    let pages = page_tree::pages(d).unwrap();
    let text = text_extract::extract_page(
        d,
        &pages[0],
        0,
        &ExtractOptions::default().with_provenance(true),
    )
    .unwrap();
    let model = decompose_page(&d.view(), &pages[0], Matrix::IDENTITY).unwrap();
    (text, model)
}

/// `(char, resolved ref, glyph x, glyph y, from a form?)` for every glyph.
fn resolve_all(text: &PageText, model: &PageObjects) -> Vec<(char, Option<TextRunRef>, f32, f32)> {
    let mut out = Vec::new();
    for run in &text.runs {
        for g in &run.glyphs {
            let ch = run.text[g.text_start as usize..].chars().next().unwrap();
            let r = g
                .provenance
                .as_ref()
                .and_then(|p| locate_text_run(model, p));
            out.push((ch, r, g.x, g.y));
        }
    }
    out
}

fn page_ref(object_index: usize, run_index: usize) -> Option<TextRunRef> {
    Some(TextRunRef::Page {
        object_index,
        run_index,
    })
}

/// Every show-operator kind, a `TJ` array, marked content, a path and an
/// inline image between text objects: each letter lands on its own run.
#[test]
fn every_page_glyph_resolves_to_the_run_that_shows_it() {
    let content = "BT /F1 12 Tf 14 TL 72 700 Td (AB) Tj [(CD) -50 (E)] TJ (FG) ' \
                   /Span << /MCID 0 >> BDC 1 2 (HI) \" EMC ET\n\
                   0 0 m 100 100 l S\n\
                   BI /W 1 /H 1 /CS /G /BPC 8 ID \u{0}\nEI\n\
                   BT /F1 12 Tf 72 500 Td (JK) Tj ET";
    let d = doc(content, "");
    let (text, model) = models(&d);
    let all = resolve_all(&text, &model);
    let want = |c: char| match c {
        'A' | 'B' => page_ref(0, 0),
        'C' | 'D' | 'E' => page_ref(0, 1),
        'F' | 'G' => page_ref(0, 2),
        'H' | 'I' => page_ref(0, 3),
        'J' | 'K' => page_ref(3, 0),
        _ => panic!("unexpected glyph {c:?}"),
    };
    let letters: String = all
        .iter()
        .map(|g| g.0)
        .filter(|c| !c.is_whitespace())
        .collect();
    assert_eq!(letters.len(), 11, "{letters}");
    for (c, got, ..) in all.iter().filter(|g| !g.0.is_whitespace()) {
        assert_eq!(*got, want(*c), "glyph {c:?}");
    }
}

/// A form drawn twice: each placement's glyphs pick the leaf whose box they
/// fall in, not merely the first leaf with matching bytes.
#[test]
fn a_repeated_form_resolves_each_placement_to_its_own_leaf() {
    let content = "q 1 0 0 1 0 0 cm /X1 Do Q q 1 0 0 1 300 400 cm /X1 Do Q";
    let d = doc(content, "BT /F1 12 Tf 10 10 Td (XY) Tj ET");
    let (text, model) = models(&d);
    assert_eq!(model.leaves.len(), 2);
    let all = resolve_all(&text, &model);
    assert_eq!(all.len(), 4);
    let mut seen = Vec::new();
    for (c, got, x, y) in &all {
        let Some(TextRunRef::Form {
            leaf_index,
            run_index,
        }) = got
        else {
            panic!("{c:?} did not resolve to a form run: {got:?}");
        };
        assert_eq!(*run_index, 0);
        let b = model.leaves[*leaf_index].object.page_bbox();
        assert!(
            f64::from(*x) >= b.min.x - 1.0
                && f64::from(*x) <= b.max.x + 1.0
                && f64::from(*y) >= b.min.y - 1.0
                && f64::from(*y) <= b.max.y + 1.0,
            "{c:?} at ({x},{y}) resolved to leaf {leaf_index} at {b:?}"
        );
        seen.push(*leaf_index);
    }
    seen.sort_unstable();
    seen.dedup();
    assert_eq!(seen.len(), 2, "both placements must be reachable");
    // The same span in a form the model never reached is not these leaves.
    let mut p = text.runs[0].glyphs[0].provenance.clone().unwrap();
    p.content_stream = ContentStreamRef::Form { object: 99 };
    assert_eq!(locate_text_run(&model, &p), None);
}

/// A glyph naming a buffer the model does not describe resolves to nothing,
/// and so does one whose operator matches no run.
#[test]
fn an_unknown_buffer_or_span_resolves_to_none() {
    let d = doc("BT /F1 12 Tf 72 700 Td (AB) Tj ET", "");
    let (text, model) = models(&d);
    let mut p = text.runs[0].glyphs[0].provenance.clone().unwrap();
    assert!(locate_text_run(&model, &p).is_some());
    p.content_stream = ContentStreamRef::Form { object: 99 };
    assert_eq!(locate_text_run(&model, &p), None);
    let mut q = text.runs[0].glyphs[0].provenance.clone().unwrap();
    q.operator_span.start += 1;
    q.operator_span.len += 5;
    assert_eq!(locate_text_run(&model, &q), None);
}

/// After an edit, the session's view is one revision: extraction and
/// decomposition of it still agree, and the resolved run is the edited one.
#[test]
fn the_join_holds_on_a_session_view_after_an_edit() {
    let d = doc("BT /F1 12 Tf 72 700 Td (AB) Tj 0 -20 Td (CD) Tj ET", "");
    let mut s = EditSession::new(d);
    s.move_text_run(0, 0, 1, 5.0, 0.0).unwrap();
    let view = s.view();
    let pages = page_tree::pages_in(&view).unwrap();
    let text = text_extract::extract_page_view(
        &view,
        &pages[0],
        0,
        &ExtractOptions::default().with_provenance(true),
    )
    .unwrap();
    let model = decompose_page(&view, &pages[0], Matrix::IDENTITY).unwrap();
    for run in text.runs.iter().filter(|r| !r.glyphs.is_empty()) {
        let refs = locate_text_runs(&model, run);
        let want = if run.text.contains('A') {
            page_ref(0, 0)
        } else {
            page_ref(0, 1)
        };
        assert_eq!(refs.len(), 1, "{:?}: {refs:?}", run.text);
        assert_eq!(refs.first().copied(), want, "{:?}", run.text);
    }
}
