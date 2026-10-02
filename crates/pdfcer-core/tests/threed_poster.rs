//! A 3D annotation's poster: the default one rendered from a PRC model, the
//! placeholder and why, and replacing it (ISO 32000-1 §13.6.2 Table 298
//! `/AP`). Synthetic documents and models only.

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, MarkupOptions};
use pdfcer_core::image_import::ImportedImage;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::threed::{
    PlaceholderReason, ThreeDEmbedError, ThreeDPoster, ThreeDSpec, extract_3d, list_3d_with_notes,
};
use pdfcer_core::writer::{SaveOptions, save_full};

const RECT: Rect = Rect {
    llx: 20.0,
    lly: 20.0,
    urx: 180.0,
    ury: 120.0,
};
const MODEL: &[u8] = b"PRC\x08\x00\x01";

/// Page 0: a `/3D` annotation (4) over a 6-byte model stream (5) with an
/// empty-form poster (6), a `/Square` (7), and a locked `/3D` (8).
fn doc() -> Document {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R 7 0 R 8 0 R] >>"
            .to_owned(),
        "<< /Type /Annot /Subtype /3D /Rect [0 0 100 50] /3DD 5 0 R /AP << /N 6 0 R >> >>"
            .to_owned(),
        "<< /Type /3D /Subtype /PRC /Length 6 >>\nstream\nPRC\x08\x00\x01\nendstream".to_owned(),
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 50] /Length 0 >>\nstream\n\nendstream"
            .to_owned(),
        "<< /Type /Annot /Subtype /Square /Rect [0 100 50 150] >>".to_owned(),
        "<< /Type /Annot /Subtype /3D /F 128 /Rect [100 0 200 50] /3DD 5 0 R >>".to_owned(),
    ];
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
    Document::from_bytes(buf).expect("synthetic 3D document parses")
}

fn fixture(rel: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel);
    std::fs::read(path).expect("synthetic fixture")
}

fn image() -> ImportedImage {
    pdfcer_core::image_import::import(&fixture("images/rgb8.png")).expect("import")
}

fn saved(session: &EditSession) -> Vec<u8> {
    save_full(
        session.document(),
        &session.dirty_set(),
        &SaveOptions::identity(),
    )
    .expect("full rewrite")
    .0
}

fn dict(doc: &Document, id: ObjId) -> Dict {
    match &doc.get(id).expect("object present").value {
        Object::Dict(d) => d.clone(),
        other => panic!("{id} is not a dictionary: {other:?}"),
    }
}

/// The image XObject `annot`'s `/AP /N` draws as `/Poster`, if it draws one.
fn poster_image(doc: &Document, annot: ObjId) -> Option<ObjId> {
    let Some(Object::Dict(ap)) = dict(doc, annot).get(b"AP").cloned() else {
        return None;
    };
    let Some(Object::Reference(n)) = ap.get(b"N") else {
        return None;
    };
    let Object::Stream(form) = &doc.get(*n)?.value else {
        return None;
    };
    let raw = form.data_span.slice(doc.bytes())?;
    let content = pdfcer_core::filters::decode_stream(&form.dict, raw).ok()?;
    if !content.windows(10).any(|w| w == b"/Poster Do") {
        return None;
    }
    let Some(Object::Dict(res)) = form.dict.get(b"Resources") else {
        return None;
    };
    let Some(Object::Dict(xobjects)) = res.get(b"XObject") else {
        return None;
    };
    let Some(Object::Reference(img)) = xobjects.get(b"Poster") else {
        return None;
    };
    let Object::Stream(s) = &doc.get(*img)?.value else {
        return None;
    };
    (s.dict.get(b"Subtype") == Some(&Object::Name(b"Image".into()))).then_some(*img)
}

#[cfg(feature = "3d")]
#[test]
fn a_prc_model_with_no_poster_gets_a_rendered_one() {
    let mut session = EditSession::new(doc());
    let spec = ThreeDSpec::new(RECT, fixture("prc/square.prc")).expect("PRC sniffs");
    let outcome = session
        .add_3d_annotation(0, &spec, &MarkupOptions::default())
        .expect("embeds");
    let ThreeDPoster::Rendered(rendered) = &outcome.poster else {
        panic!("not rendered: {:?}", outcome.poster);
    };
    assert_eq!((rendered.width, rendered.height), (320, 200), "2 px/pt");
    assert!(rendered.triangles > 0);
    let back = Document::from_bytes(saved(&session)).expect("reloads");
    assert_eq!(
        poster_image(&back, outcome.annot_id),
        outcome.poster_image_id
    );
    assert!(outcome.poster_image_id.is_some());
    assert!(
        !dict(&back, outcome.annot_id).contains_key(b"C"),
        "a rendered poster is not the placeholder pdfcer redraws on resize"
    );
}

#[test]
fn u3d_and_requested_placeholders_say_why() {
    let mut session = EditSession::new(doc());
    let spec = ThreeDSpec::new(RECT, b"U3D\0\x01\x02".to_vec()).expect("U3D sniffs");
    let outcome = session
        .add_3d_annotation(0, &spec, &MarkupOptions::default())
        .expect("embeds");
    assert_eq!(
        outcome.poster,
        ThreeDPoster::Placeholder(PlaceholderReason::NotDecoded {
            format: "U3D".into()
        })
    );
    assert!(outcome.poster_image_id.is_none());

    let mut spec = ThreeDSpec::new(RECT, fixture("prc/square.prc")).expect("PRC sniffs");
    spec.render_poster = false;
    let outcome = session
        .add_3d_annotation(0, &spec, &MarkupOptions::default())
        .expect("embeds");
    assert_eq!(
        outcome.poster,
        ThreeDPoster::Placeholder(PlaceholderReason::Requested)
    );

    let spec = ThreeDSpec::new(RECT, MODEL.to_vec()).expect("PRC sniffs");
    let outcome = session
        .add_3d_annotation(0, &spec, &MarkupOptions::default())
        .expect("embeds");
    let expected = if cfg!(feature = "3d") {
        "the model could not be drawn: "
    } else {
        "this build has no 3D model decoder"
    };
    let ThreeDPoster::Placeholder(why) = &outcome.poster else {
        panic!("{:?}", outcome.poster);
    };
    assert!(why.to_string().starts_with(expected), "{why}");
}

#[test]
fn set_3d_poster_draws_the_image_and_keeps_the_model() {
    let annot = ObjId::new(4, 0);
    let original = doc();
    let art = list_3d_with_notes(&original).0;
    let model = extract_3d(&original.view(), &art[0])
        .expect("extracts")
        .data;
    assert_eq!(model, MODEL);
    assert_eq!(poster_image(&original, annot), None, "an empty form");

    let mut session = EditSession::new(original);
    let outcome = session
        .set_3d_poster(0, annot, &image())
        .expect("replaces the poster");
    assert_eq!(outcome.annot_id, annot);

    let back = Document::from_bytes(saved(&session)).expect("reloads");
    assert_eq!(poster_image(&back, annot), Some(outcome.poster_image_id));
    let art = list_3d_with_notes(&back).0;
    let first = art
        .iter()
        .find(|a| a.annot_id == Some(annot))
        .expect("listed");
    assert!(first.has_poster);
    assert_eq!(
        extract_3d(&back.view(), first).expect("extracts").data,
        MODEL
    );
    assert_eq!(
        dict(&back, annot).get(b"3DD"),
        Some(&Object::Reference(ObjId::new(5, 0))),
        "the 3D stream is still the same object"
    );
}

#[test]
fn one_undo_restores_the_previous_poster_exactly() {
    let annot = ObjId::new(4, 0);
    let mut session = EditSession::new(doc());
    let before = saved(&session);
    let annot_before = session.value(annot).cloned();
    session.set_3d_poster(0, annot, &image()).expect("replaces");
    assert_ne!(session.value(annot).cloned(), annot_before);
    session.undo().expect("one command");
    assert!(!session.can_undo());
    assert_eq!(session.value(annot).cloned(), annot_before);
    assert_eq!(saved(&session), before);
}

/// Undoing a second replacement restores the first, not the base revision.
#[test]
fn undo_of_a_second_poster_restores_the_first() {
    let annot = ObjId::new(4, 0);
    let mut session = EditSession::new(doc());
    session.set_3d_poster(0, annot, &image()).expect("first");
    let first = session.value(annot).cloned();
    session.set_3d_poster(0, annot, &image()).expect("second");
    assert_ne!(session.value(annot).cloned(), first);
    session.undo().expect("one command");
    assert_eq!(session.value(annot).cloned(), first);
}

#[test]
fn set_3d_poster_refuses_what_is_not_a_poster_it_may_replace() {
    let mut session = EditSession::new(doc());
    let img = image();
    assert!(matches!(
        session.set_3d_poster(0, ObjId::new(7, 0), &img),
        Err(EditError::ThreeD(ThreeDEmbedError::NotA3dAnnotation { subtype })) if subtype == "Square"
    ));
    assert!(matches!(
        session.set_3d_poster(0, ObjId::new(8, 0), &img),
        Err(EditError::AnnotationLocked { .. })
    ));
    assert!(matches!(
        session.set_3d_poster(0, ObjId::new(5, 0), &img),
        Err(EditError::AnnotationNotFound { .. })
    ));
    assert!(matches!(
        session.set_3d_poster(1, ObjId::new(4, 0), &img),
        Err(EditError::PageOutOfRange { .. })
    ));
    assert!(!session.can_undo(), "a refusal records nothing");
}
