//! `EditSession::insert_pages` and `merge_document` from a TAGGED source: the
//! structure tree does not travel, so structure destinations (ISO 32000-2
//! §12.3.2.3) are re-pointed at pages, a GoTo `/SD` (§12.6.4.2) becomes `/D`,
//! `/StructParents` is dropped, and a link to a page left behind loses its
//! destination instead of dragging the page in as a phantom.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::pageops::InsertPosition;
use pdfcer_core::writer::SaveOptions;

fn build(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let max = offsets.iter().map(|(n, _)| *n).max().unwrap_or(0);
    let xref_at = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
    for n in 1..=max {
        match offsets.iter().find(|(k, _)| *k == n) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    buf
}

/// Pages 3, 4, 5. Element 6 marks page 4, element 7 page 3 (through an
/// MCR), element 9 nothing. Page 3 links: 12 → element 6, 13 → GoTo `/SD`
/// element 7, 14 → page 5 directly, 15 → element 9. Page 5 draws `PHANTOM`.
fn tagged() -> Document {
    Document::from_bytes(build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R /StructTreeRoot 10 0 R >>"),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 /MediaBox [0 0 200 200] >>",
        ),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /StructParents 0 /Annots [12 0 R 13 0 R 14 0 R 15 0 R] >>",
        ),
        (4, "<< /Type /Page /Parent 2 0 R >>"),
        (5, "<< /Type /Page /Parent 2 0 R /Contents 20 0 R >>"),
        (6, "<< /Type /StructElem /S /P /P 10 0 R /K 0 /Pg 4 0 R >>"),
        (
            7,
            "<< /Type /StructElem /S /Sect /P 10 0 R /K << /Type /MCR /Pg 3 0 R /MCID 1 >> >>",
        ),
        (9, "<< /Type /StructElem /S /Figure /P 10 0 R /K [] >>"),
        (10, "<< /Type /StructTreeRoot /K [6 0 R 7 0 R 9 0 R] >>"),
        (
            12,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 9 9] /StructParent 1 /Dest [6 0 R /XYZ 0 100 0] >>",
        ),
        (
            13,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 9 9] /A << /S /GoTo /SD [7 0 R /Fit] >> >>",
        ),
        (
            14,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 9 9] /Dest [5 0 R /Fit] >>",
        ),
        (
            15,
            "<< /Type /Annot /Subtype /Link /Rect [0 0 9 9] /Dest [9 0 R /Fit] >>",
        ),
        (20, "<< /Length 9 >>\nstream\n% PHANTOM\nendstream"),
    ]))
    .expect("source parses")
}

fn target() -> Document {
    Document::from_bytes(build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>"),
    ]))
    .expect("target parses")
}

/// The revision the edit appended, as text.
fn appended(session: &EditSession) -> String {
    let base = build(&[]).len();
    let bytes = session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let split = text.find("%%EOF").map_or(base, |i| i + 5);
    text[split..].to_owned()
}

#[test]
fn inserting_from_a_tagged_file_brings_no_tags_and_no_phantom_pages() {
    let src = tagged();
    let mut session = EditSession::new(target());
    let out = session
        .insert_pages(&src.view(), &[0, 1], InsertPosition::End)
        .expect("insert");
    assert_eq!(out.pages_inserted, 2);
    assert!(out.source_tags_dropped);
    assert_eq!(out.structure_links_to_pages, 3, "links 12, 13, 15");
    assert_eq!(out.structure_links_to_first_page, 1, "element 9");
    assert_eq!(out.links_to_pages_left_behind, 1, "link 14 names page 5");

    let text = appended(&session);
    for absent in [
        "/StructElem",
        "/StructTreeRoot",
        "/SD",
        "/StructParent",
        "PHANTOM",
    ] {
        assert!(!text.contains(absent), "{absent} was copied:\n{text}");
    }
    assert_eq!(text.matches("/Type /Page/").count(), 2, "{text}");
    assert_eq!(
        text.matches("/Dest [").count(),
        2,
        "12 and 15 kept, 14 lost:\n{text}"
    );
    assert!(text.contains("/S /GoTo/D ["), "/SD became /D:\n{text}");
}

#[test]
fn inserting_from_an_untagged_file_reports_no_tags_dropped() {
    let src = target();
    let mut session = EditSession::new(target());
    let out = session
        .insert_pages(&src.view(), &[0], InsertPosition::End)
        .expect("insert");
    assert!(!out.source_tags_dropped);
    assert_eq!(out.structure_links_to_pages, 0);
    assert_eq!(out.links_to_pages_left_behind, 0);
}

#[test]
fn merging_a_tagged_file_points_its_structure_links_at_its_pages() {
    let src = tagged();
    let mut session = EditSession::new(target());
    let out = session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");
    assert_eq!(out.pages_merged, 3);
    assert!(out.source_tags_dropped);
    assert_eq!(out.structure_links_to_pages, 3);
    assert_eq!(out.structure_links_to_first_page, 1);
    let text = appended(&session);
    assert!(!text.contains("/StructElem"), "{text}");
    assert!(
        text.contains("PHANTOM"),
        "page 5 was merged, so its content came"
    );
    assert_eq!(text.matches("/Dest [").count(), 3, "{text}");
}

#[test]
fn a_later_insert_recomputes_the_barrier() {
    let src = tagged();
    let mut session = EditSession::new(target());
    session
        .insert_pages(&src.view(), &[0], InsertPosition::End)
        .expect("first insert");
    // Page 5 was behind the first insert's barrier; this one selects it.
    let out = session
        .insert_pages(&src.view(), &[2], InsertPosition::End)
        .expect("second insert");
    assert_eq!(out.links_to_pages_left_behind, 0);
    assert!(appended(&session).contains("PHANTOM"));
}
