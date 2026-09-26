//! Chunk 15 parity fixture (`ARCHITECTURE.md` section 11, "Chunk 15
//! interfaces" item 4): 256 fixed crops, PyTorch's log-probs for them under
//! the network's dequantised tensors, and the assertion that
//! `ocrcer_core::nn::Nn::forward` agrees.
//!
//! # Fixture provenance
//!
//! `fixtures/nn/parity_256/`:
//! - `layers.json` -- the shipped network's layer list (kind + shape per
//!   layer) and provenance, copied from `tools/nn/train.py`'s shipped run
//!   (`cpu_det_run1`, seed 0, deterministic CPU) `spec.json`.
//! - `<layer_index>.weight.f32` / `<layer_index>.bias.f32` -- the
//!   **dequantised** tensors `ocrcer-build write --nn-dequant-out` wrote
//!   for that run, i.e. the network the runtime actually runs
//!   post-quantisation, not the pristine trainer output (`ocrcer-build`'s
//!   `nn.rs` module doc gives the reasoning: a parity check has to compare
//!   what ships).
//! - `crops_G.f32` / `crops_X.f32` / `crops_y.u16` -- 256 crops selected
//!   from the nn15b dump's `bank_train` group (synthetic, font-rendered
//!   positives; see `tools/nn/README.md` for what `bank_train` is), every
//!   625th of 160,099 rows (`k = n // 256`, starting at index 0 --
//!   `manifest.json` in the same directory records the exact indices).
//!   `real_train` (also part of the trainer's positive pool) is
//!   deliberately left out of this committed fixture: it is derived from
//!   `finfilings-train` page content, and this fixture is checked into
//!   git, so it stays out on the same "if in doubt, exclude" reasoning
//!   `CLAUDE.md` applies to licence-sensitive inputs generally.
//! - `manifest.json` -- selection provenance (source, indices, stride,
//!   charset hash, feature extractor id, torch version).
//!
//! `fixtures/expected/nn/parity_256.log_probs.f32` -- 256 x 188 `f32`,
//! PyTorch's `log_softmax` output for those 256 crops under the same
//! dequantised tensors, from `tools/nn/parity.py`.
//!
//! Regenerating this fixture (not a `bless` binary -- there is no bench
//! integration for this stage; see the chunk 15 integration report) means
//! re-running `tools/nn/parity.py` against a (possibly new) trainer run
//! and copying its output over this directory by hand. That is a
//! deliberate, reviewed act the same as any other fixture bless
//! (`ARCHITECTURE.md` section 8.2) -- this test does not do it.
//!
//! # Tolerance
//!
//! Top-1 (plain `argmax` over all `n_outputs`, junk included) must match on
//! every crop. The largest absolute log-probability difference must be
//! `<= 1e-4` -- a guess per `ARCHITECTURE.md` section 11, confirmed at the
//! first bless. Both figures actually measured are printed by
//! `cargo test -p ocrcer-core --test nn_parity -- --nocapture`.

use ocrcer_core::json::Json;
use ocrcer_core::nn::{Layer, LayerKind, Nn};

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/nn/parity_256");
const EXPECTED_LOG_PROBS: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/expected/nn/parity_256.log_probs.f32");

const GRID: usize = ocrcer_core::nn::GRID;
const FEATURE_DIMS: usize = 107;
const N_OUTPUTS: usize = 188;
const COUNT: usize = 256;
const TOLERANCE: f32 = 1e-4;

fn read_f32_file(path: &str) -> Vec<f32> {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(bytes.len() % 4, 0, "{path}: not a whole number of f32 values");
    bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn kind_from_json(s: &str) -> LayerKind {
    LayerKind::parse(s).unwrap_or_else(|| panic!("layers.json: unknown layer kind {s:?}"))
}

fn load_net() -> Nn {
    let spec_text = std::fs::read_to_string(format!("{FIXTURE_DIR}/layers.json"))
        .expect("fixtures/nn/parity_256/layers.json");
    let spec = Json::parse(&spec_text).expect("layers.json: invalid JSON");

    let nn_version = spec.get("nn_version").and_then(Json::as_u32).expect("nn_version");
    let junk_index = spec.get("junk_index").and_then(Json::as_u32).expect("junk_index");
    let n_outputs = spec.get("n_outputs").and_then(Json::as_u32).expect("n_outputs");
    assert_eq!(n_outputs as usize, N_OUTPUTS);

    let layers_v = spec.get("layers").and_then(Json::as_array).expect("layers");
    let mut layers = Vec::with_capacity(layers_v.len());
    for l in layers_v {
        let index = l.get("index").and_then(Json::as_u32).expect("layer.index") as usize;
        let kind_text = l.get("kind").and_then(Json::as_str).expect("layer.kind");
        let kind = kind_from_json(kind_text);
        let shape: Vec<u32> = l
            .get("shape")
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(Json::as_u32).collect())
            .unwrap_or_default();

        let (weight, bias) = if kind.has_params() {
            (
                read_f32_file(&format!("{FIXTURE_DIR}/{index}.weight.f32")),
                read_f32_file(&format!("{FIXTURE_DIR}/{index}.bias.f32")),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        layers.push(Layer { kind, shape, weight, bias });
    }

    Nn { nn_version, junk_index, n_outputs, layers }
}

fn load_crops() -> (Vec<[[f32; GRID]; GRID]>, Vec<[f32; FEATURE_DIMS]>) {
    let g_flat = read_f32_file(&format!("{FIXTURE_DIR}/crops_G.f32"));
    let x_flat = read_f32_file(&format!("{FIXTURE_DIR}/crops_X.f32"));
    assert_eq!(g_flat.len(), COUNT * GRID * GRID);
    assert_eq!(x_flat.len(), COUNT * FEATURE_DIMS);

    let mut grids = Vec::with_capacity(COUNT);
    for c in 0..COUNT {
        let mut grid = [[0f32; GRID]; GRID];
        for y in 0..GRID {
            grid[y].copy_from_slice(&g_flat[c * GRID * GRID + y * GRID..c * GRID * GRID + (y + 1) * GRID]);
        }
        grids.push(grid);
    }

    let mut feats = Vec::with_capacity(COUNT);
    for c in 0..COUNT {
        let mut f = [0f32; FEATURE_DIMS];
        f.copy_from_slice(&x_flat[c * FEATURE_DIMS..(c + 1) * FEATURE_DIMS]);
        feats.push(f);
    }

    (grids, feats)
}

fn argmax(v: &[f32]) -> usize {
    let mut best = 0;
    for i in 1..v.len() {
        if v[i] > v[best] {
            best = i;
        }
    }
    best
}

#[test]
fn rust_forward_pass_agrees_with_pytorch_on_the_dequantised_network() {
    let net = load_net();
    let (grids, feats) = load_crops();
    let expected = read_f32_file(EXPECTED_LOG_PROBS);
    assert_eq!(expected.len(), COUNT * N_OUTPUTS, "expected_log_probs.f32 has the wrong length");

    let mut max_abs_diff = 0f32;
    let mut sum_abs_diff = 0f64;
    let mut n_values = 0usize;
    let mut top1_mismatches = Vec::new();

    for (i, (grid, feat)) in grids.iter().zip(feats.iter()).enumerate() {
        let got = net.forward(grid, feat).unwrap_or_else(|e| panic!("crop {i}: forward pass failed: {e:?}"));
        assert_eq!(got.len(), N_OUTPUTS, "crop {i}: wrong output width");

        let want = &expected[i * N_OUTPUTS..(i + 1) * N_OUTPUTS];

        let got_top1 = argmax(&got);
        let want_top1 = argmax(want);
        if got_top1 != want_top1 {
            top1_mismatches.push((i, got_top1, want_top1));
        }

        for (g, w) in got.iter().zip(want.iter()) {
            let diff = (g - w).abs();
            max_abs_diff = max_abs_diff.max(diff);
            sum_abs_diff += diff as f64;
            n_values += 1;
        }
    }

    let mean_abs_diff = sum_abs_diff / n_values as f64;
    println!("nn parity: {COUNT} crops, max |delta log p| = {max_abs_diff:.3e}, mean |delta log p| = {mean_abs_diff:.3e}");

    assert!(
        top1_mismatches.is_empty(),
        "top-1 disagreed with PyTorch on {} / {COUNT} crops: {:?}",
        top1_mismatches.len(),
        &top1_mismatches[..top1_mismatches.len().min(10)]
    );
    assert!(
        max_abs_diff <= TOLERANCE,
        "max |delta log p| = {max_abs_diff:.3e} exceeds the {TOLERANCE:.0e} tolerance"
    );
}
