//! Convert colours: every colour drawn in a chosen set of source spaces,
//! moved to one destination profile as one undoable edit.

use super::{CommandKind, EditError, EditSession};
use crate::object::ObjId;
use crate::prepress::{
    ColourConversion, ColourConverter, ConvertColoursReport, ConvertPlan, ConvertScope, PlanError,
};

impl EditSession {
    /// Convert every colour `scope` draws in `conversion`'s source spaces
    /// to its destination profile, through `converter` (pdfcer-render's
    /// `IcccerConverter` is the shipped one; pdfcer-core does no colour
    /// arithmetic, decision 064).
    ///
    /// Converted, and written in the destination profile's device space
    /// (`DeviceGray`, `DeviceRGB` or `DeviceCMYK`, by its data colour space,
    /// ICC.1:2022 §7.2.6):
    ///
    /// - content colours (`g`, `rg`, `cs` + `sc`/`scn` and their stroke
    ///   forms, ISO 32000-2 §8.6.8 Table 73) in page content, forms, tiling
    ///   patterns and annotation appearances; a `/DefaultRGB` or
    ///   `/DefaultGray` that replaces a device space (§8.6.5.6) is the
    ///   source;
    /// - image samples (§8.9.5), re-encoded as 8-bit `FlateDecode`; every
    ///   one is listed with its old filter and size in
    ///   [`ConvertColoursReport::images`];
    /// - `Indexed` lookup tables over a source base (§8.6.6.3);
    /// - axial and radial shadings (§8.7.4.5.3, §8.7.4.5.4), resampled as a
    ///   stitched function (disclosed).
    ///
    /// Left as they are, and listed in [`ConvertColoursReport::unconverted`]
    /// with the reason: inline images, uncoloured patterns, mesh and
    /// function-based shadings, spot inks whose alternate is a source,
    /// colour-keyed images, images whose soft mask has `/Matte`, and any
    /// colour the converter cannot transform. Soft masks and thumbnails are
    /// never converted. The rendering intent is one value or each object's
    /// own (§8.6.5.8); black point compensation is never asked for under
    /// `AbsoluteColorimetric` (§8.6.5.9).
    ///
    /// With [`ColourConversion::embed_output_intent`] the destination
    /// profile is added as an output intent (§14.11.5); one of the same
    /// subtype is replaced, and the report says which. One undo entry;
    /// nothing is committed when nothing changes. Resolves the preflight
    /// findings "RGB in a CMYK job" and "Lab present".
    ///
    /// # Errors
    ///
    /// [`EditError::ColourConversionRefused`] when the destination is not
    /// a usable ICC profile, the document has no output intent to take one
    /// from, no source is selected or the page tree cannot be read;
    /// [`EditError::PageOutOfRange`] for a page index past the end;
    /// [`EditError::DocumentEncrypted`] without the modify permission; the
    /// certification gate.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # fn demo(
    /// #     session: &mut pdfcer_core::edit::EditSession,
    /// #     converter: &dyn pdfcer_core::prepress::ColourConverter,
    /// #     profile: Vec<u8>,
    /// # ) -> Result<(), pdfcer_core::edit::EditError> {
    /// use pdfcer_core::prepress::{ColourConversion, ColourDestination, ConvertScope};
    ///
    /// let conversion = ColourConversion::new(ColourDestination::Profile(profile));
    /// let report = session.convert_colours(&ConvertScope::All, &conversion, converter)?;
    /// for count in &report.by_source {
    ///     println!("{}: {} paints", count.source.name(), count.paints);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn convert_colours(
        &mut self,
        scope: &ConvertScope,
        conversion: &ColourConversion,
        converter: &dyn ColourConverter,
    ) -> Result<ConvertColoursReport, EditError> {
        self.check_ink_edit()?;
        let plan =
            crate::prepress::plan_convert_colours(&self.view(), scope, conversion, converter)
                .map_err(|e| match e {
                    PlanError::Refused(reason) => EditError::ColourConversionRefused { reason },
                    PlanError::Page { index, count } => EditError::PageOutOfRange { index, count },
                })?;
        let ConvertPlan {
            mut writes,
            mut report,
            embed,
        } = plan;
        if let Some(embed) = embed {
            let id = ObjId::new(self.alloc_number()?, 0);
            crate::prepress::embed_writes(&self.view(), embed, id, &mut writes, &mut report);
        }
        let objects = self.planned_writes(writes);
        report.changed = objects.iter().map(|w| w.id).collect();
        self.commit_writes(CommandKind::ConvertColours, objects);
        Ok(report)
    }
}
