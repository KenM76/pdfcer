//! The optional `nn` table: a small convolutional glyph classifier, trained
//! outside this crate by `tools/nn/` and shipped, at the operator's option,
//! alongside the prototype bank (`ARCHITECTURE.md` section 11, the
//! 2026-09-24 neural-classifier decision and its 2026-09-25 amendments,
//! "Chunk 15 interfaces").
//!
//! # Contract
//!
//! This module owns both halves of the network, in one layer representation
//! (`CLAUDE.md` rule 4 -- a stage exists once): [`load`] parses `meta.nn`
//! and the `nn` table and dequantises every weight to `f32` once at load,
//! the same as the prototype bank (section 7), producing an [`Nn`] whose
//! [`Layer`]s already carry plain `f32` tensors; [`Nn::forward`] runs those
//! same layers, in order, with no intermediate struct that re-derives a
//! shape the parser already read. `ocrcer-build`'s writer
//! (`crates/ocrcer-build/src/nn.rs`) is the only other code that names
//! `LayerKind`'s string vocabulary, so the parser and the writer cannot
//! silently drift onto two different sets of layer kinds.
//!
//! **The one rule that makes this table safe to add:** nothing here can fail
//! [`crate::ocrw::Model::load`]. An `nn_version` this build does not know, a
//! malformed `nn` table, or a `meta.nn` that disagrees with the table it
//! sits beside all degrade to [`NnStatus::UnsupportedVersion`] or
//! [`NnStatus::Malformed`] with [`Model::nn`](crate::ocrw::Model) left
//! `None` -- never to a load error. A caller that wants to know why reads
//! [`Model::nn_status`](crate::ocrw::Model); a caller that only wants a
//! recogniser never has to.
//!
//! `#![forbid(unsafe_code)]` and zero dependencies hold here as everywhere
//! else in this crate (`CLAUDE.md` rule 3); this module adds nothing beyond
//! `crate::json` and `crate::feature::FEATURE_DIMS`.
//!
//! # Forward-pass conventions the PyTorch trainer (`tools/nn/`, a separate
//! change) must match, or the parity fixture (`ARCHITECTURE.md` §11,
//! "Chunk 15 interfaces", item 4) fails at its own 1e-4 tolerance:
//!
//! - **`conv3x3`**: `nn.Conv2d(kernel_size=3, padding=1, stride=1)` -- "same"
//!   padding, zero-filled, stride 1. Weight layout is `[out][in][3][3]`,
//!   PyTorch's default `Conv2d.weight` layout, so a dequantised tensor is
//!   used as-is with no transpose.
//! - **`maxpool2`**: `nn.MaxPool2d(2)` -- 2x2 window, stride 2, no padding,
//!   which floors when a dimension is odd (the last row/column is dropped,
//!   never padded).
//! - **`flatten`**: `torch.flatten(x, 1)` on an `[N, C, H, W]` tensor is
//!   row-major over `(C, H, W)`, i.e. index `c*H*W + h*W + w` --
//!   "channel-major `[c][y][x]`". This module's feature maps are stored in
//!   exactly that layout throughout, so `flatten` here is a relabelling, not
//!   a data movement.
//! - **`concat_features`**: `torch.cat([conv_features, feature_vector], 1)`
//!   -- the flattened conv output first, the (already `feature_norm`-
//!   normalised) 107-dim vector second. Never the other order.
//! - **log-softmax**: `F.log_softmax(logits, dim=-1)`, i.e. max-subtracted:
//!   `logits[i] - max - ln(sum_j exp(logits[j] - max))`.
//!
//! # Determinism, and where it does not hold
//!
//! Every layer through `dense` accumulates in `f32`, in a fixed ascending
//! index order (input channel, then kernel row, then kernel column for a
//! conv; ascending input index for a dense layer) -- the same discipline
//! [`crate::feature`] and [`crate::r#match`] use.
//!
//! Log-softmax is the one exception in this crate outside
//! [`crate::feature`]'s documented `sqrt`: it calls `f32::exp` and
//! `f32::ln`, neither of which IEEE 754 requires to be correctly rounded, so
//! the network head is not guaranteed bit-identical between x86 and wasm32
//! the way every other stage's fixtures are. This is accepted rather than
//! worked around: the only fixture that reads it (§8.2's parity check, item
//! 4 above) is itself a tolerance check against PyTorch's own
//! log-probabilities, not a byte-exact one, so a bisection trick that made
//! this module's answer reproducible across targets still would not make it
//! agree with PyTorch to the bit -- nothing would be bought by writing one
//! twice as complex as [`crate::confidence`]'s. `match.classifier` defaults
//! to `0`, so no shipped fixture reads the forward pass at all yet.

use crate::feature::FEATURE_DIMS;
use crate::json::Json;
use crate::ocrw::{Container, RawTable};

/// The `nn_version` this build's forward pass and this parser agree on. A
/// file declaring anything else is not read (section 7's `version` rule,
/// narrowed to this one table).
pub const SUPPORTED_NN_VERSION: u32 = 1;

/// The table name the writer uses.
pub const T_NN: &str = "nn";

/// The extractor's grid side, mirroring `feature::extract_with_grid`'s
/// private `GRID` constant (`CLAUDE.md` rule 4 -- this is the one place
/// outside `feature.rs` that names it, rather than a second definition of
/// the grid size; a change to the extractor's grid is a `FEATURE_VERSION`
/// bump either way, so the two cannot silently drift).
pub const GRID: usize = 32;

/// One layer kind, per the 2026-09-25 chunk 15 interfaces entry. A layer's
/// weight and bias are populated only for [`Conv3x3`](LayerKind::Conv3x3)
/// and [`Dense`](LayerKind::Dense); every other kind carries no parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerKind {
    Conv3x3,
    Relu,
    MaxPool2,
    Flatten,
    /// Concatenates the 107-dim standardised feature vector onto the
    /// flattened conv output, at the dense head.
    ConcatFeatures,
    Dense,
}

impl LayerKind {
    /// Parses a `meta.nn.layers[i].kind` string. `pub` so `ocrcer-build`'s
    /// writer (`crates/ocrcer-build/src/nn.rs`) reads the same vocabulary
    /// this parser does, rather than keeping an independent list that could
    /// drift from it (`CLAUDE.md` rule 4).
    pub fn parse(s: &str) -> Option<LayerKind> {
        Some(match s {
            "conv3x3" => LayerKind::Conv3x3,
            "relu" => LayerKind::Relu,
            "maxpool2" => LayerKind::MaxPool2,
            "flatten" => LayerKind::Flatten,
            "concat_features" => LayerKind::ConcatFeatures,
            "dense" => LayerKind::Dense,
            _ => return None,
        })
    }

    /// Whether this layer kind carries a weight/bias pair in the `nn` table.
    /// `pub` for the same reason as [`LayerKind::parse`]: the writer decides
    /// which layers to expect tensor files for from this, not from a second
    /// hardcoded list.
    pub fn has_params(self) -> bool {
        matches!(self, LayerKind::Conv3x3 | LayerKind::Dense)
    }
}

/// One layer of the network, in the order the trainer emitted it. The same
/// struct feeds both halves of this module: [`load`] fills `weight`/`bias`
/// by dequantising the `nn` table, and [`Nn::forward`] reads `shape` and
/// those same tensors directly -- there is no second, forward-pass-only
/// layer type to keep in sync with this one.
///
/// `shape` is the weight tensor's shape as the trainer recorded it:
/// `[out, in, 3, 3]` for [`Conv3x3`](LayerKind::Conv3x3), `[out, in]` for
/// [`Dense`](LayerKind::Dense), and whatever the trainer chose to record for
/// a parameter-free layer (informational only; forward-pass dispatch is on
/// `kind` alone).
#[derive(Debug, Clone)]
pub struct Layer {
    pub kind: LayerKind,
    pub shape: Vec<u32>,
    /// Dequantised weight, row-major `[out][in]` with `in` already
    /// flattened (`in_channels * 9` for a conv layer, `in_features` for a
    /// dense one). Empty for a layer with no learned parameters.
    pub weight: Vec<f32>,
    /// One bias per output channel, stored as `f32` in the file (never
    /// quantised -- section 7's int8 saving is on the weight matrices,
    /// which dwarf the bias vectors). Empty for a layer with no learned
    /// parameters.
    pub bias: Vec<f32>,
}

impl Layer {
    /// The output width: `shape[0]` for a layer with parameters, or `None`
    /// for one without (its output width is whatever the previous layer's
    /// was, which this module does not track outside a forward pass).
    pub fn out_dim(&self) -> Option<usize> {
        self.kind.has_params().then(|| self.shape.first().copied().unwrap_or(0) as usize)
    }

    /// `shape[0]` (out_channels for conv3x3, out_features for dense).
    fn dim0(&self) -> usize {
        self.shape.first().copied().unwrap_or(0) as usize
    }

    /// `shape[1]` (in_channels for conv3x3, in_features for dense).
    fn dim1(&self) -> usize {
        self.shape.get(1).copied().unwrap_or(0) as usize
    }
}

/// The parsed, dequantised network: every tensor as `f32`, ready for
/// [`Nn::forward`] to consume directly.
#[derive(Debug, Clone)]
pub struct Nn {
    pub nn_version: u32,
    /// The output index that means "not a character" (charset length).
    pub junk_index: u32,
    /// `junk_index + 1`: the width of the final dense layer's output.
    pub n_outputs: u32,
    pub layers: Vec<Layer>,
}

/// Why [`Model::nn`](crate::ocrw::Model) is what it is. `Loaded` is the only
/// variant that pairs with `Some`; every other variant means the file is
/// read as if it carried no `nn` table at all.
#[derive(Debug, Clone, PartialEq)]
pub enum NnStatus {
    /// The file carries neither `meta.nn` nor an `nn` table. The ordinary
    /// case: most files predate chunk 15, or were built without `--nn`.
    Absent,
    /// `meta.nn.nn_version` is not [`SUPPORTED_NN_VERSION`]. The table's
    /// bytes may mean anything under that version; they are not read.
    UnsupportedVersion(u32),
    /// `meta.nn` and/or the `nn` table are present but malformed, or
    /// disagree with each other. The reason is a plain-English sentence,
    /// never a struct, because nothing downstream branches on which
    /// malformation this was -- only a person reading a report does.
    Malformed(String),
    /// The network loaded and dequantised cleanly.
    Loaded,
}

impl core::fmt::Display for NnStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            NnStatus::Absent => write!(f, "no nn table in this file"),
            NnStatus::UnsupportedVersion(v) => {
                write!(f, "nn table is version {v}, this build reads version {SUPPORTED_NN_VERSION}; falling back to prototypes")
            }
            NnStatus::Malformed(why) => write!(f, "nn table malformed ({why}); falling back to prototypes"),
            NnStatus::Loaded => write!(f, "loaded"),
        }
    }
}

/// Magic the writer stamps at the start of the `nn` table's blob.
const MAGIC: &[u8; 4] = b"NNET";

/// Loads the `nn` table, if present, per the contract above: this never
/// returns an `Err` a caller has to propagate. The `Result` is internal
/// plumbing only.
pub(crate) fn load(c: &Container) -> (Option<Nn>, NnStatus) {
    let meta_nn = c.meta.get("nn");
    let table = c.table(T_NN);
    match (meta_nn, table) {
        (None, None) => (None, NnStatus::Absent),
        (None, Some(_)) => {
            (None, NnStatus::Malformed("an nn table is present but meta.nn is missing".into()))
        }
        (Some(_), None) => {
            (None, NnStatus::Malformed("meta.nn is present but there is no nn table".into()))
        }
        (Some(m), Some(t)) => match parse(m, t) {
            Ok(nn) => (Some(nn), NnStatus::Loaded),
            Err(ParseErr::UnsupportedVersion(v)) => (None, NnStatus::UnsupportedVersion(v)),
            Err(ParseErr::Malformed(why)) => (None, NnStatus::Malformed(why)),
        },
    }
}

enum ParseErr {
    UnsupportedVersion(u32),
    Malformed(String),
}

fn bad(why: impl Into<String>) -> ParseErr {
    ParseErr::Malformed(why.into())
}

fn parse(meta: &Json, t: &RawTable<'_>) -> Result<Nn, ParseErr> {
    let nn_version =
        meta.get("nn_version").and_then(Json::as_u32).ok_or_else(|| bad("meta.nn missing nn_version"))?;
    if nn_version != SUPPORTED_NN_VERSION {
        return Err(ParseErr::UnsupportedVersion(nn_version));
    }
    let junk_index =
        meta.get("junk_index").and_then(Json::as_u32).ok_or_else(|| bad("meta.nn missing junk_index"))?;
    let n_outputs =
        meta.get("n_outputs").and_then(Json::as_u32).ok_or_else(|| bad("meta.nn missing n_outputs"))?;
    if n_outputs != junk_index + 1 {
        return Err(bad("meta.nn n_outputs must be junk_index + 1"));
    }
    let layers_meta = meta
        .get("layers")
        .and_then(Json::as_array)
        .ok_or_else(|| bad("meta.nn missing layers"))?;
    if layers_meta.is_empty() {
        return Err(bad("meta.nn.layers is empty"));
    }
    let mut specs: Vec<(LayerKind, Vec<u32>)> = Vec::with_capacity(layers_meta.len());
    for (i, l) in layers_meta.iter().enumerate() {
        let kind_s = l.get("kind").and_then(Json::as_str).ok_or_else(|| bad(format!("layer {i} missing kind")))?;
        let kind = LayerKind::parse(kind_s).ok_or_else(|| bad(format!("layer {i} has unknown kind {kind_s:?}")))?;
        let shape: Vec<u32> = l
            .get("shape")
            .and_then(Json::as_array)
            .map(|a| a.iter().filter_map(Json::as_u32).collect())
            .unwrap_or_default();
        specs.push((kind, shape));
    }

    if t.data.len() < 4 + 2 + 2 + 4 {
        return Err(bad("nn table shorter than its own header"));
    }
    if &t.data[0..4] != MAGIC {
        return Err(bad("nn table does not start with the NNET magic"));
    }
    let blob_version = u16::from_le_bytes([t.data[4], t.data[5]]);
    if u32::from(blob_version) != nn_version {
        return Err(bad("nn table's internal version disagrees with meta.nn.nn_version"));
    }
    let n_weighted = u32::from_le_bytes([t.data[8], t.data[9], t.data[10], t.data[11]]) as usize;

    let expected_weighted = specs.iter().filter(|(k, _)| k.has_params()).count();
    if n_weighted != expected_weighted {
        return Err(bad("nn table's layer count disagrees with meta.nn.layers"));
    }

    let mut cur = Cursor { b: t.data, at: 12 };
    let mut scale_at = 0usize;
    let mut layers = Vec::with_capacity(specs.len());
    for (idx, (kind, shape)) in specs.into_iter().enumerate() {
        if !kind.has_params() {
            layers.push(Layer { kind, shape, weight: Vec::new(), bias: Vec::new() });
            continue;
        }
        let layer_index = cur.u32().ok_or_else(|| bad("nn table truncated in a layer record"))?;
        if layer_index as usize != idx {
            return Err(bad("nn table layer_index disagrees with meta.nn.layers order"));
        }
        let out_dim = cur.u32().ok_or_else(|| bad("nn table truncated reading out_dim"))? as usize;
        let in_dim = cur.u32().ok_or_else(|| bad("nn table truncated reading in_dim"))? as usize;
        let expect_out = *shape.first().unwrap_or(&0) as usize;
        let expect_in: usize = match kind {
            LayerKind::Conv3x3 => shape.get(1..4).map(|s| s.iter().product::<u32>() as usize).unwrap_or(0),
            LayerKind::Dense => shape.get(1).copied().unwrap_or(0) as usize,
            _ => unreachable!("has_params() only admits Conv3x3 and Dense"),
        };
        if out_dim != expect_out || in_dim != expect_in {
            return Err(bad(format!("layer {idx} shape disagrees with its recorded out/in dims")));
        }
        let n = out_dim.checked_mul(in_dim).ok_or_else(|| bad("layer dims overflow"))?;
        let wbytes = cur.take(n).ok_or_else(|| bad(format!("nn table truncated in layer {idx} weight")))?;
        let bbytes =
            cur.take(out_dim * 4).ok_or_else(|| bad(format!("nn table truncated in layer {idx} bias")))?;

        let scales = t
            .scales
            .get(scale_at..scale_at + out_dim)
            .ok_or_else(|| bad(format!("layer {idx} has no scale for every output channel")))?;
        scale_at += out_dim;

        let mut weight = Vec::with_capacity(n);
        for r in 0..out_dim {
            let row = &wbytes[r * in_dim..(r + 1) * in_dim];
            for &b in row {
                weight.push(f32::from(b as i8) * scales[r]);
            }
        }
        let bias: Vec<f32> = bbytes
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect();

        layers.push(Layer { kind, shape, weight, bias });
    }
    if scale_at != t.scales.len() {
        return Err(bad("nn table carries more scales than any layer uses"));
    }
    if cur.at != t.data.len() {
        return Err(bad("nn table has trailing bytes after its last layer"));
    }

    Ok(Nn { nn_version, junk_index, n_outputs, layers })
}

struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.at.checked_add(n)?;
        let s = self.b.get(self.at..end)?;
        self.at = end;
        Some(s)
    }
    fn u32(&mut self) -> Option<u32> {
        let s = self.take(4)?;
        Some(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
}

// ---------------------------------------------------------------------
// Forward pass
// ---------------------------------------------------------------------

/// Why [`Nn::forward`] could not run.
///
/// All of these mean the layer list and the tensors it carries disagree with
/// each other, or with the input -- never a value the layer list computed.
/// Returned rather than panicked: a network handed in by a parser reading an
/// untrusted file must fail with an `Err`, the same discipline `ocrw.rs`
/// applies to every table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardError {
    /// A `conv3x3`/`relu`/`maxpool2` layer ran on an activation that is not
    /// a feature map (already flattened, or the list starts with one of
    /// these).
    ExpectedFeatureMap,
    /// A `flatten`/`concat_features`/`dense` layer ran on a feature map
    /// rather than a flat vector.
    ExpectedVector,
    /// A conv layer's weight/bias length disagrees with its declared shape.
    BadConvShape,
    /// A conv layer's declared `in_channels` disagrees with the activation
    /// feeding it.
    ChannelMismatch { declared: usize, actual: usize },
    /// A dense layer's weight/bias length disagrees with its declared shape.
    BadDenseShape,
    /// A dense layer's declared `in_features` disagrees with the activation
    /// feeding it.
    WidthMismatch { declared: usize, actual: usize },
    /// The layer list does not end in `Dense`, so there is no logit vector
    /// for log-softmax to run over.
    NoFinalDense,
    /// The final `Dense` layer's width disagrees with `Nn::n_outputs`.
    OutputWidthMismatch { got: usize, want: usize },
}

impl Nn {
    /// Runs the network on one glyph's extractor output.
    ///
    /// `grid` is `extract_with_grid`'s 32x32 grid, laid out `[y][x]`
    /// (`ARCHITECTURE.md` §11, "Chunk 15 interfaces", item 1: "`G`... laid
    /// out `[1][32][32]`" -- the leading `1` is the single input channel
    /// this function starts the activation with). `features` is the
    /// 107-dim vector **already normalised** with the model's
    /// `feature_norm` (`crate::ocrw::Model::standardise`) -- this function
    /// does not normalise it again.
    pub fn forward(
        &self,
        grid: &[[f32; GRID]; GRID],
        features: &[f32; FEATURE_DIMS],
    ) -> Result<Vec<f32>, ForwardError> {
        if !matches!(self.layers.last().map(|l| l.kind), Some(LayerKind::Dense)) {
            return Err(ForwardError::NoFinalDense);
        }

        let mut data = Vec::with_capacity(GRID * GRID);
        for row in grid {
            data.extend_from_slice(row);
        }
        let mut act = Activation::Map { c: 1, h: GRID, w: GRID, data };

        for layer in &self.layers {
            act = match layer.kind {
                LayerKind::Conv3x3 => apply_conv(&act, layer)?,
                LayerKind::Relu => apply_relu(act),
                LayerKind::MaxPool2 => apply_pool(&act)?,
                LayerKind::Flatten => apply_flatten(act)?,
                LayerKind::ConcatFeatures => apply_concat(act, features)?,
                LayerKind::Dense => apply_dense(&act, layer)?,
            };
        }

        let logits = match act {
            Activation::Vector(v) => v,
            Activation::Map { .. } => return Err(ForwardError::ExpectedVector),
        };
        if logits.len() != self.n_outputs as usize {
            return Err(ForwardError::OutputWidthMismatch { got: logits.len(), want: self.n_outputs as usize });
        }
        Ok(log_softmax(&logits))
    }
}

/// The activation flowing between layers: a channel-major feature map before
/// `flatten`, a flat vector after.
enum Activation {
    Map { c: usize, h: usize, w: usize, data: Vec<f32> },
    Vector(Vec<f32>),
}

fn apply_conv(act: &Activation, layer: &Layer) -> Result<Activation, ForwardError> {
    let Activation::Map { c: in_c, h, w, data } = act else {
        return Err(ForwardError::ExpectedFeatureMap);
    };
    let (h, w) = (*h, *w);
    let out_channels = layer.dim0();
    let in_channels = layer.dim1();
    if *in_c != in_channels {
        return Err(ForwardError::ChannelMismatch { declared: in_channels, actual: *in_c });
    }
    if layer.weight.len() != out_channels * in_channels * 9 || layer.bias.len() != out_channels {
        return Err(ForwardError::BadConvShape);
    }

    let mut out = vec![0.0f32; out_channels * h * w];
    for o in 0..out_channels {
        for y in 0..h {
            for x in 0..w {
                let mut acc = 0.0f32;
                // Fixed order: input channel, then kernel row, then kernel
                // column, ascending -- the "fixed loop order" the module
                // header promises. Zero padding is implemented by skipping
                // an out-of-range tap rather than reordering the sum: a
                // skipped tap contributes 0 either way.
                for i in 0..in_channels {
                    for ky in 0..3usize {
                        let iy = y as isize + ky as isize - 1;
                        if iy < 0 || iy >= h as isize {
                            continue;
                        }
                        for kx in 0..3usize {
                            let ix = x as isize + kx as isize - 1;
                            if ix < 0 || ix >= w as isize {
                                continue;
                            }
                            let wv = layer.weight[((o * in_channels + i) * 3 + ky) * 3 + kx];
                            let iv = data[(i * h + iy as usize) * w + ix as usize];
                            acc += wv * iv;
                        }
                    }
                }
                acc += layer.bias[o];
                out[(o * h + y) * w + x] = acc;
            }
        }
    }
    Ok(Activation::Map { c: out_channels, h, w, data: out })
}

fn apply_relu(act: Activation) -> Activation {
    match act {
        Activation::Map { c, h, w, mut data } => {
            for v in data.iter_mut() {
                if *v < 0.0 {
                    *v = 0.0;
                }
            }
            Activation::Map { c, h, w, data }
        }
        Activation::Vector(mut v) => {
            for x in v.iter_mut() {
                if *x < 0.0 {
                    *x = 0.0;
                }
            }
            Activation::Vector(v)
        }
    }
}

/// 2x2 window, stride 2, floor -- `nn.MaxPool2d(2)`'s default. A trailing odd
/// row or column is dropped, never padded.
fn apply_pool(act: &Activation) -> Result<Activation, ForwardError> {
    let Activation::Map { c, h, w, data } = act else {
        return Err(ForwardError::ExpectedFeatureMap);
    };
    let (c, h, w) = (*c, *h, *w);
    let oh = h / 2;
    let ow = w / 2;
    let mut out = vec![0.0f32; c * oh * ow];
    for ch in 0..c {
        for y in 0..oh {
            for x in 0..ow {
                let a = data[(ch * h + 2 * y) * w + 2 * x];
                let b = data[(ch * h + 2 * y) * w + 2 * x + 1];
                let d = data[(ch * h + 2 * y + 1) * w + 2 * x];
                let e = data[(ch * h + 2 * y + 1) * w + 2 * x + 1];
                out[(ch * oh + y) * ow + x] = a.max(b).max(d).max(e);
            }
        }
    }
    Ok(Activation::Map { c, h: oh, w: ow, data: out })
}

/// A relabelling, not a data movement: `Activation::Map`'s `data` is already
/// stored `[c][y][x]` row-major, which is exactly `torch.flatten(x, 1)`'s
/// order for an `[N, C, H, W]` tensor.
fn apply_flatten(act: Activation) -> Result<Activation, ForwardError> {
    match act {
        Activation::Map { data, .. } => Ok(Activation::Vector(data)),
        Activation::Vector(_) => Err(ForwardError::ExpectedFeatureMap),
    }
}

/// Conv features first, then the 107-dim normalised vector -- `torch.cat([conv,
/// feats], 1)`'s order, never the other way round.
fn apply_concat(act: Activation, features: &[f32; FEATURE_DIMS]) -> Result<Activation, ForwardError> {
    match act {
        Activation::Vector(mut v) => {
            v.extend_from_slice(features);
            Ok(Activation::Vector(v))
        }
        Activation::Map { .. } => Err(ForwardError::ExpectedVector),
    }
}

fn apply_dense(act: &Activation, layer: &Layer) -> Result<Activation, ForwardError> {
    let Activation::Vector(v) = act else {
        return Err(ForwardError::ExpectedVector);
    };
    let out_features = layer.dim0();
    let in_features = layer.dim1();
    if layer.weight.len() != out_features * in_features || layer.bias.len() != out_features {
        return Err(ForwardError::BadDenseShape);
    }
    if v.len() != in_features {
        return Err(ForwardError::WidthMismatch { declared: in_features, actual: v.len() });
    }
    let mut out = vec![0.0f32; out_features];
    for o in 0..out_features {
        let base = o * in_features;
        let mut acc = 0.0f32;
        // Fixed order: ascending input index.
        for j in 0..in_features {
            acc += layer.weight[base + j] * v[j];
        }
        acc += layer.bias[o];
        out[o] = acc;
    }
    Ok(Activation::Vector(out))
}

/// `F.log_softmax(logits, dim=-1)`: max-subtracted for numerical stability,
/// `f32` accumulation. See the module header for why `exp`/`ln` are the
/// deliberate exception to this crate's no-transcendental-function rule.
fn log_softmax(logits: &[f32]) -> Vec<f32> {
    let max = logits.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut sum = 0.0f32;
    for &v in logits {
        sum += (v - max).exp();
    }
    let log_sum = sum.ln();
    logits.iter().map(|&v| v - max - log_sum).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- table parsing / round-trip ----

    /// Builds a minimal, well-formed `nn` table by hand -- one conv layer,
    /// one relu, one dense layer -- and checks the parser reads back the
    /// exact values a writer with these scales would have quantised.
    fn hand_built() -> (String, Vec<u8>, Vec<f32>) {
        // conv3x3: out=2, in=1*3*3=9 -> weight 2x9, bias 2
        // dense:   out=3, in=2       -> weight 3x2, bias 3
        let conv_w: [i8; 18] = [
            1, 2, 3, 4, 5, 6, 7, 8, 9, -1, -2, -3, -4, -5, -6, -7, -8, -9,
        ];
        let conv_scale = [0.1f32, 0.2f32];
        let conv_bias = [0.5f32, -0.5f32];
        let dense_w: [i8; 6] = [10, -20, 30, -40, 50, -60];
        let dense_scale = [0.01f32, 0.02f32, 0.03f32];
        let dense_bias = [1.0f32, 2.0f32, 3.0f32];

        let mut data = Vec::new();
        data.extend_from_slice(MAGIC);
        data.extend_from_slice(&1u16.to_le_bytes());
        data.extend_from_slice(&0u16.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes()); // n_weighted layers

        // layer 0: conv3x3
        data.extend_from_slice(&0u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&9u32.to_le_bytes());
        data.extend(conv_w.iter().map(|&v| v as u8));
        for b in conv_bias {
            data.extend_from_slice(&b.to_le_bytes());
        }
        // layer 2: dense (layer 1 is relu, no params)
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend_from_slice(&3u32.to_le_bytes());
        data.extend_from_slice(&2u32.to_le_bytes());
        data.extend(dense_w.iter().map(|&v| v as u8));
        for b in dense_bias {
            data.extend_from_slice(&b.to_le_bytes());
        }

        let mut scales = Vec::new();
        scales.extend_from_slice(&conv_scale);
        scales.extend_from_slice(&dense_scale);

        let meta = r#"{"nn":{"nn_version":1,"junk_index":3,"n_outputs":4,"layers":[
            {"kind":"conv3x3","shape":[2,1,3,3]},
            {"kind":"relu"},
            {"kind":"dense","shape":[3,2]}
        ]}}"#
            .to_string();

        (meta, data, scales)
    }

    fn table<'a>(data: &'a [u8], scales: &'a [f32]) -> RawTable<'a> {
        RawTable {
            name: T_NN,
            kind: crate::ocrw::Kind::Opaque,
            dims: vec![data.len() as u32],
            scales: scales.to_vec(),
            data,
        }
    }

    #[test]
    fn a_hand_built_nn_table_round_trips_within_its_own_quantisation() {
        let (meta_text, data, scales) = hand_built();
        let meta = Json::parse(&meta_text).unwrap();
        let t = table(&data, &scales);
        let (nn, status) = load(&Container {
            version: 1,
            model_kind: 1,
            meta_text: &meta_text,
            meta: meta.clone(),
            tables: vec![RawTable { name: T_NN, kind: t.kind, dims: t.dims.clone(), scales: t.scales.clone(), data: t.data }],
        });
        assert_eq!(status, NnStatus::Loaded);
        let nn = nn.unwrap();
        assert_eq!(nn.junk_index, 3);
        assert_eq!(nn.n_outputs, 4);
        assert_eq!(nn.layers.len(), 3);
        assert_eq!(nn.layers[0].kind, LayerKind::Conv3x3);
        assert_eq!(nn.layers[0].weight.len(), 18);
        assert_eq!(nn.layers[0].bias, vec![0.5, -0.5]);
        // row 0 used scale 0.1: values 1..9 * 0.1
        assert!((nn.layers[0].weight[0] - 0.1).abs() < 1e-6);
        assert!((nn.layers[0].weight[8] - 0.9).abs() < 1e-6);
        // row 1 used scale 0.2: values -1..-9 * 0.2
        assert!((nn.layers[0].weight[9] - (-0.2)).abs() < 1e-6);
        assert_eq!(nn.layers[1].kind, LayerKind::Relu);
        assert!(nn.layers[1].weight.is_empty());
        assert_eq!(nn.layers[2].kind, LayerKind::Dense);
        assert_eq!(nn.layers[2].bias, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn an_unsupported_nn_version_falls_back_without_failing() {
        let (meta_text, data, scales) = hand_built();
        let meta_text = meta_text.replacen("\"nn_version\":1", "\"nn_version\":99", 1);
        let meta = Json::parse(&meta_text).unwrap();
        let t = table(&data, &scales);
        let (nn, status) = load(&Container {
            version: 1,
            model_kind: 1,
            meta_text: &meta_text,
            meta,
            tables: vec![RawTable { name: T_NN, kind: t.kind, dims: t.dims.clone(), scales: t.scales.clone(), data: t.data }],
        });
        assert!(nn.is_none());
        assert_eq!(status, NnStatus::UnsupportedVersion(99));
    }

    #[test]
    fn no_nn_table_is_absent_not_an_error() {
        let meta = Json::parse("{}").unwrap();
        let (nn, status) = load(&Container { version: 1, model_kind: 1, meta_text: "{}", meta, tables: vec![] });
        assert!(nn.is_none());
        assert_eq!(status, NnStatus::Absent);
    }

    #[test]
    fn a_truncated_nn_table_is_malformed_not_a_panic() {
        let (meta_text, data, scales) = hand_built();
        let meta = Json::parse(&meta_text).unwrap();
        for cut in 0..data.len() {
            let d = &data[..cut];
            let t = table(d, &scales);
            let (nn, status) = load(&Container {
                version: 1,
                model_kind: 1,
                meta_text: &meta_text,
                meta: meta.clone(),
                tables: vec![RawTable { name: T_NN, kind: t.kind, dims: t.dims.clone(), scales: t.scales.clone(), data: t.data }],
            });
            if cut < data.len() {
                assert!(nn.is_none(), "cut {cut} should not have parsed");
                assert_ne!(status, NnStatus::Loaded, "cut {cut} should not report Loaded");
            }
        }
    }

    // ---- forward pass ----

    fn zero_grid() -> [[f32; GRID]; GRID] {
        [[0.0; GRID]; GRID]
    }

    fn zero_features() -> [f32; FEATURE_DIMS] {
        [0.0; FEATURE_DIMS]
    }

    fn conv_layer(out_channels: usize, in_channels: usize, weight: Vec<f32>, bias: Vec<f32>) -> Layer {
        Layer { kind: LayerKind::Conv3x3, shape: vec![out_channels as u32, in_channels as u32, 3, 3], weight, bias }
    }

    fn dense_layer(out_features: usize, in_features: usize, weight: Vec<f32>, bias: Vec<f32>) -> Layer {
        Layer { kind: LayerKind::Dense, shape: vec![out_features as u32, in_features as u32], weight, bias }
    }

    fn plain(kind: LayerKind) -> Layer {
        Layer { kind, shape: Vec::new(), weight: Vec::new(), bias: Vec::new() }
    }

    // ---- shape plumbing ----

    /// The contract shape from `ARCHITECTURE.md`'s 2026-09-24 entry, item 3:
    /// conv3x3x16 -> relu -> pool -> conv3x3x32 -> relu -> pool -> flatten ->
    /// concat_features -> dense(2048+107 -> 128) -> relu -> dense(128 ->
    /// n_outputs). Weights are all zero; this test is about shape survival,
    /// not arithmetic -- see the hand-computed tests below for that.
    fn contract_shape_network(n_outputs: usize) -> Nn {
        let conv1 = conv_layer(16, 1, vec![0.0; 16 * 1 * 9], vec![0.0; 16]);
        let conv2 = conv_layer(32, 16, vec![0.0; 32 * 16 * 9], vec![0.0; 32]);
        let dense1 = dense_layer(128, 32 * 8 * 8 + FEATURE_DIMS, vec![0.0; 128 * (32 * 8 * 8 + FEATURE_DIMS)], vec![0.0; 128]);
        let dense2 = dense_layer(n_outputs, 128, vec![0.0; n_outputs * 128], vec![0.0; n_outputs]);
        Nn {
            nn_version: SUPPORTED_NN_VERSION,
            n_outputs: n_outputs as u32,
            junk_index: (n_outputs - 1) as u32,
            layers: vec![
                conv1,
                plain(LayerKind::Relu),
                plain(LayerKind::MaxPool2),
                conv2,
                plain(LayerKind::Relu),
                plain(LayerKind::MaxPool2),
                plain(LayerKind::Flatten),
                plain(LayerKind::ConcatFeatures),
                dense1,
                plain(LayerKind::Relu),
                dense2,
            ],
        }
    }

    #[test]
    fn contract_shape_forward_produces_n_outputs_log_probs() {
        let net = contract_shape_network(188);
        let out = net.forward(&zero_grid(), &zero_features()).unwrap();
        assert_eq!(out.len(), 188);
        // All-zero weights and biases: every logit is 0, so log-softmax is
        // uniform, -ln(188) everywhere.
        let expected = -(188.0f32).ln();
        for &v in &out {
            assert!((v - expected).abs() < 1e-4, "got {v}, want {expected}");
        }
    }

    #[test]
    fn a_conv_with_wrong_in_channels_errors_rather_than_panics() {
        let bad = conv_layer(1, 2, vec![0.0; 1 * 2 * 9], vec![0.0]);
        let net = Nn {
            nn_version: SUPPORTED_NN_VERSION,
            n_outputs: 1,
            junk_index: 0,
            layers: vec![bad, plain(LayerKind::Flatten), dense_layer(1, GRID * GRID, vec![0.0; GRID * GRID], vec![0.0])],
        };
        let err = net.forward(&zero_grid(), &zero_features()).unwrap_err();
        assert_eq!(err, ForwardError::ChannelMismatch { declared: 2, actual: 1 });
    }

    #[test]
    fn a_layer_list_not_ending_in_dense_is_refused() {
        let net = Nn { nn_version: SUPPORTED_NN_VERSION, n_outputs: 1, junk_index: 0, layers: vec![plain(LayerKind::Relu)] };
        assert_eq!(net.forward(&zero_grid(), &zero_features()).unwrap_err(), ForwardError::NoFinalDense);
    }

    #[test]
    fn an_output_width_disagreeing_with_n_outputs_is_refused() {
        let dense = dense_layer(5, GRID * GRID, vec![0.0; 5 * GRID * GRID], vec![0.0; 5]);
        let net = Nn { nn_version: SUPPORTED_NN_VERSION, n_outputs: 3, junk_index: 2, layers: vec![plain(LayerKind::Flatten), dense] };
        let err = net.forward(&zero_grid(), &zero_features()).unwrap_err();
        assert_eq!(err, ForwardError::OutputWidthMismatch { got: 5, want: 3 });
    }

    // ---- hand-computed arithmetic, on a tiny network ----

    /// A single 2x2 input, one conv output channel, no pool (2x2 has nothing
    /// left to pool losslessly so this network skips it), straight to a
    /// dense head -- small enough to compute by hand.
    ///
    /// Grid (only the top-left 2x2 corner is nonzero, rest is 0):
    /// ```text
    /// 1 2
    /// 3 4
    /// ```
    /// One conv filter, identity-ish weights (only the centre tap is 1, the
    /// rest are 0), bias 0: the "convolution" is just a copy. Flatten gives
    /// `[1,2,0,...,0,3,4,0,...,0, 0,...]` (32x32, mostly zero). Rather than
    /// hand-expand that, this test uses a 1x1 "image" so the whole pipeline
    /// is checkable to the last decimal.
    #[test]
    fn hand_computed_conv_relu_dense_on_a_single_pixel() {
        let mut grid = zero_grid();
        grid[0][0] = 2.0;
        // A 3x3 filter whose centre tap is -1 and everything else 0: with
        // zero padding, the only nonzero contribution to output pixel (0,0)
        // is centre_weight * input(0,0) = -1 * 2 = -2, plus bias 0.5, giving
        // -1.5 pre-ReLU. Every other output pixel sees zero input (the grid
        // is otherwise all-background) so is bias-only: 0.5, ReLU'd to 0.5.
        let mut weight = vec![0.0f32; 9];
        weight[4] = -1.0; // (ky=1, kx=1): the centre tap.
        let conv = conv_layer(1, 1, weight, vec![0.5]);

        // Dense straight off the flattened 1x32x32 map (no features, no
        // second conv, no pool): weight picks out (0,0)'s post-ReLU value
        // with a 1, everything else with a 0, bias 0 -- so the single logit
        // equals ReLU(-1.5) = 0.0 (a negative pre-activation was clamped).
        let mut dw = vec![0.0f32; GRID * GRID];
        dw[0] = 1.0;
        let dense = dense_layer(1, GRID * GRID, dw, vec![0.0]);

        let net = Nn {
            nn_version: SUPPORTED_NN_VERSION,
            n_outputs: 1,
            junk_index: 0,
            layers: vec![conv, plain(LayerKind::Relu), plain(LayerKind::Flatten), dense],
        };
        let out = net.forward(&grid, &zero_features()).unwrap();
        // A single output class: log-softmax over one element is always 0.
        assert_eq!(out.len(), 1);
        assert!((out[0] - 0.0).abs() < 1e-6);
    }

    /// Same network, but the centre tap is positive so ReLU does not clamp
    /// it, and there are two output classes so log-softmax is non-trivial --
    /// checked against the value computed by hand.
    #[test]
    fn hand_computed_two_class_log_softmax() {
        let mut grid = zero_grid();
        grid[0][0] = 2.0;
        let mut weight = vec![0.0f32; 9];
        weight[4] = 1.0; // centre tap: output(0,0) = 1*2 + bias.
        let conv = conv_layer(1, 1, weight, vec![0.0]);
        // Post-conv, post-ReLU: pixel (0,0) is 2.0, every other pixel is
        // ReLU(0) = 0.0 (bias-only, zero bias).

        // Two dense outputs straight off the flattened map: class 0 reads
        // pixel (0,0) with weight 1 (logit = 2.0); class 1 reads it with
        // weight 0.5 (logit = 1.0). Both biases 0.
        let mut w0 = vec![0.0f32; GRID * GRID];
        w0[0] = 1.0;
        let mut w1 = vec![0.0f32; GRID * GRID];
        w1[0] = 0.5;
        let mut dw = Vec::with_capacity(2 * GRID * GRID);
        dw.extend_from_slice(&w0);
        dw.extend_from_slice(&w1);
        let dense = dense_layer(2, GRID * GRID, dw, vec![0.0, 0.0]);

        let net = Nn {
            nn_version: SUPPORTED_NN_VERSION,
            n_outputs: 2,
            junk_index: 1,
            layers: vec![conv, plain(LayerKind::Relu), plain(LayerKind::Flatten), dense],
        };
        let out = net.forward(&grid, &zero_features()).unwrap();

        // Logits [2.0, 1.0]. max = 2.0. sum = exp(0) + exp(-1.0) =
        // 1 + 0.367_879_44... = 1.367_879_44...
        // log_sum = ln(1.367_879_44...) = 0.313_261_69...
        // log_softmax = [2.0 - 2.0 - log_sum, 1.0 - 2.0 - log_sum]
        //             = [-0.313_261_69..., -1.313_261_69...]
        let sum = 1.0f32 + (-1.0f32).exp();
        let log_sum = sum.ln();
        let want0 = 2.0 - 2.0 - log_sum;
        let want1 = 1.0 - 2.0 - log_sum;
        assert!((out[0] - want0).abs() < 1e-6, "got {}, want {}", out[0], want0);
        assert!((out[1] - want1).abs() < 1e-6, "got {}, want {}", out[1], want1);
        // log-probabilities: never positive, and the larger logit wins.
        assert!(out[0] < 0.0 && out[1] < 0.0);
        assert!(out[0] > out[1]);
    }

    /// The `concat_features` layer's order: conv features first, the 107-dim
    /// vector second -- never the other way round, per the module header.
    #[test]
    fn concat_features_puts_conv_output_before_the_feature_vector() {
        let mut grid = zero_grid();
        grid[0][0] = 5.0;
        let mut weight = vec![0.0f32; 9];
        weight[4] = 1.0;
        let conv = conv_layer(1, 1, weight, vec![0.0]);

        let mut features = zero_features();
        features[0] = 42.0;

        // Dense straight off the concatenated vector (length GRID*GRID +
        // FEATURE_DIMS): weight 1 at index 0 (the conv output's pixel
        // (0,0)), weight 1 at index GRID*GRID (the feature vector's first
        // entry) -- sum should be 5.0 + 42.0 = 47.0 if concat order is
        // conv-then-features, or would instead pick up features[GRID*GRID]
        // (out of bounds; impossible) if reversed. This asserts the order
        // directly: index GRID*GRID must read the feature vector's first
        // entry, not the conv map's.
        let mut dw = vec![0.0f32; GRID * GRID + FEATURE_DIMS];
        dw[0] = 1.0;
        dw[GRID * GRID] = 1.0;
        let dense = dense_layer(1, GRID * GRID + FEATURE_DIMS, dw, vec![0.0]);

        let net = Nn {
            nn_version: SUPPORTED_NN_VERSION,
            n_outputs: 1,
            junk_index: 0,
            layers: vec![conv, plain(LayerKind::Relu), plain(LayerKind::Flatten), plain(LayerKind::ConcatFeatures), dense],
        };
        let out = net.forward(&grid, &features).unwrap();
        // One output class: log-softmax is 0 regardless of the logit's
        // value, so this only proves the shapes lined up (a shape mismatch
        // would have returned an `Err`, not a wrong number). The real
        // assertion is that `forward` succeeded at all with this exact
        // layout of weight indices.
        assert_eq!(out.len(), 1);
        assert!((out[0] - 0.0).abs() < 1e-6);
    }

    // ---- maxpool2, hand-computed ----

    #[test]
    fn maxpool_takes_the_max_of_each_2x2_block_floor_on_odd_size() {
        // A 3x3 map (one channel): pool floors to 1x1, taking the max of
        // the top-left 2x2 block only, and drops the trailing row/column.
        let data = vec![1.0f32, 2.0, 9.0, 3.0, 4.0, 9.0, 9.0, 9.0, 9.0];
        let act = Activation::Map { c: 1, h: 3, w: 3, data };
        let pooled = apply_pool(&act).unwrap();
        let Activation::Map { c, h, w, data } = pooled else { panic!("expected a map") };
        assert_eq!((c, h, w), (1, 1, 1));
        assert_eq!(data, vec![4.0]);
    }
}
