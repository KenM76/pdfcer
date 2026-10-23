//! Glyph-positioning regressions for the text surgeon: the hit test must
//! measure every glyph where it is painted, or it removes the wrong text
//! and leaves the marked text in place.

use super::*;
use crate::edit::EditSession;
use crate::text_extract::{self, ExtractOptions};
use crate::writer::SaveOptions;

/// One page, `/F1` Helvetica and `/F2` Courier (standard 14, exact AFM
/// widths), showing `content`.
fn page_pdf(content: &str) -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 600 200] \
         /Resources << /Font << /F1 5 0 R /F2 6 0 R >> >> /Contents 4 0 R >>"
            .to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len()
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let n = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

/// Mark `needle` by search, apply, and return the extracted text after.
fn redact_and_extract(content: &str, needle: &str) -> String {
    let doc = Document::from_bytes(page_pdf(content)).unwrap();
    let mut session = EditSession::new(doc);
    let ids = session.mark_redactions_by_search(needle, false).unwrap();
    assert!(!ids.is_empty(), "the fixture must show {needle}");
    let (marked, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap();
    let doc = Document::from_bytes(marked).unwrap();
    let (out, report) = apply_redactions(&doc, &SaveOptions::identity()).unwrap();
    // The glyph box over-covers by design (module docs), so the padding
    // spaces beside the needle may go too.
    let n = needle.len() as u64;
    assert!(
        (n..=n + 2).contains(&report.glyphs_removed),
        "{} glyphs removed for a {n}-glyph needle",
        report.glyphs_removed
    );
    let back = Document::from_bytes(out).unwrap();
    let text = text_extract::extract_document(&back, &ExtractOptions::default()).unwrap();
    text.pages
        .iter()
        .flat_map(|p| p.runs.iter().map(|r| r.text.clone()))
        .collect::<Vec<_>>()
        .join(" ")
}

fn assert_only_secret_removed(after: &str) {
    assert!(!after.contains("SECRET"), "SECRET survived: {after:?}");
    for keep in ["KEEP", "ONE", "WORDS", "TWO"] {
        assert!(
            after.contains(keep),
            "{keep} was wrongly removed: {after:?}"
        );
    }
}

/// A `TJ` string with no hit, before the hit on the same line, is
/// advanced exactly once. Advancing it twice placed every later glyph to
/// the right of where it paints, so the mark removed text before the
/// needle and missed the needle itself.
#[test]
fn a_kerned_tj_removes_the_marked_fragment_not_an_earlier_one() {
    let after = redact_and_extract(
        "BT /F1 12 Tf 20 100 Td [(KEEP ONE WITH MORE WORDS.)-40 (  SECRET)20 ( TWO)] TJ ET",
        "SECRET",
    );
    assert_only_secret_removed(&after);
}

/// Same, across two `Tj` operators on one line.
#[test]
fn a_tj_after_an_unmarked_tj_on_the_same_line_is_measured_in_place() {
    let after = redact_and_extract(
        "BT /F1 12 Tf 20 100 Td (KEEP ONE WITH MORE WORDS.) Tj (  SECRET) Tj ( TWO) Tj ET",
        "SECRET",
    );
    assert_only_secret_removed(&after);
}

/// `Q` restores the font a `q` saved (text state is graphics state). The
/// Courier set inside `q`...`Q` is three times wider per `i` than the
/// Helvetica in effect after it, so measuring with it misses the needle.
#[test]
fn the_font_set_inside_q_does_not_outlive_its_q() {
    let after = redact_and_extract(
        "BT /F1 12 Tf ET q BT /F2 12 Tf ET Q \
         BT 20 100 Td (KEEP iiiiiiiiiiiiiiii WORDS.  SECRET  ONE TWO) Tj ET",
        "SECRET",
    );
    assert_only_secret_removed(&after);
}
