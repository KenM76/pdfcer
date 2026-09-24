//! `OcrEngine` binding for the OCRcer recogniser.
//!
//! This file is meant to be pasted, unmodified, into
//! `crates/pdfcer-core/src/ocr/engine_ocrcer.rs` in the `pdfcer` tree, and
//! wired in exactly the way `engine_ocrs.rs` already is — see
//! `integration/pdfcer/README.md` in the OCRcer repo for the Cargo.toml and
//! `ocr/mod.rs` lines that go with it. It is not built or tested as part of
//! the OCRcer workspace (`ocrcer-core` never depends on `pdfcer-core`, and
//! never will — the dependency runs the other way); it is proven against a
//! real `pdfcer-core` checkout by the throwaway harness recorded in
//! `docs/measurements/2026-09-24_pdfcer_binding.md`.
//!
//! # Contract
//!
//! - **Input**: `width`/`height` in pixels, `pixels` an 8-bit greyscale
//!   buffer, row-major, top-down, exactly `width * height` bytes — the same
//!   layout `OcrEngine::recognize` already documents and `engine_ocrs`
//!   already assumes.
//! - **Output coordinates**: pixel coordinates of the *input* image, y-down,
//!   unrotated. This module performs no coordinate transform of any kind —
//!   the y-flip into PDF user space is `words_to_page_space`'s job, done
//!   exactly once, by the caller, after every engine (`ocrs` or this one)
//!   has already returned. Do not add a page-space-aware code path here.
//! - **Confidence**: `Some(_)` on every word, always. `reports_confidence()`
//!   returns `true`, and unlike a stub, the number is real: OCRcer's own
//!   match-margin calibration (`ocrcer_core`'s `confidence.rs`), a
//!   geometric mean over the word's characters. There is no code path in
//!   this adapter that can produce `None`.
//! - **Errors**: `Self::Error = ocrcer_core::Error`. `ocrcer_core` already
//!   validates `pixels.len() == width as usize * height as usize` inside
//!   `recognize_bytes` and returns `Err` rather than reading past the
//!   buffer or silently truncating, so this adapter does not re-validate —
//!   doing so would be a second, possibly-diverging implementation of a
//!   check `ocrcer_core` already owns.
//!
//! # What this adapter deliberately does not do
//!
//! - It does not choose where the model file lives, or fetch it. Model
//!   bytes are handed to [`OcrcerEngine::from_bytes`] by the caller, exactly
//!   as `ocrcer_core::Engine::from_bytes` requires — `pdfcer`'s own
//!   `ocr::models::resolve_model_dir` (or `include_bytes!`, for a build that
//!   wants to embed the model) stays the thing that decides.
//! - It does not retry, cache, or pool. `ocrcer_core::pipeline::Engine` is
//!   `Sync`-safe to hold behind an `Arc` if the caller wants to reuse one
//!   loaded model across pages; this file does not impose a lifecycle.

#[cfg(feature = "ocrcer")]
use crate::ocr::{OcrEngine, RecognizedWord};
#[cfg(feature = "ocrcer")]
use crate::page_tree::Rect;

/// The OCRcer text-recognition engine, bound to [`OcrEngine`].
///
/// Construct with [`OcrcerEngine::from_bytes`]; a fresh instance holds one
/// loaded `.ocrw` model and is otherwise stateless between calls.
#[cfg(feature = "ocrcer")]
pub struct OcrcerEngine {
    inner: ocrcer_core::pipeline::Engine,
}

#[cfg(feature = "ocrcer")]
impl OcrcerEngine {
    /// Loads a model from an in-memory `.ocrw` container.
    ///
    /// `model` is the whole file's bytes — from `std::fs::read` on a path
    /// `ocr::models::resolve_model_dir` produced, or from `include_bytes!`
    /// for a build that embeds it. This is deliberately the *only*
    /// constructor: OCRcer never decides where its own model file lives,
    /// per `ARCHITECTURE.md` §8.1 in the OCRcer repo.
    ///
    /// # Errors
    ///
    /// Returns `Err` if `model` is not a well-formed `.ocrw` container of a
    /// version and kind this build of `ocrcer-core` understands — see
    /// `ocrcer_core::Error` for the specific cause (bad magic, truncated
    /// file, checksum mismatch, unsupported version, missing table, and so
    /// on). Never panics on malformed input.
    pub fn from_bytes(model: &[u8]) -> Result<Self, ocrcer_core::Error> {
        Ok(Self {
            inner: ocrcer_core::pipeline::Engine::from_bytes(model)?,
        })
    }
}

#[cfg(feature = "ocrcer")]
impl OcrEngine for OcrcerEngine {
    type Error = ocrcer_core::Error;

    fn recognize(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<RecognizedWord>, Self::Error> {
        let words = self.inner.recognize_bytes(width, height, pixels)?;
        Ok(words
            .into_iter()
            .map(|w| RecognizedWord {
                text: w.text,
                rect: Rect::from_corners(
                    f64::from(w.rect.x),
                    f64::from(w.rect.y),
                    f64::from(w.rect.x + w.rect.width),
                    f64::from(w.rect.y + w.rect.height),
                ),
                confidence: Some(w.confidence),
            })
            .collect())
    }

    fn reports_confidence(&self) -> bool {
        true
    }
}
