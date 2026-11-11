//! [`ColorBridges`]: every bridge one colour space can have, applied in
//! front of the space's own conversions.

use super::{IccBridge, PcsBridge};

/// Every bridge one colour space can have, resolved once and applied in
/// front of [`crate::color::ColorSpace::to_rgb`] / `to_cmyk` (`Pass 243.0`).
///
/// # Why this exists
///
/// Three Passes put three routes onto three object types one at a time —
/// `ICCBased` fills (`199.2`), `ICCBased` images (`214.0`/`240.0`), CIE
/// colours (`242.0`) — and each time the shading and mesh readers stayed
/// behind, because their colour is resolved inside `shading.rs`/`mesh.rs`
/// through a bare `ColorSpace` that knows nothing about the page's bridge
/// cache. The fill's and image's route decisions are each spelled out at
/// their own call sites; a fourth copy of that ladder in the ramp builder
/// and a fifth in the vertex reader would be the "two answers to one
/// question" this crate's colour design exists to prevent.
///
/// So the ladder is HERE, once, and the readers call [`Self::to_rgb`] and
/// [`Self::to_cmyk`] instead of the space's own. The order is the fill
/// path's order (`Interpreter::authored_cmyk`, `display_managed_rgb`):
///
/// | space | `to_rgb` | `to_cmyk` |
/// |---|---|---|
/// | `ICCBased N 3` | profile → sRGB | profile → output intent, else `None` |
/// | `ICCBased N 4` | profile → output intent → `from_cmyk`, else the space's own | profile → output intent, else `None` |
/// | `Lab` / `CalRGB` / `CalGray` | the space's own (`xyz_to_srgb`) | PCS → output intent, else `None` |
/// | everything else | the space's own | the space's own |
///
/// An empty bundle ([`Self::none`]) is the identity — every method falls
/// through to the space — which is what a document with no bridges gets,
/// so nothing about a plain `DeviceRGB` gradient moves.
#[derive(Debug, Clone, Default)]
pub struct ColorBridges {
    pub(super) display: Option<std::sync::Arc<IccBridge>>,
    pub(super) ink: Option<std::sync::Arc<IccBridge>>,
    pub(super) pcs: Option<std::sync::Arc<PcsBridge>>,
}

impl ColorBridges {
    /// No bridges: every conversion is the space's own.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// Whether any route here differs from the space's own.
    #[must_use]
    pub fn is_managed(&self) -> bool {
        self.display.is_some() || self.ink.is_some() || self.pcs.is_some()
    }

    /// The screen answer: the display bridge for an `ICCBased N 3` source,
    /// [`ink_on_screen`] for an `N 4` source with an ink bridge, the space's
    /// own otherwise.
    #[must_use]
    pub fn to_rgb(
        &self,
        space: &crate::color::ColorSpace,
        comps: &[f32],
        intent: pdfcer_core::settings::CmykIntent,
        diag: &mut crate::color::ColorDiagnostics,
    ) -> Option<crate::gstate::Rgb> {
        if let Some(bridge) = &self.display
            && let Some(rgb) = bridge.convert_to_rgb(comps)
        {
            return Some(rgb);
        }
        if space.components() == 4
            && let Some(bridge) = &self.ink
            && let Some(rgb) = ink_on_screen(bridge, comps, intent)
        {
            return Some(rgb);
        }
        space.to_rgb(comps, intent, diag)
    }

    /// The ink answer, in the fill path's ladder: the document's own
    /// `DeviceCMYK` answer first, then the profile bridge, then the PCS
    /// bridge, then `None` — which the caller bridges through `rgb_to_cmyk`
    /// exactly as before.
    #[must_use]
    pub fn to_cmyk(
        &self,
        space: &crate::color::ColorSpace,
        comps: &[f32],
        diag: &mut crate::color::ColorDiagnostics,
    ) -> Option<[f32; 4]> {
        if let Some(own) = space.to_cmyk(comps, diag) {
            return Some(own);
        }
        if let Some(bridge) = &self.ink
            && let Some(cmyk) = bridge.convert_components(comps)
        {
            return Some(cmyk);
        }
        if let Some(bridge) = &self.pcs
            && let Some(xyz) = space.to_pcs_xyz(comps)
        {
            return bridge.to_ink(xyz);
        }
        None
    }
}

/// The screen colour of a four-component `ICCBased` source: its components
/// converted through its own profile to the output intent's CMYK (ISO
/// 32000-2 §8.6.5.5; ICC.1:2022 §10.10 for a v2 `mft1`/`mft2` source), then
/// shown exactly as a `DeviceCMYK` value under that intent is shown.
///
/// The screen route must reach the ink route's answer: an EMF brush, an
/// additive raster and an ink-composited plate of one fill otherwise
/// disagree, and on a source profile that remaps its channels (C/M swapped)
/// they disagree by a whole hue. Going to the screen through the profile's
/// own sRGB colorimetry instead would be a second answer, measurably apart
/// from how the page's `DeviceCMYK` content of the same ink is shown.
pub(crate) fn ink_on_screen(
    bridge: &IccBridge,
    comps: &[f32],
    intent: pdfcer_core::settings::CmykIntent,
) -> Option<crate::gstate::Rgb> {
    let [c, m, y, k] = bridge.convert_components(comps)?;
    Some(crate::gstate::Rgb::from_cmyk(intent, c, m, y, k))
}
