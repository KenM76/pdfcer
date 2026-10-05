//! [`OcrLayerReport`]: what an OCR-layer write did and inferred.

use super::structure::OcrStructureSource;

/// What the layer write did, and everything it inferred — the rule-4
/// disclosure, in the off-canvas form decision 059 requires.
///
/// Every field here answers a question a shell or the CLI must be able to put
/// in front of the operator **without** marking anything on the page. Nothing
/// in this struct is optional to surface: a caller that builds a layer and
/// drops the report has made pdfcer silent about a page of guesses.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OcrLayerReport {
    /// How many words were written into the layer.
    pub words_written: usize,
    /// How many words were dropped because they could not be positioned.
    ///
    /// A word is dropped only when it is geometrically meaningless — empty
    /// text, a zero-or-negative-area box, a non-finite coordinate. It is a
    /// count rather than a silent filter because a large number here means the
    /// engine and the page geometry disagree, which is a real diagnosis and
    /// not a detail.
    pub words_skipped: usize,
    /// How many words contained at least one character with no WinAnsi code.
    ///
    /// Each such character was written as `?`. **A high count relative to
    /// [`Self::words_written`] means the page is in a script a Standard-14
    /// face cannot represent** (CJK, Cyrillic, Greek, Arabic) — the named
    /// limit from this module's header, detectable here rather than by reading
    /// a page of question marks.
    pub words_substituted: usize,
    /// How many words had their horizontal scaling clamped to
    /// [`MIN_TZ`](super::layer::MIN_TZ)/[`MAX_TZ`](super::layer::MAX_TZ).
    ///
    /// Usually an engine artefact (a merged rule line, a box collapsed to a
    /// sliver) rather than a pdfcer fault, which is exactly why it is reported:
    /// it is the operator's cue that a selection in that spot will not track
    /// the ink.
    pub words_scale_clamped: usize,
    /// How many lines were written.
    pub lines_written: usize,
    /// How many blocks were written, each one text object, in reading order.
    pub blocks_written: usize,
    /// Whether the lines and blocks were the engine's or pdfcer's inference.
    pub structure: OcrStructureSource,
    /// Mean confidence across words that reported one, or `None`.
    ///
    /// `None` means *no word reported a confidence*, which is a different
    /// statement from "confidence is low" and must be presented as one.
    pub mean_confidence: Option<f32>,
    /// Whether the engine reported per-word confidence **at all**.
    ///
    /// Carried through from [`OcrPage::confidence_available`](super::OcrPage::confidence_available) so a caller can
    /// say *"this engine reports no per-word confidence"* rather than
    /// presenting unscored guesses as though they had been checked.
    pub confidence_available: bool,
    /// The object number of the created content stream, or 0 before saving.
    pub content_object: u32,
    /// The object number of the created font dictionary, or 0 before saving.
    pub font_object: u32,
    /// How many earlier pdfcer layers on the page this write removed
    /// ([`ExistingLayers::Replace`](super::layer::ExistingLayers::Replace)).
    pub layers_replaced: usize,
}

impl OcrLayerReport {
    /// Human-readable disclosure lines, ready for a CLI to print or a panel to
    /// list.
    ///
    /// Built here rather than at each call site so the GUI and the CLI cannot
    /// disagree about what was disclosed — the same reason
    /// [`crate::text_edit::add_text`] carries its disclosures on the report.
    /// Deliberately says **nothing** when there is nothing to say: a report
    /// that always emits a paragraph trains the reader to skip it.
    #[must_use]
    pub fn disclosures(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "OCR text layer: {} text box(es) written (a word or a line each, \
             as the engine reports them), invisible (text rendering mode 3) — \
             the page renders exactly as it did before.",
            self.words_written
        ));
        let inferred = match self.structure {
            OcrStructureSource::Reported => None,
            OcrStructureSource::BlocksInferred => Some("blocks were"),
            _ => Some("lines and blocks were"),
        };
        if let Some(what) = inferred.filter(|_| self.blocks_written > 0) {
            out.push(format!(
                "Reading order: {} line(s) in {} block(s); the {what} inferred \
                 from word positions, not reported by the engine.",
                self.lines_written, self.blocks_written
            ));
        }
        if self.confidence_available {
            if let Some(mean) = self.mean_confidence {
                out.push(format!(
                    "Mean recognition confidence {:.1}%. Every word is a guess; \
                     review before relying on the text.",
                    f64::from(mean) * 100.0
                ));
            }
        } else {
            out.push(
                "This engine reports NO per-word confidence, so no word here \
                 has been scored either way — that is not the same as a high \
                 score."
                    .to_owned(),
            );
        }
        if self.layers_replaced > 0 {
            out.push(format!(
                "Replaced {} earlier OCR layer(s) pdfcer wrote on this page.",
                self.layers_replaced
            ));
        }
        if self.words_substituted > 0 {
            out.push(format!(
                "{} word(s) contained characters with no WinAnsi code and were \
                 written with '?' substitutions — a Standard-14 face cannot \
                 represent non-Latin scripts.",
                self.words_substituted
            ));
        }
        if self.words_skipped > 0 {
            out.push(format!(
                "{} word(s) were skipped: empty text or a degenerate bounding \
                 box.",
                self.words_skipped
            ));
        }
        if self.words_scale_clamped > 0 {
            out.push(format!(
                "{} word(s) had their horizontal scaling clamped; a selection \
                 there will not track the ink exactly.",
                self.words_scale_clamped
            ));
        }
        out
    }
}
