//! The PP-DocLayoutV3 layout model, behind the `ocr-vl` feature: a page
//! image in, [`LayoutRegion`](crate::ocr::layout::LayoutRegion)s out ([`layout`](crate::ocr::layout)).
//!
//! Model: PaddlePaddle/PP-DocLayoutV3 (Apache-2.0, stated in the model
//! card), the official ONNX export huggingface.co/PaddlePaddle/PP-DocLayoutV3_onnx,
//! opset 17, loaded by `rten` unchanged. Nothing ships and nothing is
//! fetched: the file arrives in the PaddleOCR-VL add-on folder as
//! [`LAYOUT_MODEL`](crate::ocr::engine_layout::LAYOUT_MODEL).
//!
//! Interface (measured against onnxruntime): inputs `im_shape` f32 `[1,2]` =
//! `[800,800]`, `image` f32 `[1,3,800,800]` RGB 0..=1 after a bicubic resize
//! that ignores the aspect ratio, `scale_factor` f32 `[1,2]` =
//! `[800/h, 800/w]`; output `fetch_name_0` f32 `[300,7]` = class, score, x1,
//! y1, x2, y2, order, boxes in the input image's pixels.

use std::path::Path;

use rten::{Model, NodeId};
use rten_tensor::NdTensor;
use rten_tensor::prelude::*;

use super::engine_paddle_vl::{PaddleVlError, run_err};
use super::layout::{DEFAULT_SCORE_THRESHOLD, LayoutRegion, decode_rows};
use super::vl_pre;

/// The layout model's file name in the add-on folder.
pub const LAYOUT_MODEL: &str = "layout.onnx";
/// The side the model's input is resized to.
pub const INPUT_SIDE: usize = 800;

/// The PP-DocLayoutV3 model, loaded.
pub struct LayoutEngine {
    model: Model,
    im_shape: NodeId,
    image: NodeId,
    scale_factor: NodeId,
    rows: NodeId,
    threshold: f32,
}

impl std::fmt::Debug for LayoutEngine {
    /// Hand-written because [`rten::Model`] is not [`Debug`].
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutEngine")
            .field("threshold", &self.threshold)
            .finish_non_exhaustive()
    }
}

impl LayoutEngine {
    /// Load [`LAYOUT_MODEL`] from an add-on folder.
    ///
    /// # Errors
    ///
    /// [`PaddleVlError::ModelMissing`], [`PaddleVlError::ModelLoad`], or
    /// [`PaddleVlError::Interface`] when the model's inputs and outputs are
    /// not PP-DocLayoutV3's.
    pub fn from_model_dir(dir: &Path) -> Result<Self, PaddleVlError> {
        let path = dir.join(LAYOUT_MODEL);
        if !path.is_file() {
            return Err(PaddleVlError::ModelMissing { path });
        }
        let model = Model::load_file(&path).map_err(|e| PaddleVlError::ModelLoad {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        let node = |name: &str| {
            model.node_id(name).map_err(|_| PaddleVlError::Interface {
                model: "layout model",
                reason: format!("no `{name}`"),
            })
        };
        Ok(Self {
            im_shape: node("im_shape")?,
            image: node("image")?,
            scale_factor: node("scale_factor")?,
            rows: node("fetch_name_0")?,
            model,
            threshold: DEFAULT_SCORE_THRESHOLD,
        })
    }

    /// Keep regions scoring at least `threshold` (clamped to 0..=1).
    #[must_use]
    pub fn with_score_threshold(mut self, threshold: f32) -> Self {
        self.threshold = if threshold.is_finite() {
            threshold.clamp(0.0, 1.0)
        } else {
            DEFAULT_SCORE_THRESHOLD
        };
        self
    }

    /// The score threshold in force.
    #[must_use]
    pub fn score_threshold(&self) -> f32 {
        self.threshold
    }

    /// Detect the regions of a greyscale page image, row-major, one byte per
    /// pixel, in reading order. Boxes are in this image's pixels.
    ///
    /// # Errors
    ///
    /// [`PaddleVlError::Prepare`] for a bad buffer or an image over
    /// [`vl_pre::MAX_INPUT_PIXELS`]; [`PaddleVlError::Recognition`] when
    /// inference fails or returns rows of the wrong shape.
    pub fn detect(
        &self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<Vec<LayoutRegion>, PaddleVlError> {
        let img = vl_pre::grey(width, height, pixels)?;
        let small = vl_pre::resize(&img, INPUT_SIDE, INPUT_SIDE);
        let plane: Vec<f32> = small.pixels.iter().map(|&p| f32::from(p) / 255.0).collect();
        let mut chw = Vec::with_capacity(plane.len() * 3);
        for _ in 0..3 {
            chw.extend_from_slice(&plane);
        }
        let side = INPUT_SIDE as f32;
        let (w, h) = (img.width as f32, img.height as f32);
        let image =
            NdTensor::try_from_data([1, 3, INPUT_SIDE, INPUT_SIDE], chw).map_err(run_err)?;
        let im_shape = NdTensor::from([[side, side]]);
        let scale = NdTensor::from([[side / h, side / w]]);
        let inputs = vec![
            (self.im_shape, im_shape.view().into()),
            (self.image, image.view().into()),
            (self.scale_factor, scale.view().into()),
        ];
        let [rows] = self
            .model
            .run_n(inputs, [self.rows], None)
            .map_err(run_err)?;
        let rows: NdTensor<f32, 2> = rows.try_into().map_err(run_err)?;
        if rows.size(1) != 7 {
            return Err(run_err(format!(
                "the layout model returned rows {} wide, not 7",
                rows.size(1)
            )));
        }
        let data = rows.to_vec();
        Ok(decode_rows(&data, w, h, self.threshold))
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
    use crate::ocr::layout::LayoutClass;

    #[test]
    fn a_missing_model_is_named() {
        let dir = std::env::temp_dir().join("pdfcer-no-layout-model-here");
        match LayoutEngine::from_model_dir(&dir) {
            Err(PaddleVlError::ModelMissing { path }) => assert!(path.ends_with(LAYOUT_MODEL)),
            other => panic!("expected ModelMissing, got {other:?}"),
        }
    }

    /// The real model, from a folder named by `PDFCER_PADDLE_VL_DIR`, runs on
    /// a blank page and on a ruled grid and returns in-bounds regions. The
    /// classes are checked end to end on a rendered page by the CLI's tests.
    #[test]
    #[ignore = "needs a PaddleOCR-VL add-on folder with layout.onnx in PDFCER_PADDLE_VL_DIR"]
    fn the_real_model_runs_and_its_boxes_are_in_bounds() {
        let dir = std::env::var("PDFCER_PADDLE_VL_DIR").expect("set the env var");
        let engine = LayoutEngine::from_model_dir(Path::new(&dir)).unwrap();
        let (w, h) = (620usize, 877usize);
        let blank = vec![255u8; w * h];
        let mut grid = blank.clone();
        for y in (200..600).step_by(50) {
            grid[y * w + 50..y * w + 570].fill(0);
        }
        for (name, px) in [("blank", blank), ("grid", grid)] {
            let got = engine.detect(w as u32, h as u32, &px).unwrap();
            for r in &got {
                eprintln!(
                    "{name}: {} {:.2} {:?} {}",
                    r.class, r.score, r.bbox, r.order
                );
                let [x0, y0, x1, y1] = r.bbox;
                assert!(x0 < x1 && y0 < y1 && x1 <= w as f32 && y1 <= h as f32);
                assert!(r.score >= DEFAULT_SCORE_THRESHOLD);
                assert_ne!(LayoutClass::from_label(r.class.as_str()), None);
            }
            assert!(got.windows(2).all(|p| p[0].order <= p[1].order));
        }
    }
}
