//! `optimize --visual-diff`: render every page before and after and report
//! what the reduction changed on screen.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_render::{PageDiff, VisualDiffOptions, diff_heatmap, render_page_with, visual_diff};

use crate::json_escape;

/// The visual-diff flags.
#[derive(clap::Args, Debug, Clone, Default)]
pub(crate) struct VisualDiffArgs {
    /// Render every page of the input and of the result and report what
    /// changed on screen: a similarity score per page (SSIM, 1 is
    /// identical), how many pixels changed and where. Also with --dry-run.
    #[arg(long)]
    pub(crate) visual_diff: bool,
    /// Resolution of the comparison renders (default 72). Higher sees finer
    /// damage and takes longer.
    #[arg(long, value_name = "DPI")]
    pub(crate) visual_diff_dpi: Option<f32>,
    /// How far any of red, green or blue (0-255) must move before a pixel
    /// counts as changed (default 8, above antialiasing noise).
    #[arg(long, value_name = "LEVELS")]
    pub(crate) visual_diff_threshold: Option<u8>,
    /// Also write a picture of each changed page to DIR, the changes in red
    /// over the result in grey (page-0001.png, ...). Implies --visual-diff.
    #[arg(long, value_name = "DIR")]
    pub(crate) visual_diff_dir: Option<PathBuf>,
}

impl VisualDiffArgs {
    /// Whether a comparison was asked for.
    pub(crate) fn requested(&self) -> bool {
        self.visual_diff || self.visual_diff_dir.is_some()
    }

    fn dpi(&self) -> f32 {
        self.visual_diff_dpi.unwrap_or(72.0)
    }

    fn options(&self) -> VisualDiffOptions {
        VisualDiffOptions::default()
            .with_scale(self.dpi() / 72.0)
            .with_threshold(self.visual_diff_threshold.unwrap_or(8))
    }
}

/// The comparison, for the report.
pub(crate) struct VisualSummary {
    dpi: f32,
    threshold: u8,
    /// One per page: the comparison, or why there is none.
    pages: Vec<Result<PageDiff, String>>,
    /// Pictures written to `--visual-diff-dir`.
    pictures: usize,
    /// Why the comparison or the pictures stopped, if they did.
    failure: Option<String>,
}

/// Compare `before` with the optimized `after` bytes (`None`: nothing was
/// produced, so nothing is compared).
pub(crate) fn compare(
    args: &VisualDiffArgs,
    before: &Document,
    after: Option<&[u8]>,
) -> VisualSummary {
    let options = args.options();
    let mut summary = VisualSummary {
        dpi: args.dpi(),
        threshold: options.threshold,
        pages: Vec::new(),
        pictures: 0,
        failure: None,
    };
    let Some(bytes) = after else {
        summary.failure = Some("no output was produced, so there is nothing to compare".into());
        return summary;
    };
    let after = match Document::from_bytes(bytes.to_vec()) {
        Ok(d) => d,
        Err(e) => {
            summary.failure = Some(format!("the result does not open: {e}"));
            return summary;
        }
    };
    match visual_diff(before, &after, &options) {
        Ok(pages) => {
            summary.pages = pages
                .into_iter()
                .map(|p| p.map_err(|e| e.to_string()))
                .collect();
        }
        Err(e) => {
            summary.failure = Some(format!("the page tree does not read: {e}"));
            return summary;
        }
    }
    if let Some(dir) = &args.visual_diff_dir
        && let Err(e) = write_pictures(&mut summary, dir, before, &after, &options)
    {
        summary.failure = Some(e);
    }
    summary
}

fn write_pictures(
    summary: &mut VisualSummary,
    dir: &Path,
    before: &Document,
    after: &Document,
    options: &VisualDiffOptions,
) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let pages = |d| pdfcer_core::page_tree::pages(d).map_err(|e| e.to_string());
    let (a, b) = (pages(before)?, pages(after)?);
    let changed = summary
        .pages
        .iter()
        .enumerate()
        .filter(|(_, p)| p.as_ref().is_ok_and(PageDiff::changed));
    for (i, _) in changed.collect::<Vec<_>>() {
        let (Some(pa), Some(pb)) = (a.get(i), b.get(i)) else {
            continue;
        };
        let render = |doc, page| {
            render_page_with(doc, page, options.scale, &options.render).map_err(|e| e.to_string())
        };
        let map = diff_heatmap(&render(before, pa)?, &render(after, pb)?, options.threshold)
            .map_err(|e| e.to_string())?;
        let path = dir.join(format!("page-{:04}.png", i + 1));
        map.save_png(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        summary.pictures += 1;
    }
    Ok(())
}

/// The text report's visual-diff section.
pub(crate) fn visual_text(s: &VisualSummary) -> String {
    let mut out = String::new();
    let changed: Vec<(usize, &PageDiff)> = s
        .pages
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.as_ref().ok().filter(|d| d.changed()).map(|d| (i + 1, d)))
        .collect();
    let lowest = s
        .pages
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.as_ref().ok().map(|d| (i + 1, d.ssim)))
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let _ = write!(
        out,
        "visual diff ({} dpi, threshold {}): {} page(s), {} changed",
        s.dpi,
        s.threshold,
        s.pages.len(),
        changed.len()
    );
    if let Some((page, ssim)) = lowest {
        let _ = write!(out, ", lowest similarity {ssim:.4} on page {page}");
    }
    out.push('\n');
    for (page, d) in changed {
        let pct = 100.0 * d.changed_pixels as f64 / d.pixels.max(1) as f64;
        let _ = write!(
            out,
            "  page {page}: similarity {:.4}, {} of {} pixels changed ({pct:.2}%), largest change {}",
            d.ssim, d.changed_pixels, d.pixels, d.max_delta
        );
        if let Some(r) = d.changed_region {
            let _ = write!(
                out,
                ", within {}x{} px at ({}, {})",
                r.width, r.height, r.x, r.y
            );
        }
        out.push('\n');
    }
    for (i, p) in s.pages.iter().enumerate() {
        if let Err(e) = p {
            let _ = writeln!(out, "  page {}: not compared: {e}", i + 1);
        }
    }
    if s.pictures > 0 {
        let _ = writeln!(out, "  {} picture(s) of the changes written", s.pictures);
    }
    if let Some(f) = &s.failure {
        let _ = writeln!(out, "  stopped: {f}");
    }
    out
}

/// The JSON report's `visual_diff` value.
pub(crate) fn visual_json(s: &VisualSummary) -> String {
    let pages: Vec<String> = s
        .pages
        .iter()
        .enumerate()
        .map(|(i, p)| match p {
            Ok(d) => format!(
                "{{\"page\":{},\"ssim\":{},\"max_delta\":{},\"changed_pixels\":{},\"pixels\":{},\"region\":{}}}",
                i + 1,
                d.ssim,
                d.max_delta,
                d.changed_pixels,
                d.pixels,
                d.changed_region.map_or_else(
                    || "null".to_owned(),
                    |r| format!(
                        "{{\"x\":{},\"y\":{},\"width\":{},\"height\":{}}}",
                        r.x, r.y, r.width, r.height
                    )
                )
            ),
            Err(e) => format!("{{\"page\":{},\"error\":\"{}\"}}", i + 1, json_escape(e)),
        })
        .collect();
    format!(
        "{{\"dpi\":{},\"threshold\":{},\"pages\":[{}],\"pictures\":{},\"failure\":{}}}",
        s.dpi,
        s.threshold,
        pages.join(","),
        s.pictures,
        s.failure
            .as_ref()
            .map_or_else(|| "null".to_owned(), |f| format!("\"{}\"", json_escape(f)))
    )
}
