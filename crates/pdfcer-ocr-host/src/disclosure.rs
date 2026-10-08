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
        Some(StopReason::Repetition) => format!(
            "; WARNING: the model repeated one line {} times and was stopped as looping, \
             so text after the repeats is lost",
            pdfcer_core::ocr::vl_decode::MAX_REPEATED_LINES
        ),
        Some(_) => format!(
            "; {} token(s) from {} image token(s)",
            r.tokens, r.image_tokens
        ),
        None => String::new(),
    };
    format!("{lead}. Last page: {placement}{stop}.")
}

/// The last PaddleOCR-VL page a runner read, kept for its disclosure.
#[cfg(feature = "ocr-vl")]
#[derive(Debug, Default)]
pub(crate) struct VlLast {
    region: Option<pdfcer_core::ocr::engine_paddle_vl::RegionReading>,
    /// The page's layout reading, when it was read by layout.
    pub(crate) layout: Option<pdfcer_core::ocr::vl_page::LayoutReading>,
}

#[cfg(feature = "ocr-vl")]
impl VlLast {
    /// A page read as one region.
    pub(crate) fn from_region(r: pdfcer_core::ocr::engine_paddle_vl::RegionReading) -> Self {
        Self {
            region: Some(r),
            layout: None,
        }
    }

    /// A page read by layout.
    pub(crate) fn from_layout(r: pdfcer_core::ocr::vl_page::LayoutReading) -> Self {
        Self {
            region: None,
            layout: Some(r),
        }
    }

    /// The rule-4 line for this page.
    pub(crate) fn disclosure(&self) -> String {
        match &self.layout {
            Some(l) => paddle_vl_layout_disclosure(l),
            None => paddle_vl_disclosure(self.region.as_ref()),
        }
    }
}

/// What reading by layout inferred: which regions were found and skipped,
/// that line boxes come from each region's ink, that table cell boxes are an
/// even grid, and any region whose decoding was cut short.
#[cfg(feature = "ocr-vl")]
#[must_use]
pub fn paddle_vl_layout_disclosure(l: &pdfcer_core::ocr::vl_page::LayoutReading) -> String {
    use pdfcer_core::ocr::vl_decode::StopReason;
    let lead = "PaddleOCR-VL read the page by layout (PP-DocLayoutV3): one box per LINE, \
                INFERRED from each region's ink";
    if l.whole_page {
        return format!(
            "{lead}. The layout model found no region, so the whole page was read as one \
             untagged region."
        );
    }
    let pictures = l.regions.iter().filter(|r| r.task.is_none()).count();
    let grids = l.regions.iter().filter(|r| r.grid_placed).count();
    let mut out = format!(
        "{lead}. {} region(s) found, {pictures} picture(s) not read",
        l.regions.len()
    );
    if grids > 0 {
        out.push_str(&format!(
            "; {grids} table(s): the model reports cell structure only, so each cell's box \
             is an even grid over the table and may sit off its text"
        ));
    }
    for (i, r) in l.regions.iter().enumerate() {
        let Some(reading) = &r.reading else { continue };
        let why = match reading.stop {
            Some(StopReason::TokenLimit) => "hit the token ceiling; its text may be cut short",
            Some(StopReason::Repetition) => {
                "was stopped as looping; text after the repeats is lost"
            }
            _ => continue,
        };
        out.push_str(&format!(
            "; WARNING: region {} ({}) {why}",
            i + 1,
            r.region.class
        ));
    }
    out.push('.');
    out
}
