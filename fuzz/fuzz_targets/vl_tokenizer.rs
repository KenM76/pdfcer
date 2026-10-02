//! Fuzz target: the PaddleOCR-VL tokenizer reader and preprocessing
//! (`pdfcer_core::ocr::{json_lite, vl_tokenizer, vl_pre}`).
//!
//! `tokenizer.json` comes from an operator-installed add-on, so it is
//! untrusted. Invariants: `json_lite::parse` and `VlTokenizer::from_json_bytes`
//! return `Ok` or an error and never panic or recurse unboundedly; a loaded
//! tokenizer's `encode`/`decode` never panic on any text or id; over the fixed
//! tokenizer below, text drawn from its vocabulary survives encode then
//! decode unchanged; `vl_pre` resize/patchify/line placement never panic and
//! keep every placed line inside the image.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::ocr::json_lite;
use pdfcer_core::ocr::vl_pre;
use pdfcer_core::ocr::vl_tokenizer::VlTokenizer;

/// The PaddleOCR-VL file's shape in miniature.
const FIXED: &str = r#"{
  "added_tokens": [
    {"id": 0, "content": "<unk>", "special": true},
    {"id": 2, "content": "</s>", "special": true},
    {"id": 20, "content": "<|IMG|>", "special": true}
  ],
  "normalizer": {"type": "Replace", "pattern": {"String": " "}, "content": "▁"},
  "pre_tokenizer": null,
  "decoder": {"type": "Sequence", "decoders": [
    {"type": "Replace", "pattern": {"String": "▁"}, "content": " "},
    {"type": "ByteFallback"}, {"type": "Fuse"}]},
  "model": {"type": "BPE", "dropout": null, "unk_token": "<unk>",
    "fuse_unk": true, "byte_fallback": true, "ignore_merges": false,
    "vocab": {"<unk>": 0, "</s>": 2, "a": 3, "b": 4, "▁": 5, "ab": 6,
      "▁ab": 7, "<0x0A>": 8},
    "merges": ["a b", ["▁", "ab"]]}
}"#;

fn ids(data: &[u8]) -> Vec<u32> {
    data.chunks_exact(4)
        .map(|q| u32::from_le_bytes([q[0], q[1], q[2], q[3]]))
        .collect()
}

fn exercise(tok: &VlTokenizer, data: &[u8]) {
    let text = String::from_utf8_lossy(data);
    let _ = tok.encode(&text);
    let _ = tok.decode(&ids(data), true);
    let _ = tok.decode(&ids(data), false);
}

fn preprocess(data: &[u8]) {
    let [w, h, tw, th, rest @ ..] = data else {
        return;
    };
    let (w, h) = (u32::from(*w % 48) + 1, u32::from(*h % 48) + 1);
    let mut pixels = rest.to_vec();
    pixels.resize((w * h) as usize, 255);
    let Ok(img) = vl_pre::grey(w, h, &pixels) else {
        return;
    };
    let _ = vl_pre::smart_resize(usize::from(*th) * 9 + 1, usize::from(*tw) * 9 + 1);
    let small = vl_pre::resize(&img, usize::from(*tw % 64) + 1, usize::from(*th % 64) + 1);
    let _ = vl_pre::patchify(&small);
    if let Some(bx) = vl_pre::ink_bbox(&img) {
        let _ = vl_pre::crop(&img, bx, 4);
        let text = String::from_utf8_lossy(rest);
        let (lines, _) = vl_pre::place_lines(&img, bx, &text, Some(0.5));
        for l in &lines {
            let r = l.rect;
            assert!(r.llx >= 0.0 && r.lly >= 0.0, "line outside the image");
            assert!(
                r.urx <= f64::from(w) && r.ury <= f64::from(h),
                "line outside the image"
            );
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let _ = json_lite::parse(data);
    if let Ok(tok) = VlTokenizer::from_json_bytes(data) {
        exercise(&tok, b"ab c\n\xc3\xa9<|x|>");
    }
    let Ok(fixed) = VlTokenizer::from_json_bytes(FIXED.as_bytes()) else {
        panic!("the fixed tokenizer must load");
    };
    exercise(&fixed, data);
    let text: String = data
        .iter()
        .map(|b| match b % 4 {
            0 => 'a',
            1 => 'b',
            2 => ' ',
            _ => '\n',
        })
        .collect();
    if let Ok(enc) = fixed.encode(&text) {
        assert_eq!(
            fixed.decode(&enc, true),
            text,
            "encode then decode changed the text"
        );
    }
    preprocess(data);
});
