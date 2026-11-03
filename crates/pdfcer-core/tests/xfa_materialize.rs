//! `EditSession::materialize_xfa`: a dynamic XFA form's computed pages
//! written into the file, the XFA layer removed, saved and reopened, and
//! undone (Pass 647.6).

#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use pdfcer_core::document::Document;
use pdfcer_core::edit::{CommandKind, EditError, EditSession, XfaSignatures};
use pdfcer_core::filters::decode_stream;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::pages;
use pdfcer_core::writer::SaveOptions;
use pdfcer_core::xfa::{
    LayoutOptions, MergeOptions, MergedForm, PaintOptions, XfaFormKind, XfaLayout, form_kind,
    layout_template, merge_data, read_template, read_xdp,
};

/// Two pages: `a` fills most of the first, `b` (showing `v`) the second.
const XDP: &str = r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="form1" layout="tb"><pageSet><pageArea name="P"><contentArea x="36pt" y="36pt" w="540pt" h="720pt"/><medium short="612pt" long="792pt"/></pageArea></pageSet><draw name="t" w="200pt" h="500pt"><value><text>Hello</text></value></draw><field name="b" w="200pt" h="400pt"/></subform></template><xfa:datasets xmlns:xfa="http://www.xfa.org/schema/xfa-data/1.0/"><xfa:data><form1><b>Ken</b></form1></xfa:data></xfa:datasets></xdp:xdp>"#;

/// A one-page PDF carrying `XDP` with `catalog` extras and `extra` objects
/// from 6 on. Object 4 is the XDP stream, 5 a placeholder field with
/// `field` extras.
fn pdf(catalog: &str, field: &str, extra: &[&str]) -> Vec<u8> {
    pdf_with(XDP, catalog, field, extra)
}

/// [`pdf`] carrying `xdp`.
pub(crate) fn pdf_with(xdp: &str, catalog: &str, field: &str, extra: &[&str]) -> Vec<u8> {
    let mut objects: Vec<Vec<u8>> = vec![
        format!(
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] /XFA 4 0 R >> {catalog} >>"
        )
        .into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 10 10] >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>".to_vec(),
        format!("<< /Length {} >>\nstream\n{xdp}\nendstream", xdp.len()).into_bytes(),
        format!("<< /FT /Tx /T (b) /Subtype /Widget /Rect [0 0 1 1] /P 3 0 R {field} >>")
            .into_bytes(),
    ];
    objects.extend(extra.iter().map(|e| e.as_bytes().to_vec()));
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    let n = objects.len() + 1;
    out.extend_from_slice(format!("xref\n0 {n}\n0000000000 65535 f \n").as_bytes());
    for o in offsets {
        out.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {n} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

pub(crate) const DYNAMIC: &str = "/NeedsRendering true /Perms << /UR3 6 0 R >>";
const UR3: &str = "<< /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached >>";

fn laid_out(doc: &Document) -> (MergedForm, XfaLayout) {
    let xdp = read_xdp(&doc.view()).unwrap().unwrap();
    let template = read_template(&xdp).unwrap().unwrap();
    let merged = merge_data(&template, &xdp, &MergeOptions::new()).unwrap();
    let layout = layout_template(&merged.template, &LayoutOptions::new()).unwrap();
    (merged, layout)
}

fn materialize(s: &mut EditSession, doc: &Document) -> Result<usize, EditError> {
    let (merged, layout) = laid_out(doc);
    s.materialize_xfa(
        &merged.template,
        &layout.pages,
        &PaintOptions::new(),
        &pdfcer_core::xfa::XfaFonts::new(),
        XfaSignatures::Refuse,
    )
    .map(|o| o.pages_written)
}

/// The decoded content of each page of `doc`.
pub(crate) fn contents(doc: &Document) -> Vec<String> {
    pages(doc)
        .unwrap()
        .iter()
        .map(|p| {
            let page = Object::Reference(p.id);
            let c = doc
                .resolve(&page)
                .as_dict()
                .unwrap()
                .get(b"Contents")
                .unwrap();
            let Object::Stream(s) = doc.resolve(c) else {
                panic!("no content stream")
            };
            let raw = s.data_span.slice(doc.bytes()).unwrap();
            String::from_utf8_lossy(&decode_stream(&s.dict, raw).unwrap()).into_owned()
        })
        .collect()
}

#[test]
fn the_computed_pages_replace_the_placeholder_and_survive_save() {
    let original = pdf(DYNAMIC, "", &[UR3]);
    let doc = Document::from_bytes(original.clone()).unwrap();
    let mut s = EditSession::new(Document::from_bytes(original.clone()).unwrap());
    let (merged, layout) = laid_out(&doc);
    assert_eq!(layout.pages.len(), 2);
    let out = s
        .materialize_xfa(
            &merged.template,
            &layout.pages,
            &PaintOptions::new(),
            &pdfcer_core::xfa::XfaFonts::new(),
            XfaSignatures::Refuse,
        )
        .unwrap();
    assert_eq!((out.pages_written, out.pages_replaced), (2, 1));
    assert_eq!(out.fields_dropped, 1);
    assert!(out.usage_rights_removed);

    for full in [false, true] {
        let bytes = if full {
            s.to_full_bytes(&SaveOptions::identity()).unwrap().0
        } else {
            let b = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
            assert!(b.starts_with(&original), "an incremental save appends");
            b
        };
        let saved = Document::from_bytes(bytes).unwrap();
        assert_eq!(form_kind(&saved), XfaFormKind::NotXfa);
        let catalog = saved.catalog_dict().unwrap();
        assert!(catalog.get(b"NeedsRendering").is_none());
        assert!(catalog.get(b"Perms").is_none());
        let ps = pages(&saved).unwrap();
        assert_eq!(ps.len(), 2);
        let page = Object::Reference(ps[0].id);
        let page = saved.resolve(&page).as_dict().unwrap();
        assert!(page.get(b"Annots").is_none());
        let media = saved.resolve(page.get(b"MediaBox").unwrap());
        let media: Vec<f64> = media
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Object::as_number)
            .collect();
        assert_eq!(media, [0.0, 0.0, 612.0, 792.0]);
        let text = contents(&saved);
        assert!(text[0].contains("Hello"), "{}", text[0]);
        assert!(text[1].contains("Ken"), "{}", text[1]);
    }
}

#[test]
fn undo_restores_the_xfa_form() {
    let original = pdf(DYNAMIC, "", &[UR3]);
    let doc = Document::from_bytes(original.clone()).unwrap();
    let mut s = EditSession::new(Document::from_bytes(original).unwrap());
    materialize(&mut s, &doc).unwrap();
    assert_eq!(s.pages().unwrap().len(), 2);
    assert_eq!(s.undo(), Some(CommandKind::MaterializeXfa { pages: 2 }));
    assert_eq!(s.pages().unwrap().len(), 1);
    assert_eq!(form_kind(&s.graph()), XfaFormKind::Dynamic);
}

#[test]
fn a_static_form_is_refused() {
    let original = pdf("", "", &[]);
    let doc = Document::from_bytes(original.clone()).unwrap();
    let mut s = EditSession::new(Document::from_bytes(original).unwrap());
    assert!(matches!(
        materialize(&mut s, &doc),
        Err(EditError::XfaNotDynamic)
    ));
}

#[test]
fn a_signed_form_is_refused() {
    let original = pdf(
        DYNAMIC,
        "/FT /Sig /V 7 0 R",
        &[UR3, "<< /Type /Sig /ByteRange [0 1 2 3] /Contents <00> >>"],
    );
    let doc = Document::from_bytes(original.clone()).unwrap();
    let mut s = EditSession::new(Document::from_bytes(original).unwrap());
    assert!(matches!(
        materialize(&mut s, &doc),
        Err(EditError::XfaSigned { signatures: 1 })
    ));
}

#[test]
fn signatures_are_removed_when_asked() {
    let original = pdf(
        "/NeedsRendering true /Perms << /UR3 6 0 R /DocMDP 7 0 R >>",
        "/FT /Sig /V 7 0 R",
        &[
            UR3,
            "<< /Type /Sig /ByteRange [0 1 2 3] /Contents <00> /Reference [<< /TransformMethod /DocMDP /TransformParams << /P 1 >> >>] >>",
        ],
    );
    let doc = Document::from_bytes(original.clone()).unwrap();
    let mut s = EditSession::new(Document::from_bytes(original).unwrap());
    assert!(materialize(&mut s, &doc).is_err());
    let (merged, layout) = laid_out(&doc);
    let out = s
        .materialize_xfa(
            &merged.template,
            &layout.pages,
            &PaintOptions::new(),
            &pdfcer_core::xfa::XfaFonts::new(),
            XfaSignatures::Remove,
        )
        .unwrap();
    assert_eq!(out.signatures_removed, 1);
    assert!(out.usage_rights_removed);
    let saved = s.to_full_bytes(&SaveOptions::identity()).unwrap().0;
    let saved = Document::from_bytes(saved).unwrap();
    assert!(saved.catalog_dict().unwrap().get(b"Perms").is_none());
    assert_eq!(pages(&saved).unwrap().len(), 2);
}
