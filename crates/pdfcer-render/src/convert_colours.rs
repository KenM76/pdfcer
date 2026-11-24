//! The shipped [`ColourConverter`]: icccer's chains behind pdfcer-core's
//! convert-colours verb (decision 064: icccer owns all colour arithmetic).
//!
//! Every source takes one of two routes into the destination profile:
//!
//! - `ICCBased` — a full icccer `Chain` from the embedded profile, with
//!   black point compensation (ISO 18619) when asked;
//! - `DeviceRGB` / `DeviceGray` (read as sRGB and neutral sRGB, disclosed),
//!   `Lab`, `CalRGB` and `CalGray` — the source's D50 XYZ (ISO 32000-2
//!   §8.6.5.2–§8.6.5.4) into the destination's B2A, with BPC applied in
//!   XYZ from the two black points.
//!
//! Out-of-gamut is judged by sending the converted colour back to the
//! screen (sRGB) and comparing with the source's own sRGB rendering, by
//! CIEDE2000. Both sides are clipped to sRGB, so a colour outside sRGB *and*
//! the destination can be under-counted; [`ColourTransform::note`] says so.

use icccer_cmm::bpc::BpcScale;
use icccer_cmm::transform::{Chain, Destination};
use icccer_color::{D50, Lab, Xyz};
use icccer_profile::Profile;
use pdfcer_core::color::RenderingIntent;
use pdfcer_core::prepress::{
    ColourConverter, ColourSource, ColourTransform, ConvertFailure, ConvertedColour,
};

use crate::color::ColorSpace;
use crate::icc::{Conversion, chain_for, to_icccer_intent};

/// The CIEDE2000 difference at or above which a converted colour counts as
/// out of the destination's gamut, by default.
pub const DEFAULT_GAMUT_TOLERANCE: f64 = 2.0;

/// pdfcer's colour converter: icccer's transforms, with a CIEDE2000
/// out-of-gamut test.
///
/// # Examples
///
/// ```
/// use pdfcer_render::convert_colours::IcccerConverter;
///
/// let converter = IcccerConverter::new().with_gamut_tolerance(3.0);
/// assert_eq!(converter.gamut_tolerance(), 3.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IcccerConverter {
    tolerance: f64,
}

impl Default for IcccerConverter {
    fn default() -> Self {
        Self::new()
    }
}

impl IcccerConverter {
    /// A converter with [`DEFAULT_GAMUT_TOLERANCE`].
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tolerance: DEFAULT_GAMUT_TOLERANCE,
        }
    }

    /// This converter, counting a colour out of gamut when its round trip
    /// differs by at least `delta_e` (CIEDE2000).
    #[must_use]
    pub const fn with_gamut_tolerance(mut self, delta_e: f64) -> Self {
        self.tolerance = delta_e;
        self
    }

    /// The out-of-gamut threshold, CIEDE2000.
    #[must_use]
    pub const fn gamut_tolerance(&self) -> f64 {
        self.tolerance
    }
}

impl ColourConverter for IcccerConverter {
    fn transform(
        &self,
        source: &ColourSource,
        destination: &[u8],
        intent: RenderingIntent,
        bpc: bool,
    ) -> Result<Box<dyn ColourTransform>, ConvertFailure> {
        let dst = Profile::parse(destination)
            .map_err(|e| ConvertFailure::new(format!("the destination profile: {e}")))?;
        let conv = Conversion::new(intent, bpc);
        let back = Chain::with_destination(
            &dst,
            Destination::None,
            to_icccer_intent(RenderingIntent::RelativeColorimetric),
        )
        .map_err(|e| ConvertFailure::new(format!("the destination profile: {e}")))?;
        let route = match source {
            ColourSource::IccBased { profile, .. } => icc_route(profile, &dst, conv)?,
            other => pcs_route(other, &dst, conv)?,
        };
        Ok(Box::new(Transform {
            route,
            back,
            tolerance: self.tolerance,
        }))
    }
}

/// One built conversion.
struct Transform {
    route: Route,
    /// Destination device → sRGB, for the gamut test.
    back: Chain,
    tolerance: f64,
}

enum Route {
    Icc {
        chain: Chain,
        /// Source device → sRGB, the reference for the gamut test.
        screen: Chain,
    },
    Pcs {
        space: PcsSource,
        chain: Chain,
        bpc: Option<BpcScale>,
    },
}

/// A source that reaches the PCS without a profile.
enum PcsSource {
    Srgb { gray: bool },
    Calibrated(ColorSpace),
}

impl PcsSource {
    fn xyz(&self, input: &[f64]) -> Option<Xyz> {
        match self {
            Self::Srgb { gray } => {
                let rgb = if *gray {
                    let g = *input.first()?;
                    [g, g, g]
                } else {
                    [*input.first()?, *input.get(1)?, *input.get(2)?]
                };
                Some(icccer_cmm::builtin::srgb().device_to_pcs(rgb))
            }
            Self::Calibrated(space) => {
                let comps: Vec<f32> = input.iter().map(|v| *v as f32).collect();
                let [x, y, z] = space.to_pcs_xyz(&comps)?;
                Some(Xyz {
                    x: f64::from(x),
                    y: f64::from(y),
                    z: f64::from(z),
                })
            }
        }
    }
}

fn icc_route(profile: &[u8], dst: &Profile, conv: Conversion) -> Result<Route, ConvertFailure> {
    let src = Profile::parse(profile)
        .map_err(|e| ConvertFailure::new(format!("the embedded profile: {e}")))?;
    let chain = chain_for(conv, |i| Chain::new(&src, dst, i))
        .ok_or_else(|| ConvertFailure::new("icccer cannot build a transform from this profile"))?;
    let screen = Chain::with_destination(
        &src,
        Destination::None,
        to_icccer_intent(RenderingIntent::RelativeColorimetric),
    )
    .map_err(|e| ConvertFailure::new(format!("the embedded profile: {e}")))?;
    Ok(Route::Icc { chain, screen })
}

fn pcs_route(
    source: &ColourSource,
    dst: &Profile,
    conv: Conversion,
) -> Result<Route, ConvertFailure> {
    let space = pcs_source(source)?;
    let intent = to_icccer_intent(conv.intent);
    let chain = Chain::new(dst, dst, intent)
        .map_err(|e| ConvertFailure::new(format!("the destination profile: {e}")))?;
    let bpc = if conv.bpc() {
        let zeros = vec![0.0; source.components()];
        let black = match source {
            // Lab's black is L* = 0, whatever a and b say.
            ColourSource::Lab { .. } => Some(Xyz {
                x: 0.0,
                y: 0.0,
                z: 0.0,
            }),
            _ => space.xyz(&zeros),
        };
        let dst_black = Chain::new(dst, dst, intent)
            .and_then(Chain::with_bpc)
            .ok()
            .and_then(|c| c.bpc_disclosure())
            .map(|s| s.destination_black());
        black.zip(dst_black).and_then(|(s, d)| BpcScale::new(s, d))
    } else {
        None
    };
    Ok(Route::Pcs { space, chain, bpc })
}

fn pcs_source(source: &ColourSource) -> Result<PcsSource, ConvertFailure> {
    let f = |v: [f64; 3]| v.map(|c| c as f32);
    Ok(match source {
        ColourSource::DeviceRgb => PcsSource::Srgb { gray: false },
        ColourSource::DeviceGray => PcsSource::Srgb { gray: true },
        ColourSource::Lab { white, range } => PcsSource::Calibrated(ColorSpace::Lab {
            white: f(*white),
            range: range.map(|c| c as f32),
        }),
        ColourSource::CalRgb {
            white,
            gamma,
            matrix,
        } => PcsSource::Calibrated(ColorSpace::CalRgb {
            white: f(*white),
            gamma: f(*gamma),
            matrix: matrix.map(|c| c as f32),
        }),
        ColourSource::CalGray { white, gamma } => PcsSource::Calibrated(ColorSpace::CalGray {
            white: f(*white),
            gamma: *gamma as f32,
        }),
        _ => {
            return Err(ConvertFailure::new(
                "a source space this converter does not know",
            ));
        }
    })
}

impl ColourTransform for Transform {
    fn convert(&self, input: &[f64]) -> Option<ConvertedColour> {
        let (out, reference) = match &self.route {
            Route::Icc { chain, screen } => {
                let out = chain.convert(input).ok()?;
                let reference = screen.convert(input).ok().and_then(|rgb| srgb_xyz(&rgb));
                (out, reference)
            }
            Route::Pcs { space, chain, bpc } => {
                let xyz = space.xyz(input)?;
                let mapped = bpc.as_ref().map_or(xyz, |s| s.apply(xyz));
                let out = chain.pcs_to_destination(mapped).ok()?;
                (out, screen_clip(xyz))
            }
        };
        let back = self.back.convert(&out).ok().and_then(|rgb| srgb_xyz(&rgb));
        let out_of_gamut = match (reference, back) {
            (Some(r), Some(b)) => {
                icccer_color::delta_e_2000(Lab::from_xyz(r, D50), Lab::from_xyz(b, D50))
                    >= self.tolerance
            }
            _ => false,
        };
        Some(ConvertedColour::new(out, out_of_gamut))
    }

    fn note(&self) -> Option<String> {
        let gamut = "out-of-gamut counts compare the colour before and after on screen (sRGB, \
                     CIEDE2000), so a colour outside sRGB as well as the destination can be \
                     under-counted";
        Some(match &self.route {
            Route::Pcs {
                space: PcsSource::Srgb { gray: false },
                ..
            } => format!("DeviceRGB was read as sRGB; {gamut}"),
            Route::Pcs {
                space: PcsSource::Srgb { gray: true },
                ..
            } => format!("DeviceGray was read as neutral sRGB; {gamut}"),
            _ => gamut.to_owned(),
        })
    }
}

/// D50 XYZ of an encoded sRGB triple.
fn srgb_xyz(rgb: &[f64]) -> Option<Xyz> {
    let [r, g, b] = [*rgb.first()?, *rgb.get(1)?, *rgb.get(2)?];
    Some(icccer_cmm::builtin::srgb().device_to_pcs([r, g, b]))
}

/// `xyz` clipped into sRGB, as the screen would show it.
fn screen_clip(xyz: Xyz) -> Option<Xyz> {
    let srgb = icccer_cmm::builtin::srgb();
    let rgb = srgb.pcs_to_device(xyz).ok()?;
    Some(srgb.device_to_pcs(rgb))
}
