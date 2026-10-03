//! Rule-4 disclosure lines for in-process engines: what the engine chose or
//! inferred on the operator's behalf, for the shell's report.

/// Which dictionary PaddleOCR's recogniser decoded with.
#[cfg(feature = "paddle")]
#[must_use]
pub fn paddle_disclosure(engine: &pdfcer_core::ocr::engine_paddle::PaddleEngine) -> String {
    use pdfcer_core::ocr::engine_paddle::{DICTIONARY, DictionarySource};
    match engine.dictionary_source() {
        DictionarySource::File(p) => format!(
            "PaddleOCR dictionary: {} ({} entries)",
            p.display(),
            engine.dictionary_len()
        ),
        _ => format!(
            "PaddleOCR dictionary: no {DICTIONARY}, so the list embedded in \
             rec.onnx was used ({} entries)",
            engine.dictionary_len()
        ),
    }
}

/// PaddleOCR-VL's text is per region and its line boxes are inferred from
/// the ink, not reported by the model; `reading` is the last page's, when
/// one was read.
#[cfg(feature = "ocr-vl")]
#[must_use]
pub fn paddle_vl_disclosure(
    reading: Option<&pdfcer_core::ocr::engine_paddle_vl::RegionReading>,
) -> String {
    use pdfcer_core::ocr::vl_decode::StopReason;
    use pdfcer_core::ocr::vl_pre::LinePlacement;
    let lead = "PaddleOCR-VL reads the page's ink as ONE region: the text layer is \
                region-aligned, one box per LINE and never per word, and those boxes are \
                INFERRED from the ink, not reported by the model";
    let Some(r) = reading else {
        return format!("{lead}.");
    };
    let placement = match r.placement {
        LinePlacement::Bands => "each line placed on its own band of inked rows",
        LinePlacement::Even => {
            "the line count did not match the inked bands, so the lines divide the ink box \
             evenly and may sit off their text"
        }
        _ => "the page had no ink, so the model was not run",
    };
    let stop = match r.stop {
        Some(StopReason::TokenLimit) => format!(
            "; WARNING: decoding hit the {}-token ceiling, so the text may be cut short",
            r.tokens
        ),
        Some(_) => format!(
            "; {} token(s) from {} image token(s)",
            r.tokens, r.image_tokens
        ),
        None => String::new(),
    };
    format!("{lead}. Last page: {placement}{stop}.")
}
