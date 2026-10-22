#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use super::*;

/// A classic-xref file from object bodies (object `i + 1` is `bodies[i]`),
/// optionally followed by an incremental update replacing object numbers.
fn build(bodies: &[Vec<u8>], update: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {size} /Root 1 0 R /Info 9 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
        )
        .as_bytes(),
    );
    for &(num, body) in update {
        let at = buf.len();
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        let xref2 = buf.len();
        buf.extend_from_slice(
            format!(
                "xref\n{num} 1\n{at:010} 00000 n\r\ntrailer\n<< /Size {size} /Root 1 0 R /Info 9 0 R /Prev {xref_at} >>\nstartxref\n{xref2}\n%%EOF\n"
            )
            .as_bytes(),
        );
    }
    buf
}

/// A stream object body with `dict` entries and an exact `/Length`.
pub(crate) fn stream(dict: &str, data: &[u8]) -> Vec<u8> {
    let mut out = format!("<< {dict} /Length {} >>\nstream\n", data.len()).into_bytes();
    out.extend_from_slice(data);
    out.extend_from_slice(b"\nendstream");
    out
}

/// Page with an image drawn at 144 x 72 pt, a font, a link, an outline,
/// metadata, and one unreferenced object.
fn sample(update: &[(u32, &str)]) -> Document {
    let content = b"q 144 0 0 72 10 10 cm /Im1 Do Q BT /F1 12 Tf (Hi) Tj ET";
    let bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R /Outlines 7 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
           /Resources << /XObject << /Im1 5 0 R >> /Font << /F1 6 0 R >> >> /Annots [8 0 R] >>"
            .to_vec(),
        stream("", content),
        stream(
            "/Type /XObject /Subtype /Image /Width 300 /Height 150 /ColorSpace /DeviceGray /BitsPerComponent 8",
            &[0x80; 300 * 150],
        ),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
        b"<< /Type /Outlines /Count 0 >>".to_vec(),
        b"<< /Type /Annot /Subtype /Link /Rect [0 0 10 10] /Border [0 0 0] >>".to_vec(),
        b"<< /Producer (synthetic) >>".to_vec(),
        b"<< /Orphan true >>".to_vec(),
    ];
    Document::from_bytes(build(&bodies, update)).unwrap()
}

fn assert_sums(report: &SpaceAudit) {
    let total: u64 = report.categories.iter().map(|c| c.bytes).sum();
    assert_eq!(total, report.file_bytes, "{:#?}", report.categories);
    assert_eq!(report.categories.len(), SpaceCategory::ALL.len());
}

fn role(report: &SpaceAudit, num: u32) -> SpaceCategory {
    report
        .objects
        .iter()
        .find(|o| o.id.num == num)
        .unwrap()
        .category
}

#[test]
fn every_byte_is_attributed_exactly_once() {
    let report = audit(&sample(&[]));
    assert_sums(&report);
    assert!(report.bytes_in(SpaceCategory::Images) > 45_000);
    assert!(report.bytes_in(SpaceCategory::CrossReference) > 0);
    assert!(report.bytes_in(SpaceCategory::FileOverhead) > 0);
    assert_eq!(report.bytes_in(SpaceCategory::Superseded), 0);
    assert_eq!(report.objects[0].id.num, 5, "largest object first");
}

#[test]
fn objects_take_their_role() {
    let report = audit(&sample(&[]));
    let expected = [
        (1, SpaceCategory::PageStructure),
        (2, SpaceCategory::PageStructure),
        (3, SpaceCategory::PageStructure),
        (4, SpaceCategory::ContentStreams),
        (5, SpaceCategory::Images),
        (6, SpaceCategory::Fonts),
        (7, SpaceCategory::Bookmarks),
        (8, SpaceCategory::Links),
        (9, SpaceCategory::Metadata),
        (10, SpaceCategory::Unreferenced),
    ];
    for (num, category) in expected {
        assert_eq!(role(&report, num), category, "object {num}");
    }
}

#[test]
fn objects_list_the_pages_that_use_them() {
    let report = audit(&sample(&[]));
    let pages = |num: u32| {
        report
            .objects
            .iter()
            .find(|o| o.id.num == num)
            .unwrap()
            .pages
            .clone()
    };
    for num in [3, 4, 5, 6, 8] {
        assert_eq!(pages(num), vec![1], "object {num}");
    }
    for num in [1, 2, 7, 9, 10] {
        assert!(pages(num).is_empty(), "object {num}");
    }
}

#[test]
fn image_placement_gives_effective_ppi() {
    let report = audit(&sample(&[]));
    let image = report.objects.iter().find(|o| o.id.num == 5).unwrap();
    let facts = image.image.as_ref().unwrap();
    assert_eq!((facts.width, facts.height), (300, 150));
    assert_eq!(facts.color_space.as_deref(), Some("DeviceGray"));
    let placement = facts.placements[0];
    assert_eq!(placement.page, 1);
    assert!((placement.width_pt - 144.0).abs() < 1e-9);
    assert!((placement.ppi_x - 150.0).abs() < 1e-9);
    assert!((placement.ppi_y - 150.0).abs() < 1e-9);
    assert_eq!(facts.binding_ppi(), Some(150.0));
    assert_eq!((facts.components, facts.image_mask), (Some(1), false));
    assert_eq!(facts.class(), crate::space_audit::ImageClass::Gray);
}

#[test]
fn an_incremental_update_leaves_superseded_bytes() {
    let report = audit(&sample(&[(
        7,
        "<< /Type /Outlines /Count 0 /Edited true >>",
    )]));
    assert_sums(&report);
    let old_body = "7 0 obj\n<< /Type /Outlines /Count 0 >>\nendobj\n".len() as u64;
    assert_eq!(report.bytes_in(SpaceCategory::Superseded), old_body - 1);
    assert_eq!(role(&report, 7), SpaceCategory::Bookmarks);
}

#[test]
fn diff_compares_category_by_category() {
    let before = audit(&sample(&[]));
    let after = audit(&sample(&[(
        7,
        "<< /Type /Outlines /Count 0 /Edited true >>",
    )]));
    let d = diff(&before, &after);
    assert!(d.after_bytes > d.before_bytes);
    let sup = d
        .categories
        .iter()
        .find(|c| c.category == SpaceCategory::Superseded)
        .unwrap();
    assert_eq!(sup.before, 0);
    assert!(sup.after > 0);
}

/// Catalog and pages compressed in an object stream, indexed by an
/// uncompressed cross-reference stream.
fn object_stream_doc() -> Vec<u8> {
    let o1 = "<< /Type /Catalog /Pages 2 0 R >>";
    let o2 =
        "<< /Type /Pages /Kids [] /Count 0 /Padding (xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx) >>";
    let table = format!("1 0 2 {} ", o1.len() + 1);
    let data = format!("{table}{o1}\n{o2}");
    let mut buf = b"%PDF-1.7\n".to_vec();
    let at3 = buf.len();
    buf.extend_from_slice(b"3 0 obj\n");
    buf.extend_from_slice(&stream(
        &format!("/Type /ObjStm /N 2 /First {}", table.len()),
        data.as_bytes(),
    ));
    buf.extend_from_slice(b"\nendobj\n");
    let at4 = buf.len();
    let mut rows = Vec::new();
    let row = |t: u8, f2: u32, f3: u16| {
        let mut r = vec![t];
        r.extend_from_slice(&f2.to_be_bytes());
        r.extend_from_slice(&f3.to_be_bytes());
        r
    };
    rows.extend(row(0, 0, 0xFFFF));
    rows.extend(row(2, 3, 0));
    rows.extend(row(2, 3, 1));
    rows.extend(row(1, u32::try_from(at3).unwrap(), 0));
    rows.extend(row(1, u32::try_from(at4).unwrap(), 0));
    buf.extend_from_slice(b"4 0 obj\n");
    buf.extend_from_slice(&stream("/Type /XRef /Size 5 /W [1 4 2] /Root 1 0 R", &rows));
    buf.extend_from_slice(format!("\nendobj\nstartxref\n{at4}\n%%EOF\n").as_bytes());
    buf
}

#[test]
fn an_object_stream_is_shared_among_its_members() {
    let doc = Document::from_bytes(object_stream_doc()).unwrap();
    let report = audit(&doc);
    assert_sums(&report);
    assert!(
        report.objects.iter().all(|o| o.id.num != 3),
        "container is shared out"
    );
    let catalog = report.objects.iter().find(|o| o.id.num == 1).unwrap();
    let pages = report.objects.iter().find(|o| o.id.num == 2).unwrap();
    assert_eq!(catalog.in_object_stream, Some(3));
    assert!(
        pages.stored_bytes > catalog.stored_bytes,
        "shares follow decoded length"
    );
    assert_eq!(
        report.bytes_in(SpaceCategory::FileOverhead),
        10,
        "header + one EOL"
    );
    assert!(report.bytes_in(SpaceCategory::CrossReference) > 0);
}

#[test]
fn linearization_dictionary_and_hint_stream_are_file_overhead() {
    let bodies = |h: usize| {
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] >>".to_vec(),
            format!("<< /Linearized 1 /H [ {h:010} 40 ] >>").into_bytes(),
            b"<< /S 0 /Length 6 0 R >>\nstream\nabc\nendstream".to_vec(),
            b"3".to_vec(),
        ]
    };
    let draft = build(&bodies(0), &[]);
    let hint_at = draft.windows(7).position(|w| w == b"5 0 obj").unwrap();
    let report = audit(&Document::from_bytes(build(&bodies(hint_at), &[])).unwrap());
    for num in [4, 5, 6] {
        assert_eq!(
            role(&report, num),
            SpaceCategory::FileOverhead,
            "object {num}"
        );
    }
    assert_eq!(report.bytes_in(SpaceCategory::Unreferenced), 0);
    assert_sums(&report);
}
