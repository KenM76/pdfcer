//! Optimize: operator-chosen size reduction (Pass 616.1, decision 191).
//!
//! An optimize run is three steps:
//! 1. **Settings.** An [`OptimizeSettings`] comes from a [`Preset`], a
//!    profile file ([`profile`]), or code. It names a default action for every
//!    eligible stream, an ordered list of [`Rule`]s, and per-object overrides.
//! 2. **Plan.** [`EditSession::plan_optimize`] performs every encoding in
//!    memory and returns an [`OptimizePlan`]. Its per-object sizes are real
//!    results, not estimates, so the plan is also the dry run.
//! 3. **Apply and save.** [`EditSession::apply_optimize`] commits the plan as
//!    one undoable command. [`EditSession::save_optimized`] writes a full
//!    rewrite and keeps the original instead when the rewrite is not smaller.
//!
//! Decision 191's guarantees:
//! - An object no action selected is copied byte-identical.
//! - **Never larger.** An object whose re-encoding is not strictly smaller
//!   keeps its stored form, and a whole file that is not strictly smaller is
//!   not written.
//! - A signed document is refused unless [`OptimizeSettings::allow_signed`]
//!   is set, and the plan then states how many signatures break.
//! - Every lossy change is listed in the plan. The actions in this Pass are
//!   all lossless.
//!
//! Precedence: a per-object override beats every rule, a later rule beats an
//! earlier one, and any rule beats the default.
//!
//! [`EditSession::plan_optimize`]: crate::edit::EditSession::plan_optimize
//! [`EditSession::apply_optimize`]: crate::edit::EditSession::apply_optimize
//! [`EditSession::save_optimized`]: crate::edit::EditSession::save_optimized

pub mod profile;
pub(crate) mod select;
pub(crate) mod streams;

use std::collections::BTreeMap;
use std::fmt;

pub use select::{PageRanges, Selector};

use crate::object::ObjId;
use crate::space_audit::SpaceCategory;

/// What to do with one stream.
///
/// `#[non_exhaustive]`: later Passes add image, font and content actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum StreamAction {
    /// Keep the stream exactly as stored.
    Leave,
    /// Decode the generic filter chain and re-encode as one `/FlateDecode`
    /// at zlib `level` (0 to 9; 9 is smallest). Lossless in decoded content.
    ///
    /// Applies to unfiltered streams and to chains of `FlateDecode`,
    /// `LZWDecode`, `ASCIIHexDecode`, `ASCII85Decode` and `RunLengthDecode`.
    /// Predictor `/DecodeParms` are dropped, because the stored data is no
    /// longer predicted.
    Recompress {
        /// zlib level, 0 to 9.
        level: u8,
    },
}

impl StreamAction {
    /// The profile spelling: `leave` or `recompress-N`.
    #[must_use]
    pub fn to_profile_string(self) -> String {
        match self {
            Self::Leave => "leave".to_owned(),
            Self::Recompress { level } => format!("recompress-{level}"),
        }
    }

    /// Parse the profile spelling. `recompress` alone means level 9.
    ///
    /// # Errors
    ///
    /// A message naming the accepted spellings.
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "leave" => Ok(Self::Leave),
            "recompress" => Ok(Self::Recompress { level: 9 }),
            _ => text
                .strip_prefix("recompress-")
                .and_then(|n| n.parse::<u8>().ok())
                .filter(|n| *n <= 9)
                .map(|level| Self::Recompress { level })
                .ok_or_else(|| {
                    format!(
                        "unknown action {text:?}; expected leave, recompress or recompress-0..9"
                    )
                }),
        }
    }

    /// Whether the action can change decoded content.
    #[must_use]
    pub const fn is_lossy(self) -> bool {
        match self {
            Self::Leave | Self::Recompress { .. } => false,
        }
    }
}

impl fmt::Display for StreamAction {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_profile_string())
    }
}

/// An action applied to every object a [`Selector`] matches.
#[derive(Debug, Clone, PartialEq)]
pub struct Rule {
    /// Which objects the rule covers.
    pub select: Selector,
    /// What to do with them.
    pub action: StreamAction,
}

/// Everything an optimize run is told to do.
///
/// `#[non_exhaustive]`: build one with [`OptimizeSettings::new`] or
/// [`Preset::settings`] and set fields.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OptimizeSettings {
    /// The action for an eligible stream no rule or override selects.
    pub streams: StreamAction,
    /// Rules, in order. A later match beats an earlier one.
    pub rules: Vec<Rule>,
    /// Per-object overrides. These beat every rule.
    pub objects: BTreeMap<ObjId, StreamAction>,
    /// Proceed on a signed document, invalidating its signatures.
    pub allow_signed: bool,
}

impl OptimizeSettings {
    /// Settings that change nothing: every stream is left, nothing allowed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            streams: StreamAction::Leave,
            rules: Vec::new(),
            objects: BTreeMap::new(),
            allow_signed: false,
        }
    }

    /// The action for one object, applying the precedence in the module docs.
    pub(crate) fn action_for(&self, id: ObjId, facts: &select::ObjectFacts<'_>) -> StreamAction {
        if let Some(action) = self.objects.get(&id) {
            return *action;
        }
        self.rules
            .iter()
            .rev()
            .find(|r| r.select.matches(id, facts))
            .map_or(self.streams, |r| r.action)
    }
}

impl Default for OptimizeSettings {
    fn default() -> Self {
        Self::new()
    }
}

/// pdfcer's built-in settings bundles.
///
/// The values are pdfcer's own, published in `--show-preset` and
/// `docs/core-api`. In this Pass every preset recompresses eligible streams
/// at level 9; they diverge as image, font and discard actions arrive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Preset {
    /// Lossless changes only.
    Lossless,
    /// The everyday default.
    Standard,
    /// Smallest file for screen and mobile reading.
    Screen,
    /// Keeps print-resolution content.
    Print,
    /// Keeps PDF/A conformance.
    Archive,
}

impl Preset {
    /// Every preset, in display order.
    pub const ALL: [Self; 5] = [
        Self::Lossless,
        Self::Standard,
        Self::Screen,
        Self::Print,
        Self::Archive,
    ];

    /// The stable name used by `--preset` and `extends`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Lossless => "lossless",
            Self::Standard => "standard",
            Self::Screen => "screen",
            Self::Print => "print",
            Self::Archive => "archive",
        }
    }

    /// Look a preset up by [`Self::as_str`]; `mobile` is an alias of
    /// `screen`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        if name == "mobile" {
            return Some(Self::Screen);
        }
        Self::ALL.into_iter().find(|p| p.as_str() == name)
    }

    /// The preset's settings.
    #[must_use]
    pub const fn settings(self) -> OptimizeSettings {
        let mut s = OptimizeSettings::new();
        s.streams = match self {
            Self::Lossless | Self::Standard | Self::Screen | Self::Print | Self::Archive => {
                StreamAction::Recompress { level: 9 }
            }
        };
        s
    }
}

impl fmt::Display for Preset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why a plan was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Refusal {
    /// The document is signed and [`OptimizeSettings::allow_signed`] is not
    /// set. A full rewrite invalidates every signature.
    Signed {
        /// Signature dictionaries found.
        signatures: usize,
        /// Of those, certification signatures.
        certifications: usize,
    },
    /// The document is encrypted. pdfcer's full rewrite does not re-encrypt,
    /// so there is no override: decrypt, optimize, then encrypt again.
    Encrypted,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Signed {
                signatures,
                certifications,
            } => write!(
                f,
                "the document has {signatures} signature(s) ({certifications} certification); \
                 optimizing invalidates all of them. Allow it explicitly to proceed"
            ),
            Self::Encrypted => f.write_str(
                "the document is encrypted and an optimized rewrite cannot be re-encrypted; \
                 decrypt it, optimize, then encrypt the result",
            ),
        }
    }
}

/// Why an optimize plan or apply failed.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum OptimizeError {
    /// The document is refused; nothing was planned.
    #[error("optimize refused: {}", join(.0))]
    Refused(Vec<Refusal>),
    /// The session changed after the plan was made.
    #[error("object {id} changed after the optimize plan was made; plan again")]
    StalePlan {
        /// The first object found changed.
        id: ObjId,
    },
}

fn join(refusals: &[Refusal]) -> String {
    refusals
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("; ")
}

/// One object a plan rewrites.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PlannedChange {
    /// The object.
    pub id: ObjId,
    /// Its space-audit category.
    pub category: SpaceCategory,
    /// The action applied.
    pub action: StreamAction,
    /// The filters before, in chain order (empty when unfiltered).
    pub filters_before: Vec<String>,
    /// Stored stream bytes before.
    pub before_bytes: u64,
    /// Stored stream bytes after.
    pub after_bytes: u64,
    /// Whether decoded content changes.
    pub lossy: bool,
}

/// Why a selected object was left as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SkipReason {
    /// Re-encoding was not strictly smaller (never larger).
    NotSmaller {
        /// Stored bytes.
        before: u64,
        /// Bytes the re-encoding produced.
        after: u64,
    },
    /// The chain holds a filter this action cannot re-encode.
    UnsupportedFilter(String),
    /// The stream would not decode.
    Undecodable(String),
    /// A role this action never rewrites: XMP metadata (PDF/A-1 §6.7.2
    /// forbids a `/Filter` on it), object and cross-reference streams (the
    /// writer owns them) and external-file streams.
    Protected(&'static str),
}

impl fmt::Display for SkipReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotSmaller { before, after } => {
                write!(f, "not smaller ({before} B -> {after} B), kept")
            }
            Self::UnsupportedFilter(name) => write!(f, "filter {name} is not re-encoded"),
            Self::Undecodable(why) => write!(f, "does not decode: {why}"),
            Self::Protected(what) => write!(f, "{what} is never rewritten"),
        }
    }
}

/// A selected object the plan leaves as stored, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Skipped {
    /// The object.
    pub id: ObjId,
    /// The action that was selected.
    pub action: StreamAction,
    /// Why it was not applied.
    pub reason: SkipReason,
}

/// What an optimize run will do, computed by performing it in memory.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OptimizePlan {
    /// The file's size when planned.
    pub file_bytes: u64,
    /// Objects the plan rewrites, in id order.
    pub changes: Vec<PlannedChange>,
    /// Objects selected for an action and left as stored.
    pub skipped: Vec<Skipped>,
    /// Disclosures: overridden refusals and coverage limits.
    pub notes: Vec<String>,
    pub(crate) writes: Vec<streams::Rewrite>,
}

impl OptimizePlan {
    /// Stored stream bytes the changes save. The file saving also includes
    /// dictionary and cross-reference changes, which only the save measures.
    #[must_use]
    pub fn stream_saving(&self) -> u64 {
        self.changes
            .iter()
            .map(|c| c.before_bytes.saturating_sub(c.after_bytes))
            .sum()
    }

    /// Whether any change is lossy.
    #[must_use]
    pub fn is_lossy(&self) -> bool {
        self.changes.iter().any(|c| c.lossy)
    }
}

/// The outcome of [`EditSession::save_optimized`].
///
/// [`EditSession::save_optimized`]: crate::edit::EditSession::save_optimized
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum OptimizedFile {
    /// The rewrite is strictly smaller; these are its bytes.
    Smaller(Vec<u8>),
    /// The rewrite was not smaller, so nothing should be written.
    KeptOriginal {
        /// The original file's length.
        original: u64,
        /// The rewrite's length.
        rewritten: u64,
    },
}

/// `bytes` only if strictly shorter than `original`.
pub(crate) fn keep_if_smaller(bytes: Vec<u8>, original: u64) -> OptimizedFile {
    let rewritten = bytes.len() as u64;
    if rewritten < original {
        OptimizedFile::Smaller(bytes)
    } else {
        OptimizedFile::KeptOriginal {
            original,
            rewritten,
        }
    }
}

#[cfg(test)]
mod tests;
