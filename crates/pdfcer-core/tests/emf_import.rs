//! EMF import (`emf_import::import`) and placement (`EditSession::add_emf`,
//! `add_emf_stamp`): what is written, what is disclosed, what is refused.
//! Every fixture is built here, record by record, from [MS-EMF]. The
//! export → import pixel round trip lives in
//! `pdfcer-render/tests/emf_import_roundtrip.rs`.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{
    CommandKind, EditError, EditSession, LayerEdit, MarkupNote, MarkupOptions,
};
use pdfcer_core::emf_import::{self, EmfImportError};
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

/// An EMF under construction: records after the header.
#[derive(Default)]
struct Emf {
    body: Vec<u8>,
    count: u32,
}

impl Emf {
    fn rec(mut self, kind: u32, fields: &[u8]) -> Self {
        let size = 8 + fields.len().div_ceil(4) * 4;
        self.body.extend_from_slice(&kind.to_le_bytes());
        self.body.extend_from_slice(&(size as u32).to_le_bytes());
        self.body.extend_from_slice(fields);
        self.body
            .resize(self.body.len() + size - 8 - fields.len(), 0);
        self.count += 1;
        self
    }

    /// Header (frame 100 × 50 mm; reference device 1000 × 1000 px over
    /// 100 × 100 mm, so one logical MM_TEXT unit is 0.1 mm), body, EOF.
    fn finish(self) -> Vec<u8> {
        let eof = [0u8, 0, 0, 0, 16, 0, 0, 0, 20, 0, 0, 0];
        let mut h = Vec::new();
        for v in [0i32, 0, 999, 499, 0, 0, 9_999, 4_999] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        h.extend_from_slice(b" EMF");
        h.extend_from_slice(&0x0001_0000u32.to_le_bytes());
        let total = 108 + self.body.len() + 20;
        h.extend_from_slice(&(total as u32).to_le_bytes());
        h.extend_from_slice(&(self.count + 2).to_le_bytes());
        h.extend_from_slice(&[8, 0, 0, 0]); // Handles, Reserved
        h.extend_from_slice(&[0; 12]); // description, palette
        for v in [1000u32, 1000, 100, 100, 0, 0, 0, 100_000, 100_000] {
            h.extend_from_slice(&v.to_le_bytes());
        }
        let mut out = 1u32.to_le_bytes().to_vec();
        out.extend_from_slice(&108u32.to_le_bytes());
        out.extend_from_slice(&h);
        out.extend_from_slice(&self.body);
        out.extend_from_slice(&0x0Eu32.to_le_bytes());
        out.extend_from_slice(&20u32.to_le_bytes());
        out.extend_from_slice(&eof);
        out
    }
}

fn le(values: &[i32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn rgb(r: u8, g: u8, b: u8) -> i32 {
    i32::from_le_bytes([r, g, b, 0])
}

/// A red brush (1) and a 10-unit blue pen (2), both selected, then a
/// rectangle from (100, 100) to (500, 300).
fn filled_rectangle() -> Emf {
    Emf::default()
        .rec(0x27, &le(&[1, 0, rgb(255, 0, 0), 0]))
        .rec(0x25, &le(&[1]))
        .rec(0x26, &le(&[2, 0, 10, 0, rgb(0, 0, 255)]))
        .rec(0x25, &le(&[2]))
        .rec(0x2B, &le(&[100, 100, 500, 300]))
}

/// EMR_COMMENT carrying an EMF+ header record with `flags`.
fn emf_plus_header(flags: u16) -> Vec<u8> {
    let mut d = le(&[28]);
    d.extend_from_slice(b"EMF+");
    d.extend_from_slice(&0x4001u16.to_le_bytes());
    d.extend_from_slice(&flags.to_le_bytes());
    d.extend_from_slice(&le(&[28, 16, 0xDBC0_1002_u32 as i32, 0, 96, 96]));
    d
}

fn minimal_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << >> >>",
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = buf.len();
    buf.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for o in offsets {
        buf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    buf
}

fn session() -> (Vec<u8>, EditSession) {
    let bytes = minimal_pdf();
    let s = EditSession::new(Document::from_bytes(bytes.clone()).unwrap());
    (bytes, s)
}

fn at(x: f64, y: f64, (w, h): (f64, f64)) -> Rect {
    Rect {
        llx: x,
        lly: y,
        urx: x + w,
        ury: y + h,
    }
}

/// A stream's bytes, inflated when `/FlateDecode`.
fn stream_text(s: &EditSession, id: ObjId) -> String {
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
        .unwrap();
        out
    } else {
        raw
    };
    String::from_utf8_lossy(&bytes).into_owned()
}

#[test]
fn a_filled_outlined_rectangle_is_vector_operators_in_a_form() {
    let emf = emf_import::import(&filled_rectangle().finish()).unwrap();
    assert!(emf.notes().is_empty(), "{}", emf.notes().summary());
    let (w, h) = emf.natural_size_pt();
    assert!((w - 283.4646).abs() < 1e-3 && (h - 141.7323).abs() < 1e-3);
    let (_, mut s) = session();
    let placed = s.add_emf(0, at(50.0, 50.0, (w, h)), &emf).unwrap();
    assert!(!placed.distorted, "the aspect is kept");
    let content = stream_text(&s, placed.form_id);
    for op in ["1 0 0 rg\n", "0 0 1 RG\n", "re\n", "B*\n"] {
        let found = content.contains(op) || (op == "re\n" && content.contains(" l\n"));
        assert!(found, "{op:?} in {content}");
    }
    // (100, 100) logical = 10 mm from the frame's top-left: x 28.3465 pt,
    // y 141.7323 − 28.3465 = 113.3858 pt; the pen is 1 mm = 2.8346 pt.
    assert!(content.contains("28.3465 113.3858 m"), "{content}");
    assert!(content.contains("2.8346 w"), "{content}");
    assert!(!content.contains(" Do\n"), "no image: {content}");
    let Some(Object::Stream(form)) = s.value(placed.form_id) else {
        panic!("form");
    };
    assert_eq!(
        form.dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n.0.clone()),
        Some(b"Form".to_vec())
    );
    assert_eq!(s.undo_kind(), Some(CommandKind::AddEmf));
}

#[test]
fn undo_removes_the_whole_placement_in_one_step() {
    let emf = emf_import::import(&filled_rectangle().finish()).unwrap();
    let (_, mut s) = session();
    let page = s.pages().unwrap()[0].id;
    let before = s.value(page).cloned();
    let placed = s.add_emf(0, at(0.0, 0.0, (100.0, 50.0)), &emf).unwrap();
    s.undo().unwrap();
    assert!(s.value(placed.form_id).is_none());
    assert!(s.value(placed.content_id.unwrap()).is_none());
    assert_eq!(s.value(page).cloned(), before);
}

#[test]
fn an_incremental_save_keeps_every_original_byte() {
    let emf = emf_import::import(&filled_rectangle().finish()).unwrap();
    let (original, mut s) = session();
    let placed = s.add_emf(0, at(10.0, 10.0, (100.0, 50.0)), &emf).unwrap();
    let out = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    assert!(out.starts_with(&original), "original bytes verbatim");
    let appended = String::from_utf8_lossy(&out[original.len()..]).into_owned();
    assert!(appended.contains("/Subtype /Form"), "the form is appended");
    let reopened = EditSession::new(Document::from_bytes(out).unwrap());
    let page = reopened.pages().unwrap()[0].id;
    let Some(Object::Dict(d)) = reopened.value(page) else {
        panic!("page");
    };
    let xobjects = format!("{:?}", d.get(b"Resources"));
    let form = format!("{:?}", placed.form_id);
    assert!(
        xobjects.contains(&form),
        "the page names {form}: {xobjects}"
    );
}

#[test]
fn an_emf_plus_only_file_is_refused_by_name_and_nothing_is_placed() {
    let only = Emf::default()
        .rec(0x46, &emf_plus_header(0))
        .rec(0x2B, &le(&[0, 0, 10, 10]))
        .finish();
    let (_, s) = session();
    assert_eq!(
        emf_import::import(&only).unwrap_err(),
        EmfImportError::EmfPlusOnly
    );
    assert!(
        EmfImportError::EmfPlusOnly
            .to_string()
            .contains("EMF+ only"),
        "named"
    );
    assert_eq!(s.undo_kind(), None, "the document is unchanged");
}

#[test]
fn a_dual_mode_file_is_drawn_from_its_emf_records_and_says_so() {
    let dual = Emf::default()
        .rec(0x46, &emf_plus_header(1))
        .rec(0x2B, &le(&[0, 0, 10, 10]))
        .finish();
    let emf = emf_import::import(&dual).unwrap();
    assert!(emf.notes().emf_plus_ignored);
    assert!(emf.notes().summary().contains("EMF+ records ignored"));
}

#[test]
fn unsupported_records_are_counted_by_name_never_dropped_silently() {
    let bytes = filled_rectangle()
        .rec(0x2D, &le(&[0, 0, 10, 10, 0, 0, 10, 10])) // EMR_ARC
        .rec(0x2D, &le(&[0, 0, 10, 10, 0, 0, 10, 10]))
        .rec(0x76, &le(&[0, 0, 10, 10, 0, 0, 0])) // EMR_GRADIENTFILL
        .rec(0x5555, &[])
        .finish();
    let emf = emf_import::import(&bytes).unwrap();
    let skipped = &emf.notes().skipped;
    assert_eq!(skipped.get("EMR_ARC"), Some(&2));
    assert_eq!(skipped.get("EMR_GRADIENTFILL"), Some(&1));
    assert_eq!(skipped.get("EMR_0x5555"), Some(&1));
    assert!(emf.notes().summary().contains("EMR_ARC \u{d7}2"));
}

/// EMR_EXTCREATEFONTINDIRECTW with `face` and lfHeight −`em`.
fn font(face: &str, em: i32) -> Vec<u8> {
    let mut d = le(&[3, -em, 0, 0, 0, 400]);
    d.extend_from_slice(&[0, 0, 0, 1, 4, 0, 0, 0]);
    let mut name = [0u8; 64];
    for (slot, u) in name.chunks_exact_mut(2).zip(face.encode_utf16()) {
        slot.copy_from_slice(&u.to_le_bytes());
    }
    d.extend_from_slice(&name);
    d
}

/// EMR_EXTTEXTOUTW at (x, y) with a Dx of `dx` per character.
fn text(x: i32, y: i32, s: &str, dx: i32) -> Vec<u8> {
    let units: Vec<u16> = s.encode_utf16().collect();
    let n = units.len() as i32;
    let string_bytes = (2 * units.len()).div_ceil(4) * 4;
    let mut d = le(&[0, 0, -1, -1, 1, 0, 0, x, y, n, 76, 0, 0, 0, -1, -1]);
    d.extend_from_slice(&le(&[76 + string_bytes as i32]));
    for u in &units {
        d.extend_from_slice(&u.to_le_bytes());
    }
    d.resize(68 + string_bytes, 0);
    d.extend_from_slice(&le(&vec![dx; units.len()]));
    d
}

#[test]
fn text_is_real_text_in_a_disclosed_substitute_face() {
    let bytes = Emf::default()
        .rec(0x52, &font("Arial", 50))
        .rec(0x25, &le(&[3]))
        .rec(0x16, &le(&[0x18]))
        .rec(0x54, &text(100, 200, "Hi\u{2603}", 30))
        .finish();
    let emf = emf_import::import(&bytes).unwrap();
    let n = emf.notes();
    assert_eq!(
        n.fonts_substituted.get("Arial").map(String::as_str),
        Some("Helvetica")
    );
    assert_eq!(n.characters_replaced, 1, "the snowman is not WinAnsi");
    let (_, mut s) = session();
    let placed = s.add_emf(0, at(0.0, 0.0, (200.0, 100.0)), &emf).unwrap();
    let content = stream_text(&s, placed.form_id);
    assert!(content.contains("BT\n/F1 1 Tf"), "{content}");
    assert!(
        content.contains("] TJ\n"),
        "Dx places each glyph: {content}"
    );
    let Some(Object::Stream(form)) = s.value(placed.form_id) else {
        panic!("form");
    };
    let res = format!("{:?}", form.dict.get(b"Resources"));
    assert!(
        res.contains("Helvetica") && res.contains("WinAnsiEncoding"),
        "{res}"
    );
}

/// EMR_STRETCHDIBITS of a 2 × 2 24-bpp bottom-up DIB to (x, y, cx, cy).
fn stretch_dibits(x: i32, y: i32, cx: i32, cy: i32, w: i32, h: i32) -> Vec<u8> {
    let bmi = le(&[40, w, h, 0x0018_0001, 0, 0, 0, 0, 0, 0]);
    let row = ((w * 3 + 3) / 4 * 4) as usize;
    let bits = vec![0x80u8; row * h.unsigned_abs() as usize];
    let mut d = le(&[0, 0, -1, -1, x, y, 0, 0, w, h]);
    d.extend_from_slice(&le(&[
        80,
        40,
        120,
        bits.len() as i32,
        0,
        0x00CC_0020,
        cx,
        cy,
    ]));
    d.extend_from_slice(&bmi);
    d.extend_from_slice(&bits);
    d
}

#[test]
fn a_bitmap_becomes_an_image_xobject() {
    let bytes = Emf::default()
        .rec(0x51, &stretch_dibits(0, 0, 200, 100, 2, 2))
        .finish();
    let emf = emf_import::import(&bytes).unwrap();
    assert_eq!(emf.image_count(), 1, "{}", emf.notes().summary());
    let (_, mut s) = session();
    let placed = s.add_emf(0, at(0.0, 0.0, (100.0, 50.0)), &emf).unwrap();
    assert!(placed.objects_written >= 1);
    let content = stream_text(&s, placed.form_id);
    assert!(content.contains("/Im1 Do\n"), "{content}");
}

#[test]
fn a_clip_narrows_with_w_n() {
    let bytes = Emf::default()
        .rec(0x1E, &le(&[0, 0, 50, 50])) // INTERSECTCLIPRECT
        .rec(0x2B, &le(&[0, 0, 100, 100]))
        .finish();
    let emf = emf_import::import(&bytes).unwrap();
    let (_, mut s) = session();
    let placed = s.add_emf(0, at(0.0, 0.0, (100.0, 50.0)), &emf).unwrap();
    let content = stream_text(&s, placed.form_id);
    assert!(content.contains("W n\n"), "{content}");
}

#[test]
fn the_stamp_variant_is_a_stamp_whose_appearance_is_the_form() {
    let emf = emf_import::import(&filled_rectangle().finish()).unwrap();
    let (_, mut s) = session();
    let placed = s
        .add_emf_stamp(
            0,
            at(10.0, 10.0, (100.0, 100.0)),
            &emf,
            &MarkupOptions::default(),
        )
        .unwrap();
    assert!(placed.distorted, "a square rect over a 2:1 frame stretches");
    assert!(placed.summary().contains("stretched"));
    let Some(Object::Dict(annot)) = s.value(placed.annot_id.unwrap()) else {
        panic!("annot");
    };
    assert_eq!(
        annot
            .get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n.0.clone()),
        Some(b"Stamp".to_vec())
    );
    assert!(placed.content_id.is_none());
}

#[test]
fn a_degenerate_rectangle_is_refused() {
    let emf = emf_import::import(&filled_rectangle().finish()).unwrap();
    let (_, mut s) = session();
    let err = s.add_emf(0, at(0.0, 0.0, (0.0, 10.0)), &emf).unwrap_err();
    assert!(
        matches!(err, EditError::ImageRectDegenerate { .. }),
        "{err:?}"
    );
}

#[test]
fn hostile_inputs_are_refused_by_name() {
    use emf_import::{MAX_BITMAP_PIXELS, MAX_INPUT_BYTES, MAX_POINTS, MAX_SAVE_DEPTH};
    assert_eq!(
        emf_import::import(b"%PDF-1.7").unwrap_err(),
        EmfImportError::NotEmf
    );

    let mut deep = Emf::default();
    for _ in 0..=MAX_SAVE_DEPTH {
        deep = deep.rec(0x21, &[]);
    }
    assert_eq!(
        emf_import::import(&deep.finish()).unwrap_err(),
        EmfImportError::SaveDepth {
            limit: MAX_SAVE_DEPTH
        }
    );

    let claimed = MAX_POINTS as i32 + 1;
    let points = Emf::default()
        .rec(0x04, &le(&[0, 0, 0, 0, claimed]))
        .finish();
    assert_eq!(
        emf_import::import(&points).unwrap_err(),
        EmfImportError::TooManyPoints { limit: MAX_POINTS }
    );

    let big = Emf::default()
        .rec(0x51, &stretch_dibits(0, 0, 10, 10, 10_000, 10_000)[..120])
        .finish();
    assert_eq!(
        emf_import::import(&big).unwrap_err(),
        EmfImportError::BitmapTooLarge {
            limit: MAX_BITMAP_PIXELS
        }
    );

    let mut huge = filled_rectangle().finish();
    huge.resize(MAX_INPUT_BYTES + 1, 0);
    assert_eq!(
        emf_import::import(&huge).unwrap_err(),
        EmfImportError::TooLarge {
            limit: MAX_INPUT_BYTES
        }
    );

    let mut broken = filled_rectangle().finish();
    let len = broken.len();
    broken[len - 16] = 3; // EOF Size 3: not a multiple of 4
    assert!(matches!(
        emf_import::import(&broken).unwrap_err(),
        EmfImportError::Corrupt { .. }
    ));
}

#[test]
fn too_many_records_is_refused_by_name() {
    let mut many = Emf::default();
    for _ in 0..emf_import::MAX_RECORDS {
        many = many.rec(0x15, &[0; 4]);
    }
    assert_eq!(
        emf_import::import(&many.finish()).unwrap_err(),
        EmfImportError::TooManyRecords {
            limit: emf_import::MAX_RECORDS
        }
    );
}

fn emf_signed() -> MarkupOptions {
    MarkupOptions {
        note: Some(MarkupNote::new("").by("Ken").at("D:20261003120000Z")),
        opacity: Some(0.5),
        ..Default::default()
    }
}

#[test]
fn an_emf_stamp_carries_the_author_date_and_opacity() {
    let emf = emf_import::import(&filled_rectangle().finish()).unwrap();
    let (_, mut s) = session();
    let placed = s
        .add_emf_stamp(0, at(10.0, 10.0, (100.0, 100.0)), &emf, &emf_signed())
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
fn an_emf_stamp_refuses_an_opacity_out_of_range() {
    let emf = emf_import::import(&filled_rectangle().finish()).unwrap();
    let (_, mut s) = session();
    let options = MarkupOptions {
        opacity: Some(1.5),
        ..Default::default()
    };
    assert!(matches!(
        s.add_emf_stamp(0, at(10.0, 10.0, (100.0, 100.0)), &emf, &options),
        Err(EditError::MarkupOpacityOutOfRange { .. })
    ));
    assert_eq!(s.undo_depth(), 0);
}

#[test]
fn an_emf_stamp_goes_on_the_named_layer() {
    let emf = emf_import::import(&filled_rectangle().finish()).unwrap();
    let (_, mut s) = session();
    let layer = s
        .add_layer("Stamps", &LayerEdit::default())
        .expect("add_layer");
    let options = MarkupOptions {
        layer: Some(layer),
        ..Default::default()
    };
    let placed = s
        .add_emf_stamp(0, at(10.0, 10.0, (100.0, 100.0)), &emf, &options)
        .expect("stamp");
    let Some(Object::Dict(annot)) = s.value(placed.annot_id.expect("annotation")) else {
        panic!("annotation dict");
    };
    assert_eq!(annot.get(b"OC").and_then(Object::as_reference), Some(layer));
    assert_eq!(s.undo_depth(), 2, "the layer, then one entry for the stamp");
}
