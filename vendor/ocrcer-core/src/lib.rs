//! OCRcer engine: segmentation-driven prototype-matching OCR with a lattice
//! decoder. See `docs/ARCHITECTURE.md` for the pipeline and the `.ocrw`
//! model format this crate reads.

#![forbid(unsafe_code)]

pub mod confidence;
pub mod decode;
pub mod feature;
pub mod image;
pub mod json;
pub mod layout;
pub mod r#match;
pub mod nn;
pub mod ocrw;
pub mod params;
pub mod pipeline;
pub mod prof;

pub use image::Gray;
pub use pipeline::{CharBox, Engine, Line, Rect, Word};

/// Errors the engine can return from model loading and recognition.
///
/// Loading a model file is the only fallible thing the engine does that a
/// caller can act on, so the variants say *which* guard refused rather than
/// carrying one opaque message: a version mismatch and a corrupt blob want
/// different responses.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// This code path is not implemented yet (scaffold stage).
    NotImplemented,
    /// The file does not start with `OCRW`.
    BadMagic,
    /// The file ends inside a structure the header promised.
    Truncated,
    /// A container version this runtime does not know. Refused rather than
    /// partially read: the version means the tables have changed meaning.
    UnsupportedVersion(u16),
    /// A `model_kind` this entry point does not load (a supplementary
    /// segment handed to the base-model loader, for instance).
    UnsupportedModelKind(u16),
    /// The file was built against a different feature-vector definition.
    /// Every prototype in it means something else.
    FeatureVersionMismatch { file: u32, runtime: u32 },
    /// The file's vectors are a different width than this runtime's.
    FeatureDimsMismatch { file: usize, runtime: usize },
    /// The `meta` block is not UTF-8.
    MetaNotUtf8,
    /// A table name in the directory is not UTF-8.
    TableNameNotUtf8,
    /// The `meta` block is not valid JSON.
    Meta(json::JsonError),
    /// A key the loader requires is absent from `meta`.
    MetaMissing(&'static str),
    /// `meta.charset` is not indexed `0..n` in order, so a class index in a
    /// table cannot be trusted to name the entry at that position.
    CharsetOutOfOrder(u32),
    /// The stored CRC-32 does not match the table blob.
    CrcMismatch { stored: u32, computed: u32 },
    /// A table the loader requires is absent.
    MissingTable(&'static str),
    /// A table is present but malformed.
    BadTable { name: String, why: &'static str },
    /// `match.classifier` names a value this runtime does not implement.
    /// Only `2` (fused prototype + network scoring) currently does this:
    /// the fusion rule is undecided (`ARCHITECTURE.md` §11), so loading a
    /// model that asks for it is refused outright rather than silently
    /// falling back, the way an unloadable `nn` table does.
    UnsupportedClassifier(u32),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::NotImplemented => write!(f, "not implemented"),
            Error::BadMagic => write!(f, "not an .ocrw file (bad magic)"),
            Error::Truncated => write!(f, "file ends inside a structure its header promised"),
            Error::UnsupportedVersion(v) => {
                write!(f, "model file version {v} is not supported by this runtime")
            }
            Error::UnsupportedModelKind(k) => write!(f, "unsupported model_kind {k}"),
            Error::FeatureVersionMismatch { file, runtime } => write!(
                f,
                "model built against feature version {file}, runtime extracts version {runtime}"
            ),
            Error::FeatureDimsMismatch { file, runtime } => {
                write!(f, "model has {file}-dimensional vectors, runtime extracts {runtime}")
            }
            Error::MetaNotUtf8 => write!(f, "meta block is not UTF-8"),
            Error::TableNameNotUtf8 => write!(f, "a table name is not UTF-8"),
            Error::Meta(e) => write!(f, "meta block is not valid JSON: {e}"),
            Error::MetaMissing(k) => write!(f, "meta block is missing {k:?}"),
            Error::CharsetOutOfOrder(i) => {
                write!(f, "meta.charset is out of order at index {i}")
            }
            Error::CrcMismatch { stored, computed } => {
                write!(f, "table blob CRC {computed:08x} does not match the stored {stored:08x}")
            }
            Error::MissingTable(n) => write!(f, "model file has no {n:?} table"),
            Error::BadTable { name, why } => write!(f, "table {name:?}: {why}"),
            Error::UnsupportedClassifier(v) => write!(
                f,
                "match.classifier = {v} is not implemented (fusion rule is undecided, ARCHITECTURE.md §11)"
            ),
        }
    }
}

impl std::error::Error for Error {}
