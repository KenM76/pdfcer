//! Why [`super::EmfVectorised::patched_not_flat`] ops are not flat.

/// The first thing that kept an op out of the flattener, one count per
/// op in [`super::EmfVectorised::patched_not_flat`]. All zero with the
/// switch off.
///
/// The cause is the first unmodellable content met, walking the op's
/// stack bottom to top; [`Self::beneath`] says how many of those causes
/// were in an earlier op the patched one overlaps rather than in the op
/// itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct EmfNotFlat {
    /// A soft mask whose content is flat-colour vector that still could
    /// not be resolved (its regions passed the ceiling, or its group
    /// composites in a way the mask scene does not model).
    pub soft_mask_flat: usize,
    /// A soft mask whose content holds a gradient, a smooth shading or a
    /// varying nested mask that could not be cut into constant-value
    /// bands, so its alpha varies continuously. A vector gap.
    pub soft_mask_varying: usize,
    /// A soft mask whose content holds an image (a drop shadow, a feather,
    /// a scanned vignette). Genuinely raster: a print flattener rasterises
    /// these too.
    pub soft_mask_image: usize,
    /// A soft mask the recording kept no vector source for.
    pub soft_mask_unrecorded: usize,
    /// An image.
    pub image: usize,
    /// A smooth shading or a gradient.
    pub shading: usize,
    /// A paint with spot or `/All` colorants on a page whose blending space
    /// is additive, where the raster keeps no spot planes to model.
    pub spot: usize,
    /// A spot paint on a page naming more spot colorants than the
    /// flattener has planes for (ISO 32000-1 §11.7.3).
    pub spot_roster: usize,
    /// An `/All` paint inside a non-isolated group, whose content the
    /// raster composites over two different sets of spot planes.
    pub spot_all_group: usize,
    /// A paint on a CMYK page that recorded no CMYK colour.
    pub cmyk: usize,
    /// A blend mode the flattener does not model.
    pub blend: usize,
    /// A combination the flattener declines: an overprint inside a
    /// knockout group.
    pub knockout: usize,
    /// A path or clip whose geometry could not be rebuilt.
    pub geometry: usize,
    /// Of the counts above: the cause was in an op beneath.
    pub beneath: usize,
}

/// The first unmodellable content in an op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cause {
    MaskFlat,
    MaskVarying,
    MaskImage,
    MaskUnrecorded,
    Image,
    Shading,
    Spot,
    SpotRoster,
    SpotAllGroup,
    Cmyk,
    Blend,
    Knockout,
    Geometry,
}

impl EmfNotFlat {
    /// Adds one patched region to `cause`'s counter, and to
    /// [`Self::beneath`] when the cause was in an op beneath the region's
    /// top one.
    pub(crate) fn count(&mut self, cause: Cause, beneath: bool) {
        let slot = match cause {
            Cause::MaskFlat => &mut self.soft_mask_flat,
            Cause::MaskVarying => &mut self.soft_mask_varying,
            Cause::MaskImage => &mut self.soft_mask_image,
            Cause::MaskUnrecorded => &mut self.soft_mask_unrecorded,
            Cause::Image => &mut self.image,
            Cause::Shading => &mut self.shading,
            Cause::Spot => &mut self.spot,
            Cause::SpotRoster => &mut self.spot_roster,
            Cause::SpotAllGroup => &mut self.spot_all_group,
            Cause::Cmyk => &mut self.cmyk,
            Cause::Blend => &mut self.blend,
            Cause::Knockout => &mut self.knockout,
            Cause::Geometry => &mut self.geometry,
        };
        *slot += 1;
        self.beneath += usize::from(beneath);
    }
}
