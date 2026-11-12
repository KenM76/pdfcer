//! What a soft mask was built from, kept beside the export recording so a
//! writer can rebuild the mask as geometry instead of sampling its raster.

use std::sync::Arc;

use pdfcer_core::function::PdfFunction;

use crate::display_list::Op;

/// The §11.6.5 soft-mask dictionary behind one recorded mask, in the
/// terms the raster used to build it.
///
/// The mask value at a point is, for a luminosity mask,
/// `transfer(lum((1 − α_g)·backdrop + α_g·C_g))` (§11.5.3, with
/// `lum = 0.30 R + 0.59 G + 0.11 B`, NOTE 3), and for an alpha mask
/// `transfer(α_g)` (§11.5.2), where `C_g`, `α_g` are what the mask group
/// composites to there. Outside the group's `/BBox` the group paints
/// nothing, so the value is `transfer(lum(backdrop))` or `transfer(0)`
/// (§11.6.5.2), which this model gives without a special case.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct MaskSource {
    /// The mask group's content, in page device space, its clips in the
    /// recording's own clip table. The `/BBox` is one of those clips.
    pub ops: Vec<Op>,
    /// `/S /Luminosity` rather than `/S /Alpha`.
    pub luminosity: bool,
    /// `/BC` as sRGB in `0..=1`, black when absent. Meaningful only for a
    /// luminosity mask (Table 144).
    pub backdrop: [f32; 3],
    /// `/TR` when it is a one-output function; `None` is `/Identity`.
    pub transfer: Option<Arc<PdfFunction>>,
    /// The mask group blends in a subtractive space (§11.3.4).
    pub subtractive: bool,
}

impl MaskSource {
    /// `/TR` applied to one mask value (Table 144: last, once, to a
    /// single scalar), the result clamped to `0..=1`. A function that
    /// fails to evaluate leaves the value as it was.
    #[must_use]
    pub fn transfer(&self, v: f32) -> f32 {
        apply_transfer(self.transfer.as_deref(), v)
    }
}

/// `/TR` applied to `v`, clamped to `0..=1`.
pub(crate) fn apply_transfer(f: Option<&PdfFunction>, v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    let Some(f) = f else { return v };
    #[allow(clippy::cast_possible_truncation)] // Clamped to 0..=1 first.
    f.eval(&[f64::from(v)])
        .ok()
        .and_then(|out| out.first().copied())
        .map_or(v, |o| o.clamp(0.0, 1.0) as f32)
}

/// `/TR` as a 256-entry table over 8-bit mask values, so a page-sized
/// mask evaluates the function 256 times rather than once per pixel.
pub(crate) fn transfer_table(f: &PdfFunction) -> [u8; 256] {
    let mut table = [0u8; 256];
    for (i, slot) in table.iter_mut().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let v = apply_transfer(Some(f), i as f32 / 255.0);
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        {
            *slot = (v * 255.0 + 0.5) as u8;
        }
    }
    table
}
