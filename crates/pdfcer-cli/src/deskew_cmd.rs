//! `deskew` — measure and straighten scanned page images.

use super::*;
use pdfcer_core::edit::EditError;

/// Arguments for [`cmd_deskew`].
pub(crate) struct DeskewArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) pages: &'a str,
    pub(crate) object: Option<usize>,
    pub(crate) angle: Option<f64>,
    pub(crate) min_angle: f64,
    pub(crate) min_confidence: f64,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
}

/// What happened to one page.
enum PageResult {
    Corrected,
    Skipped,
}

/// Implement `pdfcer deskew`. Without `--output` it is a dry run that only
/// reports what it measured.
pub(crate) fn cmd_deskew(a: &DeskewArgs<'_>) -> u8 {
    let (source, mut session) = match open_for_edit(a.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let count = match session.pages() {
        Ok(pages) => pages.len(),
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", a.input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let pages = match sign::parse_pages(a.pages, count) {
        Ok(pages) => pages,
        Err(err) => {
            eprintln!("pdfcer: {}: --pages: {err}", a.input.display());
            return exit::EDIT_REFUSED;
        }
    };
    let (mut corrected, mut skipped) = (0usize, 0usize);
    for page_index in pages {
        match deskew_page(&mut session, a, page_index) {
            Ok(PageResult::Corrected) => corrected += 1,
            Ok(PageResult::Skipped) => skipped += 1,
            // A page's own scan that cannot be deskewed is skipped, so one
            // odd page does not stop a batch; an image the operator named
            // is refused.
            Err(EditError::DeskewUnsupported { reason }) if a.object.is_none() => {
                println!(": skipped (cannot be deskewed: {reason})");
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
            "deskew {} (dry run)\n  would_correct={corrected} skipped={skipped}",
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
        "deskew {} -> {}\n  corrected={corrected} skipped={skipped}",
        a.input.display(),
        output.display()
    );
    finish_edit(a.input, &saved)
}

/// Measure (unless `--angle` was given) and, when the skew is worth
/// correcting and this is not a dry run, straighten one page's image.
/// Prints one line for the page; on an error the line is left open for the
/// caller to finish.
fn deskew_page(
    session: &mut pdfcer_core::edit::EditSession,
    a: &DeskewArgs<'_>,
    page_index: usize,
) -> Result<PageResult, EditError> {
    let page = page_index + 1;
    let object = match a.object {
        Some(object) => object,
        None => match session.page_scan_image(page_index)? {
            Some(object) => object,
            None => {
                println!("page {page}: skipped (no image)");
                return Ok(PageResult::Skipped);
            }
        },
    };
    print!("page {page} object {object}");
    let angle = match a.angle {
        Some(angle) => {
            print!(": angle={angle:+.2} (given)");
            angle
        }
        None => match session.detect_image_skew(page_index, object)? {
            None => {
                println!(": skipped (too little ink to measure)");
                return Ok(PageResult::Skipped);
            }
            Some(est) => {
                print!(
                    ": skew={:+.2} confidence={:.2}",
                    est.angle_degrees, est.confidence
                );
                if est.confidence < a.min_confidence {
                    println!(": skipped (below --min-confidence {})", a.min_confidence);
                    return Ok(PageResult::Skipped);
                }
                est.angle_degrees
            }
        },
    };
    if angle.abs() < a.min_angle {
        println!(": skipped (below --min-angle {})", a.min_angle);
        return Ok(PageResult::Skipped);
    }
    if a.output.is_none() {
        println!(": would correct");
        return Ok(PageResult::Corrected);
    }
    let out = session.deskew_image(page_index, object, angle)?;
    println!(
        ": corrected image_obj={} replaced={} stream_bytes={}->{}",
        out.image_id.num, out.replaced.num, out.old_stream_bytes, out.new_stream_bytes
    );
    for note in &out.notes {
        eprintln!("pdfcer: {}: page {page}: {note}", a.input.display());
    }
    Ok(PageResult::Corrected)
}
