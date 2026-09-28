//! `add_screen_annotation` (`Pass 261.3`, §12.5.6.18, §12.6.4.13, §13.2):
//! the screen and its rendition chain, read back from the saved bytes.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::annot_author::{MediaTempAccess, ScreenSpec, ScreenTrigger};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MarkupNote, MarkupOptions};
use pdfcer_core::object::{Dict, ObjId, Object};
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
    llx: 50.0,
    lly: 100.0,
    urx: 350.0,
    ury: 300.0,
};

const CLIP: &[u8] = b"\0\0\0\x18ftypmp42 not really a movie";

fn dict(doc: &Document, id: ObjId) -> Dict {
    match &doc.get(id).unwrap().value {
        Object::Dict(d) => d.clone(),
        other => panic!("{id:?} is not a dictionary: {other:?}"),
    }
}

fn reference(d: &Dict, key: &[u8]) -> ObjId {
    match d.get(key) {
        Some(Object::Reference(id)) => *id,
        other => panic!(
            "/{} is not a reference: {other:?}",
            String::from_utf8_lossy(key)
        ),
    }
}

fn sub(d: &Dict, key: &[u8]) -> Dict {
    match d.get(key) {
        Some(Object::Dict(d)) => d.clone(),
        other => panic!(
            "/{} is not a dictionary: {other:?}",
            String::from_utf8_lossy(key)
        ),
    }
}

fn name(d: &Dict, key: &[u8]) -> Vec<u8> {
    d.get(key).and_then(Object::as_name).unwrap().0.clone()
}

fn string(d: &Dict, key: &[u8]) -> Vec<u8> {
    match d.get(key) {
        Some(Object::String(s)) => s.clone(),
        other => panic!(
            "/{} is not a string: {other:?}",
            String::from_utf8_lossy(key)
        ),
    }
}

fn saved(s: &EditSession) -> Document {
    Document::from_bytes(s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0).unwrap()
}

#[test]
fn a_screen_carries_the_whole_viable_rendition_chain() {
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let mut spec = ScreenSpec::new(RECT, "walkthrough.mp4", "video/mp4", CLIP.to_vec());
    spec.title = Some("Site walkthrough".to_owned());
    let options = MarkupOptions {
        note: Some(MarkupNote::new("Plays the walkthrough").by("Ken")),
        ..Default::default()
    };
    let id = s.add_screen_annotation(0, &spec, &options).unwrap();
    let doc = saved(&s);

    let annot = dict(&doc, id);
    assert_eq!(name(&annot, b"Subtype"), b"Screen");
    // A screen's /T is its title; the note's author is not written.
    assert_eq!(string(&annot, b"T"), b"Site walkthrough");
    assert_eq!(string(&annot, b"Contents"), b"Plays the walkthrough");
    assert_eq!(reference(&annot, b"P"), ObjId::new(3, 0));
    assert!(matches!(annot.get(b"AP"), Some(Object::Dict(_))));
    assert!(!annot.contains_key(b"AA"));
    let page = dict(&doc, ObjId::new(3, 0));
    assert!(
        matches!(page.get(b"Annots"), Some(Object::Array(a)) if a.contains(&Object::Reference(id)))
    );

    let action = sub(&annot, b"A");
    assert_eq!(name(&action, b"S"), b"Rendition");
    assert!(matches!(action.get(b"OP"), Some(Object::Integer(0))));
    assert_eq!(reference(&action, b"AN"), id);

    let rendition = dict(&doc, reference(&action, b"R"));
    assert_eq!(name(&rendition, b"Type"), b"Rendition");
    assert_eq!(name(&rendition, b"S"), b"MR");
    let clip = dict(&doc, reference(&rendition, b"C"));
    assert_eq!(name(&clip, b"Type"), b"MediaClip");
    assert_eq!(name(&clip, b"S"), b"MCD");
    assert_eq!(string(&clip, b"CT"), b"video/mp4");
    let perms = sub(&clip, b"P");
    assert_eq!(name(&perms, b"Type"), b"MediaPermissions");
    assert_eq!(string(&perms, b"TF"), b"TEMPACCESS");

    // The /D target must carry /Type or the clip is non-viable (§13.2.4.2).
    let filespec = dict(&doc, reference(&clip, b"D"));
    assert_eq!(name(&filespec, b"Type"), b"Filespec");
    assert_eq!(string(&filespec, b"F"), b"walkthrough.mp4");
    let file_id = reference(&sub(&filespec, b"EF"), b"F");
    let Object::Stream(st) = &doc.get(file_id).unwrap().value else {
        panic!("embedded file is not a stream")
    };
    assert_eq!(name(&st.dict, b"Type"), b"EmbeddedFile");
    assert_eq!(name(&st.dict, b"Subtype"), b"video/mp4");
    let raw = st.data_span.slice(doc.bytes()).unwrap();
    assert_eq!(
        pdfcer_core::filters::flate::decode(raw, None).unwrap(),
        CLIP
    );

    // The clip is private to the screen, not a document attachment.
    let catalog = dict(&doc, ObjId::new(1, 0));
    assert!(!catalog.contains_key(b"Names"));
}

#[test]
fn page_open_puts_the_action_in_aa_po_and_the_temp_policy_is_written() {
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let mut spec = ScreenSpec::new(RECT, "tone.mp3", "audio/mpeg", CLIP.to_vec());
    spec.trigger = ScreenTrigger::PageOpen;
    spec.temp_access = MediaTempAccess::Never;
    let id = s
        .add_screen_annotation(0, &spec, &MarkupOptions::default())
        .unwrap();
    let doc = saved(&s);
    let annot = dict(&doc, id);
    assert!(!annot.contains_key(b"A"));
    assert!(!annot.contains_key(b"T"));
    let action = sub(&sub(&annot, b"AA"), b"PO");
    assert_eq!(reference(&action, b"AN"), id);
    let clip = dict(&doc, reference(&dict(&doc, reference(&action, b"R")), b"C"));
    assert_eq!(string(&clip, b"CT"), b"audio/mpeg");
    assert_eq!(string(&sub(&clip, b"P"), b"TF"), b"TEMPNEVER");
}

#[test]
fn one_undo_removes_a_screen_and_its_chain() {
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let before = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let spec = ScreenSpec::new(RECT, "clip.mp4", "video/mp4", CLIP.to_vec());
    s.add_screen_annotation(0, &spec, &MarkupOptions::default())
        .unwrap();
    assert!(s.undo().is_some());
    assert_eq!(
        s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0,
        before
    );
}

#[test]
fn a_missing_page_is_refused() {
    let mut s = EditSession::new(Document::from_bytes(one_page_pdf()).unwrap());
    let spec = ScreenSpec::new(RECT, "clip.mp4", "video/mp4", CLIP.to_vec());
    assert!(matches!(
        s.add_screen_annotation(4, &spec, &MarkupOptions::default()),
        Err(EditError::PageOutOfRange { index: 4, count: 1 })
    ));
}
