//! Embedded 3D artwork: listing, extraction and round-trip survival
//! (ISO 32000-1 §13.6, ISO 32000-2 §13.7). Synthetic documents only.

use pdfcer_core::document::Document;
use pdfcer_core::threed::{
    ThreeDError, ThreeDFormat, ThreeDSource, extract_3d, list_3d, list_3d_with_notes,
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
