//! Edits that extend a dictionary entry must extend the entry the file
//! actually holds. §7.3.10 lets `/XObject`, `/MK`, `/AP`, `/OCGs` and `/D`
//! be indirect, and a page may inherit `/Resources` (§7.7.3.4); reading any
//! of those as absent rebuilds the entry with only the new item in it.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{DEFAULT_GROUP_ID, DimensionKind};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, NewImage};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::vector::{AxisConstraint, Point};

fn assemble(bodies: &[String]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
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

fn stream_obj(keys: &str, content: &str) -> String {
    format!(
        "<< {keys} /Length {} >>\nstream\n{content}\nendstream",
        content.len() + 1
    )
}

fn session(bodies: &[&str]) -> EditSession {
    let owned: Vec<String> = bodies.iter().map(|s| (*s).to_owned()).collect();
    EditSession::new(Document::from_bytes(assemble(&owned)).expect("fixture parses"))
}

fn dict_of(s: &EditSession, o: &Object) -> Dict {
    let g = s.graph();
    match g.resolve(o) {
        Object::Dict(d) => d.clone(),
        Object::Stream(st) => st.dict.clone(),
        other => panic!("not a dictionary: {other:?}"),
    }
}

fn obj_dict(s: &EditSession, n: u32) -> Dict {
    dict_of(s, &Object::Reference(ObjId::new(n, 0)))
}

fn refs(s: &EditSession, o: Option<&Object>) -> Vec<u32> {
    let g = s.graph();
    o.map(|o| g.resolve(o).clone())
        .and_then(|o| o.as_array().map(<[Object]>::to_vec))
        .unwrap_or_default()
        .iter()
        .filter_map(Object::as_reference)
        .map(|r| r.num)
        .collect()
}

fn keys(d: &Dict) -> Vec<String> {
    let mut v: Vec<String> = d
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k.as_bytes()).into_owned())
        .collect();
    v.sort();
    v
}

fn fixture(rel: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic")
        .join(rel)
}

// ---------------------------------------------------------------------------
// Page `/XObject` stored as its own object
// ---------------------------------------------------------------------------

#[test]
fn adding_an_image_keeps_the_images_an_indirect_xobject_dict_already_holds() {
    let mut s = session(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] \
         /Resources << /XObject 4 0 R >> >>",
        "<< /Old 5 0 R >>",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 1 1] /Length 0 >>\nstream\n\nendstream",
    ]);
    let bytes = std::fs::read(fixture("images/rgba8.png")).expect("image fixture");
    let img = pdfcer_core::image_import::import(&bytes).expect("imports");
    s.add_image(&NewImage::new(
        0,
        Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 110.0,
            ury: 90.0,
        },
        &img,
    ))
    .expect("places");

    let page = obj_dict(&s, 3);
    let res = dict_of(&s, page.get(b"Resources").unwrap());
    let xobj = dict_of(&s, res.get(b"XObject").unwrap());
    assert!(
        xobj.get(b"Old").is_some(),
        "the existing /Old entry survived: {:?}",
        keys(&xobj)
    );
    assert_eq!(xobj.iter().count(), 2, "old plus new: {:?}", keys(&xobj));
}

// ---------------------------------------------------------------------------
// Widget `/MK` and `/AP` stored as their own objects
// ---------------------------------------------------------------------------

fn widget_with_indirect_mk_and_ap() -> EditSession {
    session(&[
        "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R] >> >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Annots [5 0 R 6 0 R] >>",
        "<< /FT /Tx /T (Name) /DA (/Helv 12 Tf 0 g) /Kids [5 0 R 6 0 R] >>",
        "<< /Type /Annot /Subtype /Widget /Parent 4 0 R /P 3 0 R \
         /Rect [20 300 220 324] /MK 7 0 R /AP 8 0 R >>",
        "<< /Type /Annot /Subtype /Widget /Parent 4 0 R /P 3 0 R \
         /Rect [20 200 220 224] /MK 7 0 R /AP 8 0 R >>",
        "<< /BC [1 0 0] /BG [0 0 1] >>",
        "<< /N 9 0 R /D 10 0 R >>",
        &stream_obj("/Type /XObject /Subtype /Form /BBox [0 0 200 24]", ""),
        &stream_obj("/Type /XObject /Subtype /Form /BBox [0 0 200 24]", ""),
    ])
}

#[test]
fn rotating_a_widget_keeps_the_colours_in_an_indirect_mk() {
    let mut s = widget_with_indirect_mk_and_ap();
    s.rotate_widget("Name", 0, 90).expect("rotates");

    let w = obj_dict(&s, 5);
    let mk = dict_of(&s, w.get(b"MK").expect("/MK present"));
    assert!(
        mk.get(b"BC").is_some(),
        "border colour kept: {:?}",
        keys(&mk)
    );
    assert!(mk.get(b"BG").is_some(), "background kept: {:?}", keys(&mk));
    assert_eq!(mk.get(b"R").and_then(Object::as_int), Some(90));
}

#[test]
fn regenerating_a_widget_appearance_keeps_the_other_states_of_an_indirect_ap() {
    let mut s = widget_with_indirect_mk_and_ap();
    s.fill_text_field("Name", "hello").expect("fills");

    for n in [5, 6] {
        let w = obj_dict(&s, n);
        let ap = dict_of(&s, w.get(b"AP").expect("/AP present"));
        assert!(
            ap.get(b"D").is_some(),
            "widget {n}: /D survived the /N redraw: {:?}",
            keys(&ap)
        );
        assert_ne!(
            ap.get(b"N").and_then(Object::as_reference),
            Some(ObjId::new(9, 0)),
            "widget {n}: /N was redrawn"
        );
    }
}

// ---------------------------------------------------------------------------
// `/OCProperties`: indirect `/OCGs` and `/D`, and the file's own `/D` state
// ---------------------------------------------------------------------------

fn linear() -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    }
}

fn layered(ocp: &str, extra: &[&str]) -> EditSession {
    let catalog = format!("<< /Type /Catalog /Pages 2 0 R /OCProperties {ocp} >>");
    let mut bodies: Vec<&str> = vec![
        &catalog,
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
        "<< /Type /OCG /Name (Hidden hatching) >>",
        "<< /Type /OCG /Name (Title block) >>",
    ];
    bodies.extend_from_slice(extra);
    session(&bodies)
}

fn assert_layers_survive(s: &EditSession) {
    let cat = s.graph().catalog_dict().expect("catalog").clone();
    let ocp = dict_of(s, cat.get(b"OCProperties").expect("/OCProperties"));
    let ocgs = refs(s, ocp.get(b"OCGs"));
    assert!(
        ocgs.contains(&4) && ocgs.contains(&5),
        "the file's own layers are still registered: {ocgs:?}"
    );
    let d = dict_of(s, ocp.get(b"D").expect("/D"));
    assert!(
        refs(s, d.get(b"OFF")).contains(&4),
        "the hidden layer stays hidden: {:?}",
        keys(&d)
    );
    assert_eq!(refs(s, d.get(b"Locked")), vec![5], "/Locked carried");
    let order = refs(s, d.get(b"Order"));
    assert!(
        order.contains(&4) && order.contains(&5),
        "/Order: {order:?}"
    );
}

#[test]
fn a_dimension_edit_keeps_the_files_hidden_and_locked_layers() {
    let mut s = layered(
        "<< /OCGs [4 0 R 5 0 R] /D << /Order [4 0 R 5 0 R] /OFF [4 0 R] /Locked [5 0 R] >> >>",
        &[],
    );
    s.add_dimension(0, DEFAULT_GROUP_ID, linear())
        .expect("adds");
    assert_layers_survive(&s);
}

#[test]
fn a_dimension_edit_keeps_layers_held_in_indirect_ocgs_and_d() {
    let mut s = layered(
        "<< /OCGs 6 0 R /D 7 0 R >>",
        &[
            "[4 0 R 5 0 R]",
            "<< /Order 8 0 R /OFF [4 0 R] /Locked [5 0 R] >>",
            "[4 0 R 5 0 R]",
        ],
    );
    s.add_dimension(0, DEFAULT_GROUP_ID, linear())
        .expect("adds");
    assert_layers_survive(&s);
}

// ---------------------------------------------------------------------------
// Redaction on a page that INHERITS `/Resources`
// ---------------------------------------------------------------------------

#[test]
fn a_redaction_overlay_on_an_inheriting_page_keeps_the_inherited_fonts() {
    let content = "BT /F1 12 Tf 40 300 Td (Keep this) Tj ET\n\
                   BT /F1 12 Tf 40 200 Td (secret) Tj ET";
    let mut s = session(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 \
         /Resources << /Font << /F1 5 0 R >> >> >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Contents 4 0 R >>",
        &stream_obj("", content),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
    ]);
    let spec = pdfcer_core::annot_author::RedactSpec {
        quads: vec![pdfcer_core::annot_author::Quad {
            ul: (35.0, 215.0),
            ur: (110.0, 215.0),
            ll: (35.0, 195.0),
            lr: (110.0, 195.0),
        }],
        fill: None,
        overlay_text: Some("X".to_owned()),
        quadding: pdfcer_core::vartext::Quadding::default(),
    };
    s.add_redaction(0, &spec).expect("marks");
    s.apply_redactions().expect("applies");

    let page = obj_dict(&s, 3);
    let res = dict_of(
        &s,
        page.get(b"Resources").expect("overlay wrote /Resources"),
    );
    let fonts = dict_of(&s, res.get(b"Font").expect("/Font"));
    assert!(
        fonts.get(b"F1").is_some(),
        "the surviving text's inherited font still resolves: {:?}",
        keys(&fonts)
    );
}

// ---------------------------------------------------------------------------
// A new font bound into a FORM XObject
// ---------------------------------------------------------------------------

#[test]
fn restyling_form_text_to_a_new_font_keeps_the_form_dictionary() {
    let mut s = session(&[
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
         /Resources << /XObject << /X1 5 0 R >> >> >>",
        &stream_obj("", "q 1 0 0 1 20 20 cm /X1 Do Q"),
        &stream_obj(
            "/Type /XObject /Subtype /Form /BBox [0 0 200 200] /Matrix [1 0 0 1 0 0] \
             /Resources << /Font << /F1 6 0 R >> /XObject << /Keep 7 0 R >> >>",
            "BT /F1 12 Tf 10 10 Td (TITLE) Tj ET",
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>",
        "<< /Type /XObject /Subtype /Form /BBox [0 0 1 1] /Length 0 >>\nstream\n\nendstream",
    ]);
    let report = s
        .format_text(
            &pdfcer_core::text_edit::FormatRequest::new(0, "TITLE")
                .font(pdfcer_core::text_edit::FontSelector::new("Courier")),
            &pdfcer_core::text_edit::FormatOptions::default(),
        )
        .expect("formats");
    assert_eq!(report.form_object, Some(5));

    let form = obj_dict(&s, 5);
    assert!(form.get(b"BBox").is_some(), "/BBox kept: {:?}", keys(&form));
    assert!(form.get(b"Subtype").is_some(), "/Subtype kept");
    let res = dict_of(&s, form.get(b"Resources").expect("/Resources"));
    let xobj = dict_of(&s, res.get(b"XObject").expect("the form's /XObject kept"));
    assert!(xobj.get(b"Keep").is_some());
    let fonts = dict_of(&s, res.get(b"Font").expect("/Font"));
    assert!(
        fonts.get(b"F1").is_some(),
        "old font kept: {:?}",
        keys(&fonts)
    );
    assert!(
        fonts.iter().count() >= 2,
        "new font bound: {:?}",
        keys(&fonts)
    );
}
