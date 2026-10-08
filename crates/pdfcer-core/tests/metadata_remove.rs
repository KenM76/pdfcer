//! `metadata_inventory` and `remove_metadata` (Pass 555.0) on a synthetic
//! file carrying one of most kinds of metadata.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::doc_metadata::{
    DocumentIdAction, MetadataItemId, MetadataKind, MetadataRemoveOptions,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::writer::SaveOptions;

fn stream(dict: &str, data: &str) -> String {
    format!(
        "<< {dict} /Length {} >>\nstream\n{data}\nendstream",
        data.len()
    )
}

/// 1 catalog (XMP 4, piece info -> 5, a JavaScript open action), 2 pages,
/// 3 page (thumbnail 6, a comment 7, a link 8 with a script), 9 info.
fn sample() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /Metadata 4 0 R /PieceInfo << /Acme << /Private 5 0 R >> >> /OpenAction << /S /JavaScript /JS (app.alert\\(1\\)) >> >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Thumb 6 0 R /Annots [7 0 R 8 0 R] >>".to_owned(),
        stream("/Type /Metadata /Subtype /XML", "<x:xmpmeta><dc:creator>Secret Author</dc:creator></x:xmpmeta>"),
        "<< /Owner (hidden owner) >>".to_owned(),
        stream("/Width 2 /Height 2 /ColorSpace /DeviceGray /BitsPerComponent 8", "abcd"),
        "<< /Type /Annot /Subtype /Text /Rect [10 10 30 30] /Contents (review note) >>".to_owned(),
        "<< /Type /Annot /Subtype /Link /Rect [0 0 5 5] /A << /S /JavaScript /JS (go\\(\\)) >> >>".to_owned(),
        "<< /Author (Ken Example) /Producer (pdfcer test) >>".to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    let id = "<00112233445566778899aabbccddeeff>";
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {size} /Root 1 0 R /Info 9 0 R /ID [{id} {id}] >>\nstartxref\n{xref_at}\n%%EOF\n"
        )
        .as_bytes(),
    );
    buf
}

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(sample()).expect("parse"))
}

fn ids(session: &EditSession) -> Vec<String> {
    session
        .metadata_inventory()
        .items
        .into_iter()
        .map(|i| i.id.to_string())
        .collect()
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

#[test]
fn inventory_lists_every_carrier_once() {
    let inventory = session().metadata_inventory();
    let got: Vec<(String, MetadataKind)> = inventory
        .items
        .iter()
        .map(|i| (i.id.to_string(), i.kind))
        .collect();
    let want = [
        ("info/Author", MetadataKind::InfoEntry),
        ("info/Producer", MetadataKind::InfoEntry),
        ("xmp/1-0", MetadataKind::DocumentXmp),
        ("pieceinfo/1-0", MetadataKind::PieceInfo),
        ("thumb/3-0", MetadataKind::Thumbnail),
        ("js/1-0/OpenAction", MetadataKind::JavaScript),
        ("js/8-0/A", MetadataKind::JavaScript),
        ("comment/7-0", MetadataKind::Comment),
        ("document-id", MetadataKind::DocumentId),
    ];
    let want: Vec<(String, MetadataKind)> =
        want.iter().map(|(s, k)| ((*s).to_owned(), *k)).collect();
    assert_eq!(got, want);
    assert!(!inventory.truncated);
    let preview = |id: &str| {
        let item = inventory
            .items
            .iter()
            .find(|i| i.id.as_str() == id)
            .unwrap();
        item.preview.clone()
    };
    assert_eq!(preview("info/Author"), "Ken Example");
    assert!(preview("xmp/1-0").contains("Secret Author"));
    assert_eq!(preview("js/1-0/OpenAction"), "app.alert(1)");
    assert_eq!(preview("comment/7-0"), "Text: review note");
    assert_eq!(preview("thumb/3-0"), "2 x 2 image");
}

#[test]
fn removing_everything_leaves_none_of_it_in_a_full_rewrite() {
    let mut s = session();
    let all: Vec<MetadataItemId> = s
        .metadata_inventory()
        .items
        .into_iter()
        .map(|i| i.id)
        .collect();
    let undo_before = s.undo_depth();
    let report = s
        .remove_metadata(&all, &MetadataRemoveOptions::default())
        .unwrap();
    assert_eq!(report.removed.len(), all.len(), "{report:?}");
    assert!(report.not_found.is_empty() && report.not_removed.is_empty());
    assert_eq!(report.objects_freed, 4, "XMP, piece data, thumbnail, info");
    assert!(
        report
            .disclosures
            .iter()
            .any(|d| d.contains("full rewrite"))
    );
    assert_eq!(s.undo_depth(), undo_before + 1, "one undo entry");

    assert_eq!(
        ids(&s),
        ["document-id"],
        "the regenerated identifier remains"
    );
    let (bytes, _) = s.to_full_bytes(&SaveOptions::default()).unwrap();
    for secret in [
        "Secret Author",
        "hidden owner",
        "app.alert",
        "go(",
        "review note",
        "Ken Example",
        "00112233445566778899",
    ] {
        assert!(!contains(&bytes, secret), "{secret} survived the rewrite");
    }
    let back = EditSession::new(Document::from_bytes(bytes).unwrap());
    assert_eq!(ids(&back), ["document-id"]);

    s.undo();
    assert_eq!(
        ids(&s),
        all.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
}

#[test]
fn an_unknown_id_is_reported_and_the_identifier_can_be_dropped() {
    let mut s = session();
    let options = MetadataRemoveOptions::default().with_document_id(DocumentIdAction::Remove);
    let asked = [
        MetadataItemId::new("document-id"),
        MetadataItemId::new("info/Title"),
        MetadataItemId::new("nonsense"),
    ];
    let report = s.remove_metadata(&asked, &options).unwrap();
    assert_eq!(report.removed, [MetadataItemId::new("document-id")]);
    assert_eq!(report.not_found.len(), 2);
    assert!(!ids(&s).contains(&"document-id".to_owned()));
}

#[test]
fn info_emptied_key_by_key_leaves_the_trailer() {
    let mut s = session();
    let asked = [
        MetadataItemId::new("info/Author"),
        MetadataItemId::new("info/Producer"),
    ];
    let report = s
        .remove_metadata(&asked, &MetadataRemoveOptions::default())
        .unwrap();
    assert_eq!(report.removed.len(), 2);
    assert_eq!(report.objects_freed, 1, "the empty info dictionary");
    let (bytes, _) = s.to_full_bytes(&SaveOptions::default()).unwrap();
    assert!(!contains(&bytes, "/Info"));
}

#[test]
fn an_incremental_save_drops_the_info_key_but_keeps_the_old_revision() {
    let mut s = session();
    let asked = [
        MetadataItemId::new("info/Author"),
        MetadataItemId::new("info/Producer"),
    ];
    s.remove_metadata(&asked, &MetadataRemoveOptions::default())
        .unwrap();
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::default()).unwrap();
    let back = EditSession::new(Document::from_bytes(bytes.clone()).unwrap());
    assert!(!ids(&back).iter().any(|i| i.starts_with("info/")));
    assert!(ids(&back).contains(&"revisions".to_owned()));
    assert!(
        contains(&bytes, "Ken Example"),
        "the earlier revision still holds it"
    );
}
