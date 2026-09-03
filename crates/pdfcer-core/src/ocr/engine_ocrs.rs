//! The [`ocrs`] text-recognition engine, bound to [`OcrEngine`].
//!
//! # Scope: this file is the ONLY part of OCR that knows a recogniser exists
//!
//! Everything else under [`crate::ocr`] — the types, the y-flip, the mode-3
//! sandwich writer — is engine-independent and always compiled. This module is
//! the entire surface gated by the `ocrs` Cargo feature, and that is the point
//! of the split: the operator's decision (2026-08-12) was *"just build for
//! both"*, so a second engine is expected, and it must be able to land as a
//! sibling of this file rather than as a rewrite of the OCR subsystem.
//!
//! # Why `ocrs` specifically
//!
//! From `docs/ocr-engine-survey.md`: it is the **only** surveyed engine that
//! passes pdfcer's wasm32 CI gate. Every alternative would have made OCR the
//! first capability that cannot cross into the web fork — the exact cost the
//! GUI-core separation invariant exists to avoid. PaddleOCR via `ocr-rs`
//! covers 50+ languages and is Apache-2.0 but has no WASM. **Surya is a trap
//! and must not be re-evaluated on its accuracy numbers**: Apache-2.0 code,
//! but modified Open RAIL-M *weights* with a $5M revenue cap, and a
//! field-of-use restriction cannot be bundled in an MIT application.
//!
//! # ★ This engine reports NO confidence, and that is a fact about the world
//!
//! `ocrs`'s output type is [`ocrs::TextChar`] — a `char` and a rectangle.
//! There is no score on a character, a word, a line, or the page. So
//! [`OcrsEngine::reports_confidence`] returns **`false`**, and it is not a
//! stub awaiting improvement.
//!
//! This is precisely why [`OcrEngine::reports_confidence`] was defined as a
//! required method with no default. A default of `true` would have made this
//! engine claim scores it does not have; a default of `false` would let a
//! future engine that *has* them under-report by omission. The whole
//! disclosure chain downstream — `OcrPage::confidence_available`,
//! `OcrLayerReport::confidence_available`, and the report line that says *"this
//! engine reports NO per-word confidence"* — exists to carry this one fact
//! honestly to the operator rather than letting unscored guesses pass as
//! checked. An absent score and a high score must never look the same.
//!
//! # The pipeline, and why it is four calls rather than `get_text`
//!
//! `ocrs` offers a one-call [`ocrs::OcrEngine::get_text`] that returns a
//! `String`. It is useless here: a text layer needs a **rectangle per word**,
//! and a flat string has thrown all of them away. So the staged API is used:
//!
//! 1. `prepare_input` — normalises the image into the tensor the models want.
//! 2. `detect_words` — the detection model; returns rotated rectangles.
//! 3. `find_text_lines` — groups those into reading-order lines.
//! 4. `recognize_text` — the recognition model, per line.
//!
//! Step 3 is what gives the emitted `Tj` operators a sane order, and therefore
//! what makes copied text read correctly rather than scrambled — the property
//! [`OcrPage::words`] documents as worth checking per engine.
//!
//! # Coordinates: this module does NOT flip
//!
//! [`OcrEngine::recognize`] is contractually required to return **image pixel
//! coordinates, y-down**, which is exactly what `ocrs` produces, so nothing is
//! converted here. The flip to PDF user space belongs to
//! [`words_to_page_space`](crate::ocr::words_to_page_space) and only there, so
//! that every engine is wrong or right together. A "helpful" flip in an engine
//! adapter would produce a text layer that is mirrored *twice* — i.e. correct
//! — for this engine and mirrored once for the next one, which is the kind of
//! defect that gets attributed to the wrong module for a long time.
//!
//! # No network, structurally
//!
//! `ocrs`'s model **downloader** lives in the separate `ocrs-cli` binary
//! crate, not in the `ocrs` library. Nothing reachable from here can fetch
//! anything, which is how `ARCHITECTURE.md` §1.1's privacy posture stays true
//! without depending on a denylist to catch a violation. Models are loaded
//! from disk, through paths [`crate::ocr::models`] resolved.

use std::path::{Path, PathBuf};

use ocrs::{ImageSource, OcrEngineParams, TextItem};

use super::{OcrEngine, RecognizedWord};
use crate::page_tree::Rect;

/// The directory name this engine's models are filed under.
///
/// Used with [`crate::ocr::models::resolve_model_dir`]. Engine-specific by
/// design: a second engine's weights are a different format and must not be
/// found by a search that was looking for these.
pub const MODEL_DIR: &str = "ocrs";

/// The detection model's file name, as published by `robertknight/ocrs-models`.
pub const DETECTION_MODEL: &str = "text-detection.rten";

/// The recognition model's file name, as published by `robertknight/ocrs-models`.
///
/// Note the `-checkpoint` suffix: **the Hugging Face and S3 copies of these
/// files are not byte-identical** (they differ by 13,280 and 124 bytes and use
/// different names). "The ocrs models" is therefore not one thing, and which
/// artifact ships must be pinned and hashed rather than described.
pub const RECOGNITION_MODEL: &str = "text-rec-checkpoint.rten";

/// A failure to build or run the `ocrs` engine.
///
/// Every variant names a specific, actionable cause. In particular a missing
/// model file is **not** folded into a generic "OCR failed": on a portable
/// single-folder install, "the weights are not beside the binary" is the most
/// likely failure by a wide margin, and it is entirely fixable by the operator
/// — but only if the message says so.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum OcrsEngineError {
    /// A model file was not where it was expected.
    #[error(
        "OCR model file not found: {path} — place the ocrs models in a `models/ocrs` folder beside the executable, or pass an explicit model directory"
    )]
    ModelMissing {
        /// The path that was tried.
        path: PathBuf,
    },
    /// A model file exists but could not be loaded.
    #[error("OCR model {path} could not be loaded: {reason}")]
    ModelLoad {
        /// The file that failed.
        path: PathBuf,
        /// What the runtime said.
        reason: String,
    },
    /// The pixel buffer does not match the stated dimensions.
    ///
    /// Checked rather than trusted because the trait takes a raw slice and a
    /// width/height, and a mismatch there would otherwise be read as image
    /// content — producing recognised "words" from whatever the stride error
    /// smeared across the buffer, with no error anywhere.
    #[error("image buffer is {actual} bytes but {width}x{height} 8-bit greyscale needs {expected}")]
    ImageSize {
        /// The stated width.
        width: u32,
        /// The stated height.
        height: u32,
        /// Bytes required.
        expected: usize,
        /// Bytes supplied.
        actual: usize,
    },
    /// The image was rejected by the engine's preprocessing.
    #[error("the image could not be prepared for recognition: {0}")]
    Image(String),
    /// Detection or recognition failed.
    #[error("text recognition failed: {0}")]
    Recognition(String),
}

/// The `ocrs` engine, with both models loaded.
///
/// Construction is fallible and eager: both models are loaded up front rather
/// than lazily on first use, so a missing or corrupt model is reported when the
/// operator sets OCR up, not in the middle of a batch run over 400 pages.
pub struct OcrsEngine {
    inner: ocrs::OcrEngine,
}

impl std::fmt::Debug for OcrsEngine {
    /// Hand-written because [`ocrs::OcrEngine`] is not [`Debug`].
    ///
    /// Derived would not compile, and omitting the impl entirely would violate
    /// `C-COMMON-TRAITS` and make this type unusable inside any caller's own
    /// derived `Debug` — a small omission that propagates outward.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OcrsEngine")
            .field("models", &"loaded")
            .field("reports_confidence", &false)
            .finish()
    }
}

impl OcrsEngine {
    /// Load the engine from a directory holding both `.rten` model files.
    ///
    /// # Errors
    ///
    /// [`OcrsEngineError::ModelMissing`] if either file is absent — named
    /// individually, so the operator learns *which* one, and
    /// [`OcrsEngineError::ModelLoad`] if a file exists but the runtime rejects
    /// it (a truncated download is the common cause, and it reads as a
    /// corrupt-model error rather than a missing-model one, which is the
    /// distinction that saves a wasted re-copy).
    pub fn from_model_dir(dir: &Path) -> Result<Self, OcrsEngineError> {
        Self::from_model_files(&dir.join(DETECTION_MODEL), &dir.join(RECOGNITION_MODEL))
    }

    /// Load the engine from two explicitly named model files.
    ///
    /// # Errors
    ///
    /// As [`Self::from_model_dir`].
    pub fn from_model_files(detection: &Path, recognition: &Path) -> Result<Self, OcrsEngineError> {
        let load = |path: &Path| -> Result<rten::Model, OcrsEngineError> {
            if !path.is_file() {
                return Err(OcrsEngineError::ModelMissing {
                    path: path.to_path_buf(),
                });
            }
            rten::Model::load_file(path).map_err(|e| OcrsEngineError::ModelLoad {
                path: path.to_path_buf(),
                reason: e.to_string(),
            })
        };

        let params = OcrEngineParams {
            detection_model: Some(load(detection)?),
            recognition_model: Some(load(recognition)?),
            ..Default::default()
        };

        let inner = ocrs::OcrEngine::new(params).map_err(|e| OcrsEngineError::ModelLoad {
            path: detection.to_path_buf(),
            reason: e.to_string(),
        })?;
        Ok(Self { inner })
    }
}

impl OcrEngine for OcrsEngine {
    type Error = OcrsEngineError;

    fn recognize(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<RecognizedWord>, Self::Error> {
        // Validate before handing the slice over. `ImageSource::from_bytes`
        // infers a channel count from `len / (w*h)`, so a buffer that is twice
        // the expected size is silently taken as a 2-channel image rather than
        // rejected — it would "work", and recognise nonsense.
        let expected =
            (width as usize)
                .checked_mul(height as usize)
                .ok_or(OcrsEngineError::ImageSize {
                    width,
                    height,
                    expected: usize::MAX,
                    actual: pixels.len(),
                })?;
        if expected == 0 || pixels.len() != expected {
            return Err(OcrsEngineError::ImageSize {
                width,
                height,
                expected,
                actual: pixels.len(),
            });
        }

        let source = ImageSource::from_bytes(pixels, (width, height))
            .map_err(|e| OcrsEngineError::Image(e.to_string()))?;
        let input = self
            .inner
            .prepare_input(source)
            .map_err(|e| OcrsEngineError::Image(e.to_string()))?;

        let word_rects = self
            .inner
            .detect_words(&input)
            .map_err(|e| OcrsEngineError::Recognition(e.to_string()))?;
        let lines = self.inner.find_text_lines(&input, &word_rects);
        let recognised = self
            .inner
            .recognize_text(&input, &lines)
            .map_err(|e| OcrsEngineError::Recognition(e.to_string()))?;

        let mut out = Vec::new();
        for line in recognised.into_iter().flatten() {
            for word in line.words() {
                let text = word.to_string();
                if text.is_empty() {
                    continue;
                }
                let r = word.bounding_rect();
                // Image space, y-DOWN, unflipped — the trait's contract. The
                // top edge has the SMALLER row index, so it becomes `lly`
                // under `Rect::from_corners`' normalisation, which is exactly
                // what `words_to_page_space` then reads it as.
                out.push(RecognizedWord {
                    text,
                    rect: Rect::from_corners(
                        f64::from(r.left()),
                        f64::from(r.top()),
                        f64::from(r.right()),
                        f64::from(r.bottom()),
                    ),
                    // Not "unknown yet" — this engine has no such number at
                    // all. See the module header.
                    confidence: None,
                });
            }
        }
        Ok(out)
    }

    fn reports_confidence(&self) -> bool {
        // See the module header: `ocrs::TextChar` is a char and a rectangle.
        // There is no score to report, and saying so is the honest answer.
        false
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    /// A missing model is a NAMED refusal that includes the path.
    ///
    /// The most likely real-world failure on a portable install, and the one
    /// the operator can actually fix — but only if told which file and where it
    /// was looked for.
    #[test]
    fn a_missing_model_names_the_path() {
        let dir = Path::new("this-directory-does-not-exist-ocr");
        match OcrsEngine::from_model_dir(dir) {
            Err(OcrsEngineError::ModelMissing { path }) => {
                assert!(
                    path.ends_with(DETECTION_MODEL),
                    "the DETECTION model is checked first, so it is the one \
                     named: {path:?}"
                );
                let msg = OcrsEngineError::ModelMissing { path }.to_string();
                assert!(
                    msg.contains("models/ocrs") || msg.contains("model directory"),
                    "the message must say how to fix it: {msg}"
                );
            }
            other => panic!("expected ModelMissing, got {other:?}"),
        }
    }

    /// The model file names are pinned as constants rather than built ad hoc.
    ///
    /// Guards the finding that the Hugging Face and S3 copies differ in both
    /// bytes and file name: a call site that spelled a name inline would work
    /// against one mirror and fail against the other.
    #[test]
    fn the_model_file_names_are_pinned() {
        assert_eq!(DETECTION_MODEL, "text-detection.rten");
        assert_eq!(RECOGNITION_MODEL, "text-rec-checkpoint.rten");
        assert_eq!(MODEL_DIR, "ocrs");
    }
}
