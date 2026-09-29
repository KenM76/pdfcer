//! Content appended to a page must not inherit graphics state the page's own
//! content leaves in effect. ISO 32000-2 §8.4.2 requires only that `q`/`Q`
//! balance; a top-level `cm` that is never undone is conforming, and a
//! `/Contents` array is one concatenated stream (§7.8.2), so anything appended
//! after it is drawn under that transformation unless pdfcer isolates it.
//!
//! The fixture reproduces the shape of GitHub issue #1: `/Contents` is two
//! streams, the first leaving `1.1 0 0 1.1 0 0 cm` in effect. Every assertion
//! reads the saved-and-reloaded bytes through text extraction. Built inline
//! (project rule 7).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use pdfcer_core::bates::{BatesNumbering, BatesStamp};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, NewImage, OcrPageLayer};
use pdfcer_core::filters;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::ocr::layer::{ExistingLayers, OcrLayerOptions};
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect, WRAP_RESTORE, WRAP_SAVE, is_state_neutral};
use pdfcer_core::text_edit::AddTextRequest;
use pdfcer_core::text_extract::{self, ExtractOptions, TextRun};
use pdfcer_core::vector::Matrix;
use pdfcer_core::writer::SaveOptions;

/// One 400 x 400 page whose `/Contents` is `contents` (one stream each, as
/// objects 4, 5, …; `None` omits the key). A `(true, body)` entry is written
/// FlateDecode-compressed.
fn page_with(contents: Option<&[(bool, &[u8])]>) -> Vec<u8> {
    let stream = |flate: bool, body: &[u8]| {
        let (data, filter) = if flate {
            (filters::flate::encode(body), " /Filter /FlateDecode")
        } else {
            (body.to_vec(), "")
        };
        let mut out = format!("<< /Length {}{filter} >>\nstream\n", data.len()).into_bytes();
        out.extend_from_slice(&data);
        out.extend_from_slice(b"\nendstream");
        out
    };
    let items = contents.unwrap_or_default();
    let refs: Vec<String> = (0..items.len()).map(|i| format!("{} 0 R", i + 4)).collect();
    let contents_key = match contents {
        Some(_) => format!("/Contents [{}] ", refs.join(" ")),
        None => String::new(),
    };
    let mut bodies: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] {contents_key}\
             /Resources << /ExtGState << /GS0 << /CA 0.5 /ca 0.5 /BM /Multiply >> >> >> >>"
        )
        .into_bytes(),
    ];
    bodies.extend(items.iter().map(|(flate, body)| stream(*flate, body)));
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
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

const SQUARE: &[u8] = b"q\n0.5 g\n10 10 100 100 re f\nQ";

/// Issue #1's shape: a first stream that scales by 1.1 and never restores.
fn zoomed_page() -> Vec<u8> {
    leaky_page(b"1.1 0 0 1.1 0 0 cm")
}

fn leaky_page(leak: &[u8]) -> Vec<u8> {
    page_with(Some(&[(false, leak), (false, SQUARE)]))
}

fn session() -> EditSession {
    session_of(zoomed_page())
}

fn session_of(bytes: Vec<u8>) -> EditSession {
    EditSession::new(Document::from_bytes(bytes).expect("fixture loads"))
}

fn saved(s: &EditSession) -> Document {
    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    Document::from_bytes(bytes).expect("reload")
}

/// The saved page's `/Contents` elements, each with its decoded bytes.
fn contents(doc: &Document) -> Vec<(ObjId, Vec<u8>)> {
    let Object::Dict(page) = &doc.get(ObjId::new(3, 0)).expect("page").value else {
        panic!("page is not a dict");
    };
    let Some(value) = page.get(b"Contents") else {
        return Vec::new();
    };
    let items = match doc.resolve(value) {
        Object::Array(items) => items.clone(),
        _ => vec![value.clone()],
    };
    items
        .iter()
        .map(|item| {
            let Object::Reference(id) = item else {
                panic!("direct /Contents element {item:?}");
            };
            let Object::Stream(stream) = doc.resolve(item) else {
                panic!("{id:?} is not a stream");
            };
            let raw = stream.data_span.slice(doc.bytes()).expect("in range");
            (
                *id,
                filters::decode_stream(&stream.dict, raw).expect("decodes"),
            )
        })
        .collect()
}

/// Everything before the last `/Contents` element leaves no state, and the last
/// element (the overlay just appended) leaves none either.
fn assert_isolated(doc: &Document, route: &str) {
    let items = contents(doc);
    let (last, before) = items.split_last().expect("non-empty /Contents");
    let mut prefix = Vec::new();
    for (_, bytes) in before {
        prefix.extend_from_slice(bytes);
        prefix.push(b'\n');
    }
    assert!(
        is_state_neutral(&prefix),
        "{route}: content before the overlay leaks state:\n{}",
        String::from_utf8_lossy(&prefix)
    );
    assert!(
        is_state_neutral(&last.1),
        "{route}: the overlay itself leaks state:\n{}",
        String::from_utf8_lossy(&last.1)
    );
}

fn wrap_pairs(doc: &Document) -> usize {
    contents(doc).iter().filter(|(_, b)| b == WRAP_SAVE).count()
}

fn tiny_image() -> pdfcer_core::image_import::ImportedImage {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/images/rgb8.png");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    pdfcer_core::image_import::import(&bytes).expect("import")
}

fn ocr_page(word: &str) -> OcrPage {
    OcrPage {
        words: vec![RecognizedWord {
            text: word.to_owned(),
            rect: Rect::from_corners(72.0, 200.0, 200.0, 212.0),
            confidence: Some(0.9),
        }],
        confidence_available: true,
    }
}

fn ocr(s: &mut EditSession, word: &str, opts: &OcrLayerOptions) {
    let page = ocr_page(word);
    s.add_ocr_layer(
        &[OcrPageLayer {
            page_index: 0,
            recognised: &page,
        }],
        opts,
    )
    .expect("the layer is written");
}

const LEAKS: &[&[u8]] = &[
    b"1.1 0 0 1.1 0 0 cm",
    b"0 0 50 50 re W n",
    b"/GS0 gs",
    b"1 0 0 rg 0 0 1 RG",
    b"5 w [3] 0 d 1 J",
    b"BT 2 Tc 3 Tw 4 Ts 14 TL 3 Tr 50 Tz ET",
];

/// Every append route, as `(name, verb)`.
type Route = (&'static str, fn(&mut EditSession));

const ROUTES: &[Route] = &[
    ("add_text", |s| {
        s.add_text(&AddTextRequest::new(0, (100.0, 300.0), "Hello"))
            .expect("add_text");
    }),
    ("add_ocr_layer", |s| {
        ocr(s, "INVOICE", &OcrLayerOptions::new())
    }),
    ("add_image", |s| {
        let image = tiny_image();
        s.add_image(&NewImage::new(
            0,
            Rect::from_corners(10.0, 10.0, 110.0, 110.0),
            &image,
        ))
        .expect("add_image");
    }),
    ("stamp_bates", |s| {
        s.stamp_bates(&BatesStamp::new(BatesNumbering::new("ACME-", 4, "")), 1)
            .expect("stamp_bates");
    }),
    ("paste_objects", |s| {
        let clip = s.copy_objects(0, &[0]).expect("copy the square");
        s.paste_objects(0, &clip, Matrix::translate(40.0, 0.0))
            .expect("paste");
    }),
];

fn runs(s: &EditSession) -> Vec<TextRun> {
    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    let doc = Document::from_bytes(bytes).expect("reload");
    let pages = page_tree::pages(&doc).expect("page tree walks");
    text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("extract")
        .runs
}

/// Where the run containing `text` starts, in default user space.
fn start_of(runs: &[TextRun], text: &str) -> (f32, f32) {
    let run = runs
        .iter()
        .find(|r| r.text.contains(text))
        .unwrap_or_else(|| panic!("no run containing {text:?}"));
    (run.glyphs[0].x, run.glyphs[0].y)
}

fn near(a: f32, b: f32) -> bool {
    (a - b).abs() < 0.5
}

#[test]
fn an_ocr_word_lands_on_its_box_not_on_the_zoomed_box() {
    let mut s = session();
    let page = OcrPage {
        words: vec![RecognizedWord {
            text: "INVOICE".to_owned(),
            rect: Rect::from_corners(72.0, 200.0, 200.0, 212.0),
            confidence: Some(0.9),
        }],
        confidence_available: true,
    };
    s.add_ocr_layer(
        &[OcrPageLayer {
            page_index: 0,
            recognised: &page,
        }],
        &OcrLayerOptions::new(),
    )
    .expect("the layer is written");
    let (x, _) = start_of(&runs(&s), "INVOICE");
    assert!(
        near(x, 72.0),
        "OCR word starts at x = {x}, expected 72 (79.2 = zoomed)"
    );
}

#[test]
fn added_text_lands_at_its_origin_not_at_the_zoomed_origin() {
    let mut s = session();
    s.add_text(&AddTextRequest::new(0, (100.0, 300.0), "Hello"))
        .expect("text is added");
    let (x, y) = start_of(&runs(&s), "Hello");
    assert!(
        near(x, 100.0) && near(y, 300.0),
        "added text starts at ({x}, {y}), expected (100, 300); (110, 330) = zoomed"
    );
}

#[test]
fn every_route_isolates_every_kind_of_leaked_state() {
    for leak in LEAKS {
        for (route, verb) in ROUTES {
            let mut s = session_of(leaky_page(leak));
            verb(&mut s);
            let doc = saved(&s);
            let label = format!("{route} after {:?}", String::from_utf8_lossy(leak));
            assert_isolated(&doc, &label);
            assert_eq!(wrap_pairs(&doc), 1, "{label}");
        }
    }
}

#[test]
fn the_original_streams_are_kept_verbatim_inside_the_wrapper() {
    let mut s = session();
    ROUTES[0].1(&mut s);
    let items = contents(&saved(&s));
    let ids: Vec<u32> = items.iter().map(|(id, _)| id.num).collect();
    assert_eq!(items.len(), 5, "{ids:?}");
    assert_eq!(items[0].1, WRAP_SAVE);
    assert_eq!((items[1].0.num, items[2].0.num), (4, 5));
    assert_eq!(items[3].1, WRAP_RESTORE);
}

#[test]
fn repeated_appends_share_one_wrapper() {
    let mut s = session();
    for (_, verb) in ROUTES {
        verb(&mut s);
    }
    let doc = saved(&s);
    assert_eq!(wrap_pairs(&doc), 1);
    assert_eq!(contents(&doc).len(), 4 + ROUTES.len());
    assert_isolated(&doc, "all routes in turn");
}

#[test]
fn a_clean_page_is_not_wrapped() {
    let mut s = session_of(page_with(Some(&[(false, SQUARE)])));
    ROUTES[0].1(&mut s);
    let doc = saved(&s);
    assert_eq!(contents(&doc).len(), 2);
    assert_eq!(wrap_pairs(&doc), 0);
}

#[test]
fn a_page_without_contents_gets_just_the_overlay() {
    let mut s = session_of(page_with(None));
    ROUTES[0].1(&mut s);
    assert_eq!(contents(&saved(&s)).len(), 1);
}

#[test]
fn a_clean_tail_after_the_wrapper_needs_no_new_one() {
    let page = page_with(Some(&[
        (false, WRAP_SAVE),
        (false, b"1.1 0 0 1.1 0 0 cm"),
        (false, WRAP_RESTORE),
        (false, SQUARE),
    ]));
    let mut s = session_of(page);
    ROUTES[0].1(&mut s);
    let doc = saved(&s);
    assert_eq!(contents(&doc).len(), 5);
    assert_eq!(wrap_pairs(&doc), 1);
}

#[test]
fn a_dirty_tail_after_the_wrapper_is_wrapped_again() {
    let page = page_with(Some(&[
        (false, WRAP_SAVE),
        (false, SQUARE),
        (false, WRAP_RESTORE),
        (false, b"2 0 0 2 0 0 cm"),
    ]));
    let mut s = session_of(page);
    ROUTES[0].1(&mut s);
    let doc = saved(&s);
    assert_eq!(contents(&doc).len(), 7);
    assert_eq!(wrap_pairs(&doc), 2);
    assert_isolated(&doc, "dirty tail");
}

#[test]
fn a_compressed_wrapper_is_still_recognised() {
    // Too large to classify unwrapped, so only recognising the compressed
    // pair as pdfcer's wrapper avoids a second one.
    let mut original = b"1.1 0 0 1.1 0 0 cm
"
    .to_vec();
    original.extend(
        std::iter::repeat_n(
            b"% padding
"
            .as_slice(),
            8000,
        )
        .flatten(),
    );
    let page = page_with(Some(&[
        (true, WRAP_SAVE),
        (false, &original),
        (true, WRAP_RESTORE),
    ]));
    let mut s = session_of(page);
    ROUTES[0].1(&mut s);
    let doc = saved(&s);
    assert_eq!(contents(&doc).len(), 4);
    assert_eq!(wrap_pairs(&doc), 1);
    let (x, y) = start_of(&runs(&s), "Hello");
    assert!(near(x, 100.0) && near(y, 300.0), "({x}, {y})");
}

#[test]
fn a_foreign_q_stream_is_not_taken_for_the_wrapper() {
    let page = page_with(Some(&[
        (false, b"q"),
        (false, b"2 0 0 2 0 0 cm"),
        (false, b"Q"),
    ]));
    let mut s = session_of(page);
    ROUTES[0].1(&mut s);
    // The original is state-neutral as a whole, so no wrap is needed.
    let doc = saved(&s);
    assert_eq!(wrap_pairs(&doc), 0);
    assert_eq!(contents(&doc).len(), 4);
}

#[test]
fn removing_the_ocr_layer_restores_the_original_contents() {
    let mut s = session();
    ocr(&mut s, "INVOICE", &OcrLayerOptions::new());
    let layer = s.find_ocr_layers().expect("walks").remove(0);
    s.remove_ocr_layer(&layer).expect("removed");
    let ids: Vec<u32> = contents(&saved(&s)).iter().map(|(id, _)| id.num).collect();
    assert_eq!(ids, [4, 5]);
}

#[test]
fn replacing_the_ocr_layer_keeps_one_wrapper() {
    let mut s = session();
    ocr(&mut s, "FIRST", &OcrLayerOptions::new());
    ocr(
        &mut s,
        "SECOND",
        &OcrLayerOptions::new().with_existing(ExistingLayers::Replace),
    );
    let doc = saved(&s);
    assert_eq!(wrap_pairs(&doc), 1);
    assert_eq!(contents(&doc).len(), 5);
    let (x, _) = start_of(&runs(&s), "SECOND");
    assert!(near(x, 72.0), "{x}");
}

#[test]
fn removing_bates_restores_the_original_contents() {
    let mut s = session();
    ROUTES[3].1(&mut s);
    s.remove_bates(None).expect("removes");
    let ids: Vec<u32> = contents(&saved(&s)).iter().map(|(id, _)| id.num).collect();
    assert_eq!(ids, [4, 5]);
}

#[test]
fn a_full_rewrite_keeps_the_wrapper_recognisable() {
    let mut s = session();
    ROUTES[0].1(&mut s);
    let full = s.to_full_bytes(&SaveOptions::identity()).expect("full").0;
    let mut again = session_of(full);
    ROUTES[0].1(&mut again);
    let doc = saved(&again);
    assert_eq!(wrap_pairs(&doc), 1);
    assert_isolated(&doc, "after a full rewrite");
}

#[test]
fn the_one_shot_writers_isolate_their_overlay_and_reuse_the_wrapper() {
    use pdfcer_core::ocr::layer;
    use pdfcer_core::text_edit;

    let doc = Document::from_bytes(zoomed_page()).expect("fixture");
    let once = layer::add_ocr_layer(&doc, 0, &ocr_page("INVOICE"), &OcrLayerOptions::new())
        .expect("one-shot OCR")
        .bytes;
    let doc = Document::from_bytes(once).expect("reload");
    assert_isolated(&doc, "one-shot OCR");
    assert_eq!(wrap_pairs(&doc), 1);

    let twice = text_edit::add_text(&doc, &AddTextRequest::new(0, (100.0, 300.0), "Hello"))
        .expect("one-shot add_text")
        .bytes;
    let doc = Document::from_bytes(twice).expect("reload");
    assert_isolated(&doc, "one-shot add_text");
    assert_eq!(wrap_pairs(&doc), 1);
    assert_eq!(contents(&doc).len(), 6);

    let pages = page_tree::pages(&doc).expect("walks");
    let runs = text_extract::extract_page(&doc, &pages[0], 0, &ExtractOptions::default())
        .expect("extract")
        .runs;
    let (x, _) = start_of(&runs, "INVOICE");
    let (hx, hy) = start_of(&runs, "Hello");
    assert!(
        near(x, 72.0) && near(hx, 100.0) && near(hy, 300.0),
        "{x} ({hx}, {hy})"
    );
}
