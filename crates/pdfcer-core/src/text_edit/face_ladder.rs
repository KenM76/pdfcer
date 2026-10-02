//! Pass 436.2: the replacement-face matching ladder (decision 178), shared by
//! the subset augmenter's name match (decision 173), the fallback route
//! (Pass 431.0) and the retype (decision 175).
//!
//! Rungs, best first: the run's PostScript name with its §9.6.4 subset tag
//! stripped; the same family, nearest descriptor class; any face covering
//! every needed character, nearest class; the class-matched standard-14
//! face (§9.6.2.2). A face whose OpenType `OS/2.fsType` forbids the embed is
//! skipped, with no override, and the skip is disclosed.

use std::fmt;

use crate::font_embed::FontEmbedPlan;
use crate::fontdata::{self, Std14};
use crate::fontinfo::{EmbeddingPermission, FsTypeBits};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::text_extract::font::ExtractFont;
use crate::view::DocumentView;

/// A face's design class, as ISO 32000-2 §9.8.2 Tables 120–121 (ISO
/// 32000-1 Tables 122–123) describe it, and as an installed face's `OS/2`
/// and `post` tables state it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FaceClass {
    /// `FixedPitch` (flag bit 1); `post.isFixedPitch` for an installed face.
    pub fixed_pitch: bool,
    /// `Serif` (flag bit 2); `None` when the face does not say.
    pub serif: Option<bool>,
    /// `Italic` (flag bit 7) or a non-zero `ItalicAngle`.
    pub italic: bool,
    /// `FontWeight`, 100–900; 400 normal, 700 bold.
    pub weight: u16,
    /// `FontStretch` as a step, 1 (`UltraCondensed`) to 9
    /// (`UltraExpanded`); 5 is `Normal`, as `OS/2.usWidthClass` counts.
    pub width: u8,
}

impl Default for FaceClass {
    fn default() -> Self {
        Self {
            fixed_pitch: false,
            serif: None,
            italic: false,
            weight: 400,
            width: 5,
        }
    }
}

impl FaceClass {
    /// A regular-weight, normal-width, upright proportional face of unknown
    /// serif style.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set [`Self::fixed_pitch`], returning `self`.
    #[must_use]
    pub const fn with_fixed_pitch(mut self, fixed: bool) -> Self {
        self.fixed_pitch = fixed;
        self
    }

    /// Set [`Self::serif`], returning `self`.
    #[must_use]
    pub const fn with_serif(mut self, serif: Option<bool>) -> Self {
        self.serif = serif;
        self
    }

    /// Set [`Self::italic`], returning `self`.
    #[must_use]
    pub const fn with_italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    /// Set [`Self::weight`], clamped to 100–900, returning `self`.
    #[must_use]
    pub fn with_weight(mut self, weight: u16) -> Self {
        self.weight = weight.clamp(100, 900);
        self
    }

    /// Set [`Self::width`], clamped to 1–9, returning `self`.
    #[must_use]
    pub fn with_width(mut self, width: u8) -> Self {
        self.width = width.clamp(1, 9);
        self
    }

    /// Whether the weight is semibold or heavier (600+), the threshold
    /// [`FontWeight::is_bold`](crate::text_extract::FontWeight::is_bold) uses.
    #[must_use]
    pub const fn is_bold(&self) -> bool {
        self.weight >= 600
    }

    /// How far `other` is from this class: fixed pitch outweighs serif
    /// style, which outweighs slant, which outweighs weight and width.
    #[must_use]
    pub fn distance(&self, other: &Self) -> u32 {
        let mut d = 0;
        if self.fixed_pitch != other.fixed_pitch {
            d += 100;
        }
        if let (Some(a), Some(b)) = (self.serif, other.serif)
            && a != b
        {
            d += 50;
        }
        if self.italic != other.italic {
            d += 20;
        }
        d += u32::from(self.weight.abs_diff(other.weight)) / 20;
        d + 3 * u32::from(self.width.abs_diff(other.width))
    }
}

/// The face the run needs: its name, family, class and the characters a
/// replacement face must cover.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FaceRequest {
    /// The run's `/BaseFont`, subset tag allowed; empty when there is none.
    pub base_font: String,
    /// `/FontFamily` (§9.8.2), when the descriptor declares one.
    pub family: Option<String>,
    /// The run's class.
    pub class: FaceClass,
    /// Every character the replacement face must set.
    pub chars: Vec<char>,
}

impl FaceRequest {
    /// A request for `chars` in a face like `base_font`, of default class.
    #[must_use]
    pub fn new(base_font: &str, chars: &[char]) -> Self {
        Self {
            base_font: base_font.to_owned(),
            family: None,
            class: FaceClass::default(),
            chars: chars.to_vec(),
        }
    }

    /// Set [`Self::family`], returning `self`.
    #[must_use]
    pub fn with_family(mut self, family: &str) -> Self {
        self.family = Some(family.to_owned());
        self
    }

    /// Set [`Self::class`], returning `self`.
    #[must_use]
    pub const fn with_class(mut self, class: FaceClass) -> Self {
        self.class = class;
        self
    }

    /// The request a font dictionary makes: its `/BaseFont`, its
    /// descriptor's `/FontFamily`, `/Flags`, `/ItalicAngle` and
    /// `/FontStretch` (the descendant's, for a `Type0`), and the weight
    /// [`ExtractFont::weight`] derives. A non-embedded standard-14 font
    /// without a descriptor takes its built-in descriptor (§9.6.2.2).
    pub(crate) fn from_font_dict(doc: &DocumentView<'_>, dict: &Dict, chars: &[char]) -> Self {
        let font = ExtractFont::resolve(doc, dict);
        let mut req = Self::new(&font.base_font, chars);
        let descriptor = descriptor_of(doc, dict);
        let num = |d: &Dict, k: &[u8]| d.get(k).map(|o| doc.resolve(o)).and_then(Object::as_number);
        let (flags, angle) = match &descriptor {
            Some(d) => (num(d, b"Flags").unwrap_or(0.0), num(d, b"ItalicAngle")),
            None => std14_flags(&font.base_font),
        };
        // Flags is a 32-bit field (Table 123); a malformed value reads as 0.
        let flags = if (0.0..=f64::from(u32::MAX)).contains(&flags) {
            flags as u32
        } else {
            0
        };
        let width = descriptor
            .as_ref()
            .and_then(|d| doc.resolve(d.get(b"FontStretch")?).as_name().cloned())
            .and_then(|n| stretch_step(n.as_bytes()))
            .unwrap_or(5);
        req.class = FaceClass::new()
            .with_fixed_pitch(flags & 0x1 != 0)
            .with_serif((descriptor.is_some() || flags != 0).then_some(flags & 0x2 != 0))
            .with_italic(flags & 0x40 != 0 || angle.is_some_and(|a| a != 0.0))
            .with_weight(font.weight().value)
            .with_width(width);
        req.family = descriptor
            .as_ref()
            .and_then(|d| match doc.resolve(d.get(b"FontFamily")?) {
                Object::String(s) => Some(String::from_utf8_lossy(s.as_slice()).into_owned()),
                _ => None,
            });
        req
    }
}

fn descriptor_of(doc: &DocumentView<'_>, dict: &Dict) -> Option<Dict> {
    let own = |d: &Dict| doc.resolve(d.get(b"FontDescriptor")?).as_dict().cloned();
    own(dict).or_else(|| {
        let first = doc
            .resolve(dict.get(b"DescendantFonts")?)
            .as_array()?
            .first()?;
        own(doc.resolve(first).as_dict()?)
    })
}

fn std14_flags(base_font: &str) -> (f64, Option<f64>) {
    fontdata::std14_by_base_font(strip_subset_tag(base_font)).map_or((0.0, None), |f| {
        let d = fontdata::std14_descriptor(f);
        (f64::from(d.flags), Some(f64::from(d.italic_angle)))
    })
}

/// `/FontStretch` (Table 122) as a 1–9 step.
fn stretch_step(name: &[u8]) -> Option<u8> {
    const NAMES: [&[u8]; 9] = [
        b"UltraCondensed",
        b"ExtraCondensed",
        b"Condensed",
        b"SemiCondensed",
        b"Normal",
        b"SemiExpanded",
        b"Expanded",
        b"ExtraExpanded",
        b"UltraExpanded",
    ];
    let i = NAMES.iter().position(|n| *n == name)?;
    u8::try_from(i + 1).ok()
}

/// An installed face the ladder may pick, as its provider describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FaceCandidate {
    /// The provider's own handle; [`ReplacementFaces::plan`] receives it back.
    pub id: usize,
    /// Where the face came from (a file path), for the disclosure.
    pub source: String,
    /// Name ID 6.
    pub postscript_name: String,
    /// Name ID 16, else name ID 1.
    pub family: String,
    /// The face's class.
    pub class: FaceClass,
    /// The requested characters its cmap does not map.
    pub missing: Vec<char>,
    /// `OS/2.fsType`; `None` when the face has no `OS/2` table.
    pub fs_type: Option<FsTypeBits>,
}

impl FaceCandidate {
    /// A candidate of default class, covering everything, with no `OS/2`.
    #[must_use]
    pub fn new(id: usize, source: &str, postscript_name: &str, family: &str) -> Self {
        Self {
            id,
            source: source.to_owned(),
            postscript_name: postscript_name.to_owned(),
            family: family.to_owned(),
            class: FaceClass::default(),
            missing: Vec::new(),
            fs_type: None,
        }
    }

    /// Set [`Self::class`], returning `self`.
    #[must_use]
    pub const fn with_class(mut self, class: FaceClass) -> Self {
        self.class = class;
        self
    }

    /// Set [`Self::missing`], returning `self`.
    #[must_use]
    pub fn with_missing(mut self, missing: Vec<char>) -> Self {
        self.missing = missing;
        self
    }

    /// Set [`Self::fs_type`], returning `self`.
    #[must_use]
    pub const fn with_fs_type(mut self, fs_type: FsTypeBits) -> Self {
        self.fs_type = Some(fs_type);
        self
    }
}

/// The rung a face was picked on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum FaceRung {
    /// Its PostScript name is the run's `/BaseFont` without the subset tag.
    ExactName,
    /// Same family; the nearest class within it.
    FamilyClass,
    /// It covers every needed character; the nearest class among those.
    Coverage,
    /// No candidate qualified: the class-matched standard-14 face.
    Standard14,
}

impl FaceRung {
    /// A short stable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ExactName => "exact-name",
            Self::FamilyClass => "family+class",
            Self::Coverage => "coverage",
            Self::Standard14 => "standard-14",
        }
    }
}

impl fmt::Display for FaceRung {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// A face the ladder passed over because its licence bits forbid the embed.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SkippedFace {
    /// Name ID 6.
    pub postscript_name: String,
    /// [`FaceCandidate::source`].
    pub source: String,
    /// The rung it would have qualified on.
    pub rung: FaceRung,
    /// Which `fsType` setting forbids it.
    pub reason: &'static str,
}

/// The ladder's ranking of a candidate set.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FaceLadder {
    /// Qualifying candidates, best first: `(rung, index into the slice)`.
    pub ranked: Vec<(FaceRung, usize)>,
    /// Covering candidates refused on `fsType`, in candidate order.
    pub skipped: Vec<SkippedFace>,
    /// The floor: the standard-14 face of the request's class.
    pub standard14: &'static str,
}

/// Whether `bits` forbid embedding a subset of the face into a document
/// pdfcer is editing, and why. OpenType `OS/2.fsType`: usage value 2
/// (Restricted) forbids embedding; value 4 (Preview & Print) requires the
/// document to be opened read-only with no edits applied, which an edit
/// contradicts; more than one usage bit is ambiguous; bit 8 forbids the
/// subsetting pdfcer always does; bit 9 permits bitmaps only. Bits 8–9 are
/// already cleared for `OS/2` versions 0–1 by [`FsTypeBits`].
#[must_use]
pub const fn embedding_refusal(bits: &FsTypeBits) -> Option<&'static str> {
    match bits.permission {
        EmbeddingPermission::Restricted => return Some("Restricted License embedding"),
        EmbeddingPermission::PreviewPrint => {
            return Some("Preview & Print embedding (the document may not be edited)");
        }
        EmbeddingPermission::Ambiguous => return Some("ambiguous embedding bits"),
        _ => {}
    }
    if bits.bitmap_only {
        Some("bitmap-only embedding")
    } else if bits.no_subsetting {
        Some("no-subsetting embedding")
    } else {
        None
    }
}

/// `base_font` without a §9.6.4 subset tag (`ABCDEF+`).
#[must_use]
pub fn strip_subset_tag(base_font: &str) -> &str {
    match base_font.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => base_font,
    }
}

/// Rung 1: whether `postscript_name` (name ID 6) is `base_font` with any
/// subset tag removed. Exact, case-sensitive.
#[must_use]
pub fn postscript_name_matches(postscript_name: &str, base_font: &str) -> bool {
    !postscript_name.is_empty() && postscript_name == strip_subset_tag(base_font)
}

/// A family name reduced for comparison: ASCII alphanumerics, lowercased,
/// a trailing `PSMT`/`MT`/`PS` dropped (`TimesNewRomanPSMT` and
/// `Times New Roman` compare equal).
fn family_key(name: &str) -> String {
    let key: String = name
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect();
    ["psmt", "mt", "ps"]
        .iter()
        .find_map(|s| key.strip_suffix(s).filter(|k| !k.is_empty()))
        .map_or_else(|| key.clone(), str::to_owned)
}

/// The request's family: `/FontFamily`, else the tag-stripped `/BaseFont`
/// up to its first `-` or `,` (`Arial,Bold`, `Times-Roman`).
fn request_family(req: &FaceRequest) -> String {
    req.family.as_deref().map_or_else(
        || {
            let name = strip_subset_tag(&req.base_font);
            family_key(name.split(['-', ',']).next().unwrap_or(name))
        },
        family_key,
    )
}

/// The standard-14 face of `class` (§9.6.2.2): Courier for fixed pitch,
/// Times for serif, else Helvetica, in its bold/italic variant.
#[must_use]
pub fn standard14_for(class: &FaceClass) -> &'static str {
    let base = if class.fixed_pitch {
        Std14::Courier
    } else if class.serif == Some(true) {
        Std14::TimesRoman
    } else {
        Std14::Helvetica
    };
    let face = fontdata::std14_styled(base, class.is_bold(), class.italic).unwrap_or(base);
    fontdata::std14_base_font_name(face)
}

/// Rank `candidates` for `req`. Only a candidate covering every requested
/// character qualifies; one whose `fsType` forbids the embed is skipped
/// ([`embedding_refusal`]). Within a rung the nearest class wins, then the
/// lower [`FaceCandidate::id`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::text_edit::{FaceCandidate, FaceRequest, FaceRung, rank_replacement_faces};
///
/// let req = FaceRequest::new("ABCDEF+DemoSans-Bold", &['é']);
/// let faces = [
///     FaceCandidate::new(0, "a.ttf", "Other", "Other"),
///     FaceCandidate::new(1, "b.ttf", "DemoSans-Bold", "Demo Sans"),
/// ];
/// let ladder = rank_replacement_faces(&req, &faces);
/// assert_eq!(ladder.ranked[0], (FaceRung::ExactName, 1));
/// assert_eq!(ladder.ranked[1], (FaceRung::Coverage, 0));
/// ```
#[must_use]
pub fn rank_replacement_faces(req: &FaceRequest, candidates: &[FaceCandidate]) -> FaceLadder {
    let family = request_family(req);
    let mut ranked = Vec::new();
    let mut skipped = Vec::new();
    for (i, c) in candidates.iter().enumerate() {
        if !c.missing.is_empty() {
            continue;
        }
        let rung = if postscript_name_matches(&c.postscript_name, &req.base_font) {
            FaceRung::ExactName
        } else if !family.is_empty() && family_key(&c.family) == family {
            FaceRung::FamilyClass
        } else {
            FaceRung::Coverage
        };
        if let Some(reason) = c.fs_type.as_ref().and_then(embedding_refusal) {
            skipped.push(SkippedFace {
                postscript_name: c.postscript_name.clone(),
                source: c.source.clone(),
                rung,
                reason,
            });
            continue;
        }
        ranked.push((rung, req.class.distance(&c.class), c.id, i));
    }
    ranked.sort_unstable();
    FaceLadder {
        ranked: ranked.into_iter().map(|(r, _, _, i)| (r, i)).collect(),
        skipped,
        standard14: standard14_for(&req.class),
    }
}

/// Installed faces a shell offers the ladder. Core reads no files: the
/// shell describes its faces and cuts the subset of the one picked.
pub trait ReplacementFaces: fmt::Debug + Send + Sync {
    /// Every offered face, its [`FaceCandidate::missing`] taken against
    /// `chars`.
    fn candidates(&self, chars: &[char]) -> Vec<FaceCandidate>;

    /// A subset of `candidate` carrying `chars`, to embed (§9.7.4).
    ///
    /// # Errors
    ///
    /// Why it cannot be cut, in a sentence; the ladder discloses it and
    /// tries the next face.
    fn plan(&self, candidate: &FaceCandidate, chars: &[char]) -> Result<FontEmbedPlan, String>;
}

/// How the ladder chose a replacement face (rule 4).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FaceMatch {
    /// The rung it was picked on.
    pub rung: FaceRung,
    /// Its PostScript name (a standard-14 name on [`FaceRung::Standard14`]).
    pub face: String,
    /// Its file; `None` for a standard-14 face, which embeds no program.
    pub source: Option<String>,
    /// Faces as good or better refused on `fsType`.
    pub skipped: Vec<SkippedFace>,
    /// Better-ranked faces whose subset could not be cut, and why.
    pub failed: Vec<String>,
}

/// How many skipped faces a disclosure names before summarising the rest.
const NAMED_SKIPS: usize = 5;

impl FaceMatch {
    /// The off-canvas disclosure line.
    #[must_use]
    pub fn disclosure(&self) -> String {
        let from = self.source.as_deref().map_or_else(
            || "a standard-14 face, no program embedded".to_owned(),
            |s| format!("from {s}"),
        );
        let mut line = format!(
            "replacement face: '{}' picked on the {} rung ({from})",
            self.face, self.rung
        );
        if !self.skipped.is_empty() {
            let named: Vec<String> = self
                .skipped
                .iter()
                .take(NAMED_SKIPS)
                .map(|s| format!("'{}' ({}: {})", s.postscript_name, s.source, s.reason))
                .collect();
            line.push_str("; skipped because the font's own licence bits forbid embedding it: ");
            line.push_str(&named.join(", "));
            if let Some(more) = self
                .skipped
                .len()
                .checked_sub(NAMED_SKIPS)
                .filter(|&n| n > 0)
            {
                line.push_str(&format!(" and {more} more"));
            }
        }
        for f in &self.failed {
            line.push_str("; not usable: ");
            line.push_str(f);
        }
        line
    }
}

/// Walk the ladder: the best-ranked candidate whose subset cuts, else the
/// standard-14 floor.
pub(crate) fn choose(
    req: &FaceRequest,
    faces: Option<&dyn ReplacementFaces>,
) -> (crate::text_edit::FallbackFace, FaceMatch) {
    use crate::text_edit::FallbackFace;
    let candidates = faces.map(|f| f.candidates(&req.chars)).unwrap_or_default();
    let ladder = rank_replacement_faces(req, &candidates);
    let skipped_at = |rung: FaceRung| -> Vec<SkippedFace> {
        let s = ladder.skipped.iter().filter(|s| s.rung <= rung);
        s.cloned().collect()
    };
    let mut failed = Vec::new();
    for &(rung, i) in &ladder.ranked {
        let Some((faces, c)) = faces.zip(candidates.get(i)) else {
            continue;
        };
        match faces.plan(c, &req.chars) {
            Ok(plan) => {
                let m = FaceMatch {
                    rung,
                    face: c.postscript_name.clone(),
                    source: Some(c.source.clone()),
                    skipped: skipped_at(rung),
                    failed,
                };
                return (FallbackFace::Embedded(Box::new(plan)), m);
            }
            Err(why) => failed.push(format!("'{}' ({}): {why}", c.postscript_name, c.source)),
        }
    }
    let m = FaceMatch {
        rung: FaceRung::Standard14,
        face: ladder.standard14.to_owned(),
        source: None,
        skipped: skipped_at(FaceRung::Standard14),
        failed,
    };
    (FallbackFace::Named(ladder.standard14.to_owned()), m)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test assertions
mod tests {
    use super::*;
    use crate::font_embed::{DescriptorMetrics, OutlineKind};
    use crate::text_edit::FallbackFace;

    fn face(id: usize, ps: &str, family: &str) -> FaceCandidate {
        FaceCandidate::new(id, &format!("f{id}.ttf"), ps, family)
    }

    fn bits(raw: u16) -> FsTypeBits {
        FsTypeBits::decode(raw, 4)
    }

    #[test]
    fn the_exact_name_outranks_family_and_coverage() {
        let req = FaceRequest::new("ABCDEF+Demo-Bold", &['x']);
        let faces = [
            face(0, "Other", "Other"),
            face(1, "Demo-Regular", "Demo"),
            face(2, "Demo-Bold", "Demo"),
        ];
        let ladder = rank_replacement_faces(&req, &faces);
        assert_eq!(
            ladder.ranked,
            [
                (FaceRung::ExactName, 2),
                (FaceRung::FamilyClass, 1),
                (FaceRung::Coverage, 0)
            ]
        );
    }

    #[test]
    fn within_a_rung_the_nearest_class_wins() {
        let req =
            FaceRequest::new("Demo,Bold", &['x']).with_class(FaceClass::new().with_weight(700));
        let faces = [
            face(0, "Demo-Regular", "Demo"),
            face(1, "Demo-Black", "Demo").with_class(FaceClass::new().with_weight(700)),
        ];
        let ladder = rank_replacement_faces(&req, &faces);
        assert_eq!(ladder.ranked[0], (FaceRung::FamilyClass, 1));
    }

    #[test]
    fn fixed_pitch_outweighs_every_other_class_difference() {
        let req = FaceRequest::new("Unrelated", &['x'])
            .with_class(FaceClass::new().with_fixed_pitch(true));
        let near_but_proportional = face(0, "A", "A");
        let far_but_fixed = face(1, "B", "B").with_class(
            FaceClass::new()
                .with_fixed_pitch(true)
                .with_weight(900)
                .with_italic(true),
        );
        let ladder = rank_replacement_faces(&req, &[near_but_proportional, far_but_fixed]);
        assert_eq!(ladder.ranked[0], (FaceRung::Coverage, 1));
    }

    #[test]
    fn a_family_name_matches_across_spacing_and_an_mt_suffix() {
        let req = FaceRequest::new("TimesNewRomanPSMT", &['x']);
        let ladder = rank_replacement_faces(&req, &[face(0, "TNR-Italic", "Times New Roman")]);
        assert_eq!(ladder.ranked, [(FaceRung::FamilyClass, 0)]);
        assert_eq!(family_key("MT"), "mt");
    }

    #[test]
    fn a_face_missing_a_needed_character_does_not_qualify() {
        let req = FaceRequest::new("Demo", &['x', 'y']);
        let missing = face(0, "Demo", "Demo").with_missing(vec!['y']);
        let ladder = rank_replacement_faces(&req, &[missing]);
        assert!(ladder.ranked.is_empty());
        assert!(ladder.skipped.is_empty());
    }

    #[test]
    fn a_face_whose_fs_type_forbids_the_edit_is_skipped_with_its_rung() {
        let req = FaceRequest::new("Demo", &['x']);
        let faces = [
            face(0, "Demo", "Demo").with_fs_type(bits(0x0002)),
            face(1, "Demo2", "Demo").with_fs_type(bits(0x0004)),
            face(2, "Free", "Free").with_fs_type(bits(0x0008)),
            face(3, "Bare", "Bare"),
        ];
        let ladder = rank_replacement_faces(&req, &faces);
        assert_eq!(
            ladder.ranked,
            [(FaceRung::Coverage, 2), (FaceRung::Coverage, 3)]
        );
        let skipped: Vec<(FaceRung, &str)> =
            ladder.skipped.iter().map(|s| (s.rung, s.reason)).collect();
        assert_eq!(
            skipped,
            [
                (FaceRung::ExactName, "Restricted License embedding"),
                (
                    FaceRung::FamilyClass,
                    "Preview & Print embedding (the document may not be edited)"
                ),
            ]
        );
    }

    #[test]
    fn no_subsetting_and_bitmap_only_refuse_but_only_from_os2_version_2() {
        assert_eq!(
            embedding_refusal(&bits(0x0100)),
            Some("no-subsetting embedding")
        );
        assert_eq!(
            embedding_refusal(&bits(0x0200)),
            Some("bitmap-only embedding")
        );
        assert_eq!(embedding_refusal(&FsTypeBits::decode(0x0100, 1)), None);
        assert_eq!(
            embedding_refusal(&bits(0x000C)),
            Some("ambiguous embedding bits")
        );
        assert_eq!(embedding_refusal(&bits(0)), None);
    }

    #[test]
    fn the_standard14_floor_follows_the_class() {
        let c = FaceClass::new();
        assert_eq!(standard14_for(&c), "Helvetica");
        assert_eq!(standard14_for(&c.with_weight(700)), "Helvetica-Bold");
        let serif = c.with_serif(Some(true));
        assert_eq!(
            standard14_for(&serif.with_italic(true).with_weight(700)),
            "Times-BoldItalic"
        );
        assert_eq!(standard14_for(&serif.with_fixed_pitch(true)), "Courier");
    }

    #[test]
    fn a_subset_tag_is_six_uppercase_letters() {
        assert_eq!(strip_subset_tag("ABCDEF+Demo"), "Demo");
        assert_eq!(strip_subset_tag("ABCDE+Demo"), "ABCDE+Demo");
        assert_eq!(strip_subset_tag("abcdef+Demo"), "abcdef+Demo");
        assert!(!postscript_name_matches("", "ABCDEF+"));
    }

    /// Offers `faces`; cutting a subset of any face named in `broken` fails.
    #[derive(Debug)]
    struct Fake {
        faces: Vec<FaceCandidate>,
        broken: &'static [&'static str],
    }

    impl ReplacementFaces for Fake {
        fn candidates(&self, _: &[char]) -> Vec<FaceCandidate> {
            self.faces.clone()
        }
        fn plan(&self, c: &FaceCandidate, _: &[char]) -> Result<FontEmbedPlan, String> {
            if self.broken.contains(&c.postscript_name.as_str()) {
                return Err("CFF outlines".to_owned());
            }
            Ok(FontEmbedPlan {
                program: vec![0],
                base_name: c.postscript_name.clone(),
                subset_tag: "AAAAAA".to_owned(),
                outline_kind: OutlineKind::TrueType,
                glyphs: Vec::new(),
                metrics: DescriptorMetrics {
                    bbox: [0; 4],
                    italic_angle: 0,
                    ascent: 0,
                    descent: 0,
                    cap_height: 0,
                    stem_v: 0,
                    flags: 0,
                },
            })
        }
    }

    #[test]
    fn choose_falls_past_an_uncuttable_face_and_names_it() {
        let fake = Fake {
            faces: vec![face(0, "Demo", "Demo"), face(1, "Other", "Other")],
            broken: &["Demo"],
        };
        let (picked, m) = choose(&FaceRequest::new("Demo", &['x']), Some(&fake));
        let FallbackFace::Embedded(plan) = picked else {
            panic!("expected an embedded face");
        };
        assert_eq!(plan.base_name, "Other");
        assert_eq!(
            (m.rung, m.source.as_deref()),
            (FaceRung::Coverage, Some("f1.ttf"))
        );
        assert_eq!(m.failed, ["'Demo' (f0.ttf): CFF outlines"]);
        assert!(
            m.disclosure().contains("not usable: 'Demo'"),
            "{}",
            m.disclosure()
        );
    }

    #[test]
    fn choose_discloses_only_skips_ranked_at_or_above_the_pick() {
        let fake = Fake {
            faces: vec![
                face(0, "Demo", "Demo").with_fs_type(bits(2)),
                face(1, "Demo-X", "Demo"),
                face(2, "Elsewhere", "Elsewhere").with_fs_type(bits(2)),
            ],
            broken: &[],
        };
        let (_, m) = choose(&FaceRequest::new("Demo", &['x']), Some(&fake));
        assert_eq!(m.rung, FaceRung::FamilyClass);
        let names: Vec<&str> = m
            .skipped
            .iter()
            .map(|s| s.postscript_name.as_str())
            .collect();
        assert_eq!(names, ["Demo"]);
    }

    #[test]
    fn with_no_faces_choose_names_the_standard14_floor() {
        let req =
            FaceRequest::new("Demo", &['x']).with_class(FaceClass::new().with_serif(Some(true)));
        let (picked, m) = choose(&req, None);
        assert_eq!(picked, FallbackFace::Named("Times-Roman".to_owned()));
        assert_eq!((m.rung, m.source.as_deref()), (FaceRung::Standard14, None));
        assert!(m.disclosure().contains("no program embedded"));
    }

    fn request_for(font: &str, descriptor: &str) -> FaceRequest {
        use crate::object::ObjId;
        let doc = crate::pageops::tests_support::build_pdf(&[
            (1, "<< /Type /Catalog >>"),
            (5, font),
            (6, descriptor),
        ]);
        let view = DocumentView::new(&doc, doc.bytes(), doc.version());
        let dict = view
            .value(ObjId::new(5, 0))
            .unwrap()
            .as_dict()
            .unwrap()
            .clone();
        FaceRequest::from_font_dict(&view, &dict, &['x'])
    }

    #[test]
    fn a_request_reads_its_class_and_family_from_the_descriptor() {
        let req = request_for(
            "<< /Type /Font /Subtype /TrueType /BaseFont /ABCDEF+Demo-BoldItalic \
             /FontDescriptor 6 0 R >>",
            "<< /Type /FontDescriptor /Flags 34 /ItalicAngle -12 /FontWeight 700 \
             /FontStretch /Condensed /FontFamily (Demo Serif) >>",
        );
        let want = FaceClass::new()
            .with_serif(Some(true))
            .with_italic(true)
            .with_weight(700)
            .with_width(3);
        assert_eq!(req.class, want);
        assert_eq!(req.family.as_deref(), Some("Demo Serif"));
        assert_eq!(standard14_for(&req.class), "Times-BoldItalic");
    }

    #[test]
    fn a_standard14_run_without_a_descriptor_takes_its_own_flags() {
        let req = request_for(
            "<< /Type /Font /Subtype /Type1 /BaseFont /Courier >>",
            "<< /Type /FontDescriptor >>",
        );
        assert_eq!(standard14_for(&req.class), "Courier");
    }

    #[test]
    fn a_disclosure_names_five_skips_then_counts_the_rest() {
        let faces = (0..7)
            .map(|i| face(i, &format!("R{i}"), "R").with_fs_type(bits(2)))
            .collect();
        let fake = Fake { faces, broken: &[] };
        let (_, m) = choose(&FaceRequest::new("Q", &['x']), Some(&fake));
        let line = m.disclosure();
        assert_eq!(m.skipped.len(), 7);
        assert!(line.contains("'R4'") && !line.contains("'R5'"), "{line}");
        assert!(line.ends_with(" and 2 more"), "{line}");
    }
}
