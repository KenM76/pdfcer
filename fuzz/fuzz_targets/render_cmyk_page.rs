//! Fuzz target: a page RASTERISED through the subtractive colorant buffer.
//!
//! Every other `pdfcer-render` target stops at a parser or at the export
//! recorder; none rasterises into `cmyk_buffer`. Arbitrary whole-file bytes
//! would almost never declare a `/DeviceCMYK` page group, so this target
//! fixes the page and its resources (`../render_cmyk_page.rs`) and fuzzes
//! only the content stream, which reaches groups, knockout, overprint,
//! spot colorants, shadings and CMYK images by name.
//!
//! Input: byte 0 picks the render options (overprint zero-tint scope, spot
//! device model); the rest is the content stream. Rendered at 1x on a
//! 48x48 pt page, so an iteration stays cheap.

#![no_main]

#[path = "../render_cmyk_page.rs"]
mod render_cmyk_page;

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::settings::{OverprintZeroTintScope, SpotColorantDeviceModel};
use pdfcer_render::RenderOptions;

fuzz_target!(|data: &[u8]| {
    let Some((&mode, content)) = data.split_first() else {
        return;
    };
    let Ok(doc) = Document::from_bytes(render_cmyk_page::page_pdf(content)) else {
        return;
    };
    let Ok(pages) = pdfcer_core::page_tree::pages(&doc) else {
        return;
    };
    let Some(page) = pages.first() else {
        return;
    };
    let scope = match mode % 3 {
        0 => OverprintZeroTintScope::DeviceCmykOnly,
        1 => OverprintZeroTintScope::GreyAsKOnly,
        _ => OverprintZeroTintScope::AllProcessSpaces,
    };
    let model = if mode & 0x80 == 0 {
        SpotColorantDeviceModel::SimulateSeparations
    } else {
        SpotColorantDeviceModel::AlternateSpaceSubstitution
    };
    let options = RenderOptions::default()
        .with_overprint_zero_tint_scope(scope)
        .with_spot_colorant_device_model(model)
        .with_ink_probe(24, 24);
    let _ = pdfcer_render::render_page_with(&doc, page, 1.0, &options);
});
