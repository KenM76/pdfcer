//! A shading recorded for replay: `sh` (§8.7.4.3) and a shading pattern
//! used as a fill (§8.7.3.3).
//!
//! A shading reads no backdrop: each destination pixel's colour is a
//! function of where it lands in shading space, then source-over. So the
//! recording keeps the shading and its anchoring, and replay calls the same
//! painter a direct render calls, with the transform moved into the
//! replay target's device space exactly as a fill's is
//! ([`MaskBuilder::to_region`]).
//!
//! The one thing replay cannot share is the page's work ceiling: a mesh is
//! metered against a fresh [`crate::page_budget::PageBudget`] sized to the
//! recorded canvas, so a page whose direct render cut a mesh at the
//! coverage ceiling replays that mesh whole.

use std::sync::Arc;

use tiny_skia::{FillRule, Mask, Path, Pixmap, Transform};

use super::{ClipId, DeviceBounds, MaskBuilder};
use crate::device_clip::FitMask;

/// One recorded shading paint.
#[derive(Debug)]
#[doc(hidden)]
pub struct ShadingOp {
    /// The resolved shading, ramp or mesh included.
    pub(crate) shading: Arc<crate::shading::Shading>,
    /// Shading space → page-device space at the list's scale.
    pub(crate) to_device: Transform,
    /// The page-device pixels the painter walks: `(left, top, right,
    /// bottom)`, right and bottom exclusive.
    pub(crate) region: (i32, i32, i32, i32),
    /// The clip in force, if any.
    pub(crate) clip: Option<ClipId>,
    /// Constant alpha, `/ca` or `/CA`.
    pub(crate) alpha: f32,
    /// A pattern fill's paint area; `None` for `sh`, which paints the clip.
    pub(crate) area: Option<ShadingArea>,
    /// Pixels in the recording canvas, which sets the mesh meter's ceiling.
    pub(crate) canvas_pixels: u64,
}

/// The path a shading pattern fills, intersected with the clip at replay.
#[derive(Debug)]
#[doc(hidden)]
pub struct ShadingArea {
    /// The path, in the space `ctm` maps to page-device space.
    pub(crate) path: Arc<Path>,
    /// Nonzero or even-odd.
    pub(crate) rule: FillRule,
    /// Path space → page-device space.
    pub(crate) ctm: Transform,
    /// The interpreter's edge anti-aliasing, as the direct mask used.
    pub(crate) anti_alias: bool,
}

impl ShadingOp {
    /// Device bounds for the replay cull.
    pub(crate) fn bounds(&self) -> DeviceBounds {
        let (l, t, r, b) = self.region;
        #[allow(clippy::cast_precision_loss)] // device pixel coordinates
        DeviceBounds {
            left: l as f32,
            top: t as f32,
            right: r as f32,
            bottom: b as f32,
        }
    }

    /// Approximate heap footprint.
    pub(crate) fn bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + std::mem::size_of::<crate::shading::Shading>()
            + self.area.as_ref().map_or(0, |a| super::path_bytes(&a.path))
    }
}

/// Paint `op` into `pixmap`, whose device space `masks` describes.
pub(super) fn replay(op: &ShadingOp, pixmap: &mut Pixmap, masks: &mut MaskBuilder<'_>) {
    #[allow(clippy::cast_precision_loss)] // a row count
    let row0 = crate::device_clip::row_origin() as f32;
    let Some(to_target) = masks
        .to_region(op.to_device)
        .post_translate(0.0, -row0)
        .invert()
    else {
        return;
    };
    let clip = masks.mask_for(op.clip);
    let area = match &op.area {
        Some(a) => {
            let Some(m) = area_mask(a, clip.as_deref(), masks) else {
                return;
            };
            Some(m)
        }
        None => None,
    };
    let mask = area.as_ref().or(clip.as_deref());
    let (x0, y0) = masks.origin();
    let (l, t, r, b) = op.region;
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let region = (
        (l as f32 - x0).floor() as i32,
        (t as f32 - y0 - row0).floor() as i32,
        (r as f32 - x0).ceil() as i32,
        (b as f32 - y0 - row0).ceil() as i32,
    );
    let budget = crate::page_budget::PageBudget::default();
    let meter = crate::page_budget::MeshMeter::new(&budget, op.canvas_pixels);
    let _ = op
        .shading
        .paint_metered(to_target, region, mask, op.alpha, pixmap, meter);
}

/// `tiling::pattern_area_mask` at replay: the path's coverage times the
/// clip's, in the target's device space.
fn area_mask(a: &ShadingArea, clip: Option<&Mask>, masks: &MaskBuilder<'_>) -> Option<Mask> {
    let (width, height) = masks.size();
    let mut mask = Mask::new(width, height)?;
    mask.fill_path_fit(&a.path, a.rule, a.anti_alias, masks.to_region(a.ctm));
    if let Some(old) = clip {
        for (n, o) in mask.data_mut().iter_mut().zip(old.data()) {
            *n = u8::try_from((u16::from(*n) * u16::from(*o)) / 255).unwrap_or(255);
        }
    }
    Some(mask)
}
