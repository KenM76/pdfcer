//! Content-stream compaction (Pass 616.7): the `content(...)` stream action.
//!
//! A page's `/Contents`, a form XObject and a tiling pattern
//! (`/PatternType 1`) are re-written token by token, then re-encoded:
//! - **whitespace** `minimal` drops every separator a reader does not need
//!   to tell two tokens apart, and every comment (ISO 32000-2 §7.2.2: only
//!   two adjacent regular characters need white-space between them);
//! - **numbers** `shortest` writes each number in its shortest form with the
//!   same value (`0.500` → `.5`, `1.0` → `1`; an integer may stand wherever a
//!   real is expected, §7.3.3);
//! - **redundant** `on` removes operators that change nothing: an empty
//!   `q Q` pair, an identity `cm`, and a state setter repeating the value
//!   the state already holds.
//!
//! All three are lossless. **round** `T` (off by default) is not: it rounds
//! the coordinates of path construction (`m l c v y re`) and the line starts
//! of text (`Tm`'s translation, `Td`) to as few decimals as keep every point
//! within `T` user-space units of where it was, through the `cm` matrices in
//! force, and reports the largest movement
//! ([`ContentChange::max_displacement`]). It applies to page content: a
//! page's streams run as if concatenated in order (§7.7.3.3, Table 30), so
//! a later stream starts under the matrix the streams before it leave, and
//! its tolerance is divided by the most that matrix can stretch a move. A
//! stream several pages draw takes the largest. Forms, patterns, and a
//! stream after one whose effect is unknown report [`Unrounded`].
//!
//! Inside an array or dictionary operand (`TJ`, `d`, a `BDC` property list)
//! the numbers and separators follow the same two settings. Strings, names,
//! operators and inline images are copied byte for byte. A rewrite is re-read and compared with the
//! original, operation by operation (a rounded number with its rounded
//! value), before it is kept ([`ContentSkip::Mismatch`] otherwise). The leading and trailing
//! white-space of a stream is kept as one byte, because a page's `/Contents`
//! array may split between any two tokens (§7.7.3.3, Table 30).
//!
//! Type 3 glyph procedures are not rewritten: nothing in their dictionaries
//! marks them as content.

mod check;
mod rewrite;
mod round;

pub(crate) use round::starts;

use std::fmt;

use super::{SkipReason, StreamAction};
use crate::filters::{self, flate};
use crate::object::{Dict, Name, Object};
use crate::space_audit::SpaceCategory;

/// Content-stream compaction settings, the `content(...)` spelling of a
/// [`StreamAction`]. Unset fields take the settings before it, then the
/// defaults: `whitespace=minimal`, `numbers=shortest`, `redundant=on`.
///
/// ```
/// use pdfcer_core::optimize::{ContentPatch, StreamAction, Whitespace};
///
/// let action = StreamAction::parse("content(whitespace=keep)")?;
/// let StreamAction::Content(patch) = action else { unreachable!() };
/// assert_eq!(patch.whitespace, Some(Whitespace::Keep));
/// assert_eq!(action.to_string(), "content(whitespace=keep)");
/// assert_eq!(StreamAction::parse("content()")?, StreamAction::Content(ContentPatch::new()));
/// # Ok::<(), String>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct ContentPatch {
    /// Clear every field earlier `content(...)` actions set first.
    pub reset: bool,
    /// Separators and comments.
    pub whitespace: Option<Whitespace>,
    /// Number spelling.
    pub numbers: Option<Numbers>,
    /// Remove operators that change nothing.
    pub redundant: Option<bool>,
    /// Round path coordinates and text line starts (lossy).
    pub round: Option<Rounding>,
}

/// Whether [`ContentPatch::round`] rounds path coordinates and text line
/// starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Rounding {
    /// Every coordinate keeps its value.
    Off,
    /// Each point moves at most this far.
    Within(Tolerance),
}

/// A distance in user-space units (1/72 inch unless the page sets
/// `/UserUnit`), held in millionths so settings stay `Eq` and `Hash`.
///
/// ```
/// use pdfcer_core::optimize::Tolerance;
///
/// let t = Tolerance::from_units(0.01).unwrap();
/// assert_eq!(t.as_units(), 0.01);
/// assert!(Tolerance::from_units(0.0).is_none());
/// assert!(Tolerance::from_units(100.0).is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Tolerance(u32);

impl Tolerance {
    /// The largest tolerance accepted: one inch.
    pub const MAX_UNITS: f64 = 72.0;

    /// `units`, when it is above zero, at most [`Self::MAX_UNITS`] and at
    /// least one millionth.
    #[must_use]
    pub fn from_units(units: f64) -> Option<Self> {
        let micro = (units * 1e6).round();
        (units.is_finite() && units > 0.0 && units <= Self::MAX_UNITS && micro >= 1.0)
            .then_some(Self(micro as u32))
    }

    /// The distance in user-space units.
    #[must_use]
    pub fn as_units(self) -> f64 {
        f64::from(self.0) / 1e6
    }
}

impl fmt::Display for Rounding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => f.write_str("off"),
            Self::Within(t) => write!(f, "{}", t.as_units()),
        }
    }
}

/// How [`ContentPatch::whitespace`] treats separators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Whitespace {
    /// Every separator and comment as stored.
    Keep,
    /// Only the separators a reader needs; no comments.
    Minimal,
}

/// How [`ContentPatch::numbers`] spells numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Numbers {
    /// As stored.
    Keep,
    /// The shortest spelling of the same value; exponents are never written.
    Shortest,
}

impl ContentPatch {
    /// A patch that sets nothing: every field takes its default.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            reset: false,
            whitespace: None,
            numbers: None,
            redundant: None,
            round: None,
        }
    }

    /// Parse the `content(...)` spelling: `whitespace=keep|minimal`,
    /// `numbers=keep|shortest`, `redundant=on|off`, `round=off|T` (`T` in
    /// user-space units, above 0 and at most 72) and `reset`,
    /// comma-separated.
    ///
    /// # Errors
    ///
    /// A message naming an unknown or repeated key or a bad value.
    pub fn parse(text: &str) -> Result<Self, String> {
        let inner = text
            .trim()
            .strip_prefix("content(")
            .and_then(|t| t.strip_suffix(')'))
            .ok_or_else(|| format!("expected content(key=value,...), found {text:?}"))?;
        let mut patch = Self::new();
        let mut seen: Vec<&str> = Vec::new();
        for item in inner.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let (key, value) = item
                .split_once('=')
                .map_or((item, ""), |(k, v)| (k.trim(), v.trim()));
            if seen.contains(&key) {
                return Err(format!("content(): duplicate key {key:?}"));
            }
            seen.push(key);
            patch.set(key, value)?;
        }
        Ok(patch)
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), String> {
        let bad = |want: &str| format!("content(): bad value {value:?} for {key}; expected {want}");
        match key {
            "reset" if value.is_empty() || value == "true" => self.reset = true,
            "whitespace" => {
                self.whitespace = Some(match value {
                    "keep" => Whitespace::Keep,
                    "minimal" => Whitespace::Minimal,
                    _ => return Err(bad("keep or minimal")),
                });
            }
            "numbers" => {
                self.numbers = Some(match value {
                    "keep" => Numbers::Keep,
                    "shortest" => Numbers::Shortest,
                    _ => return Err(bad("keep or shortest")),
                });
            }
            "redundant" => {
                self.redundant = Some(match value {
                    "on" => true,
                    "off" => false,
                    _ => return Err(bad("on or off")),
                });
            }
            "round" => {
                self.round = Some(match value {
                    "off" => Rounding::Off,
                    _ => Rounding::Within(
                        value
                            .parse::<f64>()
                            .ok()
                            .and_then(Tolerance::from_units)
                            .ok_or_else(|| bad("off or a distance above 0 and at most 72"))?,
                    ),
                });
            }
            _ => {
                return Err(format!(
                    "content(): unknown key {key:?}; expected whitespace, numbers, redundant, round or reset"
                ));
            }
        }
        Ok(())
    }
}

impl fmt::Display for ContentPatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts: Vec<&str> = Vec::new();
        if let Some(n) = self.numbers {
            parts.push(match n {
                Numbers::Keep => "numbers=keep",
                Numbers::Shortest => "numbers=shortest",
            });
        }
        if let Some(r) = self.redundant {
            parts.push(if r { "redundant=on" } else { "redundant=off" });
        }
        if self.reset {
            parts.push("reset");
        }
        let round = self.round.map(|r| format!("round={r}"));
        if let Some(r) = &round {
            parts.push(r);
        }
        if let Some(w) = self.whitespace {
            parts.push(match w {
                Whitespace::Keep => "whitespace=keep",
                Whitespace::Minimal => "whitespace=minimal",
            });
        }
        write!(f, "content({})", parts.join(","))
    }
}

/// The content settings one stream folds to, and the encoding its result is
/// stored with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResolvedContent {
    pub(crate) whitespace: Whitespace,
    pub(crate) numbers: Numbers,
    pub(crate) redundant: bool,
    pub(crate) round: Option<Tolerance>,
    /// How far the matrix the stream starts under can stretch a move, for
    /// page content; `round` applies only when it is known. Set by the
    /// planner, not a setting.
    pub(crate) start: Option<Scale>,
    /// `Recompress` or `Brotli`: the whole-state action the patches landed
    /// on, `recompress-9` when that was `leave`.
    pub(crate) encode: StreamAction,
}

impl ResolvedContent {
    /// The defaults, encoded as `base` when it re-encodes, else `recompress-9`.
    pub(crate) fn base(base: StreamAction) -> Self {
        Self {
            whitespace: Whitespace::Minimal,
            numbers: Numbers::Shortest,
            redundant: true,
            round: None,
            start: None,
            encode: match base {
                StreamAction::Recompress { .. } | StreamAction::Brotli { .. } => base,
                _ => StreamAction::Recompress { level: 9 },
            },
        }
    }

    /// Apply the keys `p` sets; its `reset` first returns to [`Self::base`].
    pub(crate) fn merge(&mut self, p: &ContentPatch, base: StreamAction) {
        if p.reset {
            *self = Self::base(base);
        }
        if let Some(v) = p.whitespace {
            self.whitespace = v;
        }
        if let Some(v) = p.numbers {
            self.numbers = v;
        }
        if let Some(v) = p.redundant {
            self.redundant = v;
        }
        if let Some(v) = p.round {
            self.round = match v {
                Rounding::Off => None,
                Rounding::Within(t) => Some(t),
            };
        }
    }

    /// The patch that reproduces these settings over the defaults.
    pub(crate) fn to_patch(self) -> ContentPatch {
        ContentPatch {
            reset: false,
            whitespace: Some(self.whitespace),
            numbers: Some(self.numbers),
            redundant: Some(self.redundant),
            round: Some(self.round.map_or(Rounding::Off, Rounding::Within)),
        }
    }
}

/// What a content rewrite did to one stream.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ContentChange {
    /// Decoded content bytes before.
    pub decoded_before: u64,
    /// Decoded content bytes after.
    pub decoded_after: u64,
    /// Operators removed as redundant (their operands go with them).
    pub operators_removed: u64,
    /// Numbers written shorter with the same value.
    pub numbers_shortened: u64,
    /// Path coordinates rounded to another value.
    pub numbers_rounded: u64,
    /// The farthest any rounded point moved, in user-space units; `0.0`
    /// when nothing was rounded.
    pub max_displacement: f64,
    /// Why rounding was asked for and stopped or never started.
    pub unrounded: Option<Unrounded>,
    /// The encoding the result is stored with: [`StreamAction::Recompress`]
    /// or [`StreamAction::Brotli`].
    pub encoded_as: StreamAction,
}

/// The largest factor a matrix stretches a move by (its largest singular
/// value): finite and above zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Scale(u64);

impl Scale {
    /// `None` unless `factor` is finite and above zero. Stored as bits so
    /// the type can be `Eq`.
    pub(crate) fn new(factor: f64) -> Option<Self> {
        (factor.is_finite() && factor > 0.0).then_some(Self(factor.to_bits()))
    }

    /// The factor.
    pub(crate) fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

/// Why `round` left (part of) a stream's coordinates as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Unrounded {
    /// A form or a pattern, or page content after a stream whose effect on
    /// the matrix is unknown: the scale it starts at is not known here.
    UnknownStart,
    /// A `cm` with operands that are not six numbers, or a `Q` with no `q`:
    /// nothing after it is rounded.
    UnknownMatrix,
}

impl fmt::Display for Unrounded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::UnknownStart => {
                "not rounded: the scale it starts at is unknown (a form, a pattern, or page content after an unreadable stream)"
            }
            Self::UnknownMatrix => {
                "rounding stopped at a cm or Q whose effect on the scale is unknown"
            }
        })
    }
}

/// Why a content stream selected for `content(...)` is left as stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ContentSkip {
    /// The rewrite did not re-read as the same operations; it is a defect
    /// to report, and the stream is kept.
    Mismatch,
}

impl fmt::Display for ContentSkip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mismatch => f.write_str("the rewrite did not re-read as the same operations"),
        }
    }
}

/// Whether `content(...)` applies to the stream under `dict`: page content
/// (by `category`), a form XObject or a tiling pattern; never an image.
pub(crate) fn eligible(dict: &Dict, category: Option<SpaceCategory>) -> bool {
    let name = |key: &[u8], want: &[u8]| matches!(dict.get(key), Some(Object::Name(n)) if n.as_bytes() == want);
    category == Some(SpaceCategory::ContentStreams)
        || name(b"Subtype", b"Form")
        || matches!(dict.get(b"PatternType"), Some(Object::Integer(1)))
}

/// Rewrite the content stored as `raw` under `dict` and encode it as
/// `settings.encode`. Refused, like every stream action, when not strictly
/// smaller than `raw`.
pub(crate) fn apply(
    settings: ResolvedContent,
    dict: &Dict,
    raw: &[u8],
) -> Result<(Dict, Vec<u8>, ContentChange), SkipReason> {
    for name in super::streams::chain(dict) {
        if !super::streams::GENERIC.contains(&name.as_bytes()) {
            return Err(SkipReason::UnsupportedFilter(name));
        }
    }
    let decoded =
        filters::decode_stream(dict, raw).map_err(|e| SkipReason::Undecodable(e.to_string()))?;
    let out = rewrite::compact(&decoded, settings)?;
    let (filter, data): (&[u8], Vec<u8>) = match settings.encode {
        StreamAction::Brotli { quality } => (
            b"BrotliDecode",
            filters::brotli::encode(&out.bytes, u32::from(quality))
                .map_err(|e| SkipReason::Undecodable(e.to_string()))?,
        ),
        StreamAction::Recompress { level } => (
            b"FlateDecode",
            flate::encode_with_level(&out.bytes, u32::from(level)),
        ),
        _ => (b"FlateDecode", flate::encode_with_level(&out.bytes, 9)),
    };
    if data.len() >= raw.len() {
        return Err(SkipReason::NotSmaller {
            before: raw.len() as u64,
            after: data.len() as u64,
        });
    }
    let mut dict = dict.clone();
    dict.remove(b"DecodeParms");
    dict.remove(b"DP");
    dict.insert(Name::from(b"Filter"), Object::Name(Name::from(filter)));
    dict.insert(
        Name::from(b"Length"),
        Object::Integer(i64::try_from(data.len()).unwrap_or(i64::MAX)),
    );
    let change = ContentChange {
        decoded_before: decoded.len() as u64,
        decoded_after: out.bytes.len() as u64,
        operators_removed: out.removed,
        numbers_shortened: out.shortened,
        numbers_rounded: out.rounded.count,
        max_displacement: out.rounded.max,
        unrounded: out.rounded.unrounded,
        encoded_as: settings.encode,
    };
    Ok((dict, data, change))
}

#[cfg(test)]
mod round_tests;
#[cfg(test)]
mod tests;
