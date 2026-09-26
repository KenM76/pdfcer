//! `.ocrw` model container: format parser and dequantisation.
//!
//! # Contract
//!
//! Reads what `ocrcer-build` writes, per `ARCHITECTURE.md` section 7.
//! Little-endian throughout, table data 64-byte aligned, a CRC-32 over the
//! table blob including its interior padding.
//!
//! Two forward-compatibility rules, asymmetric on purpose:
//!
//! - **An unknown `version` is refused.** It means the tables you already
//!   know have changed meaning, and there is no safe partial read of that.
//!   `meta.feature_version` is the same guard narrowed to the feature vector
//!   and is refused the same way.
//! - **An unknown table name is skipped, silently.** A table the reader
//!   cannot name is one it does not consume, so skipping it cannot change an
//!   answer. This is what makes an additive table cost no version bump.
//!
//! Every slice index in this module is bounds-checked before use: the input
//! is a file, and a malformed one must produce an `Err`, never a panic.

use crate::feature::{FEATURE_DIMS, FEATURE_VERSION};
use crate::json::Json;
use crate::Error;

/// The container version this runtime understands. A file declaring anything
/// else is refused.
pub const SUPPORTED_VERSION: u16 = 1;

/// `model_kind` for a document recogniser: the base model.
pub const KIND_RECOGNISER: u16 = 1;
/// `model_kind` for a supplementary prototype segment (section 7.1).
pub const KIND_SEGMENT: u16 = 2;

/// How a table's bytes are to be read, per section 7's `kind` field. The
/// discriminants are the on-disk values; the writer casts this enum straight
/// into the byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    F32 = 0,
    I8 = 1,
    Opaque = 2,
}

impl Kind {
    pub fn from_u8(v: u8) -> Option<Kind> {
        match v {
            0 => Some(Kind::F32),
            1 => Some(Kind::I8),
            2 => Some(Kind::Opaque),
            _ => None,
        }
    }
}

/// One table, borrowed from the file bytes.
#[derive(Debug)]
pub struct RawTable<'a> {
    pub name: &'a str,
    pub kind: Kind,
    pub dims: Vec<u32>,
    /// Per-column dequantisation scales; empty unless `kind` is `I8`.
    pub scales: Vec<f32>,
    pub data: &'a [u8],
}

impl RawTable<'_> {
    /// The product of `dims`, which is how many elements the table holds.
    pub fn len(&self) -> usize {
        self.dims.iter().fold(1usize, |a, &d| a.saturating_mul(d as usize))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// An `f32` table's values.
    pub fn f32s(&self) -> Result<Vec<f32>, Error> {
        if self.kind != Kind::F32 {
            return Err(Error::BadTable { name: self.name.into(), why: "expected an f32 table" });
        }
        if self.data.len() != self.len() * 4 {
            return Err(Error::BadTable { name: self.name.into(), why: "f32 data length disagrees with dims" });
        }
        Ok(self
            .data
            .chunks_exact(4)
            .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
            .collect())
    }

    /// An `i8` matrix dequantised to `f32`, row-major, one scale per column.
    ///
    /// Int8 is a storage format only (section 7): this runs once at load and
    /// no kernel downstream ever sees an integer.
    pub fn dequantised(&self) -> Result<Vec<f32>, Error> {
        if self.kind != Kind::I8 {
            return Err(Error::BadTable { name: self.name.into(), why: "expected an i8 table" });
        }
        if self.dims.len() != 2 {
            return Err(Error::BadTable { name: self.name.into(), why: "an i8 table must be two-dimensional" });
        }
        let cols = self.dims[1] as usize;
        if self.scales.len() != cols {
            return Err(Error::BadTable { name: self.name.into(), why: "one scale per column is required" });
        }
        if self.data.len() != self.len() {
            return Err(Error::BadTable { name: self.name.into(), why: "i8 data length disagrees with dims" });
        }
        let mut out = Vec::with_capacity(self.data.len());
        for row in self.data.chunks_exact(cols) {
            for (c, &b) in row.iter().enumerate() {
                out.push(f32::from(b as i8) * self.scales[c]);
            }
        }
        Ok(out)
    }

    /// An opaque table read as little-endian `u16`, the encoding `meta`
    /// declares for `prototype_class`.
    pub fn u16s(&self) -> Result<Vec<u16>, Error> {
        if self.data.len() != self.len() * 2 {
            return Err(Error::BadTable { name: self.name.into(), why: "u16 data length disagrees with dims" });
        }
        Ok(self.data.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
    }
}

/// A parsed `.ocrw` file: header, `meta`, and the table directory.
///
/// Borrows the file bytes; nothing is copied until a table is asked for.
pub struct Container<'a> {
    pub version: u16,
    pub model_kind: u16,
    /// The `meta` block as written, kept so a digest over it can be taken
    /// without re-serialising.
    pub meta_text: &'a str,
    pub meta: Json,
    pub tables: Vec<RawTable<'a>>,
}

/// Reads little-endian scalars with a bounds check on every access.
struct Cursor<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Cursor<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.at.checked_add(n).ok_or(Error::Truncated)?;
        let s = self.b.get(self.at..end).ok_or(Error::Truncated)?;
        self.at = end;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16, Error> {
        let s = self.take(2)?;
        Ok(u16::from_le_bytes([s[0], s[1]]))
    }
    fn u32(&mut self) -> Result<u32, Error> {
        let s = self.take(4)?;
        Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn u64(&mut self) -> Result<u64, Error> {
        let s = self.take(8)?;
        Ok(u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]))
    }
}

fn slice_at(bytes: &[u8], off: u64, len: u64) -> Result<&[u8], Error> {
    let off = usize::try_from(off).map_err(|_| Error::Truncated)?;
    let len = usize::try_from(len).map_err(|_| Error::Truncated)?;
    let end = off.checked_add(len).ok_or(Error::Truncated)?;
    bytes.get(off..end).ok_or(Error::Truncated)
}

impl<'a> Container<'a> {
    /// Parses a `.ocrw` file and verifies its blob CRC.
    pub fn load(bytes: &'a [u8]) -> Result<Container<'a>, Error> {
        let mut c = Cursor { b: bytes, at: 0 };
        if c.take(4)? != b"OCRW" {
            return Err(Error::BadMagic);
        }
        let version = c.u16()?;
        if version != SUPPORTED_VERSION {
            return Err(Error::UnsupportedVersion(version));
        }
        let model_kind = c.u16()?;
        let n_tables = c.u32()? as usize;
        let meta_len = c.u32()? as usize;
        let meta_bytes = c.take(meta_len)?;
        let meta_text = core::str::from_utf8(meta_bytes).map_err(|_| Error::MetaNotUtf8)?;
        let blob_crc = c.u32()?;
        let _reserved = c.take(8)?;

        let mut tables = Vec::with_capacity(n_tables.min(1024));
        let mut blob_lo = usize::MAX;
        let mut blob_hi = 0usize;
        for _ in 0..n_tables {
            let name_len = c.u16()? as usize;
            let name = core::str::from_utf8(c.take(name_len)?).map_err(|_| Error::TableNameNotUtf8)?;
            let kind = Kind::from_u8(c.u8()?)
                .ok_or_else(|| Error::BadTable { name: name.into(), why: "unknown table kind" })?;
            let ndim = c.u8()? as usize;
            let mut dims = Vec::with_capacity(ndim);
            for _ in 0..ndim {
                dims.push(c.u32()?);
            }
            let n_scales = c.u32()? as usize;
            let scale_off = c.u64()?;
            let data_off = c.u64()?;
            let data_len = c.u64()?;

            let scale_bytes = slice_at(bytes, scale_off, (n_scales as u64) * 4)?;
            let scales = scale_bytes
                .chunks_exact(4)
                .map(|s| f32::from_le_bytes([s[0], s[1], s[2], s[3]]))
                .collect();
            if data_off % 64 != 0 {
                return Err(Error::BadTable { name: name.into(), why: "table data is not 64-byte aligned" });
            }
            let data = slice_at(bytes, data_off, data_len)?;
            let lo = data_off as usize;
            blob_lo = blob_lo.min(lo);
            blob_hi = blob_hi.max(lo + data.len());

            tables.push(RawTable { name, kind, dims, scales, data });
        }

        // The CRC covers the blob including the padding between tables, so a
        // file that passes is byte-for-byte the file that was written.
        let computed = if blob_lo == usize::MAX {
            crc32(&[])
        } else {
            crc32(bytes.get(blob_lo..blob_hi).ok_or(Error::Truncated)?)
        };
        if computed != blob_crc {
            return Err(Error::CrcMismatch { stored: blob_crc, computed });
        }

        let meta = Json::parse(meta_text).map_err(Error::Meta)?;
        Ok(Container { version, model_kind, meta_text, meta, tables })
    }

    /// The table with this name, or `None`. A caller that does not name a
    /// table never sees it, which is the skip-unknown-names rule.
    pub fn table(&self, name: &str) -> Option<&RawTable<'a>> {
        self.tables.iter().find(|t| t.name == name)
    }

    fn need(&self, name: &'static str) -> Result<&RawTable<'a>, Error> {
        self.table(name).ok_or(Error::MissingTable(name))
    }
}

/// One character class, as `meta.charset` describes it.
#[derive(Debug, Clone)]
pub struct Class {
    pub index: u16,
    pub codepoint: char,
    pub category: String,
    /// The class index of the other case of this letter, when there is one.
    pub case_twin: Option<u16>,
}

/// One source face, as `meta.faces` describes it.
#[derive(Debug, Clone)]
pub struct Face {
    pub family: String,
    pub style: String,
    pub distribution: String,
    /// Present once the writer emits it (section 7.1); `None` in files built
    /// before that.
    pub licence: Option<String>,
    pub licence_source: Option<String>,
}

/// A loaded recogniser: the prototype bank, its class labels, the
/// normalisation constants, and the charset that names the class indices.
pub struct Model {
    pub build_id: String,
    pub feature_version: u32,
    pub classes: Vec<Class>,
    pub faces: Vec<Face>,
    /// px/em sizes the bank was rendered at, carried so a rebuild can be
    /// reproduced from the file. Optional and additive (`meta.sizes`,
    /// ARCHITECTURE.md section 11, 2026-09-23): empty means the file predates
    /// the field, not that the bank was built at no size.
    pub sizes: Vec<f32>,
    /// `n_prototypes * FEATURE_DIMS`, row-major, standardised and
    /// dequantised.
    pub prototypes: Vec<f32>,
    /// One class index per prototype row.
    pub prototype_class: Vec<u16>,
    /// One flag per prototype row: whether its face's style names it Italic
    /// or Oblique (`ARCHITECTURE.md` section 11, 2026-09-24 decision,
    /// "match-time gating by face style"). Derived at load from the optional
    /// `prototype_face` table plus `faces[].style`, never from a second table
    /// of booleans — a style rename in `meta.faces` and this flag must never
    /// be able to disagree. Empty when the file carries no `prototype_face`
    /// table, which a caller reads the same way as "no prototype is italic":
    /// a file built before this decision has no italic prototypes to gate.
    pub prototype_italic: Vec<bool>,
    /// Per-dimension mean used to standardise a query before matching.
    pub mean: [f32; FEATURE_DIMS],
    /// Per-dimension standard deviation. Never zero: the builder substitutes
    /// `1.0` for a constant dimension.
    pub sd: [f32; FEATURE_DIMS],
    /// One bitmask per class; bit *n* means a prototype of this class was
    /// measured with *n* holes.
    pub class_holes: Vec<u8>,
    /// Per-dimension multiplier applied to the squared term in the matcher's
    /// distance.
    ///
    /// All `1.0` when the file carries no `feature_weights` table. The table
    /// is optional rather than required so that authoring weights costs no
    /// format version bump (section 7): a reader that does not know the name
    /// skips it and gets the unweighted distance, which is what it would
    /// have computed anyway.
    pub weights: [f32; FEATURE_DIMS],
    /// What each class is, for the decoder's context tests. Derived from the
    /// charset at load so the decoder never converts a class to a `char` in
    /// its inner loop.
    pub class_info: Vec<crate::decode::viterbi::ClassInfo>,
    /// Thresholds, starting from the authored defaults and overridden by the
    /// file's `params` table when it carries one.
    pub params: crate::params::Params,
    /// The word graph, when the file carries one. `None` means the decoder
    /// runs without a lexicon term, which is a weaker reading and never a
    /// wrong one (`CLAUDE.md` rule 6).
    pub lexicon: Option<crate::decode::lexicon::Lexicon>,
    /// Character-pair log-probabilities, when the file carries them.
    pub bigrams: Option<crate::decode::bigram::Bigrams>,
    /// Context priors for look-alike pairs, when the file carries them.
    pub confusions: Option<crate::decode::confusion::Confusions>,
    /// The optional neural classifier, dequantised, when the file carries
    /// one this build's `nn_version` recognises. `None` for every file
    /// written before chunk 15, and for one whose `nn` table this build
    /// cannot or will not read -- see `nn_status` for which. `Engine`
    /// (`crate::pipeline`) falls back to prototype scoring when
    /// `match.classifier == 1` and this is `None`, and reports why via
    /// `Engine::classifier_fallback`, the same shape of fallback an
    /// unreadable `nn` table itself uses.
    pub nn: Option<crate::nn::Nn>,
    /// Why `nn` is `Some` or `None`. Reading this is how a caller (or
    /// `ocrcer-build inspect`) reports the reason without the load itself
    /// ever failing over it.
    pub nn_status: crate::nn::NnStatus,
}

impl Model {
    pub fn n_prototypes(&self) -> usize {
        self.prototype_class.len()
    }

    /// Loads a base recogniser from `.ocrw` bytes.
    ///
    /// Refuses a `model_kind` that is not a recogniser and a
    /// `meta.feature_version` that is not this build's: a runtime reading a
    /// mismatched file does not fail, it answers wrongly, which is the
    /// failure the field exists to make impossible.
    pub fn load(bytes: &[u8]) -> Result<Model, Error> {
        let c = Container::load(bytes)?;
        if c.model_kind != KIND_RECOGNISER {
            return Err(Error::UnsupportedModelKind(c.model_kind));
        }
        let feature_version = meta_u32(&c.meta, "feature_version")?;
        if feature_version != FEATURE_VERSION {
            return Err(Error::FeatureVersionMismatch { file: feature_version, runtime: FEATURE_VERSION });
        }
        let dims = meta_u32(&c.meta, "feature_dims")? as usize;
        if dims != FEATURE_DIMS {
            return Err(Error::FeatureDimsMismatch { file: dims, runtime: FEATURE_DIMS });
        }

        let protos = c.need(T_PROTOTYPES)?;
        if protos.dims.len() != 2 || protos.dims[1] as usize != FEATURE_DIMS {
            return Err(Error::BadTable { name: T_PROTOTYPES.into(), why: "prototype rows must be FEATURE_DIMS wide" });
        }
        let prototypes = protos.dequantised()?;
        let n = protos.dims[0] as usize;

        let prototype_class = c.need(T_PROTOTYPE_CLASS)?.u16s()?;
        if prototype_class.len() != n {
            return Err(Error::BadTable {
                name: T_PROTOTYPE_CLASS.into(),
                why: "one class per prototype row is required",
            });
        }

        let norm = c.need(T_FEATURE_NORM)?.f32s()?;
        if norm.len() != 2 * FEATURE_DIMS {
            return Err(Error::BadTable { name: T_FEATURE_NORM.into(), why: "expected mean then sd" });
        }
        let mut mean = [0.0f32; FEATURE_DIMS];
        let mut sd = [1.0f32; FEATURE_DIMS];
        mean.copy_from_slice(&norm[..FEATURE_DIMS]);
        for (i, s) in norm[FEATURE_DIMS..].iter().enumerate() {
            // A zero here would divide by zero on every query. The builder
            // substitutes 1.0 for a constant dimension; this is the belt to
            // that brace, because the file may not be this build's.
            sd[i] = if *s > 0.0 { *s } else { 1.0 };
        }

        let class_holes = c.need(T_CLASS_HOLES)?.data.to_vec();

        let mut weights = [1.0f32; FEATURE_DIMS];
        if let Some(t) = c.table(T_FEATURE_WEIGHTS) {
            let w = t.f32s()?;
            if w.len() != FEATURE_DIMS {
                return Err(Error::BadTable {
                    name: T_FEATURE_WEIGHTS.into(),
                    why: "one weight per feature dimension is required",
                });
            }
            for (i, v) in w.iter().enumerate() {
                if !(*v >= 0.0) || !v.is_finite() {
                    return Err(Error::BadTable {
                        name: T_FEATURE_WEIGHTS.into(),
                        why: "weights must be finite and non-negative",
                    });
                }
                weights[i] = *v;
            }
        }
        let faces = parse_faces(&c.meta);

        // Optional and additive: a `prototype_face` table names each row's
        // face index, and italic-ness is derived from that face's own style
        // string rather than stored twice. Absent means every prototype
        // predates the decision that would need this, so none is italic.
        let mut prototype_italic: Vec<bool> = Vec::new();
        if let Some(t) = c.table(T_PROTOTYPE_FACE) {
            let idx = t.u16s()?;
            if idx.len() != n {
                return Err(Error::BadTable {
                    name: T_PROTOTYPE_FACE.into(),
                    why: "one face index per prototype row is required",
                });
            }
            prototype_italic = idx
                .iter()
                .map(|&f| faces.get(f as usize).is_some_and(|face| is_italic_style(&face.style)))
                .collect();
        }

        let classes = parse_charset(&c.meta)?;
        let class_info: Vec<crate::decode::viterbi::ClassInfo> =
            classes.iter().map(|k| crate::decode::viterbi::ClassInfo::of(k.codepoint)).collect();

        // The four authored tables below are all optional and all additive.
        // A file written before any of them existed loads and runs; what it
        // loses is an argument the decoder could have made, never a
        // correctness guarantee (section 7's rule that an unknown table is
        // skipped, applied from the other side).
        let mut params = crate::params::Params::DEFAULT;
        if let Some(t) = c.table(T_PARAMS) {
            if params.apply(t.data).is_none() {
                return Err(Error::BadTable {
                    name: T_PARAMS.into(),
                    why: "the parameter block is malformed, or names a parameter this build does not have — either way it was not applied, and reading by the defaults it meant to override would answer by a rule the file does not describe",
                });
            }
        }
        // `match.classifier == 2` (fused prototype + network scoring) is a
        // hard load-time refusal, not a fallback: the fusion rule is
        // undecided (`ARCHITECTURE.md` §11, "Chunk 15 interfaces", item 5),
        // so there is no reading of this file that reflects what the
        // parameter asked for. `0` and `1` both load; `1` degrades to
        // prototype-only scoring when no network is present, which is a
        // reportable fact, not a format error.
        if params.matching.classifier == 2 {
            return Err(Error::UnsupportedClassifier(2));
        }

        let lexicon = match c.table(T_LEXICON) {
            None => None,
            Some(t) => {
                let mut lex = crate::decode::lexicon::Lexicon::parse(t.data, classes.len())
                    .map_err(|e| Error::BadTable {
                        name: T_LEXICON.into(),
                        why: lexicon_why(e),
                    })?;
                // The graph is keyed on case-folded class indices, and the
                // charset is the only thing that knows which classes pair.
                // Handing the map over here is what lets the decoder walk the
                // graph without ever converting a class to a character.
                lex.set_fold(fold_map(&classes));
                Some(lex)
            }
        };

        let bigrams = match c.table(T_BIGRAMS) {
            None => None,
            Some(t) => Some(
                crate::decode::bigram::Bigrams::parse(t.data, classes.len()).map_err(|_| {
                    Error::BadTable { name: T_BIGRAMS.into(), why: "malformed bigram table" }
                })?,
            ),
        };

        let confusions = match c.table(T_CONFUSIONS) {
            None => None,
            Some(t) => Some(crate::decode::confusion::Confusions::parse(t.data).map_err(|_| {
                Error::BadTable {
                    name: T_CONFUSIONS.into(),
                    why: "malformed or mismatched confusion table",
                }
            })?),
        };

        // Additive and never load-critical (`ARCHITECTURE.md` section 11,
        // 2026-09-25 chunk 15 interfaces): an unknown `nn_version` or a
        // malformed `nn` table degrades to no network, not a load failure.
        let (nn, nn_status) = crate::nn::load(&c);

        if let Some(&worst) = prototype_class.iter().max() {
            if worst as usize >= classes.len() {
                return Err(Error::BadTable {
                    name: T_PROTOTYPE_CLASS.into(),
                    why: "a prototype names a class the charset does not have",
                });
            }
        }

        Ok(Model {
            build_id: c.meta.get("build_id").and_then(Json::as_str).unwrap_or("").to_string(),
            feature_version,
            classes,
            faces,
            sizes: c
                .meta
                .get("sizes")
                .and_then(Json::as_array)
                .map(|a| a.iter().filter_map(Json::as_f64).map(|v| v as f32).collect())
                .unwrap_or_default(),
            prototypes,
            prototype_class,
            prototype_italic,
            mean,
            sd,
            class_holes,
            weights,
            class_info,
            params,
            lexicon,
            bigrams,
            confusions,
            nn,
            nn_status,
        })
    }

    /// Standardises a raw feature vector with the file's own constants. A
    /// query measured with different constants than the bank is the silent
    /// wrong-answer mode section 7 exists to prevent, so the constants come
    /// from the file and never from a compiled-in table.
    pub fn standardise(&self, raw: &[f32; FEATURE_DIMS]) -> [f32; FEATURE_DIMS] {
        let mut out = [0.0f32; FEATURE_DIMS];
        for i in 0..FEATURE_DIMS {
            out[i] = (raw[i] - self.mean[i]) / self.sd[i];
        }
        out
    }

    /// The character a class index names.
    pub fn char_of(&self, class: u16) -> Option<char> {
        self.classes.get(class as usize).map(|c| c.codepoint)
    }
}

/// Table names, matching what `ocrcer-build`'s `emit` writes.
pub const T_PROTOTYPES: &str = "prototypes";
pub const T_PROTOTYPE_CLASS: &str = "prototype_class";
pub const T_FEATURE_NORM: &str = "feature_norm";
pub const T_CLASS_HOLES: &str = "class_holes";
/// Optional: per-dimension matcher weights. Absent from every file written so
/// far; a runtime that does not find it uses `1.0` throughout.
pub const T_FEATURE_WEIGHTS: &str = "feature_weights";
/// Optional: the word graph the lexicon bonus is read from.
pub const T_LEXICON: &str = "lexicon";
/// Optional: character-pair log-probabilities with a category backoff.
pub const T_BIGRAMS: &str = "bigrams";
/// Optional: context priors for the pairs the matcher cannot separate.
pub const T_CONFUSIONS: &str = "confusions";
/// Optional: the parameter block. Absent means the authored defaults.
pub const T_PARAMS: &str = "params";
/// Optional: one face index per prototype row, into `meta.faces`. Absent from
/// every file written before the 2026-09-24 italic-gating decision; a reader
/// that does not find it treats every prototype as non-italic, which is the
/// only fact such a file's prototypes can attest to (`ARCHITECTURE.md`
/// section 11).
pub const T_PROTOTYPE_FACE: &str = "prototype_face";

/// The case-fold map the lexicon graph is keyed on: every uppercase class
/// maps to its lowercase twin and everything else to itself.
fn fold_map(classes: &[Class]) -> Vec<u16> {
    classes
        .iter()
        .map(|k| match k.case_twin {
            Some(t) if k.codepoint.is_uppercase() => t,
            _ => k.index,
        })
        .collect()
}

fn lexicon_why(e: crate::decode::lexicon::Malformed) -> &'static str {
    use crate::decode::lexicon::Malformed as M;
    match e {
        M::Magic => "not a lexicon table",
        M::Version => "lexicon layout version this build does not read",
        M::Truncated => "lexicon table is truncated",
        M::NodeRange => "a lexicon node's edge range is out of bounds",
        M::EdgeTarget => "a lexicon edge points outside the graph",
        M::EdgeOrder => "lexicon edges are not in ascending symbol order",
    }
}

fn meta_u32(meta: &Json, key: &'static str) -> Result<u32, Error> {
    meta.get(key).and_then(Json::as_u32).ok_or(Error::MetaMissing(key))
}

fn parse_charset(meta: &Json) -> Result<Vec<Class>, Error> {
    let arr = meta.get("charset").and_then(Json::as_array).ok_or(Error::MetaMissing("charset"))?;
    let mut out = Vec::with_capacity(arr.len());
    for (i, e) in arr.iter().enumerate() {
        let index = e.get("index").and_then(Json::as_u32).ok_or(Error::MetaMissing("charset[].index"))?;
        if index as usize != i {
            return Err(Error::CharsetOutOfOrder(index));
        }
        let cp = e.get("cp").and_then(Json::as_u32).ok_or(Error::MetaMissing("charset[].cp"))?;
        let codepoint = char::from_u32(cp).ok_or(Error::MetaMissing("charset[].cp"))?;
        let twin = e.get("twin").and_then(Json::as_i64).unwrap_or(-1);
        out.push(Class {
            index: index as u16,
            codepoint,
            category: e.get("category").and_then(Json::as_str).unwrap_or("").to_string(),
            case_twin: if (0..=i64::from(u16::MAX)).contains(&twin) { Some(twin as u16) } else { None },
        });
    }
    Ok(out)
}

/// Whether a face's style string names it italic or oblique. One place this
/// test is written, so match-time gating and any future report of "which
/// faces are italic" read the same rule off the same field.
pub fn is_italic_style(style: &str) -> bool {
    let s = style.to_ascii_lowercase();
    s.contains("italic") || s.contains("oblique")
}

fn parse_faces(meta: &Json) -> Vec<Face> {
    let Some(arr) = meta.get("faces").and_then(Json::as_array) else {
        return Vec::new();
    };
    arr.iter()
        .map(|f| Face {
            family: f.get("family").and_then(Json::as_str).unwrap_or("").to_string(),
            style: f.get("style").and_then(Json::as_str).unwrap_or("").to_string(),
            distribution: f.get("distribution").and_then(Json::as_str).unwrap_or("").to_string(),
            licence: f.get("licence").and_then(Json::as_str).map(str::to_string),
            licence_source: f.get("licence_source").and_then(Json::as_str).map(str::to_string),
        })
        .collect()
}

/// CRC-32 (IEEE 802.3, reflected, initial and final `0xFFFF_FFFF`), computed
/// bitwise.
///
/// Lives here rather than in the writer because `ocrcer-build` depends on
/// this crate and not the other way round, and two implementations of a
/// checksum that must agree forever is exactly the duplication `CLAUDE.md`
/// rule 4 forbids.
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The check value every CRC-32 implementation agrees on. Getting this
    /// wrong would make every written file fail its own load check.
    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    /// Builds a minimal but valid container by hand, so the reader is tested
    /// against the format rather than against the writer's habits.
    fn minimal(version: u16, kind: u16, meta: &str, extra_table: bool) -> Vec<u8> {
        let mut dir: Vec<u8> = Vec::new();
        let mut names: Vec<(&str, u8, Vec<u32>, usize, Vec<u8>)> = vec![
            ("known", 2, vec![4], 0, vec![1, 2, 3, 4]),
        ];
        if extra_table {
            names.push(("a_table_from_the_future", 2, vec![3], 0, vec![9, 9, 9]));
        }
        let header_len = 4 + 2 + 2 + 4 + 4 + meta.len() + 4 + 8;
        let dir_len: usize = names
            .iter()
            .map(|(n, _, d, _, _)| 2 + n.len() + 1 + 1 + 4 * d.len() + 4 + 8 + 8 + 8)
            .sum();
        let scales_off = header_len + dir_len;
        let blob_start = scales_off.div_ceil(64) * 64;
        let mut blob: Vec<u8> = Vec::new();
        let mut offs = Vec::new();
        for (_, _, _, _, data) in &names {
            let at = (blob_start + blob.len()).div_ceil(64) * 64;
            blob.resize(at - blob_start, 0);
            offs.push(at);
            blob.extend_from_slice(data);
        }
        for (i, (n, k, d, ns, data)) in names.iter().enumerate() {
            dir.extend_from_slice(&(n.len() as u16).to_le_bytes());
            dir.extend_from_slice(n.as_bytes());
            dir.push(*k);
            dir.push(d.len() as u8);
            for v in d {
                dir.extend_from_slice(&v.to_le_bytes());
            }
            dir.extend_from_slice(&(*ns as u32).to_le_bytes());
            dir.extend_from_slice(&(scales_off as u64).to_le_bytes());
            dir.extend_from_slice(&(offs[i] as u64).to_le_bytes());
            dir.extend_from_slice(&(data.len() as u64).to_le_bytes());
        }
        let mut out = Vec::new();
        out.extend_from_slice(b"OCRW");
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&kind.to_le_bytes());
        out.extend_from_slice(&(names.len() as u32).to_le_bytes());
        out.extend_from_slice(&(meta.len() as u32).to_le_bytes());
        out.extend_from_slice(meta.as_bytes());
        out.extend_from_slice(&crc32(&blob).to_le_bytes());
        out.extend_from_slice(&[0u8; 8]);
        out.extend_from_slice(&dir);
        out.resize(blob_start, 0);
        out.extend_from_slice(&blob);
        out
    }

    #[test]
    fn a_hand_built_container_parses() {
        let bytes = minimal(1, 1, "{\"a\":1}", false);
        let c = Container::load(&bytes).unwrap();
        assert_eq!(c.version, 1);
        assert_eq!(c.model_kind, 1);
        assert_eq!(c.meta.get("a").unwrap().as_u32(), Some(1));
        assert_eq!(c.table("known").unwrap().data, &[1, 2, 3, 4]);
    }

    /// Section 7's asymmetry, both halves, in one test.
    #[test]
    fn an_unknown_version_is_refused_and_an_unknown_table_is_skipped() {
        let future = minimal(2, 1, "{}", false);
        assert!(matches!(Container::load(&future), Err(Error::UnsupportedVersion(2))));

        let extra = minimal(1, 1, "{}", true);
        let c = Container::load(&extra).unwrap();
        assert!(c.table("known").is_some());
        // Present in the directory, never consumed, and no error.
        assert!(c.table("a_table_from_the_future").is_some());
        assert!(c.table("neither").is_none());
    }

    #[test]
    fn a_corrupt_blob_is_caught_by_the_crc() {
        let mut bytes = minimal(1, 1, "{}", false);
        let n = bytes.len();
        bytes[n - 1] ^= 0xFF;
        assert!(matches!(Container::load(&bytes), Err(Error::CrcMismatch { .. })));
    }

    /// A truncated or scrambled file must return an error from every path,
    /// never panic: the input is a file and files arrive damaged.
    #[test]
    fn damaged_files_error_rather_than_panic() {
        let good = minimal(1, 1, "{\"a\":1}", true);
        for cut in 0..good.len() {
            let _ = Container::load(&good[..cut]);
        }
        for i in (0..good.len()).step_by(7) {
            let mut b = good.clone();
            b[i] ^= 0xA5;
            let _ = Container::load(&b);
        }
        assert!(matches!(Container::load(b"NOPE"), Err(Error::BadMagic)));
        assert!(matches!(Container::load(b""), Err(Error::Truncated)));
    }

    /// `match.classifier == 2` is undecided (fusion), so a model asking for
    /// it is refused at load rather than silently falling back
    /// (`ARCHITECTURE.md` §11, "Chunk 15 interfaces", item 5).
    #[test]
    fn classifier_two_is_refused_at_load() {
        let mut params_bytes: Vec<u8> = Vec::new();
        params_bytes.extend_from_slice(b"PARM");
        params_bytes.extend_from_slice(&1u16.to_le_bytes());
        params_bytes.extend_from_slice(&0u16.to_le_bytes());
        params_bytes.extend_from_slice(&1u32.to_le_bytes());
        let name = "match.classifier";
        params_bytes.push(name.len() as u8);
        params_bytes.push(1u8); // u32 tag
        params_bytes.extend_from_slice(name.as_bytes());
        params_bytes.extend_from_slice(&2u32.to_le_bytes());

        let bytes = model_with_params_table(&params_bytes);
        assert!(matches!(Model::load(&bytes), Err(Error::UnsupportedClassifier(2))));
    }

    /// Builds a minimal loadable recogniser model, with the given `params`
    /// table bytes, so `Model::load`'s classifier gate can be tested without
    /// a real prototype bank. Kept file-local: this is scaffolding for one
    /// test, not a second builder to keep in sync with `ocrcer-build`.
    fn model_with_params_table(params_bytes: &[u8]) -> Vec<u8> {
        let meta = format!(
            "{{\"feature_version\":{},\"feature_dims\":{},\"charset\":[{{\"index\":0,\"cp\":65,\"category\":\"letter\"}}]}}",
            FEATURE_VERSION, FEATURE_DIMS
        );
        let proto_data: Vec<u8> = {
            let mut v = Vec::new();
            for _ in 0..FEATURE_DIMS {
                v.extend_from_slice(&0i8.to_le_bytes());
            }
            v
        };
        let class_data: Vec<u8> = 0u16.to_le_bytes().to_vec();
        let norm_data: Vec<u8> = {
            let mut v = Vec::new();
            for _ in 0..FEATURE_DIMS {
                v.extend_from_slice(&0.0f32.to_le_bytes());
            }
            for _ in 0..FEATURE_DIMS {
                v.extend_from_slice(&1.0f32.to_le_bytes());
            }
            v
        };
        let holes_data: Vec<u8> = vec![0u8];

        let names: Vec<(&str, u8, Vec<u32>, Vec<f32>, Vec<u8>)> = vec![
            (T_PROTOTYPES, 1, vec![1, FEATURE_DIMS as u32], vec![1.0; FEATURE_DIMS], proto_data),
            (T_PROTOTYPE_CLASS, 2, vec![1], vec![], class_data),
            (T_FEATURE_NORM, 0, vec![2 * FEATURE_DIMS as u32], vec![], norm_data),
            (T_CLASS_HOLES, 2, vec![1], vec![], holes_data),
            (T_PARAMS, 2, vec![params_bytes.len() as u32], vec![], params_bytes.to_vec()),
        ];

        let header_len = 4 + 2 + 2 + 4 + 4 + meta.len() + 4 + 8;
        let dir_len: usize = names
            .iter()
            .map(|(n, _, d, _, _)| 2 + n.len() + 1 + 1 + 4 * d.len() + 4 + 8 + 8 + 8)
            .sum();
        // Scales are stored contiguously ahead of the blob, one region per
        // table that has any; simplest correct layout is to concatenate all
        // scale arrays and record each table's own offset into it.
        let mut scales_blob: Vec<u8> = Vec::new();
        let mut scale_offs = Vec::new();
        for (_, _, _, s, _) in &names {
            scale_offs.push(scales_blob.len());
            for v in s {
                scales_blob.extend_from_slice(&v.to_le_bytes());
            }
        }
        let scales_start = header_len + dir_len;
        let blob_start = (scales_start + scales_blob.len()).div_ceil(64) * 64;

        let mut blob: Vec<u8> = Vec::new();
        let mut data_offs = Vec::new();
        for (_, _, _, _, data) in &names {
            let at = (blob_start + blob.len()).div_ceil(64) * 64;
            blob.resize(at - blob_start, 0);
            data_offs.push(at);
            blob.extend_from_slice(data);
        }

        let mut dir: Vec<u8> = Vec::new();
        for (i, (n, k, d, s, data)) in names.iter().enumerate() {
            dir.extend_from_slice(&(n.len() as u16).to_le_bytes());
            dir.extend_from_slice(n.as_bytes());
            dir.push(*k);
            dir.push(d.len() as u8);
            for v in d {
                dir.extend_from_slice(&v.to_le_bytes());
            }
            dir.extend_from_slice(&(s.len() as u32).to_le_bytes());
            dir.extend_from_slice(&((scales_start + scale_offs[i]) as u64).to_le_bytes());
            dir.extend_from_slice(&(data_offs[i] as u64).to_le_bytes());
            dir.extend_from_slice(&(data.len() as u64).to_le_bytes());
        }

        let mut out = Vec::new();
        out.extend_from_slice(b"OCRW");
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&KIND_RECOGNISER.to_le_bytes());
        out.extend_from_slice(&(names.len() as u32).to_le_bytes());
        out.extend_from_slice(&(meta.len() as u32).to_le_bytes());
        out.extend_from_slice(meta.as_bytes());
        out.extend_from_slice(&crc32(&blob).to_le_bytes());
        out.extend_from_slice(&[0u8; 8]);
        out.extend_from_slice(&dir);
        out.resize(scales_start, 0);
        out.extend_from_slice(&scales_blob);
        out.resize(blob_start, 0);
        out.extend_from_slice(&blob);
        out
    }
}
