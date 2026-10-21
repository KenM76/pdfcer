//! Fuzz target: image rewrites in the optimizer
//! (`pdfcer_core::edit::EditSession::plan_optimize` / `apply_optimize`).
//!
//! The first byte picks an image action (downsample, JPEG, grey, bilevel,
//! palette); the rest is loaded as a PDF. Colour spaces, `/Indexed`
//! lookups, `/Decode` arrays and sample data all come from the input, so
//! planning and applying must never panic, and an applied plan must save.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::optimize::{OptimizeSettings, StreamAction};

const ACTIONS: [&str; 6] = [
    "image(ppi=36,above=36)",
    "image(encode=jpeg,quality=50)",
    "image(convert=gray)",
    "image(convert=bilevel,threshold=dither,ppi=36,above=36)",
    "image(convert=indexed,colors=16)",
    "image(convert=bilevel,threshold=128,encode=flate)",
];

fuzz_target!(|data: &[u8]| {
    let Some((&pick, pdf)) = data.split_first() else {
        return;
    };
    let Ok(doc) = Document::from_bytes(pdf.to_vec()) else {
        return;
    };
    let Some(action) = ACTIONS.get(usize::from(pick) % ACTIONS.len()) else {
        return;
    };
    let Ok(streams) = StreamAction::parse(action) else {
        return;
    };
    let mut settings = OptimizeSettings::new();
    settings.streams = streams;
    let mut session = EditSession::new(doc);
    let Ok(plan) = session.plan_optimize(&settings) else {
        return;
    };
    if session.apply_optimize(&plan).is_ok() {
        let _ = session.to_full_bytes(&Default::default());
    }
});
