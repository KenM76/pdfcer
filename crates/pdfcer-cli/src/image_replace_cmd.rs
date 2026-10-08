//! `replace-image` — replace a placed image's pixels in place.

use super::*;
use pdfcer_core::edit::ImageFit;
use pdfcer_core::image_import;

/// Arguments for [`cmd_replace_image`].
pub(crate) struct ReplaceImageArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: usize,
    pub(crate) object: usize,
    pub(crate) image: &'a Path,
    pub(crate) stretch: bool,
    pub(crate) compression: image_import::ImageCompression,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
}

/// Implement `pdfcer replace-image`. The picture is parsed before the PDF is
/// opened, so a bad file is refused without touching the document.
pub(crate) fn cmd_replace_image(a: &ReplaceImageArgs<'_>) -> u8 {
    let Some(page_index) = a.page.checked_sub(1) else {
        eprintln!("pdfcer: --page is 1-based; 0 is not a page");
        return exit::EDIT_REFUSED;
    };
    let bytes = match std::fs::read(a.image) {
        Ok(b) => b,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.image.display());
            return exit::IO_ERROR;
        }
    };
    let options = image_import::ImportOptions::new().with_compression(a.compression);
    let img = match image_import::import_with(&bytes, &options) {
        Ok(img) => img,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.image.display());
            return exit::EDIT_REFUSED;
        }
    };
    let (source, mut session) = match open_for_edit(a.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let fit = if a.stretch {
        ImageFit::Stretch
    } else {
        ImageFit::Contain
    };
    let out = match session.replace_image(page_index, a.object, &img, fit) {
        Ok(out) => out,
        Err(err) => return report_edit_error(a.input, &err),
    };
    report_image_disclosures(a.image, &out.disclosures, None);
    let saved = match save_edited(
        &mut session,
        &source,
        a.output,
        a.mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    let d = &out.disclosures;
    println!(
        "replace-image {} page {} object {} -> {}\n  image_obj={} smask={} name={} replaced={} \
         fit={} letterboxed={} distorted={} eff_dpi={:.1},{:.1} low_res={} \
         compression_applied={}",
        a.input.display(),
        a.page,
        a.object,
        a.output.display(),
        out.image_id.num,
        out.soft_mask_id
            .map_or_else(|| "-".to_owned(), |id| id.num.to_string()),
        String::from_utf8_lossy(&out.resource_name),
        out.replaced
            .map_or_else(|| "inline".to_owned(), |id| id.num.to_string()),
        if a.stretch { "stretch" } else { "contain" },
        u32::from(d.letterboxed),
        u32::from(d.aspect_distorted),
        d.effective_dpi.0,
        d.effective_dpi.1,
        u32::from(d.below_screen_resolution),
        d.applied_compression.key(),
    );
    finish_edit(a.input, &saved)
}
