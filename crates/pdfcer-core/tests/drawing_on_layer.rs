//! `add_svg_on_layer` / `add_emf_on_layer`: a placed drawing goes on a
//! layer in the same undo entry as the add (`Pass 509.0`, pdfcer-gui G130).

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::ObjId;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

const LAYER: ObjId = ObjId::new(4, 0);
const RECT: Rect = Rect {
    llx: 72.0,
    lly: 72.0,
    urx: 272.0,
    ury: 172.0,
};

/// One page with a stroked path and a registered layer (object 4).
fn session() -> EditSession {
    let content = "q 0 0 m 10 10 l S Q";
    let stream = format!(
        "<< /Length {} >>\nstream\n{content}\nendstream",
        content.len() + 1
    );
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R /OCProperties << /OCGs [4 0 R] /D << /Order [4 0 R] >> >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 5 0 R /Resources << >> >>",
        "<< /Type /OCG /Name (Drawings) >>",
        stream.as_str(),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for o in offsets {
        buf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    EditSession::new(Document::from_bytes(buf).expect("parses"))
}

fn emf() -> pdfcer_core::emf_import::ImportedEmf {
    pdfcer_core::emf_import::import(&super::emf_import::filled_rectangle().finish())
        .expect("imports")
}

/// The layer each page object sits on, in paint order.
fn layers_of(s: &mut EditSession) -> Vec<Option<ObjId>> {
    s.page_objects(0)
        .unwrap()
        .objects
        .iter()
        .map(|o| o.oc())
        .collect()
}

fn assert_on_layer(s: &mut EditSession) {
    assert_eq!(s.undo_depth(), 1, "the layered add is one undo entry");
    let layers = layers_of(s);
    assert_eq!(
        layers.first(),
        Some(&None),
        "the original path is untouched"
    );
    assert_eq!(layers.get(1..), Some(&[Some(LAYER)][..]), "{layers:?}");
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("/OC /OC1 BDC"), "the section survives a save");
    s.undo().unwrap();
    assert_eq!(layers_of(s), vec![None], "one undo removes the drawing");
}

#[test]
fn an_emf_drawing_is_placed_on_the_layer() {
    let mut s = session();
    s.add_emf_on_layer(0, RECT, &emf(), Some(LAYER)).unwrap();
    assert_on_layer(&mut s);
}

fn save(s: &EditSession) -> Vec<u8> {
    s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0
}

#[test]
fn no_layer_is_exactly_the_plain_add() {
    let mut a = session();
    let mut b = session();
    a.add_emf(0, RECT, &emf()).unwrap();
    b.add_emf_on_layer(0, RECT, &emf(), None).unwrap();
    assert_eq!(save(&a), save(&b));
}

#[test]
fn an_unregistered_layer_is_refused_before_any_write() {
    let mut s = session();
    assert!(matches!(
        s.add_emf_on_layer(0, RECT, &emf(), Some(ObjId::new(99, 0))),
        Err(EditError::LayerNotFound { .. })
    ));
    assert_eq!(s.undo_depth(), 0);
}

#[cfg(feature = "svg-import")]
mod svg {
    use super::*;

    fn svg() -> pdfcer_core::svg_import::ImportedSvg {
        pdfcer_core::svg_import::import(
            br#"<svg xmlns="http://www.w3.org/2000/svg" width="20" height="10"><rect width="20" height="10" fill="red"/></svg>"#,
        )
        .expect("imports")
    }

    #[test]
    fn an_svg_drawing_is_placed_on_the_layer() {
        let mut s = session();
        s.add_svg_on_layer(0, RECT, &svg(), Some(LAYER)).unwrap();
        assert_on_layer(&mut s);
    }

    #[test]
    fn no_layer_is_exactly_the_plain_add() {
        let mut a = session();
        let mut b = session();
        a.add_svg(0, RECT, &svg()).unwrap();
        b.add_svg_on_layer(0, RECT, &svg(), None).unwrap();
        assert_eq!(save(&a), save(&b));
    }

    #[test]
    fn an_unregistered_layer_is_refused_before_any_write() {
        let mut s = session();
        assert!(matches!(
            s.add_svg_on_layer(0, RECT, &svg(), Some(ObjId::new(99, 0))),
            Err(EditError::LayerNotFound { .. })
        ));
        assert_eq!(s.undo_depth(), 0);
    }
}
