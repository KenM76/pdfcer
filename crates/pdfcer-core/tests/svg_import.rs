//! SVG import (`svg_import::import`) and placement (`EditSession::add_svg`,
//! `add_svg_stamp`): what is written, what is disclosed, what is refused.
//! Pixel parity against resvg lives in `pdfcer-render/tests/svg_import.rs`.
#![cfg(feature = "svg-import")]

use std::io::Write as _;
use std::path::Path;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    CommandKind, EditError, EditSession, LayerEdit, MarkupNote, MarkupOptions,
};
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::svg_import::{self, SvgFeature, SvgImportError};

fn session() -> EditSession {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/dimension/plain-base.pdf");
    EditSession::new(Document::load(&path).expect("load fixture"))
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Rect {
    Rect {
        llx: x,
        lly: y,
        urx: x + w,
        ury: y + h,
    }
}

const PLAIN: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
  <defs><linearGradient id="g"><stop offset="0" stop-color="red"/><stop offset="1" stop-color="blue"/></linearGradient></defs>
  <rect x="10" y="10" width="80" height="40" fill="#336699"/>
  <path d="M 110 10 L 190 50" stroke="black" stroke-width="4"/>
  <rect x="10" y="60" width="180" height="30" fill="url(#g)"/>
</svg>"##;

/// A stream's bytes, inflated when `/FlateDecode`.
fn stream_text(s: &EditSession, id: pdfcer_core::object::ObjId) -> String {
    let view = s.view();
    let Some(Object::Stream(st)) = view.graph().value(id) else {
        panic!("expected a stream at {id:?}");
    };
    let raw = view.slice(st.data_span).unwrap_or_default().to_vec();
    let bytes = if st.dict.contains_key(b"Filter") {
        let mut out = Vec::new();
        std::io::Read::read_to_end(
            &mut flate2::read::ZlibDecoder::new(raw.as_slice()),
            &mut out,
        )
        .expect("inflates");
        out
    } else {
        raw
    };
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn a_plain_svg_places_as_a_form_xobject_of_vector_operators() {
    let svg = svg_import::import(PLAIN.as_bytes()).expect("imports");
    assert!(
        svg.notes().is_empty(),
        "nothing to disclose: {}",
        svg.notes().summary()
    );
    let mut s = session();
    let placed = s
        .add_svg(0, rect(50.0, 50.0, 200.0, 100.0), &svg)
        .expect("add_svg");
    let Some(Object::Stream(form)) = s.value(placed.form_id) else {
        panic!("the form is a stream");
    };
    assert_eq!(
        form.dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n.0.clone()),
        Some(b"Form".to_vec())
    );
    let content = stream_text(&s, placed.form_id);
    for op in [" m\n", " l\n", "\nf\n", "\nS\n", " scn\n"] {
        assert!(
            content.contains(op),
            "the form draws with {op:?}: {content}"
        );
    }
    assert!(
        !content.contains(" Do\n"),
        "no image or nested form is invoked: {content}"
    );
    assert_eq!(s.undo_kind(), Some(CommandKind::AddSvg));
    assert!((placed.scale_x - 1.0).abs() < 1e-9 && (placed.scale_y - 1.0).abs() < 1e-9);
    assert!(!placed.distorted);
}

#[test]
fn undo_removes_the_whole_placement_in_one_step() {
    let svg = svg_import::import(PLAIN.as_bytes()).expect("imports");
    let mut s = session();
    let page = s.pages().expect("pages")[0].id;
    let page_before = s.value(page).cloned();
    let placed = s
        .add_svg(0, rect(0.0, 0.0, 100.0, 50.0), &svg)
        .expect("add_svg");
    assert_ne!(
        s.value(page).cloned(),
        page_before,
        "the page gained content"
    );
    s.undo().expect("one undo");
    assert!(s.value(placed.form_id).is_none(), "the form is gone");
    assert!(
        s.value(placed.content_id.unwrap()).is_none(),
        "the content stream is gone"
    );
    assert_eq!(s.value(page).cloned(), page_before, "the page is as it was");
}

#[test]
fn a_filter_places_and_is_named_in_the_outcome() {
    let src = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
      <filter id="b"><feGaussianBlur stdDeviation="3"/></filter>
      <rect width="50" height="50" fill="green" filter="url(#b)"/>
      <rect x="50" y="50" width="50" height="50" fill="blue"/>
    </svg>"##;
    let svg = svg_import::import(src.as_bytes()).expect("imports");
    let mut s = session();
    let placed = s
        .add_svg(0, rect(0.0, 0.0, 100.0, 100.0), &svg)
        .expect("add_svg");
    assert_eq!(placed.notes.skipped.get(&SvgFeature::Filter), Some(&1));
    assert!(
        placed.notes.summary().contains("filter"),
        "{}",
        placed.notes.summary()
    );
}

#[test]
fn text_and_external_images_are_skipped_and_counted_never_fetched() {
    let src = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="100" height="100">
      <text x="10" y="20">hello</text>
      <image href="Cargo.toml" width="10" height="10"/>
      <image xlink:href="file:///C:/Windows/win.ini" width="10" height="10"/>
      <rect width="10" height="10"/>
    </svg>"##;
    let svg = svg_import::import(src.as_bytes()).expect("imports");
    assert_eq!(svg.notes().skipped.get(&SvgFeature::Text), Some(&1));
    assert_eq!(
        svg.notes().skipped.get(&SvgFeature::ExternalImage),
        Some(&2)
    );
    let mut s = session();
    let placed = s
        .add_svg(0, rect(0.0, 0.0, 100.0, 100.0), &svg)
        .expect("add_svg");
    assert!(
        !stream_text(&s, placed.form_id).contains(" Do\n"),
        "no image was drawn"
    );
}

/// An embedded PNG data URL becomes an image XObject inside the form.
#[test]
fn an_embedded_png_becomes_an_image_xobject() {
    let png = std::fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/synthetic/images/rgba8.png"),
    )
    .expect("fixture");
    let src = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><image href="data:image/png;base64,{}" width="40" height="40"/></svg>"#,
        base64(&png)
    );
    let svg = svg_import::import(src.as_bytes()).expect("imports");
    assert!(svg.notes().is_empty(), "{}", svg.notes().summary());
    let mut s = session();
    let placed = s
        .add_svg(0, rect(0.0, 0.0, 100.0, 100.0), &svg)
        .expect("add_svg");
    let Some(Object::Stream(form)) = s.value(placed.form_id) else {
        panic!("form");
    };
    let xobjects = form
        .dict
        .get(b"Resources")
        .and_then(Object::as_dict)
        .and_then(|r| r.get(b"XObject"))
        .and_then(Object::as_dict)
        .expect("the form names an XObject");
    let (_, image_ref) = xobjects.iter().next().expect("one XObject");
    let Object::Reference(id) = image_ref else {
        panic!("a reference")
    };
    let Some(Object::Stream(image)) = s.value(*id) else {
        panic!("the image is a stream");
    };
    assert_eq!(
        image
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n.0.clone()),
        Some(b"Image".to_vec())
    );
}

#[test]
fn a_stretched_placement_reports_distortion() {
    let svg = svg_import::import(PLAIN.as_bytes()).expect("imports");
    let mut s = session();
    let placed = s
        .add_svg(0, rect(0.0, 0.0, 200.0, 200.0), &svg)
        .expect("add_svg");
    assert!(placed.distorted);
    assert!((placed.scale_x - 1.0).abs() < 1e-9);
    assert!((placed.scale_y - 2.0).abs() < 1e-9);
}

#[test]
fn a_degenerate_rectangle_is_refused() {
    let svg = svg_import::import(PLAIN.as_bytes()).expect("imports");
    let mut s = session();
    for r in [rect(0.0, 0.0, 0.0, 10.0), rect(0.0, 0.0, f64::NAN, 10.0)] {
        assert!(matches!(
            s.add_svg(0, r, &svg),
            Err(EditError::ImageRectDegenerate { .. })
        ));
    }
    assert!(matches!(
        s.add_svg(9, rect(0.0, 0.0, 10.0, 10.0), &svg),
        Err(EditError::PageOutOfRange { .. })
    ));
}

#[test]
fn the_stamp_variant_is_a_stamp_whose_appearance_is_the_form() {
    let svg = svg_import::import(PLAIN.as_bytes()).expect("imports");
    let mut s = session();
    let placed = s
        .add_svg_stamp(
            0,
            rect(10.0, 10.0, 100.0, 50.0),
            &svg,
            &MarkupOptions::default(),
        )
        .expect("add_svg_stamp");
    let annot_id = placed.annot_id.expect("an annotation");
    let Some(Object::Dict(annot)) = s.value(annot_id) else {
        panic!("annotation dict");
    };
    assert_eq!(
        annot
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n.0.clone()),
        Some(b"Stamp".to_vec())
    );
    let n = annot
        .get(b"AP")
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N"))
        .cloned();
    assert_eq!(n, Some(Object::Reference(placed.form_id)));
    assert!(placed.content_id.is_none());
    s.undo().expect("undo");
    assert!(s.value(annot_id).is_none() && s.value(placed.form_id).is_none());
}

#[test]
fn hostile_inputs_are_refused_by_name() {
    assert!(matches!(
        svg_import::import(&vec![b' '; svg_import::MAX_INPUT_BYTES + 1]),
        Err(SvgImportError::TooLarge { .. })
    ));
    assert!(matches!(
        svg_import::import(b"\xff\xfe<svg/>"),
        Err(SvgImportError::NotUtf8)
    ));
    assert!(matches!(
        svg_import::import(b"<svg"),
        Err(SvgImportError::Xml(_))
    ));

    let depth = svg_import::MAX_ELEMENT_DEPTH + 10;
    let deep = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">{}<rect width="1" height="1"/>{}</svg>"#,
        "<g>".repeat(depth),
        "</g>".repeat(depth)
    );
    assert!(matches!(
        svg_import::import(deep.as_bytes()),
        Err(SvgImportError::TooDeep { .. })
    ));

    // A `use` chain: few elements, deep after expansion.
    let mut chain = String::from(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10"><rect id="u0" width="1" height="1"/>"#,
    );
    for i in 1..=depth {
        chain.push_str(&format!(r##"<use id="u{i}" href="#u{}"/>"##, i - 1));
    }
    chain.push_str("</svg>");
    assert!(matches!(
        svg_import::import(chain.as_bytes()),
        Err(SvgImportError::TooDeep { .. })
    ));
}

#[test]
fn svgz_imports_and_a_decompression_bomb_is_refused() {
    let z = gzip(PLAIN.as_bytes());
    let svg = svg_import::import(&z).expect("svgz imports");
    assert_eq!(svg.size_px(), (200.0, 100.0));

    let mut bomb = b"<svg xmlns=\"http://www.w3.org/2000/svg\"><!--".to_vec();
    bomb.resize(svg_import::MAX_DECOMPRESSED_BYTES + 16, b'a');
    assert!(matches!(
        svg_import::import(&gzip(&bomb)),
        Err(SvgImportError::DecompressedTooLarge { .. })
    ));
}

fn gzip(data: &[u8]) -> Vec<u8> {
    let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    e.write_all(data).unwrap();
    e.finish().unwrap()
}

fn base64(data: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= c.len() {
                out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn svg_signed() -> MarkupOptions {
    MarkupOptions {
        note: Some(MarkupNote::new("").by("Ken").at("D:20261003120000Z")),
        opacity: Some(0.5),
        ..Default::default()
    }
}

#[test]
fn a_svg_stamp_carries_the_author_date_and_opacity() {
    let svg = svg_import::import(PLAIN.as_bytes()).expect("imports");
    let mut s = session();
    let placed = s
        .add_svg_stamp(0, rect(10.0, 10.0, 100.0, 50.0), &svg, &svg_signed())
        .expect("stamp");
    let id = placed.annot_id.expect("an annotation");
    let Some(Object::Dict(annot)) = s.value(id) else {
        panic!("annotation dict");
    };
    assert_eq!(annot.get(b"T"), Some(&Object::String(b"Ken".to_vec())));
    assert_eq!(
        annot.get(b"M"),
        Some(&Object::String(b"D:20261003120000Z".to_vec()))
    );
    assert_eq!(annot.get(b"CA"), Some(&Object::Real(0.5)));
    assert_eq!(s.undo_depth(), 1);
    s.undo().expect("undo");
    assert!(s.value(id).is_none());
}

#[test]
fn a_svg_stamp_refuses_an_opacity_out_of_range() {
    let svg = svg_import::import(PLAIN.as_bytes()).expect("imports");
    let mut s = session();
    let options = MarkupOptions {
        opacity: Some(1.5),
        ..Default::default()
    };
    assert!(matches!(
        s.add_svg_stamp(0, rect(10.0, 10.0, 100.0, 50.0), &svg, &options),
        Err(EditError::MarkupOpacityOutOfRange { .. })
    ));
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn a_svg_stamp_goes_on_the_named_layer() {
    let svg = svg_import::import(PLAIN.as_bytes()).expect("imports");
    let mut s = session();
    let layer = s
        .add_layer("Stamps", &LayerEdit::default())
        .expect("add_layer");
    let options = MarkupOptions {
        layer: Some(layer),
        ..Default::default()
    };
    let placed = s
        .add_svg_stamp(0, rect(10.0, 10.0, 100.0, 50.0), &svg, &options)
        .expect("stamp");
    let Some(Object::Dict(annot)) = s.value(placed.annot_id.expect("annotation")) else {
        panic!("annotation dict");
    };
    assert_eq!(annot.get(b"OC").and_then(Object::as_reference), Some(layer));
    assert_eq!(s.undo_depth(), 2, "the layer, then one entry for the stamp");
}
