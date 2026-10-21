//! `G222` — a fill opacity (ExtGState `/ca`, ISO 32000-1 §8.4.5 Table 58) on
//! a text format edit: the run is faded by a `gs` before it and the ambient
//! alpha is put back by a second `gs` after it, on every save path.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{
    AddTextError, AddTextRequest, FormatError, FormatOptions, FormatRequest, add_text, set_format,
};
use pdfcer_core::writer::SaveOptions;

fn pdf(objects: &[(u32, String)]) -> Vec<u8> {
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = vec![0usize; objects.len() + 1];
    for (n, body) in objects {
        offsets[*n as usize] = out.len();
        out.extend_from_slice(format!("{n} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let xref = out.len();
    out.extend_from_slice(
        format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes(),
    );
    for off in &offsets[1..] {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn helvetica() -> String {
    let widths = (0..95).map(|_| "500").collect::<Vec<_>>().join(" ");
    format!(
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica \
         /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 126 /Widths [{widths}] >>"
    )
}

fn stream(content: &str, extra: &str) -> String {
    format!(
        "<< {extra} /Length {} >>\nstream\n{content}\nendstream",
        content.len()
    )
}

/// A page drawing `content` with `/F1` and the extra resources `res`.
fn page(content: &str, res: &str) -> Document {
    Document::from_bytes(pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
                 /Resources << /Font << /F1 5 0 R >> {res} >> >>"
            ),
        ),
        (4, stream(content, "")),
        (5, helvetica()),
    ]))
    .unwrap()
}

/// A page whose text lives in a form XObject with its own `/Resources`.
fn form_page() -> Document {
    Document::from_bytes(pdf(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned()),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Resources << /XObject << /Fm1 6 0 R >> >> >>"
                .to_owned(),
        ),
        (4, stream("/Fm1 Do", "")),
        (5, helvetica()),
        (
            6,
            stream(
                "BT /F1 12 Tf 72 700 Td (hello) Tj 0 -20 Td (world) Tj ET",
                "/Type /XObject /Subtype /Form /BBox [0 0 612 792] \
                 /Resources << /Font << /F1 5 0 R >> >>",
            ),
        ),
    ]))
    .unwrap()
}

const PLAIN: &str = "BT /F1 12 Tf 72 700 Td (hello) Tj 0 -20 Td (world) Tj ET";

fn decoded(doc: &Document, obj: &Object) -> String {
    let Object::Stream(st) = doc.resolve(obj) else {
        panic!("not a stream");
    };
    let raw = st.data_span.slice(doc.bytes()).unwrap();
    String::from_utf8_lossy(&pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap())
        .into_owned()
}

fn dict<'a>(doc: &'a Document, o: Option<&'a Object>) -> &'a Dict {
    match o.map(|o| doc.resolve(o)) {
        Some(Object::Dict(d)) => d,
        Some(Object::Stream(s)) => &s.dict,
        other => panic!("not a dict: {other:?}"),
    }
}

/// `/ca` of `/ExtGState /key` under `resources`.
fn ca(doc: &Document, resources: &Dict, key: &[u8]) -> f64 {
    let gs = dict(doc, dict(doc, resources.get(b"ExtGState")).get(key));
    match gs.get(b"ca").map(|o| doc.resolve(o)) {
        Some(Object::Real(v)) => *v,
        Some(Object::Integer(i)) => *i as f64,
        other => panic!("no /ca: {other:?}"),
    }
}

/// The text of `content` between the two show operators, and the alphas the
/// `gs` before `hello` and the one after it select under `resources`.
fn faded(doc: &Document, content: &str, resources: &Dict) -> (f64, f64) {
    let hello = content.find("(hello)").expect("hello");
    let world = content.find("(world)").expect("world");
    let before = content[..hello].rfind(" gs").expect("a gs before the run");
    let set_key = content[..before].rsplit('/').next().unwrap().trim();
    let after = &content[hello..world];
    let restore = after.find(" gs").expect("a gs restoring the ambient alpha");
    let restore_key = after[..restore].rsplit('/').next().unwrap().trim();
    (
        ca(doc, resources, set_key.as_bytes()),
        ca(doc, resources, restore_key.as_bytes()),
    )
}

fn saved_page(bytes: Vec<u8>) -> (Document, String, Dict) {
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let content = decoded(&doc, &Object::Reference(pages[0].contents[0]));
    let res = pages[0].resources.clone();
    (doc, content, res)
}

#[test]
fn a_faded_run_on_the_session_path_restores_full_opacity_after_it() {
    let mut s = EditSession::new(page(PLAIN, ""));
    let r = s
        .format_text(
            &FormatRequest::new(0, "hello").fill_alpha(0.5),
            &FormatOptions::default(),
        )
        .unwrap();
    assert_eq!(r.fill_alpha_change, Some((1.0, 0.5)));
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let (doc, content, res) = saved_page(bytes);
    assert_eq!(faded(&doc, &content, &res), (0.5, 1.0), "{content}");
}

#[test]
fn the_one_shot_path_binds_the_graphics_states_it_names() {
    let doc = page(PLAIN, "");
    let out = set_format(
        &doc,
        &FormatRequest::new(0, "hello").fill_alpha(0.25),
        &FormatOptions::default(),
    )
    .unwrap();
    let (doc, content, res) = saved_page(out.bytes);
    assert_eq!(faded(&doc, &content, &res), (0.25, 1.0), "{content}");
}

#[test]
fn a_producer_alpha_in_effect_is_the_one_put_back() {
    let doc = page(
        "/GS0 gs BT /F1 12 Tf 72 700 Td (hello) Tj 0 -20 Td (world) Tj ET",
        "/ExtGState << /GS0 << /Type /ExtGState /ca 0.3 /LW 4 >> >>",
    );
    let mut s = EditSession::new(doc);
    let r = s
        .format_text(
            &FormatRequest::new(0, "hello").fill_alpha(0.8),
            &FormatOptions::default(),
        )
        .unwrap();
    assert_eq!(r.fill_alpha_change, Some((0.3, 0.8)));
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let (doc, content, res) = saved_page(bytes);
    assert_eq!(faded(&doc, &content, &res), (0.8, 0.3), "{content}");
    // The producer's own state is still there, untouched.
    assert!((ca(&doc, &res, b"GS0") - 0.3).abs() < 1e-9);
}

#[test]
fn an_unreadable_ambient_alpha_is_refused_not_guessed() {
    let doc = page(
        "/GSX gs BT /F1 12 Tf 72 700 Td (hello) Tj 0 -20 Td (world) Tj ET",
        "",
    );
    let mut s = EditSession::new(doc);
    let e = s
        .format_text(
            &FormatRequest::new(0, "hello").fill_alpha(0.5),
            &FormatOptions::default(),
        )
        .unwrap_err();
    assert!(matches!(e, FormatError::OpacityUnrestorable), "{e:?}");
}

#[test]
fn an_opacity_outside_zero_to_one_is_refused() {
    for bad in [-0.1, 1.5, f64::NAN] {
        let mut s = EditSession::new(page(PLAIN, ""));
        let e = s
            .format_text(
                &FormatRequest::new(0, "hello").fill_alpha(bad),
                &FormatOptions::default(),
            )
            .unwrap_err();
        assert!(matches!(e, FormatError::BadOpacity(_)), "{bad}: {e:?}");
    }
}

#[test]
fn a_faded_run_inside_a_form_binds_into_the_forms_own_resources() {
    let mut s = EditSession::new(form_page());
    s.format_text(
        &FormatRequest::new(0, "hello").fill_alpha(0.5),
        &FormatOptions::default(),
    )
    .unwrap();
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let xobjects = dict(&doc, pages[0].resources.get(b"XObject"));
    let form_ref = xobjects.get(b"Fm1").unwrap().clone();
    let form = dict(&doc, Some(&form_ref)).clone();
    let content = decoded(&doc, &form_ref);
    let res = dict(&doc, form.get(b"Resources")).clone();
    assert!(form.get(b"BBox").is_some(), "the form kept its dictionary");
    assert_eq!(faded(&doc, &content, &res), (0.5, 1.0), "{content}");
}

/// The last content stream of the saved page and that page's resources.
fn last_stream(bytes: Vec<u8>) -> (Document, String, Dict) {
    let doc = Document::from_bytes(bytes).unwrap();
    let pages = page_tree::pages(&doc).unwrap();
    let last = *pages[0].contents.last().unwrap();
    let content = decoded(&doc, &Object::Reference(last));
    let res = pages[0].resources.clone();
    (doc, content, res)
}

/// The key the first `gs` in `content` selects, which must sit inside the
/// run's own `q`.
fn first_gs_key(content: &str) -> String {
    let q = content.find("q\n").expect("the run's own q");
    let gs = content.find(" gs").expect("a gs");
    assert!(q < gs, "the gs is inside the q: {content}");
    content[..gs].rsplit('/').next().unwrap().trim().to_owned()
}

#[test]
fn added_text_is_faded_inside_its_own_q() {
    let doc = page(
        "/GS0 gs BT /F1 12 Tf 72 700 Td (hello) Tj ET",
        "/ExtGState << /GS0 << /Type /ExtGState /ca 0.3 >> >>",
    );
    let mut s = EditSession::new(doc);
    s.add_text(&AddTextRequest::new(0, (72.0, 600.0), "draft").with_fill_alpha(0.4))
        .unwrap();
    let (bytes, _) = s.to_incremental_bytes(&SaveOptions::identity()).unwrap();
    let (doc, content, res) = last_stream(bytes);
    let key = first_gs_key(&content);
    assert_ne!(key, "GS0");
    assert!(
        (ca(&doc, &res, key.as_bytes()) - 0.4).abs() < 1e-9,
        "{content}"
    );
    assert!(
        (ca(&doc, &res, b"GS0") - 0.3).abs() < 1e-9,
        "the producer's state survives"
    );
    assert!(content.trim_end().ends_with('Q'), "{content}");
}

#[test]
fn the_one_shot_add_text_binds_its_opacity() {
    let out = add_text(
        &page(PLAIN, ""),
        &AddTextRequest::new(0, (72.0, 600.0), "draft").with_fill_alpha(0.0),
    )
    .unwrap();
    let (doc, content, res) = last_stream(out.bytes);
    let key = first_gs_key(&content);
    assert!(ca(&doc, &res, key.as_bytes()).abs() < 1e-9, "{content}");
}

#[test]
fn added_text_without_an_opacity_writes_no_graphics_state() {
    let out = add_text(
        &page(PLAIN, ""),
        &AddTextRequest::new(0, (72.0, 600.0), "plain"),
    )
    .unwrap();
    let (_, content, res) = last_stream(out.bytes);
    assert!(!content.contains(" gs"), "{content}");
    assert!(res.get(b"ExtGState").is_none());
}

#[test]
fn an_added_text_opacity_outside_zero_to_one_is_refused() {
    for bad in [-0.5, 1.01, f64::NAN] {
        let mut s = EditSession::new(page(PLAIN, ""));
        let e = s
            .add_text(&AddTextRequest::new(0, (72.0, 600.0), "x").with_fill_alpha(bad))
            .unwrap_err();
        assert!(matches!(e, AddTextError::InvalidOpacity(_)), "{bad}: {e:?}");
    }
}
