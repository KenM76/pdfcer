//! Reading and setting page labels (ISO 32000-1 §12.4.2): what each page
//! displays after `EditSession::set_page_labels` / `clear_page_labels`,
//! asserted through saved bytes reparsed.

use std::num::NonZeroU32;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession};
use pdfcer_core::page_labels::{LabelFormat, LabelStyle, label_ranges, page_labels};
use pdfcer_core::writer::SaveOptions;

fn build(objects: &[(u32, String)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (num, body) in objects {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n", objects.len() + 1).as_bytes());
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /ID [<0102> <0304>] >>\nstartxref\n{xref_at}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    buf
}

/// `count` pages, the catalog carrying `catalog_extra`.
fn doc(count: u32, catalog_extra: &str) -> Document {
    let kids: Vec<String> = (0..count).map(|i| format!("{} 0 R", 3 + i)).collect();
    let mut objects = vec![
        (
            1,
            format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>"),
        ),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [{}] /Count {count} >>",
                kids.join(" ")
            ),
        ),
    ];
    for i in 0..count {
        objects.push((
            3 + i,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>".to_owned(),
        ));
    }
    Document::from_bytes(build(&objects)).expect("fixture parses")
}

/// The labels the saved, reparsed document displays.
fn saved(session: &EditSession) -> Vec<String> {
    let (bytes, _) = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save must succeed");
    let reopened = Document::from_bytes(bytes).expect("reparse");
    page_labels(&reopened).expect("labels read")
}

fn start(n: u32) -> NonZeroU32 {
    NonZeroU32::new(n).expect("non-zero")
}

/// Would catch: the set range not starting where asked, `/P` or `/St`
/// dropped, the page after the range not keeping its number, or a document
/// without a tree not numbering the untouched pages from 1.
#[test]
fn a_document_without_labels_gains_a_range_and_keeps_the_rest() {
    let mut session = EditSession::new(doc(4, ""));
    let format = LabelFormat::new(LabelStyle::LowerRoman)
        .with_prefix("A-")
        .with_start(start(5));
    let ranges = session
        .set_page_labels(1, 2, &format)
        .expect("set succeeds");
    assert_eq!(ranges, 3);
    assert_eq!(saved(&session), ["1", "A-v", "A-vi", "4"]);
    assert_eq!(
        session.undo(),
        Some(CommandKind::SetPageLabels { ranges: 3 }),
        "the set must be one undo entry"
    );
    assert_eq!(saved(&session), ["1", "2", "3", "4"]);
}

/// Would catch: setting a single page inside a range restarting the pages
/// after it at 1 instead of the numeral they showed.
#[test]
fn a_page_inside_a_range_is_relabelled_alone() {
    let mut session = EditSession::new(doc(
        6,
        "/PageLabels << /Nums [0 << /S /r >> 3 << /S /D >>] >>",
    ));
    session
        .set_page_labels(1, 1, &LabelFormat::new(LabelStyle::UpperLetters))
        .expect("set succeeds");
    assert_eq!(saved(&session), ["i", "A", "iii", "1", "2", "3"]);
}

/// Would catch: a range key inside the set range surviving it, or the
/// range covering the page after it resuming at the wrong number.
#[test]
fn a_range_spanning_an_old_boundary_replaces_it() {
    let mut session = EditSession::new(doc(
        6,
        "/PageLabels << /Nums [0 << /S /r >> 3 << /S /D >>] >>",
    ));
    let ranges = session
        .set_page_labels(
            2,
            4,
            &LabelFormat::new(LabelStyle::Decimal).with_start(start(10)),
        )
        .expect("set succeeds");
    assert_eq!(saved(&session), ["i", "ii", "10", "11", "12", "3"]);
    assert_eq!(ranges, 3, "keys 0, 2 and 5 only");
}

#[test]
fn a_bad_range_is_refused() {
    let mut session = EditSession::new(doc(3, ""));
    let format = LabelFormat::new(LabelStyle::Decimal);
    assert!(matches!(
        session.set_page_labels(2, 1, &format),
        Err(EditError::InvertedPageRange { first: 2, last: 1 })
    ));
    assert!(matches!(
        session.set_page_labels(0, 3, &format),
        Err(EditError::PageOutOfRange { index: 3, count: 3 })
    ));
    assert!(session.undo().is_none(), "a refusal commits nothing");
}

#[test]
fn clearing_numbers_every_page_from_one() {
    let mut session = EditSession::new(doc(3, "/PageLabels << /Nums [0 << /S /R >>] >>"));
    assert!(session.clear_page_labels().expect("clear succeeds"));
    assert_eq!(saved(&session), ["1", "2", "3"]);
    assert!(
        !session.clear_page_labels().expect("clear succeeds"),
        "nothing left to clear"
    );
    assert_eq!(session.undo(), Some(CommandKind::ClearPageLabels));
    assert_eq!(saved(&session), ["I", "II", "III"]);
}

/// Would catch: a `/St` below 1 (forbidden by §12.4.2) read as 0 or
/// negative, or a prefix-only range numbering its pages.
#[test]
fn stored_ranges_read_with_clamped_start() {
    let d = doc(
        3,
        "/PageLabels << /Nums [0 << /S /D /St -4 >> 2 << /P (Cover) >>] >>",
    );
    let ranges = label_ranges(&d);
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0].format.start.get(), 1);
    assert_eq!(ranges[1].format.style, LabelStyle::PrefixOnly);
    assert_eq!(ranges[1].format.prefix, "Cover");
    assert_eq!(page_labels(&d).expect("read"), ["1", "2", "Cover"]);
}

#[test]
fn numerals_follow_the_spec() {
    let r = LabelStyle::UpperRoman;
    assert_eq!(r.numeral(4), "IV");
    assert_eq!(r.numeral(1994), "MCMXCIV");
    assert_eq!(r.numeral(4000), "MMMM");
    assert_eq!(LabelStyle::LowerRoman.numeral(9), "ix");
    let a = LabelStyle::UpperLetters;
    assert_eq!(a.numeral(26), "Z");
    assert_eq!(a.numeral(27), "AA");
    assert_eq!(a.numeral(28), "BB", "letters repeat, not AB (§12.4.2)");
    assert_eq!(a.numeral(53), "AAA");
    assert_eq!(LabelStyle::LowerLetters.numeral(2), "b");
    assert_eq!(a.numeral(2601), "2601", "past the ceiling: decimal");
    assert_eq!(r.numeral(100_001), "100001", "past the ceiling: decimal");
    assert_eq!(LabelStyle::PrefixOnly.numeral(7), "");
}
