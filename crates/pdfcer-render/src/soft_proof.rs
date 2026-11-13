//! Soft proofing ([`SoftProof`]): the separation preview's ink shown as a
//! chosen CMYK output profile prints it, instead of through the fixed
//! process-ink model.
//!
//! The profile does two jobs. Colour that is not already CMYK separates into
//! *its* ink (it replaces the output intent as the bridge destination), and
//! the composited ink is converted to screen through it (ICC.1:2022 §6.3, a
//! device-to-PCS-to-sRGB chain). Spot inks are not proofed: they fold in
//! after, exactly as the unproofed preview folds them.
//!
//! The two switches are Acrobat Output Preview's *Simulate Paper Color* and
//! *Simulate Black Ink*:
//!
//! | switches | intent (ICC.1:2022 §6.2) | black point compensation |
//! |---|---|---|
//! | neither | media-relative colorimetric | on (ISO 18619) |
//! | black ink | media-relative colorimetric | off |
//! | paper colour (implies black ink) | ICC-absolute colorimetric | off |
//!
//! So with neither the press's black and paper map to the screen's, black
//! ink alone shows the press's black as grey, and paper colour also shows
//! the sheet's white as the profile measured it.

use std::sync::{Arc, OnceLock};

use icccer_cmm::compiled::CompiledTransform;
use icccer_cmm::matrix_trc::Intent;
use icccer_cmm::transform::Chain;
use icccer_profile::Profile;

/// The largest profile [`SoftProof::from_bytes`] reads: 16 MiB, the cap
/// the print pipeline uses for an output profile.
pub const MAX_SOFT_PROOF_PROFILE_BYTES: usize = 16 * 1024 * 1024;

/// Grid points per axis of the compiled CMYK-to-sRGB transform: 17⁴ = 83 521
/// chain evaluations, once per mode per [`SoftProof`] value.
const GRID_POINTS: usize = 17;

/// A CMYK output profile to proof the separation preview against.
///
/// Attach with [`crate::SeparationPreview::with_soft_proof`]. Cloning is
/// cheap and shares the compiled transforms, so a viewer that keeps one
/// value and changes only the switches pays the compile once per mode.
///
/// # Example
///
/// ```
/// use pdfcer_render::{RenderOptions, SeparationPreview, SoftProof};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let icc = std::fs::read(concat!(
///     env!("CARGO_MANIFEST_DIR"),
///     "/../../fixtures/synthetic/icc-rgb/dest-cmyk.icc"
/// ))?;
/// let proof = SoftProof::from_bytes(icc)?.with_paper_colour(true);
/// assert!(proof.simulates_black_ink());
/// let options = RenderOptions::default()
///     .with_separation_preview(Some(SeparationPreview::new().with_soft_proof(Some(proof))));
/// # let _ = options;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct SoftProof {
    profile: Arc<[u8]>,
    simulate_paper: bool,
    simulate_black_ink: bool,
    transforms: Arc<ProofTransforms>,
}

/// One lazily compiled transform per mode, `None` once a mode failed.
#[derive(Debug, Default)]
struct ProofTransforms {
    compensated: OnceLock<Option<(CompiledTransform, bool)>>,
    relative: OnceLock<Option<CompiledTransform>>,
    absolute: OnceLock<Option<CompiledTransform>>,
}

/// Why [`SoftProof::from_bytes`] refused a profile. Every message reads as
/// a sentence a settings panel can show as it is.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SoftProofError {
    /// Larger than [`MAX_SOFT_PROOF_PROFILE_BYTES`].
    #[error("the profile is {bytes} bytes, larger than the 16 MiB a proof profile may be")]
    TooLarge {
        /// The profile's size.
        bytes: usize,
    },
    /// Not a profile ICC.1 can read.
    #[error("the file is not a readable ICC profile: {0}")]
    Unreadable(String),
    /// Its colour space is not CMYK.
    #[error("the profile is for {found} colour, not CMYK")]
    NotCmyk {
        /// The header's colour space signature (`RGB `, `GRAY`, …).
        found: String,
    },
    /// Its class is not an output (printer) profile.
    #[error("the profile is a {found} profile, not an output (printer) profile")]
    NotOutput {
        /// The header's device class signature (`mntr`, `link`, …).
        found: String,
    },
    /// No transform from it to the screen can be built.
    #[error("the profile cannot be converted to the screen: {0}")]
    NoTransform(String),
}

impl SoftProof {
    /// Read a CMYK output profile, both switches off.
    ///
    /// # Errors
    ///
    /// [`SoftProofError`]: too large, unreadable, not CMYK, not an output
    /// (`prtr`) profile, or no media-relative transform to sRGB.
    pub fn from_bytes(bytes: impl Into<Arc<[u8]>>) -> Result<Self, SoftProofError> {
        let profile: Arc<[u8]> = bytes.into();
        if profile.len() > MAX_SOFT_PROOF_PROFILE_BYTES {
            return Err(SoftProofError::TooLarge {
                bytes: profile.len(),
            });
        }
        let parsed =
            Profile::parse(&profile).map_err(|err| SoftProofError::Unreadable(err.to_string()))?;
        let space = parsed.header.color_space.0.to_be_bytes();
        if &space != b"CMYK" {
            return Err(SoftProofError::NotCmyk {
                found: signature_text(space),
            });
        }
        let class = parsed.header.device_class.0.to_be_bytes();
        if &class != b"prtr" {
            return Err(SoftProofError::NotOutput {
                found: signature_text(class),
            });
        }
        chain(&parsed, Intent::MediaRelative).map_err(SoftProofError::NoTransform)?;
        Ok(Self {
            profile,
            simulate_paper: false,
            simulate_black_ink: false,
            transforms: Arc::default(),
        })
    }

    /// The document's own output intent as a proof profile: the
    /// destination profile of the intent the renderer already separates
    /// into (ISO 32000-2 §14.11.5), so a viewer can offer it beside the
    /// installed profiles. `None` when the document has no usable intent.
    ///
    /// # Errors
    ///
    /// As [`Self::from_bytes`], inside the `Some`: an intent profile that
    /// is not a CMYK output profile (an RGB intent, say) cannot proof.
    #[must_use]
    pub fn from_output_intent(
        doc: &crate::DocumentView<'_>,
    ) -> Option<Result<Self, SoftProofError>> {
        crate::interpret::output_intent_profile(doc).map(Self::from_bytes)
    }

    /// Show the paper's white as the profile measured it. Implies
    /// [`Self::with_black_ink`].
    #[must_use]
    pub fn with_paper_colour(mut self, on: bool) -> Self {
        self.simulate_paper = on;
        self
    }

    /// Show the press's black as the profile measured it, not as the
    /// screen's black.
    #[must_use]
    pub fn with_black_ink(mut self, on: bool) -> Self {
        self.simulate_black_ink = on;
        self
    }

    /// The profile's bytes.
    #[must_use]
    pub const fn profile(&self) -> &Arc<[u8]> {
        &self.profile
    }

    /// Whether the paper colour is simulated.
    #[must_use]
    pub const fn simulates_paper_colour(&self) -> bool {
        self.simulate_paper
    }

    /// Whether the black ink is simulated (always, when the paper is).
    #[must_use]
    pub const fn simulates_black_ink(&self) -> bool {
        self.simulate_black_ink || self.simulate_paper
    }

    /// Whether, with both switches off, black point compensation is
    /// applied. `false` when the colour engine cannot estimate this
    /// profile's black (a LUT-based profile at media-relative intent):
    /// the proof is then media-relative without it, which looks the same
    /// as [`Self::with_black_ink`], and a caller showing the proof should
    /// say so.
    #[must_use]
    pub fn compensates_black(&self) -> bool {
        self.compensated().is_some_and(|(_, bpc)| *bpc)
    }

    /// The compiled CMYK (`0.0..=1.0`) to sRGB transform for the current
    /// switches, or `None` if it cannot be built.
    pub(crate) fn transform(&self) -> Option<&CompiledTransform> {
        if self.simulate_paper {
            self.compiled(&self.transforms.absolute, Intent::Absolute)
        } else if self.simulate_black_ink {
            self.compiled(&self.transforms.relative, Intent::MediaRelative)
        } else {
            self.compensated().map(|(t, _)| t)
        }
    }

    fn compiled<'s>(
        &self,
        slot: &'s OnceLock<Option<CompiledTransform>>,
        intent: Intent,
    ) -> Option<&'s CompiledTransform> {
        slot.get_or_init(|| {
            let parsed = Profile::parse(&self.profile).ok()?;
            CompiledTransform::new(&chain(&parsed, intent).ok()?, GRID_POINTS).ok()
        })
        .as_ref()
    }

    /// Media-relative with black point compensation, or without it when it
    /// is refused; the flag says which.
    fn compensated(&self) -> Option<&(CompiledTransform, bool)> {
        self.transforms
            .compensated
            .get_or_init(|| {
                let parsed = Profile::parse(&self.profile).ok()?;
                let base = chain(&parsed, Intent::MediaRelative).ok()?;
                let (chain, bpc) = match base.clone().with_bpc() {
                    Ok(with) => (with, true),
                    Err(_) => (base, false),
                };
                Some((CompiledTransform::new(&chain, GRID_POINTS).ok()?, bpc))
            })
            .as_ref()
    }

    /// `cmyk` (`0.0..=1.0` per ink) as the profile prints it, in sRGB.
    pub(crate) fn to_srgb(transform: &CompiledTransform, cmyk: [f32; 4]) -> [f32; 3] {
        let input = cmyk.map(f64::from);
        let mut out = [0.0_f64; 3];
        if !transform.convert(&input, &mut out) {
            return [0.0; 3];
        }
        #[allow(clippy::cast_possible_truncation)] // An sRGB channel in 0..=1.
        out.map(|v| v.clamp(0.0, 1.0) as f32)
    }
}

impl PartialEq for SoftProof {
    fn eq(&self, other: &Self) -> bool {
        self.profile == other.profile
            && self.simulate_paper == other.simulate_paper
            && self.simulate_black_ink == other.simulate_black_ink
    }
}

impl Eq for SoftProof {}

impl std::hash::Hash for SoftProof {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.profile.hash(state);
        self.simulate_paper.hash(state);
        self.simulate_black_ink.hash(state);
    }
}

/// A chain from `profile` to icccer's built-in sRGB under `intent`.
fn chain(profile: &Profile, intent: Intent) -> Result<Chain, String> {
    Chain::with_destination(profile, icccer_cmm::transform::Destination::None, intent)
        .map_err(|err| err.to_string())
}

/// A header signature as text, trailing padding trimmed.
fn signature_text(sig: [u8; 4]) -> String {
    String::from_utf8_lossy(&sig).trim_end().to_owned()
}
