//! Print a page raster into a printer's ICC profile.
//!
//! Applied where [`crate::output_color`] is: after the page is rendered and
//! flattened, before the raster reaches the driver. The source is the
//! raster's sRGB; the destination is an RGB or grey output (`prtr`) or
//! display (`mntr`) profile. Every conversion is icccer's (decision 064):
//! this module parses, validates, builds the chain and moves bytes.
//!
//! # What it refuses, and why
//!
//! - **CMYK and n-colour destinations.** The raster route hands the driver
//!   RGB; a four-ink result has nowhere to go. Use the driver's own colour
//!   management for a CMYK printer.
//! - **Input (`scnr`), device-link, abstract, colour-space and named-colour
//!   classes.** None describes a device the page prints on.
//! - **Files over [`MAX_PROFILE_BYTES`]**, refused from the file length
//!   before the contents are read.

use std::collections::HashMap;
use std::io::Read as _;
use std::path::Path;

/// The largest profile file [`PrinterProfile::read`] accepts: 16 MiB. Real
/// RGB printer profiles with full A2B/B2A tables are 1–4 MB.
pub const MAX_PROFILE_BYTES: u64 = 16 * 1024 * 1024;

/// Why a profile cannot be used as a print destination.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProfileError {
    /// The bytes are not an ICC profile icccer can parse.
    #[error("not a usable ICC profile: {0}")]
    NotAProfile(String),
    /// The profile's device space is CMYK; the raster route is RGB.
    #[error(
        "CMYK printer profiles are not supported on the raster print route; leave colour to the printer driver"
    )]
    Cmyk,
    /// The device space is neither RGB nor grey (n-colour, Lab, ...).
    #[error("profile device space '{0}' is not RGB or grey")]
    UnsupportedSpace(String),
    /// The class is not output or display (input, device link, abstract...).
    #[error(
        "profile class '{0}' cannot be a print destination (need an output or display profile)"
    )]
    UnsupportedClass(String),
    /// The file exceeds [`MAX_PROFILE_BYTES`].
    #[error("profile file is {size} bytes, over the {limit}-byte limit")]
    TooLarge {
        /// The file's length.
        size: u64,
        /// [`MAX_PROFILE_BYTES`].
        limit: u64,
    },
    /// icccer could not build a transform into this profile.
    #[error("cannot build a colour transform into this profile: {0}")]
    Transform(String),
    /// The file could not be read.
    #[error("cannot read profile: {0}")]
    Io(#[from] std::io::Error),
}

/// A profile's device colour space, from header offset 16 (ICC.1:2022 §7.2.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProfileSpace {
    /// `GRAY`.
    Gray,
    /// `RGB `.
    Rgb,
    /// `CMYK`.
    Cmyk,
    /// Anything else, as its four-byte signature.
    Other([u8; 4]),
}

impl ProfileSpace {
    fn from_signature(sig: u32) -> Self {
        match &sig.to_be_bytes() {
            b"GRAY" => Self::Gray,
            b"RGB " => Self::Rgb,
            b"CMYK" => Self::Cmyk,
            other => Self::Other(*other),
        }
    }
}

/// The four header facts the print route judges a profile by.
pub(crate) struct HeaderFacts {
    pub(crate) space: ProfileSpace,
    pub(crate) class: [u8; 4],
}

impl HeaderFacts {
    pub(crate) fn of(profile: &icccer_profile::Profile) -> Self {
        Self {
            space: ProfileSpace::from_signature(profile.header.color_space.0),
            class: profile.header.device_class.0.to_be_bytes(),
        }
    }

    /// RGB or grey, output or display class.
    pub(crate) fn is_print_destination(&self) -> bool {
        matches!(self.space, ProfileSpace::Rgb | ProfileSpace::Gray)
            && matches!(&self.class, b"prtr" | b"mntr")
    }
}

fn sig_text(sig: [u8; 4]) -> String {
    String::from_utf8_lossy(&sig).trim_end().to_owned()
}

/// The profile's description (`desc` tag; `mluc` in v4, `textDescription`
/// in v2). Empty when absent or undecodable.
pub(crate) fn description(profile: &icccer_profile::Profile) -> String {
    use icccer_profile::tag_types::TagData;
    let Some(entry) = profile
        .tags
        .iter()
        .find(|t| &t.sig.0.to_be_bytes() == b"desc")
    else {
        return String::new();
    };
    let text = match profile.decode_tag(entry) {
        Some(Ok(tag)) => match tag.data {
            TagData::Mluc(m) => m.records.first().and_then(|r| r.to_string_lossy()),
            TagData::TextDescription(d) => Some(String::from_utf8_lossy(&d.ascii).into_owned()),
            TagData::Text(t) => Some(t.to_string_lossy()),
            _ => None,
        },
        _ => None,
    };
    text.unwrap_or_default()
        .trim_end_matches('\0')
        .trim()
        .to_owned()
}

/// Read at most [`MAX_PROFILE_BYTES`] from `path`, refusing a longer file
/// from its length before reading it.
pub(crate) fn read_capped(path: &Path) -> Result<Vec<u8>, ProfileError> {
    let file = std::fs::File::open(path)?;
    let size = file.metadata()?.len();
    if size > MAX_PROFILE_BYTES {
        return Err(ProfileError::TooLarge {
            size,
            limit: MAX_PROFILE_BYTES,
        });
    }
    let mut bytes = Vec::new();
    // `take` bounds a file that grows between the length check and the read.
    file.take(MAX_PROFILE_BYTES + 1).read_to_end(&mut bytes)?;
    let read = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if read > MAX_PROFILE_BYTES {
        return Err(ProfileError::TooLarge {
            size: read,
            limit: MAX_PROFILE_BYTES,
        });
    }
    Ok(bytes)
}

/// An RGB or grey printer (or display) profile, parsed and checked once.
#[derive(Debug, Clone)]
pub struct PrinterProfile {
    profile: icccer_profile::Profile,
    name: String,
    space: ProfileSpace,
}

impl PrinterProfile {
    /// Parse and check a profile. Refuses CMYK, n-colour, non-output classes
    /// and non-ICC bytes with a named [`ProfileError`]; never panics.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ProfileError> {
        let profile = icccer_profile::Profile::parse(bytes)
            .map_err(|e| ProfileError::NotAProfile(e.to_string()))?;
        let facts = HeaderFacts::of(&profile);
        match facts.space {
            ProfileSpace::Rgb | ProfileSpace::Gray => {}
            ProfileSpace::Cmyk => return Err(ProfileError::Cmyk),
            ProfileSpace::Other(sig) => return Err(ProfileError::UnsupportedSpace(sig_text(sig))),
        }
        if !facts.is_print_destination() {
            return Err(ProfileError::UnsupportedClass(sig_text(facts.class)));
        }
        let name = description(&profile);
        let checked = Self {
            profile,
            name,
            space: facts.space,
        };
        // Build once now so a profile icccer cannot convert into is refused
        // when it is chosen, not part-way through a print job.
        checked.chain(RenderingIntent::RelativeColorimetric, false)?;
        Ok(checked)
    }

    /// Read and check a profile file, refusing one over
    /// [`MAX_PROFILE_BYTES`] before reading it.
    pub fn read(path: &Path) -> Result<Self, ProfileError> {
        Self::from_bytes(&read_capped(path)?)
    }

    /// The profile's description; empty if it carries none.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// RGB or grey.
    #[must_use]
    pub fn space(&self) -> ProfileSpace {
        self.space
    }

    fn chain(
        &self,
        intent: RenderingIntent,
        bpc: bool,
    ) -> Result<icccer_cmm::transform::Chain, ProfileError> {
        let src = icccer_profile::Profile::parse(&crate::icc_encode::srgb_source())
            .map_err(|e| ProfileError::Transform(e.to_string()))?;
        let chain = icccer_cmm::transform::Chain::new(&src, &self.profile, intent.to_icccer())
            .map_err(|e| ProfileError::Transform(e.to_string()))?;
        // ISO 32000-2 §8.6.5.9 treats BPC as off for absolute colorimetric,
        // and icccer refuses it there.
        if !bpc || intent == RenderingIntent::AbsoluteColorimetric {
            return Ok(chain);
        }
        chain
            .with_bpc()
            .map_err(|e| ProfileError::Transform(e.to_string()))
    }
}

/// The ICC rendering intents (ICC.1:2022 §6.2.2).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RenderingIntent {
    /// Perceptual.
    Perceptual,
    /// Media-relative colorimetric — the default, as for a PDF's `/RI`.
    #[default]
    RelativeColorimetric,
    /// Saturation.
    Saturation,
    /// ICC-absolute colorimetric. Black point compensation does not apply.
    AbsoluteColorimetric,
}

impl RenderingIntent {
    fn to_icccer(self) -> icccer_cmm::matrix_trc::Intent {
        use icccer_cmm::matrix_trc::Intent;
        match self {
            Self::Perceptual => Intent::Perceptual,
            Self::RelativeColorimetric => Intent::MediaRelative,
            Self::Saturation => Intent::Saturation,
            Self::AbsoluteColorimetric => Intent::Absolute,
        }
    }
}

/// What to convert the printed raster into.
#[derive(Debug, Clone)]
pub struct PrinterColour {
    /// The destination profile.
    pub profile: PrinterProfile,
    /// The rendering intent; relative colorimetric by default.
    pub intent: RenderingIntent,
    /// Black point compensation (ISO 18619, via icccer); `true` by default
    /// in [`PrinterColour::new`]. Ignored under absolute colorimetric.
    pub black_point_compensation: bool,
}

impl PrinterColour {
    /// Relative colorimetric with black point compensation.
    #[must_use]
    pub fn new(profile: PrinterProfile) -> Self {
        Self {
            profile,
            intent: RenderingIntent::default(),
            black_point_compensation: true,
        }
    }

    /// Build the transform. Do this once per job and reuse it per page.
    pub fn compile(&self) -> Result<PrinterTransform, ProfileError> {
        let chain = self
            .profile
            .chain(self.intent, self.black_point_compensation)?;
        Ok(PrinterTransform {
            grey: self.profile.space == ProfileSpace::Gray,
            chain,
        })
    }
}

/// A built sRGB → printer transform. See [`PrinterColour::compile`].
pub struct PrinterTransform {
    chain: icccer_cmm::transform::Chain,
    grey: bool,
}

impl std::fmt::Debug for PrinterTransform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrinterTransform")
            .field("grey", &self.grey)
            .finish_non_exhaustive()
    }
}

/// Distinct colours remembered per [`PrinterTransform::apply`] call before
/// the memo is cleared; bounds its memory at a few tens of MB.
const MEMO_LIMIT: usize = 1 << 20;

impl PrinterTransform {
    /// Convert `rgba` (RGBA8, premultiplied, as `tiny_skia` renders) in
    /// place. Alpha is kept; a grey destination's one channel is written to
    /// R, G and B. Returns how many pixels changed.
    ///
    /// Each distinct colour goes through icccer's chain once per call — exact,
    /// not a sampled grid — which keeps the cost proportional to the page's
    /// distinct colours rather than its pixels.
    pub fn apply(&self, rgba: &mut [u8]) -> usize {
        let mut memo: HashMap<[u8; 3], [u8; 3]> = HashMap::new();
        let mut changed = 0;
        for px in rgba.chunks_exact_mut(4) {
            let Ok(px): Result<&mut [u8; 4], _> = px.try_into() else {
                continue;
            };
            let [r, g, b, a] = *px;
            if a == 0 {
                continue;
            }
            let straight = [r, g, b].map(|c| unpremultiply(c, a));
            if memo.len() >= MEMO_LIMIT {
                memo.clear();
            }
            let out = *memo
                .entry(straight)
                .or_insert_with(|| self.convert(straight));
            let next = [
                premultiply(out[0], a),
                premultiply(out[1], a),
                premultiply(out[2], a),
                a,
            ];
            if next != *px {
                *px = next;
                changed += 1;
            }
        }
        changed
    }

    /// One straight-alpha sRGB colour into the printer's device values.
    /// A chain failure leaves the colour as it was.
    fn convert(&self, rgb: [u8; 3]) -> [u8; 3] {
        let input = rgb.map(|c| f64::from(c) / 255.0);
        let Ok(out) = self.chain.convert(&input) else {
            return rgb;
        };
        let byte = |i: usize| to_byte(out.get(i).copied().unwrap_or(0.0));
        if self.grey {
            let y = byte(0);
            [y, y, y]
        } else {
            [byte(0), byte(1), byte(2)]
        }
    }
}

/// Convert `rgba` into `colour`'s profile in place, building the transform
/// for this call. For a multi-page job, [`PrinterColour::compile`] once and
/// call [`PrinterTransform::apply`] per page instead.
pub fn apply_printer_colour(
    rgba: &mut [u8],
    colour: &PrinterColour,
) -> Result<usize, ProfileError> {
    Ok(colour.compile()?.apply(rgba))
}

fn to_byte(v: f64) -> u8 {
    // Clamped to 0..=255 first, so the cast is exact.
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let b = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    b
}

fn unpremultiply(c: u8, a: u8) -> u8 {
    if a == 255 {
        return c;
    }
    let v = (u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a);
    u8::try_from(v.min(255)).unwrap_or(255)
}

fn premultiply(c: u8, a: u8) -> u8 {
    if a == 255 {
        return c;
    }
    let v = (u32::from(c) * u32::from(a) + 127) / 255;
    u8::try_from(v).unwrap_or(a)
}

#[cfg(test)]
mod tests;
