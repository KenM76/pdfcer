//! `add_file_attachment_annotation` (`Pass 261.0`, §12.5.6.15): a page-level
//! `/FileAttachment` annotation whose file is private to it, read back through
//! the attachment lister from the saved bytes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::annot_author::{AttachmentIcon, FileAttachmentSpec};
use pdfcer_core::attachments::{AttachmentKind, attachment_bytes, list_attachments};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MarkupNote, MarkupOptions};
use pdfcer_core::object::{Name, ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

fn one_page_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
    );
    buf
}

const RECT: Rect = Rect {
    llx: 72.0,
    lly: 300.0,
    urx: 92.0,
    ury: 324.0,
};

fn session() -> EditSession {
    EditSession::new(Document::from_bytes(one_page_pdf()).unwrap())
}

fn saved(s: &EditSession) -> Document {
    Document::from_bytes(s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0).unwrap()
}

fn dict(doc: &Document, id: ObjId) -> pdfcer_core::object::Dict {
    match &doc.get(id).unwrap().value {
        Object::Dict(d) => d.clone(),
        other => panic!("{id:?} is not a dictionary: {other:?}"),
    }
}

#[test]
fn the_file_is_listed_extractable_and_described_after_save() {
    let mut s = session();
    let mut spec = FileAttachmentSpec::new(RECT, "résumé.txt", b"hello, attachment".to_vec());
    spec.icon = AttachmentIcon::Paperclip;
    let options = MarkupOptions {
        note: Some(MarkupNote::new("the source data").by("Ken")),
        ..Default::default()
    };
    let annot = s
        .add_file_attachment_annotation(0, &spec, &options)
        .unwrap();

    let doc = saved(&s);
    let found = list_attachments(&doc);
    assert_eq!(found.len(), 1, "{found:?}");
    let a = &found[0];
    assert!(
        matches!(a.kind, AttachmentKind::PageAnnotation { .. }),
        "{:?}",
        a.kind
    );
    assert_eq!(a.name, "résumé.txt");
    assert_eq!(a.description.as_deref(), Some("the source data"));
    assert_eq!(
        attachment_bytes(&doc.view(), a).unwrap(),
        b"hello, attachment"
    );

    let d = dict(&doc, annot);
    assert_eq!(
        d.get(b"Subtype").and_then(Object::as_name),
        Some(&Name::from(b"FileAttachment"))
    );
    assert_eq!(
        d.get(b"Name").and_then(Object::as_name),
        Some(&Name::from(b"Paperclip"))
    );
    assert!(matches!(d.get(b"T"), Some(Object::String(t)) if t == b"Ken"));
    assert!(matches!(d.get(b"AP"), Some(Object::Dict(_))));
    let Some(Object::Reference(fs)) = d.get(b"FS") else {
        panic!("/FS is not an indirect reference")
    };
    assert!(dict(&doc, *fs).contains_key(b"EF"));

    // Private to the annotation: no document-level name tree was created.
    let root = match doc.trailer().get(b"Root") {
        Some(Object::Reference(r)) => *r,
        other => panic!("{other:?}"),
    };
    assert!(!dict(&doc, root).contains_key(b"Names"));
}

#[test]
fn the_default_icon_is_push_pin_and_an_unmodelled_name_is_written_verbatim() {
    let mut s = session();
    let a = s
        .add_file_attachment_annotation(
            0,
            &FileAttachmentSpec::new(RECT, "a.bin", vec![0, 1, 2]),
            &MarkupOptions::default(),
        )
        .unwrap();
    let mut spec = FileAttachmentSpec::new(RECT, "b.bin", vec![3]);
    spec.icon = AttachmentIcon::from_name_lossless(b"Sparkle");
    let b = s
        .add_file_attachment_annotation(0, &spec, &MarkupOptions::default())
        .unwrap();
    let doc = saved(&s);
    let name = |id| {
        dict(&doc, id)
            .get(b"Name")
            .and_then(Object::as_name)
            .cloned()
    };
    assert_eq!(name(a), Some(Name::from(b"PushPin")));
    assert_eq!(name(b), Some(Name::from(b"Sparkle")));
    assert!(!dict(&doc, a).contains_key(b"Contents"));
}

#[test]
fn one_undo_removes_the_annotation_and_its_file() {
    let mut s = session();
    let before = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    s.add_file_attachment_annotation(
        0,
        &FileAttachmentSpec::new(RECT, "a.txt", b"x".to_vec()),
        &MarkupOptions::default(),
    )
    .unwrap();
    assert!(s.undo().is_some());
    assert_eq!(
        s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0,
        before
    );
    assert!(list_attachments(&saved(&s)).is_empty());
}

#[test]
fn a_page_out_of_range_is_refused_by_name() {
    let mut s = session();
    let err = s
        .add_file_attachment_annotation(
            3,
            &FileAttachmentSpec::new(RECT, "a.txt", Vec::new()),
            &MarkupOptions::default(),
        )
        .unwrap_err();
    assert!(
        matches!(err, EditError::PageOutOfRange { index: 3, count: 1 }),
        "{err:?}"
    );
}

/// The shared filespec helper writes `/UF` as a text string, so a
/// document-level attachment with a non-ASCII name reads back as written.
#[test]
fn a_document_level_non_ascii_name_reads_back_as_written() {
    let mut s = session();
    s.attach_file("résumé.txt", b"x", Some("Ø 12,5 mm"))
        .unwrap();
    let found = list_attachments(&saved(&s));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "résumé.txt");
    assert_eq!(found[0].description.as_deref(), Some("Ø 12,5 mm"));
}
