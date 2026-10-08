//! The PaddleOCR-VL recogniser, bound to [`OcrEngine`], behind the
//! (default-on) `ocr-vl` Cargo feature. Decision 183.
//!
//! PaddleOCR-VL is a vision-language model: a vision encoder turns the image
//! into embeddings, which replace placeholder tokens in a fixed OCR prompt,
//! and a decoder generates the text greedily with a key/value cache. It
//! reads a whole region and returns **text, not word boxes**, so this engine
//! places each decoded line on the region's ink (`vl_pre::place_lines`):
//! the text layer is region-aligned and inferred, and a shell says so.
//!
//! Model: PaddlePaddle/PaddleOCR-VL (Apache-2.0, the `LICENSE` file of
//! huggingface.co/PaddlePaddle/PaddleOCR-VL), as the q8 ONNX export
//! onnx-community/PaddleOCR-VL-1.5-ONNX (Apache-2.0), rewritten for `rten` by
//! `tools/build-paddle-vl-addon.py`. Nothing ships and nothing is fetched:
//! the files arrive as a decision-182 add-on folder.
//!
//! Ceilings: input pixels (`vl_pre::MAX_INPUT_PIXELS`), resized pixels
//! (`vl_pre::MAX_PIXELS`), new tokens and so decoder steps
//! (`vl_decode::MAX_NEW_TOKENS_CAP`), the tokenizer file and encoded text
//! (`vl_tokenizer`).

use std::path::{Path, PathBuf};

use rten::{Dimension, Model, NodeId, Value, ValueOrView};
use rten_tensor::prelude::*;
use rten_tensor::{NdTensor, Tensor};

use super::vl_decode::{self, StopReason};
use super::vl_pre::{self, LinePlacement, PixelBox, PrepError, Prompt};
use super::vl_tokenizer::{TokenizerError, VlTokenizer};
use super::{OcrEngine, RecognizedWord};

/// The directory name this engine's files are filed under when not in an
/// add-on folder.
pub const MODEL_DIR: &str = "paddle-vl";
/// The vision encoder, already rewritten for `rten`.
pub const VISION_MODEL: &str = "vision_encoder.onnx";
/// The text decoder (q8, with key/value cache inputs and outputs).
pub const DECODER_MODEL: &str = "decoder.onnx";
/// The token-embedding model.
pub const EMBEDDING_MODEL: &str = "embedding.onnx";
/// The embedding model's external weights, beside it.
pub const EMBEDDING_DATA: &str = "embedding.onnx.data";
/// The Hugging Face tokenizer file.
pub const TOKENIZER: &str = "tokenizer.json";
/// Every file the engine reads, in the order a shell lists them.
pub const REQUIRED_FILES: [&str; 5] = [
    VISION_MODEL,
    DECODER_MODEL,
    EMBEDDING_MODEL,
    EMBEDDING_DATA,
    TOKENIZER,
];

/// Pixels of white kept around the ink box when cropping a region.
const CROP_MARGIN: usize = 16;

/// A failure to build or run the PaddleOCR-VL engine.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PaddleVlError {
    /// A required file was not where it was expected.
    #[error(
        "PaddleOCR-VL file not found: {path}. Build the add-on folder with tools/build-paddle-vl-addon.py and select it with --ocr-model"
    )]
    ModelMissing {
        /// The path that was tried.
        path: PathBuf,
    },
    /// A model file exists but could not be loaded.
    #[error("PaddleOCR-VL model {path} could not be loaded: {reason}")]
    ModelLoad {
        /// The file that failed.
        path: PathBuf,
        /// What the runtime said.
        reason: String,
    },
    /// A model's inputs or outputs are not the ones this engine drives.
    #[error("PaddleOCR-VL {model} has an unexpected interface: {reason}")]
    Interface {
        /// Which model.
        model: &'static str,
        /// What is missing or wrong.
        reason: String,
    },
    /// The tokenizer file could not be read or used.
    #[error("PaddleOCR-VL tokenizer {path}: {source}")]
    Tokenizer {
        /// The file.
        path: PathBuf,
        /// Why.
        source: TokenizerError,
    },
    /// The image or prompt could not be prepared.
    #[error(transparent)]
    Prepare(#[from] PrepError),
    /// Inference failed or produced an output of the wrong shape.
    #[error("PaddleOCR-VL recognition failed: {0}")]
    Recognition(String),
}

pub(super) fn run_err(e: impl std::fmt::Display) -> PaddleVlError {
    PaddleVlError::Recognition(e.to_string())
}

/// What one region read produced, for the shell's disclosure.
#[derive(Debug, Clone, PartialEq)]
pub struct RegionReading {
    /// The decoded text, special tokens dropped.
    pub text: String,
    /// One entry per non-empty line, placed in image pixels (y down), each
    /// carrying the region's mean token probability.
    pub lines: Vec<RecognizedWord>,
    /// How the lines were placed.
    pub placement: LinePlacement,
    /// Why decoding stopped; `None` when the region was blank and the model
    /// was not run.
    pub stop: Option<StopReason>,
    /// Tokens generated.
    pub tokens: usize,
    /// Image tokens the region became.
    pub image_tokens: usize,
    /// The ink box read, in image pixels, `None` when blank.
    pub ink_box: Option<PixelBox>,
}

/// The decoder's node ids.
#[derive(Debug, Clone)]
struct DecoderIo {
    embeds: NodeId,
    mask: NodeId,
    logits: NodeId,
    /// `(past input, present output)` per cache tensor.
    cache: Vec<(NodeId, NodeId)>,
    kv_heads: usize,
    head_dim: usize,
}

/// The PaddleOCR-VL engine, with all three models and the tokenizer loaded.
pub struct PaddleVlEngine {
    vision: Model,
    embed: Model,
    decoder: Model,
    tok: VlTokenizer,
    io: DecoderIo,
    max_new_tokens: usize,
}

impl std::fmt::Debug for PaddleVlEngine {
    /// Hand-written because [`rten::Model`] is not [`Debug`].
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaddleVlEngine")
            .field("vocab", &self.tok.vocab_len())
            .field("cache_tensors", &self.io.cache.len())
            .field("max_new_tokens", &self.max_new_tokens)
            .finish_non_exhaustive()
    }
}

fn require(dir: &Path, name: &str) -> Result<PathBuf, PaddleVlError> {
    let path = dir.join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(PaddleVlError::ModelMissing { path })
    }
}

fn load(path: &Path) -> Result<Model, PaddleVlError> {
    Model::load_file(path).map_err(|e| PaddleVlError::ModelLoad {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}

fn node(model: &Model, which: &'static str, name: &str) -> Result<NodeId, PaddleVlError> {
    model.node_id(name).map_err(|_| PaddleVlError::Interface {
        model: which,
        reason: format!("no `{name}`"),
    })
}

fn node_name(model: &Model, id: NodeId) -> String {
    model
        .node_info(id)
        .and_then(|n| n.name().map(str::to_owned))
        .unwrap_or_default()
}

/// A fixed dimension of a cache input, which the empty initial cache needs.
fn fixed_dim(model: &Model, id: NodeId, axis: usize) -> Option<usize> {
    match model.node_info(id)?.shape()?.get(axis)? {
        Dimension::Fixed(n) => Some(*n),
        Dimension::Symbolic(_) => None,
    }
}

/// Pair every `present.*` output with its `past_key_values.*` input.
fn decoder_io(model: &Model) -> Result<DecoderIo, PaddleVlError> {
    let which = "decoder";
    let bad = |reason: String| PaddleVlError::Interface {
        model: which,
        reason,
    };
    let mut cache = Vec::new();
    for &out in model.output_ids() {
        let name = node_name(model, out);
        if let Some(rest) = name.strip_prefix("present") {
            let past = node(model, which, &format!("past_key_values{rest}"))?;
            cache.push((past, out));
        }
    }
    let first = cache
        .first()
        .ok_or_else(|| bad("no key/value cache outputs".into()))?
        .0;
    let dims = (fixed_dim(model, first, 1), fixed_dim(model, first, 3));
    let (Some(kv_heads), Some(head_dim)) = dims else {
        return Err(bad("cache inputs lack fixed head dimensions".into()));
    };
    if cache.len() + 2 != model.input_ids().len() {
        return Err(bad(
            "inputs other than the embeddings, mask and cache".into()
        ));
    }
    Ok(DecoderIo {
        embeds: node(model, which, "inputs_embeds")?,
        mask: node(model, which, "attention_mask")?,
        logits: node(model, which, "logits")?,
        cache,
        kv_heads,
        head_dim,
    })
}

impl PaddleVlEngine {
    /// Load the engine from a folder holding [`REQUIRED_FILES`].
    ///
    /// # Errors
    ///
    /// [`PaddleVlError::ModelMissing`] naming the first absent file,
    /// [`PaddleVlError::ModelLoad`], [`PaddleVlError::Tokenizer`], or
    /// [`PaddleVlError::Interface`] when a model's inputs and outputs are
    /// not the expected export's.
    pub fn from_model_dir(dir: &Path) -> Result<Self, PaddleVlError> {
        for f in REQUIRED_FILES {
            require(dir, f)?;
        }
        let tok_path = dir.join(TOKENIZER);
        let tok_err = |source| PaddleVlError::Tokenizer {
            path: tok_path.clone(),
            source,
        };
        let meta = std::fs::metadata(&tok_path).map_err(|e| PaddleVlError::ModelLoad {
            path: tok_path.clone(),
            reason: e.to_string(),
        })?;
        let len = usize::try_from(meta.len()).unwrap_or(usize::MAX);
        if len > super::vl_tokenizer::MAX_TOKENIZER_BYTES {
            return Err(tok_err(TokenizerError::TooLarge(len)));
        }
        let bytes = std::fs::read(&tok_path).map_err(|e| PaddleVlError::ModelLoad {
            path: tok_path.clone(),
            reason: e.to_string(),
        })?;
        let tok = VlTokenizer::from_json_bytes(&bytes).map_err(tok_err)?;
        vl_pre::prompt(&tok, 0)?;
        let vision = load(&dir.join(VISION_MODEL))?;
        for name in ["pixel_values", "image_grid_thw", "image_embeds"] {
            node(&vision, "vision encoder", name)?;
        }
        let embed = load(&dir.join(EMBEDDING_MODEL))?;
        if embed.input_ids().len() != 1 || embed.output_ids().len() != 1 {
            return Err(PaddleVlError::Interface {
                model: "embedding",
                reason: "expected one input and one output".into(),
            });
        }
        let decoder = load(&dir.join(DECODER_MODEL))?;
        let io = decoder_io(&decoder)?;
        Ok(Self {
            vision,
            embed,
            decoder,
            tok,
            io,
            max_new_tokens: vl_decode::DEFAULT_MAX_NEW_TOKENS,
        })
    }

    /// Cap the tokens generated per region; clamped to
    /// [`vl_decode::MAX_NEW_TOKENS_CAP`].
    #[must_use]
    pub fn with_max_new_tokens(mut self, n: usize) -> Self {
        self.max_new_tokens = n.min(vl_decode::MAX_NEW_TOKENS_CAP);
        self
    }

    /// The tokens-per-region ceiling in force.
    #[must_use]
    pub fn max_new_tokens(&self) -> usize {
        self.max_new_tokens
    }

    /// Read one region: a greyscale image, row-major, one byte per pixel.
    /// The ink box (plus a margin) is what the model sees.
    ///
    /// # Errors
    ///
    /// [`PaddleVlError::Prepare`] for a bad buffer or an image outside the
    /// size and aspect ceilings; [`PaddleVlError::Recognition`] when
    /// inference fails.
    pub fn read_region(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<RegionReading, PaddleVlError> {
        let img = vl_pre::grey(width, height, pixels)?;
        let Some(ink) = vl_pre::ink_bbox(&img) else {
            return Ok(RegionReading {
                text: String::new(),
                lines: Vec::new(),
                placement: LinePlacement::None,
                stop: None,
                tokens: 0,
                image_tokens: 0,
                ink_box: None,
            });
        };
        let (region, _) = vl_pre::crop(&img, ink, CROP_MARGIN);
        let (rh, rw) = vl_pre::smart_resize(region.height, region.width)?;
        let resized = vl_pre::resize(&region, rw, rh);
        let image = self.encode_image(&resized)?;
        let n_image = image.size(0);
        let prompt = vl_pre::prompt(&self.tok, n_image)?;
        let decoded = self.generate(&prompt, &image)?;
        let text = self.tok.decode(&decoded.tokens, true);
        let (lines, placement) = vl_pre::place_lines(&img, ink, &text, decoded.mean_probability);
        Ok(RegionReading {
            text,
            lines,
            placement,
            stop: Some(decoded.stop),
            tokens: decoded.tokens.len(),
            image_tokens: n_image,
            ink_box: Some(ink),
        })
    }

    /// Vision encoder: `[n_image, hidden]` embeddings.
    fn encode_image(&self, img: &vl_pre::Grey) -> Result<NdTensor<f32, 2>, PaddleVlError> {
        let (patches, gh, gw) = vl_pre::patchify(img);
        let n = gh * gw;
        let pix = NdTensor::try_from_data([1, n, 3, vl_pre::PATCH, vl_pre::PATCH], patches)
            .map_err(run_err)?;
        let thw: Vec<i32> = [1, gh, gw]
            .iter()
            .map(|&v| i32::try_from(v).map_err(run_err))
            .collect::<Result<_, _>>()?;
        let thw = NdTensor::try_from_data([1, 3], thw).map_err(run_err)?;
        let v = &self.vision;
        let ids = [
            node(v, "vision encoder", "pixel_values")?,
            node(v, "vision encoder", "image_grid_thw")?,
        ];
        let out = node(v, "vision encoder", "image_embeds")?;
        let inputs = vec![(ids[0], pix.view().into()), (ids[1], thw.view().into())];
        let [embeds] = v.run_n(inputs, [out], None).map_err(run_err)?;
        let embeds: NdTensor<f32, 2> = embeds.try_into().map_err(run_err)?;
        if embeds.size(0) != vl_pre::image_tokens(gh, gw) {
            return Err(run_err(format!(
                "the vision encoder returned {} image tokens for a {gh}x{gw} patch grid",
                embeds.size(0)
            )));
        }
        Ok(embeds)
    }

    /// Embedding model: `[1, ids.len(), hidden]`.
    fn embed_ids(&self, ids: &[u32]) -> Result<NdTensor<f32, 3>, PaddleVlError> {
        // ONNX int64 inputs are i32 tensors in rten.
        let ids: Vec<i32> = ids
            .iter()
            .map(|&i| i32::try_from(i).map_err(run_err))
            .collect::<Result<_, _>>()?;
        let t = NdTensor::try_from_data([1, ids.len()], ids).map_err(run_err)?;
        let out = self.embed.run_one(t.view().into(), None).map_err(run_err)?;
        out.try_into().map_err(run_err)
    }

    /// The prompt's embeddings with the image embeddings in place of the
    /// placeholders.
    fn prompt_embeddings(
        &self,
        prompt: &Prompt,
        image: &NdTensor<f32, 2>,
    ) -> Result<NdTensor<f32, 3>, PaddleVlError> {
        let mut x = self.embed_ids(&prompt.ids)?;
        let hidden = x.size(2);
        if image.size(1) != hidden {
            return Err(run_err(format!(
                "image embeddings are {} wide; token embeddings are {hidden}",
                image.size(1)
            )));
        }
        let src = image
            .data()
            .ok_or_else(|| run_err("non-contiguous image embeddings"))?;
        let at = prompt.image_at * hidden;
        let dst = x
            .data_mut()
            .and_then(|d| d.get_mut(at..at + src.len()))
            .ok_or_else(|| run_err("image placeholders fall outside the prompt"))?;
        dst.copy_from_slice(src);
        Ok(x)
    }

    /// Greedy decoding with the key/value cache.
    fn generate(
        &self,
        prompt: &Prompt,
        image: &NdTensor<f32, 2>,
    ) -> Result<vl_decode::Decoded, PaddleVlError> {
        let mut prefill = Some(self.prompt_embeddings(prompt, image)?);
        let shape = [1, self.io.kv_heads, 0, self.io.head_dim];
        let mut past: Vec<Value> = self
            .io
            .cache
            .iter()
            .map(|_| NdTensor::<f32, 4>::zeros(shape).into())
            .collect();
        let mut total = 0usize;
        let step = |tok: Option<u32>| -> Result<Vec<f32>, PaddleVlError> {
            let x = match tok {
                None => prefill.take().ok_or_else(|| run_err("prefill ran twice"))?,
                Some(t) => self.embed_ids(&[t])?,
            };
            total += x.size(1);
            let (logits, next) = self.decoder_step(&x, total, &past)?;
            past = next;
            Ok(logits)
        };
        vl_decode::greedy(prompt.eos, self.max_new_tokens, step).map_err(|e| match e {
            vl_decode::DecodeError::Step(e) => e,
            other => run_err(other),
        })
    }

    /// One decoder run: the last position's logits and the new cache.
    fn decoder_step(
        &self,
        x: &NdTensor<f32, 3>,
        total: usize,
        past: &[Value],
    ) -> Result<(Vec<f32>, Vec<Value>), PaddleVlError> {
        let io = &self.io;
        let mask = NdTensor::<i32, 2>::full([1, total], 1);
        let mut inputs: Vec<(NodeId, ValueOrView)> =
            vec![(io.embeds, x.view().into()), (io.mask, mask.view().into())];
        inputs.extend(
            io.cache
                .iter()
                .zip(past)
                .map(|((id, _), v)| (*id, v.into())),
        );
        let mut outputs = vec![io.logits];
        outputs.extend(io.cache.iter().map(|(_, out)| *out));
        let mut res = self.decoder.run(inputs, &outputs, None).map_err(run_err)?;
        if res.len() != outputs.len() {
            return Err(run_err("the decoder returned too few outputs"));
        }
        let logits: Tensor<f32> = res.remove(0).try_into().map_err(run_err)?;
        let seq = match logits.shape() {
            [1, seq, _] if *seq > 0 => *seq,
            other => return Err(run_err(format!("logits have shape {other:?}"))),
        };
        let last = logits.slice((0, seq - 1)).to_vec();
        Ok((last, res))
    }
}

impl OcrEngine for PaddleVlEngine {
    type Error = PaddleVlError;

    /// The whole image is one region; see [`PaddleVlEngine::read_region`].
    fn recognize(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<RecognizedWord>, Self::Error> {
        self.read_region(width, height, pixels).map(|r| r.lines)
    }

    /// The region's mean token probability, on every line.
    fn reports_confidence(&self) -> bool {
        true
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_names_the_path_and_the_fix() {
        match PaddleVlEngine::from_model_dir(Path::new("no-such-dir-paddle-vl")) {
            Err(e @ PaddleVlError::ModelMissing { .. }) => {
                let msg = e.to_string();
                assert!(msg.contains(VISION_MODEL), "{msg}");
                assert!(msg.contains("build-paddle-vl-addon.py"), "{msg}");
            }
            other => panic!("expected ModelMissing, got {other:?}"),
        }
    }

    #[test]
    fn the_file_names_are_pinned() {
        assert_eq!(
            REQUIRED_FILES,
            [
                "vision_encoder.onnx",
                "decoder.onnx",
                "embedding.onnx",
                "embedding.onnx.data",
                "tokenizer.json"
            ]
        );
    }

    /// The real model, from a folder named by `PDFCER_PADDLE_VL_DIR`, reads a
    /// synthetic two-line image drawn in a blocky bitmap font.
    #[test]
    #[ignore = "needs a PaddleOCR-VL add-on folder in PDFCER_PADDLE_VL_DIR"]
    fn reads_text_with_the_real_model() {
        let dir = PathBuf::from(std::env::var("PDFCER_PADDLE_VL_DIR").expect("set the env var"));
        let bytes = std::fs::read(dir.join(TOKENIZER)).unwrap();
        let tok = VlTokenizer::from_json_bytes(&bytes).unwrap();
        // Reference ids from the Hugging Face `tokenizers` library.
        assert_eq!(
            tok.encode("<|begin_of_sentence|>User: <|IMAGE_START|>")
                .unwrap(),
            [100_273, 2969, 93963, 93919, 101_305]
        );
        assert_eq!(
            tok.encode("<|IMAGE_END|>OCR:\nAssistant:\n").unwrap(),
            [101_306, 93972, 2497, 93963, 23, 92267, 93963, 23]
        );
        assert_eq!(
            tok.encode("Hello w\u{f6}rld \u{4f60}\u{597d} \u{e9}\u{1f600}")
                .unwrap(),
            [16276, 296, 2235, 494, 93919, 5300, 1697, 253, 172, 165, 141]
        );
        assert_eq!(
            tok.decode(&[5, 6, 7, 100, 2000, 1, 2, 100_295], true),
            "234W </"
        );

        let engine = PaddleVlEngine::from_model_dir(&dir)
            .unwrap()
            .with_max_new_tokens(64);
        let (w, h, px) = test_image::two_lines();
        let r = engine.read_region(w, h, &px).unwrap();
        eprintln!("{r:?}");
        let text = r.text.to_uppercase();
        assert!(text.contains("CREEP 42"), "{}", r.text);
        assert!(text.contains("HELLO"), "{}", r.text);
        assert_eq!(r.stop, Some(StopReason::EndOfSequence));
        assert_eq!(r.placement, LinePlacement::Bands);
    }

    /// A 3x5 bitmap font, scaled up. No `D`: at 3x5 it reads as `O`.
    mod test_image {
        const GLYPHS: [(char, [&str; 5]); 9] = [
            ('P', ["###", "#.#", "###", "#..", "#.."]),
            ('C', ["###", "#..", "#..", "#..", "###"]),
            ('E', ["###", "#..", "##.", "#..", "###"]),
            ('R', ["##.", "#.#", "##.", "#.#", "#.#"]),
            ('4', ["#.#", "#.#", "###", "..#", "..#"]),
            ('2', ["###", "..#", "###", "#..", "###"]),
            ('H', ["#.#", "#.#", "###", "#.#", "#.#"]),
            ('L', ["#..", "#..", "#..", "#..", "###"]),
            ('O', ["###", "#.#", "#.#", "#.#", "###"]),
        ];
        const SCALE: usize = 8;

        fn draw(px: &mut [u8], width: usize, text: &str, top: usize) {
            let mut left = 40;
            for c in text.chars() {
                if let Some((_, rows)) = GLYPHS.iter().find(|(g, _)| *g == c) {
                    for (ry, row) in rows.iter().enumerate() {
                        for (rx, b) in row.bytes().enumerate() {
                            if b != b'#' {
                                continue;
                            }
                            for dy in 0..SCALE {
                                for dx in 0..SCALE {
                                    let (x, y) = (left + rx * SCALE + dx, top + ry * SCALE + dy);
                                    px[y * width + x] = 0;
                                }
                            }
                        }
                    }
                }
                left += 4 * SCALE;
            }
        }

        /// A 640x200 grey page with "CREEP 42" over "HELLO" in block glyphs.
        pub(super) fn two_lines() -> (u32, u32, Vec<u8>) {
            let (w, h) = (640usize, 200usize);
            let mut px = vec![255u8; w * h];
            draw(&mut px, w, "CREEP 42", 30);
            draw(&mut px, w, "HELLO", 120);
            (640, 200, px)
        }
    }
}
