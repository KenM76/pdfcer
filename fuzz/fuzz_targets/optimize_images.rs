//! Fuzz target: image rewrites in the optimizer
//! (`pdfcer_core::edit::EditSession::plan_optimize` / `apply_optimize`).
//!
//! The first byte picks an image action (downsample, JPEG, grey, bilevel,
//! palette, PNG-predicted Flate, RunLength, a pixel cap for an image drawn on no
//! page), and its top bit plans byte-identical images as one
//! (`merge_duplicates`); the rest is loaded as a PDF. Colour spaces, `/Indexed` lookups,
//! `/Decode` arrays and sample data all come from the input, so planning
//! and applying must never panic, and an applied plan must save.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::optimize::{OptimizeSettings, StreamAction};

const ACTIONS: [&str; 11] = [
    "image(ppi=36,above=36)",
    "image(encode=jpeg,quality=50)",
    "image(convert=gray)",
    "image(convert=bilevel,threshold=dither,ppi=36,above=36)",
    "image(convert=indexed,colors=16)",
    "image(convert=bilevel,threshold=128,encode=flate)",
    "image(encode=flate,predictor=png)",
    "image(convert=indexed,colors=4,predictor=png,ppi=36,above=36)",
    "image(ppi=36,above=36,max-pixels=64)",
    "image(convert=bilevel,threshold=128,encode=runlength)",
    "image(encode=lzw,predictor=auto)",
];

fuzz_target!(|data: &[u8]| {
    let Some((&pick, pdf)) = data.split_first() else {
        return;
    };
    let Ok(doc) = Document::from_bytes(pdf.to_vec()) else {
        return;
    };
    let Some(action) = ACTIONS.get(usize::from(pick & 0x7F) % ACTIONS.len()) else {
        return;
    };
    let Ok(streams) = StreamAction::parse(action) else {
        return;
    };
    let mut settings = OptimizeSettings::new();
    settings.streams = streams;
    settings.structure.merge_duplicates = pick & 0x80 != 0;
    let mut session = EditSession::new(doc);
    let Ok(plan) = session.plan_optimize(&settings) else {
        return;
    };
    if session.apply_optimize(&plan).is_ok() {
        let _ = session.to_full_bytes(&Default::default());
    }
});
