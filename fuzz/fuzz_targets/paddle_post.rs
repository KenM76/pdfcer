//! Fuzz target: PaddleOCR post-processing (`pdfcer_core::ocr::paddle_post`).
//!
//! The ONNX models are operator-supplied, so their outputs are untrusted.
//! Invariant: for arbitrary probability maps, CTC outputs and dictionaries,
//! `db_boxes` and `ctc_decode` return `Ok` or `PostError` and never panic;
//! every box lies inside the destination image, and every word has a finite,
//! non-inverted rectangle and a confidence in 0..=1.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::ocr::paddle_post::{
    ModelInput, ctc_decode, db_boxes, parse_dictionary, sort_reading_order, words_from_line,
};

fuzz_target!(|data: &[u8]| {
    let [a, b, c, d, rest @ ..] = data else {
        return;
    };
    let floats: Vec<f32> = rest
        .chunks_exact(4)
        .map(|q| f32::from_le_bytes([q[0], q[1], q[2], q[3]]))
        .collect();

    // Detection: a map of up to 64x64 scaled to up to 1024x1024.
    let (map_w, map_h) = (usize::from(*a % 64) + 1, usize::from(*b % 64) + 1);
    let (dest_w, dest_h) = (u32::from(*c) * 4 + 1, u32::from(*d) * 4 + 1);
    // Sized to the map so most inputs reach the box finder, not the shape check.
    let mut map = floats.clone();
    map.resize(map_w * map_h, 0.9);
    if let Ok(mut boxes) = db_boxes(&map, map_w, map_h, dest_w, dest_h) {
        sort_reading_order(&mut boxes);
        for b in &boxes {
            assert!(b.x0 < b.x1 && b.x1 <= dest_w && b.y0 < b.y1 && b.y1 <= dest_h);
        }
    }

    // Recognition: the same floats as a steps x classes matrix.
    let dict = parse_dictionary(&String::from_utf8_lossy(
        rest.get(..usize::from(*c % 32)).unwrap_or(&[]),
    ));
    let classes = dict.len() + 1 + usize::from(*d % 2);
    let steps = floats.len() / classes;
    let probs = floats.get(..steps * classes).unwrap_or(&[]);
    if let Ok(chars) = ctc_decode(probs, steps, classes, &dict) {
        let input = ModelInput {
            data: Vec::new(),
            width: u32::from(*a) + 1,
            height: 48,
            content_width: u32::from(*b) + 1,
        };
        let crop = pdfcer_core::ocr::paddle_post::TextBox {
            x0: 10,
            y0: 10,
            x1: 10 + u32::from(*c) + 1,
            y1: 40,
            score: 1.0,
        };
        for w in words_from_line(&chars, steps, &input, &crop) {
            let r = w.rect;
            assert!(
                r.llx.is_finite() && r.lly.is_finite() && r.urx.is_finite() && r.ury.is_finite()
            );
            assert!(r.llx <= r.urx && r.lly <= r.ury);
            if let Some(c) = w.confidence {
                assert!((0.0..=1.0).contains(&c));
            }
        }
    }
});
