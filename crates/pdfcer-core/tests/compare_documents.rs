//! Document comparison end to end over synthetic in-memory PDFs.

use pdfcer_core::compare::{ChangeKind, CompareError, CompareOptions, Side, compare_documents};
use pdfcer_core::document::Document;

fn build_pdf(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        let body = match body.strip_prefix("STREAM:") {
            Some(data) => format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()),
            None => body.clone(),
        };
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// One page per entry; each entry is lines of `(font, size, text)` drawn
/// 20 pt apart from the top. F1 Helvetica, F2 Helvetica-Bold.
fn doc(pages: &[&[(&str, f32, &str)]]) -> Document {
    Document::from_bytes(doc_bytes(pages)).expect("loads")
}

fn doc_bytes(pages: &[&[(&str, f32, &str)]]) -> Vec<u8> {
    let n = pages.len();
    let kids: Vec<String> = (0..n).map(|i| format!("{} 0 R", 5 + 2 * i)).collect();
    let mut bodies = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        format!("<< /Type /Pages /Kids [{}] /Count {n} >>", kids.join(" ")),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
            .to_owned(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
            .to_owned(),
    ];
    for (i, lines) in pages.iter().enumerate() {
        let mut c = String::new();
        for (j, (font, size, text)) in lines.iter().enumerate() {
            let y = 700 - 20 * j;
            c.push_str(&format!("BT /{font} {size} Tf 72 {y} Td ({text}) Tj ET\n"));
        }
        bodies.push(format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents {} 0 R \
             /Resources << /Font << /F1 3 0 R /F2 4 0 R >> >> >>",
            6 + 2 * i
        ));
        bodies.push(format!("STREAM:{c}"));
    }
    build_pdf(&bodies)
}

fn one(text: &str) -> Document {
    doc(&[&[("F1", 12.0, text)]])
}

fn compare(old: &Document, new: &Document) -> pdfcer_core::compare::CompareReport {
    compare_documents(old, new, &CompareOptions::default()).expect("compares")
}

#[test]
fn identical_text_has_no_changes() {
    let r = compare(&one("The quick brown fox"), &one("The quick brown fox"));
    assert!(r.is_identical());
    assert_eq!((r.old_word_count, r.new_word_count), (4, 4));
    assert!(!r.truncated);
}

#[test]
fn an_inserted_word_is_one_insertion_anchored_in_the_old_document() {
    let r = compare(&one("The quick fox"), &one("The quick brown fox"));
    assert_eq!(r.changes.len(), 1);
    let c = &r.changes[0];
    assert_eq!(c.kind, ChangeKind::Inserted);
    assert_eq!(c.new_text, "brown");
    assert_eq!(c.new_words, 2..3);
    assert_eq!(c.new_places.len(), 1);
    assert_eq!(c.new_places[0].page, 0);
    let (side, at) = c.anchor.expect("anchored");
    assert_eq!(side, Side::Old);
    // At the left edge of "fox" in the old document, on its line.
    assert!(at.rect.llx > 72.0 && at.rect.width() == 0.0);
    assert!(at.rect.lly < 700.0 && at.rect.ury > 700.0);
}

#[test]
fn a_deleted_word_and_a_replaced_word() {
    let r = compare(&one("one two three four"), &one("one three FOUR"));
    let kinds: Vec<_> = r.changes.iter().map(|c| c.kind).collect();
    assert_eq!(kinds, [ChangeKind::Deleted, ChangeKind::Replaced]);
    assert_eq!(r.changes[0].old_text, "two");
    assert_eq!(r.changes[0].anchor.map(|a| a.0), Some(Side::New));
    assert_eq!(r.changes[1].old_text, "four");
    assert_eq!(r.changes[1].new_text, "FOUR");
    assert_eq!(r.count(ChangeKind::Replaced), 1);
}

#[test]
fn text_reflowed_onto_the_next_page_is_not_a_change() {
    let old = doc(&[
        &[("F1", 12.0, "alpha beta gamma")],
        &[("F1", 12.0, "delta")],
    ]);
    let new = doc(&[
        &[("F1", 12.0, "alpha beta")],
        &[("F1", 12.0, "gamma delta")],
    ]);
    assert!(compare(&old, &new).is_identical());
}

#[test]
fn a_change_spanning_two_lines_has_one_place_per_line() {
    let old = doc(&[&[("F1", 12.0, "keep"), ("F1", 12.0, "end")]]);
    let new = doc(&[&[
        ("F1", 12.0, "keep new words"),
        ("F1", 12.0, "more here end"),
    ]]);
    let r = compare(&old, &new);
    assert_eq!(r.changes.len(), 1);
    assert_eq!(r.changes[0].new_text, "new words more here");
    assert_eq!(r.changes[0].new_places.len(), 2);
}

#[test]
fn formatting_changes_are_reported_only_when_asked() {
    let old = doc(&[&[("F1", 12.0, "plain words here")]]);
    let new = doc(&[&[("F1", 14.0, "plain words here")]]);
    assert!(compare(&old, &new).is_identical());
    let opts = CompareOptions::default().with_formatting(true);
    let r = compare_documents(&old, &new, &opts).expect("compares");
    assert_eq!(r.changes.len(), 1);
    let c = &r.changes[0];
    assert_eq!(c.kind, ChangeKind::Formatting);
    assert_eq!(c.new_text, "plain words here");
    let delta = c.formatting.expect("delta");
    assert_eq!(delta.size, Some((12.0, 14.0)));
    assert_eq!(delta.color, None);
}

#[test]
fn page_ranges_select_what_each_side_reads() {
    let old = doc(&[&[("F1", 12.0, "cover")], &[("F1", 12.0, "body text")]]);
    let new = one("body text");
    let opts = CompareOptions::default().with_pages(Side::Old, vec![1]);
    let r = compare_documents(&old, &new, &opts).expect("compares");
    assert!(r.is_identical());
    assert_eq!(r.old_pages, [1]);
    let bad = CompareOptions::default().with_pages(Side::New, vec![3]);
    assert!(matches!(
        compare_documents(&old, &new, &bad),
        Err(CompareError::NoSuchPage {
            side: Side::New,
            index: 3,
            count: 1
        })
    ));
}

#[test]
fn an_exhausted_budget_is_disclosed_and_still_complete() {
    let old = one("a b c d e f g h");
    let new = one("a x c y e z g w");
    let opts = CompareOptions::default().with_step_budget(1);
    let r = compare_documents(&old, &new, &opts).expect("compares");
    assert!(r.truncated);
    assert_eq!(r.changes.len(), 1);
    assert_eq!(r.changes[0].old_text, "b c d e f g h");
}

#[test]
fn a_document_withholding_copy_is_refused() {
    let dir = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/encryption/"
    );
    let bytes = std::fs::read(format!("{dir}enc-emptyuser-print-only.pdf")).expect("fixture");
    let locked = Document::from_bytes_with_password(bytes, None).expect("opens");
    let plain = one("x");
    assert!(matches!(
        compare_documents(&plain, &locked, &CompareOptions::default()),
        Err(CompareError::CopyNotPermitted { side: Side::New })
    ));
}

mod result {
    use super::*;
    use pdfcer_core::annot::page_annotations;
    use pdfcer_core::annot_author::Color;
    use pdfcer_core::compare::{
        DELETED_COLOR, INSERTED_COLOR, REPLACED_COLOR, ResultOptions, mark_changes,
    };
    use pdfcer_core::edit::EditSession;
    use pdfcer_core::object::Object;
    use pdfcer_core::writer::SaveOptions;

    const OLD: &[(&str, f32, &str)] = &[("F1", 12.0, "one two three four five")];
    const NEW: &[(&str, f32, &str)] = &[("F1", 12.0, "one three FOUR five six")];

    /// (subtype, /C, /Rect llx, /Contents) of every annotation on `page`.
    fn marks(doc: &Document, page: usize) -> Vec<(String, Vec<f64>, f64, String)> {
        let pages = pdfcer_core::page_tree::pages(doc).expect("pages");
        page_annotations(doc, pages[page].id)
            .into_iter()
            .map(|a| {
                let d = doc.get(a.id.expect("indirect")).expect("object");
                let d = d.value.as_dict().expect("dict").clone();
                let c = d
                    .get(b"C")
                    .and_then(Object::as_array)
                    .map(|v| v.iter().filter_map(Object::as_number).collect())
                    .unwrap_or_default();
                let note = match d.get(b"Contents") {
                    Some(Object::String(s)) => String::from_utf8_lossy(s).into_owned(),
                    _ => String::new(),
                };
                let sub = String::from_utf8_lossy(&a.subtype).into_owned();
                (sub, c, a.rect.expect("rect").llx, note)
            })
            .collect()
    }

    fn rgb(c: Color) -> Vec<f64> {
        match c {
            Color::Rgb(r, g, b) => vec![r, g, b],
            _ => unreachable!(),
        }
    }

    fn build(options: &ResultOptions) -> (Vec<u8>, Vec<u8>, Document, usize) {
        let (old_bytes, new_bytes) = (doc_bytes(&[OLD]), doc_bytes(&[NEW]));
        let old = Document::from_bytes(old_bytes.clone()).expect("old");
        let new = Document::from_bytes(new_bytes.clone()).expect("new");
        let report = compare(&old, &new);
        let mut session = EditSession::new(new);
        let r = mark_changes(&mut session, &report, options).expect("marks");
        assert!(r.unmarked.is_empty(), "{:?}", r.unmarked);
        assert_eq!(r.changes_marked, report.changes.len());
        let saved = session
            .to_incremental_bytes(&SaveOptions::identity())
            .expect("saves")
            .0;
        assert!(
            saved.starts_with(&new_bytes),
            "an incremental update of NEW"
        );
        let reopened = Document::from_bytes(saved).expect("reopens");
        // The sources are only read.
        assert_eq!(old_bytes, doc_bytes(&[OLD]));
        (old_bytes, new_bytes, reopened, r.annotations)
    }

    #[test]
    fn one_mark_per_change_coloured_by_kind_with_the_old_text() {
        let (_, _, doc, n) = build(&ResultOptions::default().with_summary_page(false));
        assert_eq!(pdfcer_core::page_tree::pages(&doc).expect("pages").len(), 1);
        let m = marks(&doc, 0);
        assert_eq!(n, 3);
        assert_eq!(
            m.len(),
            3,
            "deleted two, replaced four, inserted six: {m:?}"
        );
        let by = |sub: &str, c: Color| {
            m.iter()
                .find(|x| x.0 == sub && x.1 == rgb(c))
                .unwrap_or_else(|| panic!("no {sub} in {m:?}"))
                .clone()
        };
        let del = by("Caret", DELETED_COLOR);
        assert_eq!(del.3, "Deleted: two");
        let rep = by("Highlight", REPLACED_COLOR);
        assert_eq!(rep.3, "Replaced. Was: four");
        let ins = by("Highlight", INSERTED_COLOR);
        assert_eq!(ins.3, "Inserted: six");
        // The caret sits where "two" was: between "one" and "three".
        assert!(del.2 > 72.0 && del.2 < rep.2, "{del:?} vs {rep:?}");
    }

    #[test]
    fn the_summary_page_goes_first_and_the_marks_stay_on_their_page() {
        let (_, _, doc, _) = build(&ResultOptions::default().with_labels("a.pdf", "b.pdf"));
        assert_eq!(pdfcer_core::page_tree::pages(&doc).expect("pages").len(), 2);
        assert!(marks(&doc, 0).is_empty());
        assert_eq!(marks(&doc, 1).len(), 3);
        let opts = pdfcer_core::text_extract::ExtractOptions::default();
        let text = pdfcer_core::text_extract::extract_pages(&doc, &[0], &opts).expect("reads");
        let first: String = text.pages[0].runs.iter().map(|r| r.text.as_str()).collect();
        assert!(first.contains("Comparison summary"), "{first}");
        assert!(first.contains("a.pdf"), "{first}");
        assert!(
            first.contains("1 inserted, 1 deleted, 1 replaced"),
            "{first}"
        );
    }
}
