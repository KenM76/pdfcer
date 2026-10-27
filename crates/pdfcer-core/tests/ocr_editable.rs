//! The editable-text OCR output end to end over a synthetic scan: glyph
//! pixels erased, visible coloured text written, suspects left in the image,
//! one undo, shared images copied, and the refusals.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::Read;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, OcrPageLayer};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::ocr::editable::{EditableTextOptions, OcrOutput, SuspectReason};
use pdfcer_core::ocr::layer::{OcrLayerError, OcrLayerOptions, OcrLayerReport, add_ocr_layer};
use pdfcer_core::ocr::{OcrPage, RecognizedWord};
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::writer::SaveOptions;

const W: usize = 300;
const H: usize = 100;
const PAPER: u8 = 235;
const INK: u8 = 30;

/// Glyph-like strokes: three columns on, two off, rows 30..50.
const BARS: [(usize, usize); 3] = [(20, 90), (110, 180), (200, 280)];

fn scan() -> Vec<u8> {
    let mut px = vec![PAPER; W * H];
    for &(c0, c1) in &BARS {
        for r in 30..50 {
            for c in c0..c1 {
                if (c - c0) % 5 < 3 {
                    px[r * W + c] = INK;
                }
            }
        }
    }
    px
}

/// Object 1 catalog, 2 pages, 3 the image, then (page, content) per entry
/// of `draws` (each page's content draws `Im1` that many times).
fn build(draws: &[usize]) -> Document {
    let image = scan();
    let mut bodies: Vec<Vec<u8>> = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        Vec::new(),
        [
            format!(
                "<< /Type /XObject /Subtype /Image /Width {W} /Height {H} \
                 /ColorSpace /DeviceGray /BitsPerComponent 8 /Length {} >>\nstream\n",
                image.len()
            )
            .into_bytes(),
            image,
            b"\nendstream".to_vec(),
        ]
        .concat(),
    ];
    let mut kids = Vec::new();
    for (i, &n) in draws.iter().enumerate() {
        let page = 4 + 2 * i;
        kids.push(format!("{page} 0 R"));
        bodies.push(
            format!(
                "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {W} {H}] \
                 /Resources << /XObject << /Im1 3 0 R >> >> /Contents {} 0 R >>",
                page + 1
            )
            .into_bytes(),
        );
        let data = format!("q {W} 0 0 {H} 0 0 cm /Im1 Do Q\n").repeat(n);
        bodies
            .push(format!("<< /Length {} >>\nstream\n{data}\nendstream", data.len()).into_bytes());
    }
    bodies[1] = format!(
        "<< /Type /Pages /Kids [{}] /Count {} >>",
        kids.join(" "),
        draws.len()
    )
    .into_bytes();
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        buf.extend_from_slice(body);
        buf.extend_from_slice(b"\nendobj\n");
    }
    let xref = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
    for off in offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    Document::from_bytes(buf).expect("fixture parses")
}

/// A word over bar `k` (pixel row r is y = H - r).
fn word(text: &str, k: usize, confidence: f32) -> RecognizedWord {
    let (c0, c1) = BARS[k];
    RecognizedWord {
        text: text.to_owned(),
        rect: Rect::from_corners(c0 as f64 - 4.0, 46.0, c1 as f64 + 4.0, 74.0),
        confidence: Some(confidence),
    }
}

fn recognised() -> OcrPage {
    OcrPage {
        words: vec![
            word("ALPHA", 0, 0.9),
            word("BETA", 1, 0.9),
            word("GAMMA", 2, 0.2),
        ],
        confidence_available: true,
        ..OcrPage::default()
    }
}

fn editable() -> OcrLayerOptions {
    OcrLayerOptions::new().with_output(OcrOutput::EditableText(EditableTextOptions::new()))
}

fn run(
    s: &mut EditSession,
    page_index: usize,
    page: &OcrPage,
) -> Result<Vec<OcrLayerReport>, OcrLayerError> {
    s.add_ocr_layer(
        &[OcrPageLayer {
            page_index,
            recognised: page,
        }],
        &editable(),
    )
}

/// The decoded samples of the image page `page_index` draws as `Im1`.
fn page_image(doc: &Document, page_index: usize) -> (ObjId, Vec<u8>) {
    let pages = page_tree::pages(doc).unwrap();
    let view = doc.view();
    let xobjects = view
        .resolve(pages[page_index].resources.get(b"XObject").unwrap())
        .as_dict()
        .unwrap()
        .clone();
    let id = xobjects.get(b"Im1").unwrap().as_reference().unwrap();
    let Object::Stream(s) = view.resolved(id) else {
        panic!("image is a stream")
    };
    let raw = view.slice(s.data_span).unwrap().to_vec();
    let flate =
        matches!(s.dict.get(b"Filter"), Some(Object::Name(n)) if n.as_bytes() == b"FlateDecode");
    let data = if flate {
        let mut out = Vec::new();
        flate2::read::ZlibDecoder::new(raw.as_slice())
            .read_to_end(&mut out)
            .unwrap();
        out
    } else {
        raw
    };
    (id, data)
}

fn ink_left(px: &[u8], k: usize) -> usize {
    let (c0, c1) = BARS[k];
    (30..50)
        .flat_map(|r| (c0..c1).map(move |c| r * W + c))
        .filter(|&i| px[i] < 128)
        .count()
}

fn saved(s: &EditSession) -> Document {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    Document::from_bytes(bytes).unwrap()
}

#[test]
fn recognised_words_leave_the_scan_and_suspects_stay() {
    let mut s = EditSession::new(build(&[1]));
    let r = run(&mut s, 0, &recognised()).unwrap().remove(0);
    assert!(r.editable);
    assert_eq!(r.words_visible, 2);
    assert_eq!(r.words_suspect, 1);
    assert_eq!(r.suspects, vec![(SuspectReason::LowConfidence, 1)]);
    assert_eq!(r.images_rewritten.len(), 1);
    let img = &r.images_rewritten[0];
    assert!(!img.cloned);
    assert_eq!(img.image, 3);
    assert_eq!(img.filter_before, "none");
    assert_eq!(img.filter_after, "FlateDecode");
    assert_eq!(img.words_erased, 2);
    assert!(r.disclosures().iter().any(|l| l.contains("Helvetica")));

    let doc = saved(&s);
    let (_, px) = page_image(&doc, 0);
    assert_eq!(px.len(), W * H);
    assert_eq!(ink_left(&px, 0), 0, "ALPHA erased");
    assert_eq!(ink_left(&px, 1), 0, "BETA erased");
    assert!(ink_left(&px, 2) > 0, "GAMMA still in the image");

    let layer = &s.find_ocr_layers().unwrap()[0];
    assert!(layer.editable);
    let view = s.view();
    let Object::Stream(st) = view.resolved(layer.content) else {
        panic!()
    };
    let content = String::from_utf8_lossy(view.slice(st.data_span).unwrap()).into_owned();
    assert!(content.contains("/Output /EditableText"), "{content}");
    assert!(content.contains("0 Tr"), "{content}");
    assert!(content.contains(" g\n"), "ink colour written: {content}");
    // Visible ink-coloured words, then back to mode 3 for the suspect.
    let visible_at = content.find("0 Tr").unwrap();
    let hidden_at = content.rfind("3 Tr").unwrap();
    assert!(content.find("0.11764705882352941 g").unwrap() < visible_at);
    assert!(visible_at < content.find("(B)").unwrap());
    assert!(hidden_at > visible_at && hidden_at < content.find("(G)").unwrap());
}

#[test]
fn one_undo_restores_the_scan() {
    let mut s = EditSession::new(build(&[1]));
    run(&mut s, 0, &recognised()).unwrap();
    assert_eq!(s.undo_depth(), 1);
    s.undo().unwrap();
    let (_, px) = page_image(&saved(&s), 0);
    assert_eq!(px, scan());
    assert!(s.find_ocr_layers().unwrap().is_empty());
}

#[test]
fn an_editable_page_refuses_rerun_and_removal() {
    let mut s = EditSession::new(build(&[1]));
    run(&mut s, 0, &recognised()).unwrap();
    for opts in [editable(), OcrLayerOptions::new()] {
        let again = s.add_ocr_layer(
            &[OcrPageLayer {
                page_index: 0,
                recognised: &recognised(),
            }],
            &opts,
        );
        assert!(matches!(
            again,
            Err(OcrLayerError::EditableLayerPresent { page_index: 0 })
        ));
    }
    let layer = s.find_ocr_layers().unwrap().remove(0);
    assert!(matches!(
        s.remove_ocr_layer(&layer),
        Err(OcrLayerError::EditableLayerPresent { page_index: 0 })
    ));
}

#[test]
fn a_shared_image_is_copied_for_the_recognised_page() {
    let mut s = EditSession::new(build(&[1, 1]));
    let r = run(&mut s, 0, &recognised()).unwrap().remove(0);
    let img = &r.images_rewritten[0];
    assert!(img.cloned);
    assert_eq!(img.source, 3);
    assert_ne!(img.image, 3);
    let doc = saved(&s);
    let (id0, px0) = page_image(&doc, 0);
    let (id1, px1) = page_image(&doc, 1);
    assert_eq!(id0.num, img.image);
    assert_eq!(id1.num, 3);
    assert_eq!(ink_left(&px0, 0), 0);
    assert_eq!(px1, scan(), "the other page keeps the original");
}

#[test]
fn an_image_drawn_twice_converts_nothing() {
    let mut s = EditSession::new(build(&[2]));
    let r = run(&mut s, 0, &recognised()).unwrap().remove(0);
    assert_eq!(r.words_visible, 0);
    assert!(r.images_rewritten.is_empty());
    assert_eq!(
        r.suspects,
        vec![
            (SuspectReason::LowConfidence, 1),
            (SuspectReason::ImageDrawnTwice, 2)
        ]
    );
}

#[test]
fn the_one_shot_writes_the_same_output() {
    let doc = build(&[1, 1]);
    let out = add_ocr_layer(&doc, 0, &recognised(), &editable()).unwrap();
    assert_eq!(out.report.words_visible, 2);
    let saved = Document::from_bytes(out.bytes).unwrap();
    let (id0, px0) = page_image(&saved, 0);
    let (_, px1) = page_image(&saved, 1);
    assert_eq!(id0.num, out.report.images_rewritten[0].image);
    assert_eq!(ink_left(&px0, 1), 0);
    assert_eq!(px1, scan());
}

#[test]
fn the_searchable_default_leaves_the_scan_alone() {
    let mut s = EditSession::new(build(&[1]));
    let r = s
        .add_ocr_layer(
            &[OcrPageLayer {
                page_index: 0,
                recognised: &recognised(),
            }],
            &OcrLayerOptions::new(),
        )
        .unwrap()
        .remove(0);
    assert!(!r.editable);
    assert!(r.images_rewritten.is_empty());
    assert_eq!(page_image(&saved(&s), 0).1, scan());
    assert!(!s.find_ocr_layers().unwrap()[0].editable);
}

/// The layer content of page `page_index`'s editable layer.
fn layer_text(s: &EditSession) -> String {
    let layer = &s.find_ocr_layers().unwrap()[0];
    let view = s.view();
    let Object::Stream(st) = view.resolved(layer.content) else {
        panic!()
    };
    String::from_utf8_lossy(view.slice(st.data_span).unwrap()).into_owned()
}

/// The operand before the first `op` in `content`.
fn operand(content: &str, op: &str, back: usize) -> f64 {
    let toks: Vec<&str> = content.split_whitespace().collect();
    let at = toks.iter().position(|t| *t == op).unwrap();
    toks[at - back].parse().unwrap()
}

#[test]
fn visible_text_is_sized_from_the_ink_not_the_padded_box() {
    let mut s = EditSession::new(build(&[1]));
    run(&mut s, 0, &recognised()).unwrap();
    let content = layer_text(&s);
    // Ink rows 30..50 are y 50..70: cap height 20 over Helvetica's 0.718.
    let size = operand(&content, "Tf", 1);
    assert!((size - 20.0 / 0.718).abs() < 0.5, "size {size}: {content}");
    let baseline = operand(&content, "Tm", 1);
    assert!(
        (baseline - 50.0).abs() < 0.5,
        "baseline {baseline}: {content}"
    );
}

#[test]
fn a_glyph_the_engine_box_clips_is_erased_whole() {
    let mut page = recognised();
    // Starting inside the first stroke and ending inside the last.
    page.words[0].rect = Rect::from_corners(21.0, 46.0, 86.0, 74.0);
    let mut s = EditSession::new(build(&[1]));
    run(&mut s, 0, &page).unwrap();
    let (_, px) = page_image(&saved(&s), 0);
    assert_eq!(ink_left(&px, 0), 0, "clipped strokes erased whole");
    assert!(ink_left(&px, 2) > 0, "the suspect keeps its pixels");
}
