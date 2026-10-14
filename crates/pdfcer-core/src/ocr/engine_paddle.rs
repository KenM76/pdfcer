//! The PaddleOCR (PP-OCR) recogniser, bound to [`OcrEngine`], behind the
//! (default-on) `paddle` Cargo feature.
//!
//! Runs PP-OCR ONNX exports — a text **detection** model and
//! a text **recognition** model — through `rten`, the pure-Rust runtime the
//! `ocrs` engine already uses, so this engine adds no dependency beyond
//! `rten-tensor` and crosses into wasm32 like `ocrs` does. Pre- and
//! post-processing (DB box finding, CTC decoding) live in
//! `paddle_post`, which is always compiled so the fuzz
//! harness reaches it; their RapidOCR-default parameters and the one
//! departure (axis-aligned boxes) are documented there.
//!
//! No angle classifier runs: a crop at least 1.5× taller than wide is turned
//! 90° counter-clockwise, as PaddleOCR does, and emitted as one word covering
//! the crop, because its time steps no longer run along the page's x axis.
//!
//! Confidence is the mean CTC probability of a word's characters, so
//! `PaddleEngine::reports_confidence` is `true`. Lines scoring below
//! `LINE_SCORE_MIN` are dropped, as
//! RapidOCR drops them.
//!
//! Models are never compiled in and never downloaded; they load from disk
//! through paths [`crate::ocr::models`] resolved. The portable package ships
//! PP-OCRv4 Chinese/English (Apache-2.0) in `models/paddle`, from
//! `assets/models/paddle`.

use std::path::{Path, PathBuf};

use rten_tensor::NdTensor;
use rten_tensor::prelude::*;

use super::paddle_config;
use super::paddle_post::{self, ModelInput, TextBox};
use super::{OcrEngine, RecognizedWord};

/// The directory name this engine's models are filed under.
pub const MODEL_DIR: &str = "paddle";

/// The detection model's file name inside [`MODEL_DIR`]: a PP-OCR `det`
/// export in ONNX form (input `[1, 3, H, W]`, output `[1, 1, H, W]`).
pub const DETECTION_MODEL: &str = "det.onnx";

/// The recognition model's file name inside [`MODEL_DIR`]: a PP-OCR `rec`
/// export in ONNX form (input `[1, 3, 48, W]`, output `[1, T, classes]`).
pub const RECOGNITION_MODEL: &str = "rec.onnx";

/// The optional character dictionary inside [`MODEL_DIR`], one entry per
/// line. Absent, [`INFERENCE_CONFIG`] is read, and failing that the
/// recognition model's embedded `character` metadata (RapidOCR's exports
/// carry one).
pub const DICTIONARY: &str = "dict.txt";

/// The `inference.yml` PaddlePaddle publishes beside a recogniser export;
/// its `PostProcess.character_dict` is the dictionary when no
/// [`DICTIONARY`] is present.
pub const INFERENCE_CONFIG: &str = "inference.yml";

/// Where the engine's character dictionary came from — a shell discloses it,
/// since a mismatched dictionary reads as confident nonsense.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum DictionarySource {
    /// A dictionary file.
    File(PathBuf),
    /// The `character_dict` list of an [`INFERENCE_CONFIG`] file.
    Config(PathBuf),
    /// The recognition model's `character` metadata.
    Embedded,
}

/// A failure to build or run the PaddleOCR engine.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PaddleEngineError {
    /// A model file was not where it was expected.
    #[error(
        "PaddleOCR model file not found: {path} — place PP-OCR ONNX exports named det.onnx and rec.onnx (and optionally dict.txt) in a `models/paddle` folder beside the executable, or pass an explicit model directory"
    )]
    ModelMissing {
        /// The path that was tried.
        path: PathBuf,
    },
    /// A model file exists but could not be loaded.
    #[error("PaddleOCR model {path} could not be loaded: {reason}")]
    ModelLoad {
        /// The file that failed.
        path: PathBuf,
        /// What the runtime said.
        reason: String,
    },
    /// No usable dictionary: the file is unreadable, the model embeds none,
    /// or it does not fit the model's class count.
    #[error("PaddleOCR dictionary {path}: {reason}")]
    Dictionary {
        /// The file, or the recognition model when the dictionary is embedded.
        path: PathBuf,
        /// What is wrong.
        reason: String,
    },
    /// The pixel buffer does not match the stated dimensions.
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
    /// Detection or recognition failed, or a model's output had the wrong
    /// shape.
    #[error("PaddleOCR recognition failed: {0}")]
    Recognition(String),
}

/// The PaddleOCR engine, with both models and the dictionary loaded.
pub struct PaddleEngine {
    det: rten::Model,
    rec: rten::Model,
    dictionary: Vec<String>,
    source: DictionarySource,
}

impl std::fmt::Debug for PaddleEngine {
    /// Hand-written because [`rten::Model`] is not [`Debug`].
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaddleEngine")
            .field("dictionary_entries", &self.dictionary.len())
            .field("dictionary_source", &self.source)
            .finish_non_exhaustive()
    }
}

fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

fn load(path: &Path) -> Result<rten::Model, PaddleEngineError> {
    if !path.is_file() {
        return Err(PaddleEngineError::ModelMissing {
            path: path.to_path_buf(),
        });
    }
    let fail = |reason: String| PaddleEngineError::ModelLoad {
        path: path.to_path_buf(),
        reason,
    };
    let bytes = std::fs::read(path).map_err(|e| fail(e.to_string()))?;
    // rten reads every operator in its opset-13 form; older exports
    // (PaddlePaddle's own PP-OCRv5 recognisers are opset 7) are rewritten
    // first, the same rewrites `onnx.version_converter` makes.
    let up = super::onnx_upgrade::upgrade(bytes).map_err(|e| fail(e.to_string()))?;
    let opset = up.opset;
    rten::Model::load(up.bytes).map_err(|e| {
        let mut reason = e.to_string();
        if let Some(v) = opset.filter(|&v| v < super::onnx_upgrade::TARGET_OPSET) {
            reason.push_str(&format!(
                " (the model is opset {v}; pdfcer upgrades BatchNormalization, Slice \
                 and Softmax from older opsets, and anything else must be \
                 converted to opset 13, e.g. with onnx.version_converter)"
            ));
        }
        fail(reason)
    })
}

impl PaddleEngine {
    /// Load from a directory holding [`DETECTION_MODEL`], [`RECOGNITION_MODEL`]
    /// and, optionally, [`DICTIONARY`] or [`INFERENCE_CONFIG`] (the first
    /// present is the dictionary).
    ///
    /// # Errors
    ///
    /// As [`Self::from_model_files`]; an [`INFERENCE_CONFIG`] whose
    /// `character_dict` cannot be read exactly is
    /// [`PaddleEngineError::Dictionary`].
    pub fn from_model_dir(dir: &Path) -> Result<Self, PaddleEngineError> {
        let (dict, config) = (dir.join(DICTIONARY), dir.join(INFERENCE_CONFIG));
        let source = if dict.is_file() {
            Some(DictionarySource::File(dict))
        } else if config.is_file() {
            Some(DictionarySource::Config(config))
        } else {
            None
        };
        Self::load_with(
            &dir.join(DETECTION_MODEL),
            &dir.join(RECOGNITION_MODEL),
            source,
        )
    }

    /// Load from explicitly named files. `dictionary: None` uses the
    /// recognition model's embedded `character` metadata.
    ///
    /// # Errors
    ///
    /// [`PaddleEngineError::ModelMissing`] naming the absent file (detection
    /// first); [`PaddleEngineError::ModelLoad`] when the runtime rejects one
    /// (for a model below opset 13 the reason names its opset: the forms
    /// `ocr::onnx_upgrade` rewrites load, others must be converted);
    /// [`PaddleEngineError::Dictionary`] when no dictionary is available or,
    /// for a model with a fixed class count, it does not fit.
    pub fn from_model_files(
        detection: &Path,
        recognition: &Path,
        dictionary: Option<&Path>,
    ) -> Result<Self, PaddleEngineError> {
        let source = dictionary.map(|p| DictionarySource::File(p.to_path_buf()));
        Self::load_with(detection, recognition, source)
    }

    fn load_with(
        detection: &Path,
        recognition: &Path,
        source: Option<DictionarySource>,
    ) -> Result<Self, PaddleEngineError> {
        let det = load(detection)?;
        let rec = load(recognition)?;
        let source = source.unwrap_or(DictionarySource::Embedded);
        let named = match &source {
            DictionarySource::File(p) | DictionarySource::Config(p) => p.clone(),
            DictionarySource::Embedded => recognition.to_path_buf(),
        };
        let fail = |reason: String| PaddleEngineError::Dictionary {
            path: named.clone(),
            reason,
        };
        let entries = match &source {
            DictionarySource::File(path) => {
                paddle_post::parse_dictionary(&read_text(path).map_err(fail)?)
            }
            DictionarySource::Config(path) => {
                paddle_config::parse_config_dictionary(&read_text(path).map_err(fail)?)
                    .map_err(fail)?
            }
            DictionarySource::Embedded => {
                let text = rec.metadata().get("character").ok_or_else(|| {
                    fail(format!(
                        "the recognition model embeds no `character` list — supply a \
                         {DICTIONARY} or {INFERENCE_CONFIG}"
                    ))
                })?;
                paddle_post::parse_dictionary(text)
            }
        };
        if entries.is_empty() {
            return Err(fail("the dictionary is empty".to_owned()));
        }
        if let Some(classes) = fixed_classes(&rec) {
            paddle_post::space_class(entries.len(), classes).map_err(|e| fail(e.to_string()))?;
        }
        Ok(Self {
            det,
            rec,
            dictionary: entries,
            source,
        })
    }

    /// Where the dictionary came from.
    #[must_use]
    pub fn dictionary_source(&self) -> &DictionarySource {
        &self.source
    }

    /// Entries in the dictionary.
    #[must_use]
    pub fn dictionary_len(&self) -> usize {
        self.dictionary.len()
    }

    fn recognize_line(
        &self,
        pixels: &[u8],
        width: u32,
        height: u32,
        b: &TextBox,
        out: &mut Vec<RecognizedWord>,
    ) -> Result<(), PaddleEngineError> {
        let Some((crop, cw, ch)) = paddle_post::crop(pixels, width, height, b) else {
            return Ok(());
        };
        let rotated = ch as f32 / cw as f32 >= 1.5;
        let (crop, cw, ch) = if rotated {
            paddle_post::rotate_ccw(&crop, cw, ch)
        } else {
            (crop, cw, ch)
        };
        let input = paddle_post::rec_input(&crop, cw, ch);
        let (probs, shape) = infer(&self.rec, &input)?;
        let [_, steps, classes] = shape;
        let chars = paddle_post::ctc_decode(&probs, steps, classes, &self.dictionary)
            .map_err(|e| PaddleEngineError::Recognition(e.to_string()))?;
        // A NaN line score is dropped too.
        let score = paddle_post::line_score(&chars);
        if score.is_nan() || score < paddle_post::LINE_SCORE_MIN {
            return Ok(());
        }
        if rotated {
            out.extend(paddle_post::line_as_word(&chars, b));
        } else {
            out.extend(paddle_post::words_from_line(&chars, steps, &input, b));
        }
        Ok(())
    }
}

/// The recognition model's class count, when its output shape fixes one.
fn fixed_classes(rec: &rten::Model) -> Option<usize> {
    let id = *rec.output_ids().first()?;
    match rec.node_info(id)?.shape()?.last()? {
        rten::Dimension::Fixed(n) => Some(*n),
        rten::Dimension::Symbolic(_) => None,
    }
}

/// Run a one-input model and return its first output flattened, with the
/// output's trailing three dimensions (`[C, H, W]` for detection with the
/// batch dropped, `[1, T, classes]` for recognition).
fn infer(
    model: &rten::Model,
    input: &ModelInput,
) -> Result<(Vec<f32>, [usize; 3]), PaddleEngineError> {
    let err = |e: &dyn std::fmt::Display| PaddleEngineError::Recognition(e.to_string());
    let tensor = NdTensor::from_data(
        [1, 3, input.height as usize, input.width as usize],
        input.data.clone(),
    );
    let value = model
        .run_one(tensor.view().into(), None)
        .map_err(|e| err(&e))?;
    let out: rten_tensor::Tensor<f32> = value.try_into().map_err(|e| err(&e))?;
    let dims = out.shape().to_vec();
    let shape = match dims.as_slice() {
        [a, b, c] => [*a, *b, *c],
        [1, a, b, c] => [*a, *b, *c],
        other => {
            return Err(PaddleEngineError::Recognition(format!(
                "unexpected model output shape {other:?}"
            )));
        }
    };
    Ok((out.to_vec(), shape))
}

impl OcrEngine for PaddleEngine {
    type Error = PaddleEngineError;

    fn recognize(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<RecognizedWord>, Self::Error> {
        let expected = (width as usize).checked_mul(height as usize).unwrap_or(0);
        if expected == 0 || pixels.len() != expected {
            return Err(PaddleEngineError::ImageSize {
                width,
                height,
                expected,
                actual: pixels.len(),
            });
        }
        let input = paddle_post::det_input(width, height, pixels);
        let (prob, [_, map_h, map_w]) = infer(&self.det, &input)?;
        let mut boxes = paddle_post::db_boxes(&prob, map_w, map_h, width, height)
            .map_err(|e| PaddleEngineError::Recognition(e.to_string()))?;
        paddle_post::sort_reading_order(&mut boxes);
        let mut out = Vec::new();
        for b in &boxes {
            self.recognize_line(pixels, width, height, b, &mut out)?;
        }
        Ok(out)
    }

    fn reports_confidence(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    fn onnx_fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/synthetic/onnx")
            .join(name)
    }

    fn run_fixture(model: &rten::Model) -> Vec<f32> {
        let x: Vec<f32> = (0..120)
            .map(|i| ((i * 37) % 23) as f32 / 7.0 - 1.5)
            .collect();
        let tensor = NdTensor::from_data([2, 3, 4, 5], x);
        let out: rten_tensor::Tensor<f32> = model
            .run_one(tensor.view().into(), None)
            .unwrap()
            .try_into()
            .unwrap();
        assert_eq!(out.shape(), [2, 3, 4, 3]);
        out.to_vec()
    }

    #[test]
    fn an_opset_7_model_computes_what_its_opset_13_conversion_does() {
        let old = std::fs::read(onnx_fixture("opset7.onnx")).unwrap();
        assert!(
            rten::Model::load(old).is_err(),
            "the fixture must be one rten cannot read unaided"
        );
        let upgraded = load(&onnx_fixture("opset7.onnx")).unwrap();
        let reference = load(&onnx_fixture("opset7-as13.onnx")).unwrap();
        let (got, want) = (run_fixture(&upgraded), run_fixture(&reference));
        for (i, (g, w)) in got.iter().zip(&want).enumerate() {
            assert!((g - w).abs() < 1e-5, "element {i}: {g} vs {w}");
        }
    }

    /// A model folder from the synthetic ONNX fixture, which loads as both
    /// models; `files` are extra `(name, text)` files written beside them.
    fn model_dir(tag: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pdfcer-paddle-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in [DETECTION_MODEL, RECOGNITION_MODEL] {
            std::fs::copy(onnx_fixture("opset7-as13.onnx"), dir.join(name)).unwrap();
        }
        for (name, text) in files {
            std::fs::write(dir.join(name), text).unwrap();
        }
        dir
    }

    #[test]
    fn an_inference_config_supplies_the_dictionary_when_no_dict_txt_does() {
        let config = "PostProcess:\n  character_dict:\n  - a\n  - 'b'\n";
        let dir = model_dir("config", &[(INFERENCE_CONFIG, config)]);
        let engine = PaddleEngine::from_model_dir(&dir).unwrap();
        assert_eq!(
            engine.dictionary_source(),
            &DictionarySource::Config(dir.join(INFERENCE_CONFIG))
        );
        assert_eq!(engine.dictionary_len(), 2);

        let both = model_dir(
            "both",
            &[(INFERENCE_CONFIG, config), (DICTIONARY, "x\ny\n")],
        );
        let engine = PaddleEngine::from_model_dir(&both).unwrap();
        assert_eq!(
            engine.dictionary_source(),
            &DictionarySource::File(both.join(DICTIONARY))
        );
        for d in [dir, both] {
            let _ = std::fs::remove_dir_all(d);
        }
    }

    #[test]
    fn an_unreadable_inference_config_is_refused_by_name() {
        let dir = model_dir(
            "badconfig",
            &[(INFERENCE_CONFIG, "character_dict:\n- 'a\n")],
        );
        match PaddleEngine::from_model_dir(&dir) {
            Err(PaddleEngineError::Dictionary { path, reason }) => {
                assert_eq!(path, dir.join(INFERENCE_CONFIG));
                assert!(reason.contains("unterminated"), "{reason}");
            }
            other => panic!("expected Dictionary, got {other:?}"),
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_missing_model_names_the_path_and_the_fix() {
        match PaddleEngine::from_model_dir(Path::new("no-such-dir-paddle")) {
            Err(e @ PaddleEngineError::ModelMissing { .. }) => {
                let msg = e.to_string();
                assert!(
                    msg.contains(DETECTION_MODEL) && msg.contains("models/paddle"),
                    "{msg}"
                );
            }
            other => panic!("expected ModelMissing, got {other:?}"),
        }
    }

    #[test]
    fn the_model_file_names_are_pinned() {
        assert_eq!(
            (MODEL_DIR, DETECTION_MODEL, RECOGNITION_MODEL, DICTIONARY),
            ("paddle", "det.onnx", "rec.onnx", "dict.txt")
        );
    }
}
