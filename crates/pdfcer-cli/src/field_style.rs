//! `--font`/`--font-resource`/`--font-size`/`--font-color`: the `/DA` a
//! field's value is drawn in (ISO 32000-2 §12.7.4.3), shared by
//! `add-text-field`, `add-choice-field` and `edit-field`.

use super::*;

/// The four `/DA` options, flattened into each field verb.
#[derive(Debug, Clone, Default, clap::Args)]
pub(crate) struct FieldStyleArgs {
    /// `/DA` font — the face the field's VALUE is drawn in. One of the
    /// standard 14: helvetica, helvetica-bold, helvetica-oblique,
    /// helvetica-bold-oblique, times, times-bold, times-italic,
    /// times-bold-italic, courier, courier-bold, courier-oblique,
    /// courier-bold-oblique, symbol, zapf-dingbats.
    ///
    /// pdfcer adds the font to the form's default resources, so the name
    /// always resolves. To use a face the document already embeds, pass
    /// its resource key with `--font-resource` instead.
    ///
    /// Requires `--font-size`. The field is drawn in it, so the text you see
    /// matches what the file says. Without any of these a new field is
    /// Helvetica, auto size, black.
    #[arg(long, value_name = "NAME")]
    pub(crate) font: Option<String>,
    /// `/DA` font, named by a resource key ALREADY in the form's `/DR`
    /// `/Font` — an embedded face the document's own author put there.
    ///
    /// REFUSED if the key is not there, listing what is. pdfcer will not
    /// write a `/DA` naming a font that does not resolve: the field would
    /// draw in a substituted face, look normal, and be wrong.
    #[arg(long, value_name = "KEY", conflicts_with = "font")]
    pub(crate) font_resource: Option<String>,
    /// `/DA` size in points. `0` means AUTO — the reader fits the text to
    /// the box, which is what Acrobat calls Auto and what a new text
    /// field defaults to.
    #[arg(long, value_name = "PT")]
    pub(crate) font_size: Option<f64>,
    /// `/DA` text colour: one number for gray, three for RGB, four for
    /// CMYK, comma separated, each 0-1. Defaults to black.
    ///
    /// This is the colour of the VALUE's glyphs. The box's own fill and
    /// border are `--background` and `--border-color`, a different
    /// dictionary entirely.
    #[arg(long, value_name = "G|R,G,B|C,M,Y,K")]
    pub(crate) font_color: Option<String>,
}

impl FieldStyleArgs {
    /// The appearance these options state; `Ok(None)` when no font was
    /// given. A bad value is reported and returned as its exit code.
    pub(crate) fn appearance(&self) -> Result<Option<pdfcer_core::edit::FieldAppearance>, u8> {
        // All three parts travel together because the string carries all
        // three: a size without a face would mean inventing the face.
        if self.font.is_none() && self.font_resource.is_none() {
            if self.font_size.is_some() || self.font_color.is_some() {
                eprintln!("pdfcer: --font-size/--font-color need --font or --font-resource");
                return Err(exit::RUNTIME_ERROR);
            }
            return Ok(None);
        }
        let Some(size) = self.font_size else {
            eprintln!(
                "pdfcer: --font/--font-resource needs --font-size (0 means auto-size, which is what a new text field uses)"
            );
            return Err(exit::RUNTIME_ERROR);
        };
        let colour = match self.font_color.as_deref() {
            None => pdfcer_core::vartext::TextColor::Gray(0.0),
            Some(raw) => parse_text_colour(raw).ok_or_else(|| {
                eprintln!(
                    "pdfcer: --font-color {raw:?} -- expected 1 (gray), 3 (RGB) or 4 (CMYK) comma-separated components in 0-1"
                );
                exit::RUNTIME_ERROR
            })?,
        };
        if let Some(name) = self.font.as_deref() {
            let face = parse_std14(name).ok_or_else(|| {
                eprintln!(
                    "pdfcer: --font {name:?} -- known: helvetica, helvetica-bold, helvetica-oblique, helvetica-bold-oblique, times, times-bold, times-italic, times-bold-italic, courier, courier-bold, courier-oblique, courier-bold-oblique, symbol, zapf-dingbats"
                );
                exit::RUNTIME_ERROR
            })?;
            return Ok(Some(pdfcer_core::edit::FieldAppearance::standard(
                face, size, colour,
            )));
        }
        let key = self.font_resource.as_deref().unwrap_or_default();
        Ok(Some(pdfcer_core::edit::FieldAppearance::resource(
            key.as_bytes().to_vec(),
            size,
            colour,
        )))
    }
}
