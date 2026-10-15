//! `upscale-images` — enlarge scanned page images in place.

use super::*;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::ocr::upscale::{Upscale, UpscaleFilter};

/// Arguments for [`cmd_upscale_images`].
pub(crate) struct UpscaleArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) pages: &'a str,
    pub(crate) object: Option<usize>,
    pub(crate) factor: f32,
    pub(crate) filter: UpscaleFilter,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// Implement `pdfcer upscale-images`. Without `--output` it is a dry run
/// that only reports the sizes.
pub(crate) fn cmd_upscale_images(a: &UpscaleArgs<'_>) -> u8 {
    let upscale = match Upscale::new(a.factor, a.filter) {
        Ok(u) if !u.is_identity() => u,
        Ok(_) => {
            eprintln!("pdfcer: --factor 1 changes nothing; give a factor above 1");
            return exit::EDIT_REFUSED;
        }
        Err(err) => {
            eprintln!("pdfcer: --factor: {err}");
            return exit::EDIT_REFUSED;
        }
    };
    let (source, mut session) = match open_for_edit(a.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let pages = match session
        .pages()
        .map_err(|e| e.to_string())
        .and_then(|p| sign::parse_pages(a.pages, p.len()).map_err(|e| format!("--pages: {e}")))
    {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let (mut enlarged, mut skipped) = (0usize, 0usize);
    for page_index in pages {
        match upscale_page(&mut session, a, upscale, page_index) {
            Ok(true) => enlarged += 1,
            Ok(false) => skipped += 1,
            // A page's own scan that cannot be enlarged is skipped, so one
            // odd page does not stop a batch; an image the operator named
            // is refused.
            Err(EditError::ImageUpscaleUnsupported { reason }) if a.object.is_none() => {
                println!(": skipped (cannot be enlarged: {reason})");
                skipped += 1;
            }
            Err(err) => {
                println!(": refused");
                return report_edit_error(a.input, &err);
            }
        }
    }
    let Some(output) = a.output else {
        println!(
            "upscale-images {} (dry run)\n  would_enlarge={enlarged} skipped={skipped}",
            a.input.display()
        );
        return exit::SUCCESS;
    };
    let saved = match save_edited(
        &mut session,
        &source,
        output,
        a.mode,
        ProducerArg::Preserve,
        false,
    ) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    println!(
        "upscale-images {} -> {}\n  enlarged={enlarged} skipped={skipped}",
        a.input.display(),
        output.display()
    );
    finish_edit(a.input, &saved)
}

/// Enlarge one page's image unless this is a dry run; `Ok(false)` when the
/// page has no image. Prints one line for the page; on an error the line is
/// left open for the caller to finish.
fn upscale_page(
    session: &mut EditSession,
    a: &UpscaleArgs<'_>,
    upscale: Upscale,
    page_index: usize,
) -> Result<bool, EditError> {
    let page = page_index + 1;
    let object = match a.object {
        Some(object) => object,
        None => match session.page_scan_image(page_index)? {
            Some(object) => object,
            None => {
                println!("page {page}: skipped (no image)");
                return Ok(false);
            }
        },
    };
    print!("page {page} object {object}");
    if a.output.is_none() {
        println!(": would enlarge by {}", upscale.factor());
        return Ok(true);
    }
    let out = session.upscale_image(page_index, object, upscale)?;
    println!(
        ": {}x{} -> {}x{}: enlarged image_obj={} replaced={} stream_bytes={}->{}",
        out.old_size.0,
        out.old_size.1,
        out.new_size.0,
        out.new_size.1,
        out.image_id.num,
        out.replaced.num,
        out.old_stream_bytes,
        out.new_stream_bytes
    );
    for note in &out.notes {
        eprintln!("pdfcer: {}: page {page}: {note}", a.input.display());
    }
    Ok(true)
}
