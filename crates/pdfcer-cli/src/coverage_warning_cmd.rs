//! `coverage-warning`: the pixels of a page that break a total-area-coverage
//! limit or are rich black (`pdfcer_render::coverage_warning_masks`).

use super::*;

/// What `coverage-warning` was asked.
pub(crate) struct CoverageArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) scale: f32,
    /// One or both rules, `--tac` first.
    pub(crate) warnings: Vec<pdfcer_render::CoverageWarning>,
    pub(crate) region: Option<&'a str>,
    pub(crate) hide_ink: Vec<String>,
    pub(crate) output: Option<&'a Path>,
}

/// One stdout line per rule with its flagged pixel count; `-o` also writes
/// the mask of a single rule.
pub(crate) fn cmd_coverage_warning(args: &CoverageArgs<'_>) -> u8 {
    let input = args.input;
    let (doc, page_list, indices, options) = match open_selected(input, &args.page.to_string()) {
        Ok(opened) => opened,
        Err(code) => return code,
    };
    let Some(page) = indices.first().and_then(|&i| page_list.get(i)) else {
        eprintln!(
            "pdfcer: {}: page {} is out of range",
            input.display(),
            args.page
        );
        return exit::RUNTIME_ERROR;
    };
    let region = match args.region.map(parse_region).transpose() {
        Ok(region) => region,
        Err(message) => {
            eprintln!("pdfcer: {}: --region {message}", input.display());
            return exit::EDIT_REFUSED;
        }
    };
    if args.output.is_some() && args.warnings.len() > 1 {
        eprintln!(
            "pdfcer: {}: -o writes one rule's mask; give --tac or --rich-black, not both",
            input.display()
        );
        return exit::EDIT_REFUSED;
    }
    let options = options.with_separation_preview(Some(
        pdfcer_render::SeparationPreview::new().with_hidden(args.hide_ink.clone()),
    ));
    let view = doc.view();
    let masks = match pdfcer_render::coverage_warning_masks(
        &view,
        page,
        args.scale,
        region,
        &options,
        &args.warnings,
    ) {
        Ok(masks) => masks,
        Err(err) => {
            eprintln!("pdfcer: {}: page {}: {err}", input.display(), args.page);
            return exit::RUNTIME_ERROR;
        }
    };
    if let (Some(path), Some(mask)) = (args.output, masks.first())
        && let Err(message) = write_mask_png(mask, path)
    {
        eprintln!("pdfcer: {}: {message}", path.display());
        return exit::RUNTIME_ERROR;
    }
    for (mask, &warning) in masks.iter().zip(&args.warnings) {
        let total = u64::from(mask.width) * u64::from(mask.height);
        #[allow(clippy::cast_precision_loss)] // A percentage for display.
        let percent = if total == 0 {
            0.0
        } else {
            mask.flagged as f64 * 100.0 / total as f64
        };
        println!(
            "coverage-warning page={} rule={} flagged={} of={total} percent={percent:.2}",
            args.page,
            rule_token(warning),
            mask.flagged
        );
    }
    exit::SUCCESS
}

fn rule_token(warning: pdfcer_render::CoverageWarning) -> String {
    match warning {
        pdfcer_render::CoverageWarning::TotalAreaCoverage { limit_percent } => {
            format!("tac:{limit_percent}")
        }
        pdfcer_render::CoverageWarning::RichBlack {
            black_start_percent,
        } => format!("rich-black:{black_start_percent}"),
        _ => "other".to_owned(),
    }
}

fn write_mask_png(mask: &pdfcer_render::CoverageMask, path: &Path) -> Result<(), String> {
    let mut pixmap = pdfcer_render::tiny_skia::Pixmap::new(mask.width.max(1), mask.height.max(1))
        .ok_or_else(|| "the mask is too large to write".to_owned())?;
    for (slot, &bit) in pixmap.pixels_mut().iter_mut().zip(&mask.mask) {
        let v = if bit == 1 { 255 } else { 0 };
        if let Some(c) = pdfcer_render::tiny_skia::PremultipliedColorU8::from_rgba(v, v, v, 255) {
            *slot = c;
        }
    }
    let png = pixmap.encode_png().map_err(|err| err.to_string())?;
    std::fs::write(path, png).map_err(|err| err.to_string())
}
