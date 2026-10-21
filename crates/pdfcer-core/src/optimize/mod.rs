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
//!    rewrite, rebuilds its structure under [`OptimizeSettings::structure`],
//!    and keeps the original instead when the result is not smaller.
//!
//! Decision 191's guarantees:
//! - An object no action selected is copied byte-identical, unless a
//!   [`OptimizeSettings::structure`] option is on: a structure rebuild
//!   renumbers and re-serializes every object, still without changing any
//!   decoded content.
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

pub(crate) mod actions;
pub(crate) mod annots;
pub mod cleanup;
pub mod discard;
pub(crate) mod fragments;
pub mod image;
pub(crate) mod object_data;
pub mod objects;
pub mod profile;
pub(crate) mod select;
pub(crate) mod smooth;
pub(crate) mod smooth_fit;
pub(crate) mod streams;

use std::collections::BTreeMap;
use std::fmt;

pub use crate::space_audit::ImageClass;
pub use cleanup::{BrokenBookmark, BrokenLink, BrokenTarget, CleanupOptions, CleanupPlan};
pub use discard::DiscardOptions;
pub use image::jpeg_quality::estimate_jpeg_quality;
pub use image::pipeline::{ImageChange, JpegQuality};
pub use image::{
    COLORS_RANGE, Chroma, Conversion, ImageConvert, ImageEncode, ImagePatch, ImageSkip, ImageStep,
    Predictor, QUALITY_LEVELS, ResampleMethod, ResolvedImage, SettingSource, Sourced, Threshold,
};
pub use objects::{DiscardObject, ObjectDiscard};
pub use select::{PageRanges, Selector};

pub use crate::writer::{RebuildOptions, RebuildReport};

use crate::doc_metadata::{MetadataItem, MetadataItemId, MetadataKind};
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
    /// Image settings, merged field by field over the actions before it
    /// (decision 192). A non-image stream gets the last `leave` or
    /// `recompress-N` before it instead.
    Image(ImagePatch),
}

impl StreamAction {
    /// The profile spelling: `leave` or `recompress-N`.
    #[must_use]
    pub fn to_profile_string(self) -> String {
        match self {
            Self::Leave => "leave".to_owned(),
            Self::Recompress { level } => format!("recompress-{level}"),
            Self::Image(patch) => patch.to_string(),
        }
    }

    /// Parse the profile spelling. `recompress` alone means level 9;
    /// `image(...)` is [`ImagePatch::parse`].
    ///
    /// # Errors
    ///
    /// A message naming the accepted spellings.
    pub fn parse(text: &str) -> Result<Self, String> {
        if text.trim_start().starts_with("image(") {
            return ImagePatch::parse(text).map(Self::Image);
        }
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
                        "unknown action {text:?}; expected leave, recompress, recompress-0..9 or image(...)"
                    )
                }),
        }
    }

    /// Whether the action can change decoded content: an image patch that
    /// sets a target `ppi` with a method other than `off`, asks for JPEG, or
    /// converts to another format. A plan says per image whether it did
    /// ([`ImageChange::is_lossy`]); an exact palette is not lossy there.
    #[must_use]
    pub fn is_lossy(self) -> bool {
        match self {
            Self::Leave | Self::Recompress { .. } => false,
            Self::Image(p) => {
                (p.ppi.is_some() && p.method != Some(ResampleMethod::Off))
                    || p.encode == Some(ImageEncode::Jpeg)
                    || p.convert.is_some_and(|c| c != ImageConvert::Keep)
            }
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
    /// The action for an eligible stream no rule or override selects. An
    /// [`StreamAction::Image`] here leaves every non-image stream as stored;
    /// image settings that keep a preset's recompression go in
    /// [`Self::images`].
    pub streams: StreamAction,
    /// Image settings for every image, merged over `streams` and under every
    /// rule and override; [`ImagePatch::new`] sets none. Non-image streams
    /// keep `streams`.
    pub images: ImagePatch,
    /// Rules, in order. A later match beats an earlier one.
    pub rules: Vec<Rule>,
    /// Per-object overrides. These beat every rule.
    pub objects: BTreeMap<ObjId, StreamAction>,
    /// Proceed on a signed document, invalidating its signatures.
    pub allow_signed: bool,
    /// The structure rebuild done at save: drop unreferenced objects, merge
    /// identical ones, pack objects into object streams. All off keeps the
    /// file's structure.
    pub structure: RebuildOptions,
    /// Links and bookmarks that go nowhere, and named destinations nothing
    /// names, removed before saving. See [`cleanup`].
    pub cleanup: CleanupOptions,
    /// User data removed before saving, by metadata kind. See [`discard`].
    pub discard: DiscardOptions,
    /// How far, in points, [`DiscardObject::SmoothLines`] may move the
    /// drawing; [`DEFAULT_SMOOTH_TOLERANCE`] to start. Not a positive
    /// finite number smooths nothing.
    pub smooth_tolerance: f64,
}

/// The tolerance [`OptimizeSettings::new`] starts with, in points: a fifth
/// of a typical 0.5 pt drawing line, under a pixel at 600 dpi.
pub const DEFAULT_SMOOTH_TOLERANCE: f64 = 0.1;

impl OptimizeSettings {
    /// Settings that change nothing: every stream is left, nothing allowed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            streams: StreamAction::Leave,
            images: ImagePatch::new(),
            rules: Vec::new(),
            objects: BTreeMap::new(),
            allow_signed: false,
            structure: RebuildOptions::none(),
            cleanup: CleanupOptions::none(),
            discard: DiscardOptions::none(),
            smooth_tolerance: DEFAULT_SMOOTH_TOLERANCE,
        }
    }

    /// Every action matching one object, folded in the precedence in the
    /// module docs: `streams`, then `images`, then matching rules in order,
    /// then the per-object override.
    pub(crate) fn resolve(&self, id: ObjId, facts: &select::ObjectFacts<'_>) -> image::Resolved {
        let rules = self
            .rules
            .iter()
            .enumerate()
            .filter(|(_, r)| r.select.matches(id, facts))
            .map(|(n, r)| (r.action, SettingSource::Rule(n)));
        let object = self.objects.get(&id).map(|a| (*a, SettingSource::Object));
        image::fold(
            std::iter::once((self.streams, SettingSource::Streams))
                .chain(
                    (self.images != ImagePatch::new())
                        .then_some((StreamAction::Image(self.images), SettingSource::Images)),
                )
                .chain(rules)
                .chain(object),
        )
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
/// `docs/core-api`. Today every preset recompresses eligible streams at
/// level 9 and turns every [`RebuildOptions`] option on; they diverge as
/// image, font and discard actions arrive.
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
        s.structure = RebuildOptions::all();
        // Removing an unreferenced named destination can break a link from
        // another file, so only the smallest-file preset does it.
        s.cleanup = CleanupOptions {
            unreferenced_destinations: matches!(self, Self::Screen),
            ..CleanupOptions::all()
        };
        // Page thumbnails are redrawn by every viewer that shows them; the
        // smallest-file preset also drops producers' private data,
        // per-object XMP and print-only alternate images. Archive keeps
        // everything PDF/A may need.
        s.discard = match self {
            Self::Lossless | Self::Archive => DiscardOptions::none(),
            Self::Standard | Self::Print => DiscardOptions::none().with(MetadataKind::Thumbnail),
            Self::Screen => DiscardOptions::none()
                .with(MetadataKind::Thumbnail)
                .with(MetadataKind::PieceInfo)
                .with(MetadataKind::ObjectXmp)
                .with_object(DiscardObject::AlternateImages),
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
    /// A clean-up deletion failed; everything the apply had committed was
    /// undone.
    #[error("clean-up of object {id} failed: {reason}; nothing was changed")]
    CleanupFailed {
        /// The link, bookmark or catalog being changed.
        id: ObjId,
        /// The deletion's error.
        reason: String,
    },
    /// A discarded item could not be removed; everything the apply had
    /// committed was undone.
    #[error("discarding {item} failed: {reason}; nothing was changed")]
    DiscardFailed {
        /// The metadata item.
        item: MetadataItemId,
        /// Why it was not removed.
        reason: String,
    },
    /// A document object could not be discarded; everything the apply had
    /// committed was undone.
    #[error("discarding {object} from object {owner} failed: {reason}; nothing was changed")]
    ObjectDiscardFailed {
        /// The class.
        object: DiscardObject,
        /// The object holding it.
        owner: ObjId,
        /// Why it was not removed.
        reason: String,
    },
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
    /// For a downsampled image: sizes, the binding placement and the
    /// settings with their sources.
    pub image: Option<ImageChange>,
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
    /// An image drawn on no page: with no placement there is no resolution
    /// to measure, so it is not resampled.
    Unplaced,
    /// The result was smaller, but by less than the `min-saving` percent.
    BelowMinSaving {
        /// Stored bytes.
        before: u64,
        /// Bytes the re-encoding produced.
        after: u64,
        /// The required saving, percent.
        pct: u8,
    },
    /// An image the settings ask to rewrite that is not rewritten, and why.
    ImageUnsupported(ImageSkip),
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
            Self::Unplaced => f.write_str("drawn on no page, so not resampled"),
            Self::BelowMinSaving { before, after, pct } => {
                write!(f, "saves less than {pct}% ({before} B -> {after} B), kept")
            }
            Self::ImageUnsupported(why) => write!(f, "not rewritten: {why}"),
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
    /// Links, bookmarks and named destinations the apply removes.
    pub cleanup: CleanupPlan,
    /// User data the apply removes, from the metadata inventory.
    pub discard: Vec<MetadataItem>,
    /// Document objects the apply removes.
    pub discard_objects: Vec<ObjectDiscard>,
    pub(crate) writes: Vec<streams::Rewrite>,
    /// The settings' smoothing tolerance, for the apply to redo it with.
    pub(crate) smooth_tolerance: f64,
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

    /// Whether the plan removes content: a clean-up or a discard, as
    /// opposed to a pure size saving.
    #[must_use]
    pub fn removes_content(&self) -> bool {
        !self.cleanup.is_empty() || !self.discard.is_empty() || !self.discard_objects.is_empty()
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
    /// The rewrite is not smaller, but the plan removes content the
    /// operator asked to remove (clean-up or discard), so these bytes are
    /// written anyway: the never-larger rule covers what is only a size
    /// saving. A caller holding to the size rule alone treats this as
    /// [`Self::KeptOriginal`].
    NotSmaller {
        /// The rewrite.
        bytes: Vec<u8>,
        /// The original file's length.
        original: u64,
    },
    /// The rewrite was not smaller and removes nothing, so nothing should be
    /// written.
    KeptOriginal {
        /// The original file's length.
        original: u64,
        /// The rewrite's length.
        rewritten: u64,
    },
}

/// `bytes` if strictly shorter than `original`, or if `removes` content.
pub(crate) fn keep_if_smaller(bytes: Vec<u8>, original: u64, removes: bool) -> OptimizedFile {
    let rewritten = bytes.len() as u64;
    if rewritten < original {
        OptimizedFile::Smaller(bytes)
    } else if removes {
        OptimizedFile::NotSmaller { bytes, original }
    } else {
        OptimizedFile::KeptOriginal {
            original,
            rewritten,
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod cleanup_tests;

#[cfg(test)]
mod discard_tests;

#[cfg(test)]
mod actions_tests;
#[cfg(test)]
mod annots_tests;
#[cfg(test)]
mod flatten_layers_tests;
#[cfg(test)]
mod fragments_tests;
#[cfg(test)]
mod object_data_tests;
#[cfg(test)]
mod objects_tests;
#[cfg(test)]
mod search_index_tests;
#[cfg(test)]
mod smooth_tests;
