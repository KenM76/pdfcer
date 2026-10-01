//! Embedded 3D artwork: listing, extraction and round-trip survival
//! (ISO 32000-1 §13.6, ISO 32000-2 §13.7). Synthetic documents only.

use pdfcer_core::document::Document;
use pdfcer_core::threed::{
    OrthoBinding, ThreeDError, ThreeDFormat, ThreeDSource, default_3d_view, extract_3d, list_3d,
    list_3d_with_notes,
};

/// The U3D stream body (object 7), kept verbatim so the round-trip test can
/// look for it byte-for-byte in a rewritten file.
const U3D_OBJ: &str = "<< /Type /3D /Subtype /U3D /Filter /ASCIIHexDecode \
     /VA [<< /Type /3DView /XN (Front) >> << /Type /3DView /XN (Back) >>] /Length 15 >>\n\
     stream\n55334400AABBCC>\nendstream";
const PRC_ASSET_OBJ: &str = "<< /Type /EmbeddedFile /Subtype /model#2Fprc /Length 6 >>\n\
     stream\nPRC\x01\x02\x03\nendstream";

/// Page 0: a `/3D` annotation with a poster, a `/3DRef` to the same stream,
/// and a `/3D` annotation whose `/3DD` dangles. Page 1: a RichMedia
/// annotation naming one PRC asset twice, and a `/3D` stream that declares
/// U3D but carries PRC bytes.
fn three_d_pdf() -> Vec<u8> {
    let bodies: Vec<String> = vec![
        "<< /Type /Catalog /Pages 2 0 R >>".into(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [5 0 R 6 0 R 12 0 R] >>"
            .into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [9 0 R 13 0 R] >>".into(),
        "<< /Type /Annot /Subtype /3D /Rect [0 0 100 100] /3DD 7 0 R /AP << /N 8 0 R >> >>".into(),
        "<< /Type /Annot /Subtype /3D /Rect [100 0 200 100] /3DD << /Type /3DRef /3D 7 0 R >> >>"
            .into(),
        U3D_OBJ.into(),
        "<< /Type /XObject /Subtype /Form /BBox [0 0 100 100] /Length 0 >>\nstream\n\nendstream"
            .into(),
        "<< /Type /Annot /Subtype /RichMedia /Rect [0 0 100 100] /RichMediaContent \
         << /Configurations [<< /Type /RichMediaConfiguration /Subtype /3D /Instances \
         [<< /Subtype /3D /Asset 10 0 R >> << /Subtype /3D /Asset 10 0 R >>] >>] >> >>"
            .into(),
        "<< /Type /Filespec /F (part.prc) /UF (part.prc) /EF << /F 11 0 R >> >>".into(),
        PRC_ASSET_OBJ.into(),
        "<< /Type /Annot /Subtype /3D /Rect [0 100 100 200] /3DD 99 0 R >>".into(),
        "<< /Type /Annot /Subtype /3D /Rect [100 100 200 200] /3DD 14 0 R >>".into(),
        "<< /Type /3D /Subtype /U3D /Length 4 >>\nstream\nPRC!\nendstream".into(),
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
    buf
}

fn load() -> Document {
    Document::from_bytes(three_d_pdf()).expect("synthetic 3D document parses")
}

#[test]
fn both_routes_are_listed_once_each_and_a_dangling_one_is_counted() {
    let doc = load();
    let (found, notes) = list_3d_with_notes(&doc);
    assert_eq!(found.len(), 4, "{found:#?}");
    assert_eq!(notes.annotations_without_stream, 1);
    assert!(!notes.truncated && !notes.page_tree_unwalkable);

    let direct = &found[0];
    assert_eq!(direct.page_index, 0);
    assert_eq!(direct.source, ThreeDSource::Stream { shared: false });
    assert_eq!(direct.declared, Some(ThreeDFormat::U3d));
    assert_eq!(direct.view_count, 2);
    assert!(direct.has_poster);

    let shared = &found[1];
    assert_eq!(shared.source, ThreeDSource::Stream { shared: true });
    assert_eq!(
        shared.stream_id, direct.stream_id,
        "a /3DRef names the same stream"
    );
    assert!(!shared.has_poster);

    let asset = &found[2];
    assert_eq!(asset.page_index, 1);
    assert_eq!(asset.declared, Some(ThreeDFormat::Prc));
    let ThreeDSource::RichMediaAsset { name, filespec_id } = &asset.source else {
        panic!("expected a RichMedia asset, got {:?}", asset.source);
    };
    assert_eq!(name.as_deref(), Some("part.prc"));
    assert!(filespec_id.is_some());
    assert_eq!(asset.view_count, 0);
}

#[test]
fn extraction_decodes_the_filter_and_sniffs_the_bytes() {
    let doc = load();
    let view = doc.view();
    let found = list_3d(&doc);

    let u3d = extract_3d(&view, &found[0]).expect("U3D stream extracts");
    assert_eq!(u3d.data, b"U3D\0\xAA\xBB\xCC");
    assert_eq!(u3d.sniffed, Some(ThreeDFormat::U3d));
    assert!(!u3d.contradicts(found[0].declared.as_ref()));

    let prc = extract_3d(&view, &found[2]).expect("PRC asset extracts");
    assert_eq!(prc.data, b"PRC\x01\x02\x03");
    assert!(!prc.contradicts(found[2].declared.as_ref()));

    let lying = extract_3d(&view, &found[3]).expect("mislabelled stream still extracts");
    assert_eq!(lying.sniffed, Some(ThreeDFormat::Prc));
    assert!(lying.contradicts(found[3].declared.as_ref()));
}

#[test]
fn an_artwork_without_a_stream_is_refused() {
    let doc = load();
    let mut art = list_3d(&doc).remove(0);
    art.stream_id = None;
    assert_eq!(extract_3d(&doc.view(), &art), Err(ThreeDError::NoStream));
}

/// ARCHITECTURE §5: an unrelated edit rewrites the file and leaves every 3D
/// object byte-identical, and the model is still listed and extractable.
#[test]
fn an_unrelated_edit_leaves_the_3d_objects_byte_identical() {
    use pdfcer_core::edit::{EditSession, InfoField};
    use pdfcer_core::writer::{SaveOptions, save_full};

    let mut session = EditSession::new(load());
    session
        .set_info_field(InfoField::Title, Some("unrelated"))
        .expect("set title");
    let (bytes, _) = save_full(
        session.document(),
        &session.dirty_set(),
        &SaveOptions::identity(),
    )
    .expect("full rewrite");
    let contains = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
    assert!(contains(U3D_OBJ.as_bytes()), "U3D stream object changed");
    assert!(
        contains(PRC_ASSET_OBJ.as_bytes()),
        "PRC asset object changed"
    );

    let back = Document::from_bytes(bytes).expect("reloads");
    let found = list_3d(&back);
    assert_eq!(found.len(), 4);
    let view = back.view();
    assert_eq!(
        extract_3d(&view, &found[0]).expect("extracts").data,
        b"U3D\0\xAA\xBB\xCC"
    );
}

mod embed {
    use super::{PRC_ASSET_OBJ, U3D_OBJ, load};
    use pdfcer_core::document::Document;
    use pdfcer_core::edit::{EditError, EditSession, MarkupOptions};
    use pdfcer_core::object::{ObjId, Object};
    use pdfcer_core::page_tree::Rect;
    use pdfcer_core::threed::{
        ThreeDActivation, ThreeDEmbedError, ThreeDFormat, ThreeDSource, ThreeDSpec, extract_3d,
        list_3d,
    };
    use pdfcer_core::writer::{SaveOptions, save_full};

    const RECT: Rect = Rect {
        llx: 20.0,
        lly: 20.0,
        urx: 180.0,
        ury: 120.0,
    };
    const PRC: &[u8] = b"PRC\x08\x00\x01\x02\x03\x04";

    fn saved(session: &EditSession) -> Vec<u8> {
        save_full(
            session.document(),
            &session.dirty_set(),
            &SaveOptions::identity(),
        )
        .expect("full rewrite")
        .0
    }

    fn dict(doc: &Document, id: ObjId) -> pdfcer_core::object::Dict {
        match &doc.get(id).expect("object present").value {
            Object::Dict(d) => d.clone(),
            other => panic!("{id} is not a dictionary: {other:?}"),
        }
    }

    fn tiny_image() -> pdfcer_core::image_import::ImportedImage {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/images/rgb8.png");
        pdfcer_core::image_import::import(&std::fs::read(path).expect("read")).expect("import")
    }

    /// Embed, save, reload: the new model lists as a direct stream with a
    /// poster, extracts to the original bytes, and every pre-existing 3D
    /// object is byte-identical (ARCHITECTURE §5).
    #[test]
    fn an_embedded_prc_lists_extracts_and_leaves_the_rest_byte_identical() {
        let mut session = EditSession::new(load());
        let mut spec = ThreeDSpec::new(RECT, PRC.to_vec()).expect("PRC sniffs");
        spec.activation = ThreeDActivation::PageOpen;
        let outcome = session
            .add_3d_annotation(1, &spec, &MarkupOptions::default())
            .expect("embeds");
        assert!(outcome.poster_image_id.is_none());
        assert!(outcome.below_required_version(), "1.7 < 2.0 for PRC");

        let bytes = saved(&session);
        let contains = |needle: &[u8]| bytes.windows(needle.len()).any(|w| w == needle);
        assert!(contains(U3D_OBJ.as_bytes()), "U3D stream object changed");
        assert!(contains(PRC_ASSET_OBJ.as_bytes()), "PRC asset changed");

        let back = Document::from_bytes(bytes).expect("reloads");
        let found = list_3d(&back);
        let new = found
            .iter()
            .find(|a| a.annot_id == Some(outcome.annot_id))
            .expect("new model listed");
        assert_eq!(new.page_index, 1);
        assert_eq!(new.source, ThreeDSource::Stream { shared: false });
        assert_eq!(new.declared, Some(ThreeDFormat::Prc));
        assert!(new.has_poster);
        assert_eq!(extract_3d(&back.view(), new).expect("extracts").data, PRC);

        let annot = dict(&back, outcome.annot_id);
        let Some(Object::Dict(activation)) = annot.get(b"3DA") else {
            panic!("no /3DA: {annot:?}");
        };
        assert_eq!(activation.get(b"A"), Some(&Object::Name(b"PO".into())));
    }

    #[test]
    fn a_supplied_poster_becomes_the_appearance_image() {
        let mut session = EditSession::new(load());
        let mut spec = ThreeDSpec::new(RECT, b"U3D\0\x01\x02".to_vec()).expect("U3D sniffs");
        spec.poster = Some(tiny_image());
        let outcome = session
            .add_3d_annotation(0, &spec, &MarkupOptions::default())
            .expect("embeds");
        let image_id = outcome.poster_image_id.expect("poster image written");
        session
            .resize_annotation(
                outcome.annot_id,
                (20.0, 20.0),
                1.0,
                2.0,
                &pdfcer_core::edit::ResizeOptions::default(),
            )
            .expect("an image poster re-fits in the new box");
        assert!(!outcome.below_required_version(), "1.7 >= 1.6 for U3D");

        let back = Document::from_bytes(saved(&session)).expect("reloads");
        let image = back.get(image_id).expect("image object");
        let Object::Stream(s) = &image.value else {
            panic!("poster is not a stream");
        };
        assert_eq!(s.dict.get(b"Subtype"), Some(&Object::Name(b"Image".into())));
    }

    #[test]
    fn step_mismatched_and_empty_models_are_refused() {
        assert_eq!(
            ThreeDSpec::with_format(RECT, ThreeDFormat::U3d, PRC.to_vec()),
            Err(ThreeDEmbedError::Mismatch {
                stated: "U3D".into(),
                sniffed: "PRC".into()
            })
        );
        assert_eq!(
            ThreeDSpec::new(RECT, Vec::new()),
            Err(ThreeDEmbedError::Empty)
        );
        assert!(matches!(
            ThreeDSpec::new(RECT, b"ISO-10303-21;\nHEADER;".to_vec()),
            Err(ThreeDEmbedError::NotEmbeddable { .. })
        ));
        assert!(
            ThreeDSpec::with_format(RECT, ThreeDFormat::Prc, b"opaque".to_vec()).is_ok(),
            "a stated format accepts unsigned bytes"
        );

        // The session re-validates: a spec mutated after construction.
        let mut spec = ThreeDSpec::new(RECT, PRC.to_vec()).expect("sniffs");
        spec.format = ThreeDFormat::U3d;
        let mut session = EditSession::new(load());
        assert!(matches!(
            session.add_3d_annotation(0, &spec, &MarkupOptions::default()),
            Err(EditError::ThreeD(ThreeDEmbedError::Mismatch { .. }))
        ));
        assert!(!session.can_undo(), "a refusal records nothing");
    }

    #[test]
    fn one_undo_removes_the_whole_embed() {
        let mut session = EditSession::new(load());
        let before = list_3d(&session.view()).len();
        let spec = ThreeDSpec::new(RECT, PRC.to_vec()).expect("sniffs");
        session
            .add_3d_annotation(0, &spec, &MarkupOptions::default())
            .expect("embeds");
        assert_eq!(list_3d(&session.view()).len(), before + 1);
        session.undo().expect("one command");
        assert_eq!(list_3d(&session.view()).len(), before);
        assert!(!session.can_undo());
    }
    /// The generic annotation verbs reach a 3D annotation: move, resize and
    /// copy/paste each succeed and the result still lists as a 3D model.
    #[test]
    fn move_resize_and_copy_paste_reach_a_3d_annotation() {
        use pdfcer_core::edit::ResizeOptions;
        use pdfcer_core::vector::Matrix;
        let mut session = EditSession::new(load());
        let spec = ThreeDSpec::new(RECT, PRC.to_vec()).expect("sniffs");
        let id = session
            .add_3d_annotation(1, &spec, &MarkupOptions::default())
            .expect("embeds")
            .annot_id;
        session.move_annotation(id, 5.0, 5.0).expect("moves");
        // pdfcer drew the placeholder, so a resize redraws it at the new
        // size, non-uniform included, and recognises its own redraw again.
        session
            .resize_annotation(id, (20.0, 20.0), 0.5, 1.5, &ResizeOptions::default())
            .expect("resizes");
        session
            .resize_annotation(id, (20.0, 20.0), 2.0, 2.0, &ResizeOptions::default())
            .expect("resizes again");
        // Another producer's poster (the fixture's empty form) is not
        // pdfcer's to redraw.
        assert!(matches!(
            session.resize_annotation(
                ObjId::new(5, 0),
                (0.0, 0.0),
                2.0,
                1.0,
                &ResizeOptions::default()
            ),
            Err(EditError::ResizeAppearanceNotRebuildable { .. })
        ));
        let annots = pdfcer_core::annot::page_annotations(&session.view(), ObjId::new(4, 0));
        let at = annots
            .iter()
            .position(|a| a.id == Some(id))
            .expect("still on page");
        let clip = session.copy_annotations(1, &[at]).expect("copies");
        session
            .paste_objects(0, &clip, Matrix::IDENTITY)
            .expect("pastes");
        let found = list_3d(&session.view());
        assert_eq!(found.len(), 6, "{found:#?}");
        assert!(
            found.iter().any(|a| a.page_index == 0
                && a.annot_id != Some(id)
                && a.declared == Some(ThreeDFormat::Prc)),
            "the pasted copy is a PRC model on page 0"
        );
    }
}

/// One page, one `/3D` annotation with `/3DV` set to `choice` (omitted when
/// empty), over a stream whose `/VA` holds "Front" (perspective, no
/// matrix) and "Iso" (orthographic, `/IN (iso)`, a C2W), plus `stream_extra`.
fn view_doc(choice: &str, stream_extra: &str) -> Document {
    let dv = if choice.is_empty() {
        String::new()
    } else {
        format!("/3DV {choice}")
    };
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Annots [4 0 R] >>".to_owned(),
        format!("<< /Type /Annot /Subtype /3D /Rect [0 0 100 100] /3DD 5 0 R {dv} >>"),
        format!(
            "<< /Type /3D /Subtype /PRC {stream_extra} /VA [\
             << /Type /3DView /XN (Front) /MS /M /C2W [1 0 0] >> \
             << /Type /3DView /XN (Iso) /IN (iso) /MS /M /CO 50 /P << /Subtype /O >> \
             /C2W [1 0 0 0 1 0 0 0 1 10 20 30] >>] /Length 3 >>\nstream\nPRC\nendstream"
        ),
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
    Document::from_bytes(buf).expect("synthetic 3D view document parses")
}

fn opening_view(choice: &str, stream_extra: &str) -> Option<String> {
    let doc = view_doc(choice, stream_extra);
    let art = &list_3d(&doc)[0];
    default_3d_view(&doc, art).map(|v| v.name)
}

#[test]
fn the_opening_view_follows_every_selector_form() {
    let iso = Some("Iso".to_owned());
    let front = Some("Front".to_owned());
    assert_eq!(opening_view("", ""), front, "no selector: /VA[0]");
    assert_eq!(opening_view("", "/DV 1"), iso, "stream /DV index");
    assert_eq!(opening_view("", "/DV /L"), iso, "stream /DV /L");
    assert_eq!(opening_view("1", ""), iso, "annotation index");
    assert_eq!(opening_view("(iso)", ""), iso, "annotation /IN name");
    assert_eq!(opening_view("(Front)", ""), front, "/IN defaults to /XN");
    assert_eq!(opening_view("/L", ""), iso);
    assert_eq!(opening_view("/F", "/DV 1"), front);
    assert_eq!(opening_view("/D", "/DV 1"), iso, "/D defers to the stream");
    assert_eq!(
        opening_view("<< /Type /3DView /XN (Own) >>", ""),
        Some("Own".to_owned())
    );
    assert_eq!(
        opening_view("7", ""),
        None,
        "an index past /VA names nothing"
    );
    assert_eq!(opening_view("(nope)", ""), None);
}

#[test]
fn the_opening_view_carries_its_camera() {
    let doc = view_doc("1", "");
    let view = default_3d_view(&doc, &list_3d(&doc)[0]).expect("a view");
    assert_eq!(
        view.camera_to_world,
        Some([
            1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 10.0, 20.0, 30.0
        ])
    );
    assert_eq!(view.orbit_distance, Some(50.0));
    assert!(view.orthographic);
    assert_eq!(view.ortho_scale, 1.0, "/OS defaults to 1");
    assert_eq!(view.ortho_binding, OrthoBinding::Absolute, "/OB defaults");
    assert_eq!(view.view_box, Some([100.0, 100.0]), "no /3DB: the /Rect");

    let doc = view_doc("<< /XN (Own) /P << /Subtype /O /OS 0.25 /OB /Max >> >>", "");
    let own = default_3d_view(&doc, &list_3d(&doc)[0]).expect("a view");
    assert_eq!(own.ortho_scale, 0.25);
    assert_eq!(own.ortho_binding, OrthoBinding::Max);

    let doc = view_doc("0", "");
    let front = default_3d_view(&doc, &list_3d(&doc)[0]).expect("a view");
    assert_eq!(
        front.camera_to_world, None,
        "a 3-number C2W is not a matrix"
    );
    assert!(!front.orthographic, "absent /P is perspective");
}
