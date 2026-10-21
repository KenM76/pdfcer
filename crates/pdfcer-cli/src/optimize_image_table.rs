//! The `images:` table closing `pdfcer optimize`'s change list: one aligned
//! row per image the settings touched, rewritten or skipped, so a run over
//! many images can be scanned without reading each change's detail lines.

use std::fmt::Write as _;

use pdfcer_core::optimize::{OptimizePlan, PlannedChange, StreamAction};

use crate::optimize_report::format_pair;

const HEADER: [&str; 6] = ["object", "page", "pixels", "ppi", "format", "bytes"];

/// The table, or nothing when the plan touches no image.
pub(crate) fn images_table(plan: &OptimizePlan) -> String {
    let mut rows: Vec<[String; 6]> = plan.changes.iter().filter_map(changed_row).collect();
    let rewritten = rows.len();
    rows.extend(
        plan.skipped
            .iter()
            .filter(|s| matches!(s.action, StreamAction::Image(_)))
            .map(|s| {
                let dash = || "-".to_owned();
                [
                    s.id.to_string(),
                    dash(),
                    dash(),
                    dash(),
                    dash(),
                    format!("skipped: {}", s.reason),
                ]
            }),
    );
    if rows.is_empty() {
        return String::new();
    }
    let mut widths = HEADER.map(str::len);
    for row in &rows {
        for (w, cell) in widths.iter_mut().zip(row) {
            *w = (*w).max(cell.chars().count());
        }
    }
    let mut out = format!(
        "images: {rewritten} rewritten, {} skipped\n",
        rows.len() - rewritten
    );
    out.push_str(&line(&HEADER.map(str::to_owned), &widths));
    for row in &rows {
        out.push_str(&line(row, &widths));
    }
    out
}

fn changed_row(c: &PlannedChange) -> Option<[String; 6]> {
    let img = c.image.as_ref()?;
    let ((bw, bh), (aw, ah)) = (img.size_before, img.size_after);
    let (page, ppi) = match (&img.binding, img.binding_ppi_after) {
        (Some(b), Some(after)) => (
            b.page.to_string(),
            format!("{:.0} -> {after:.0}", b.effective_ppi()),
        ),
        _ => ("-".to_owned(), "-".to_owned()),
    };
    // Floored, so a 99.8% saving never reads as the whole stream.
    let pct = c.before_bytes.saturating_sub(c.after_bytes) * 100 / c.before_bytes.max(1);
    Some([
        c.id.to_string(),
        page,
        format!("{bw}x{bh} -> {aw}x{ah}"),
        ppi,
        format_pair(img, &c.filters_before),
        format!("{} -> {} (-{pct}%)", c.before_bytes, c.after_bytes),
    ])
}

/// One row, every column but the last padded to its width.
fn line(cells: &[String; 6], widths: &[usize; 6]) -> String {
    let mut out = String::new();
    for (i, (cell, w)) in cells.iter().zip(widths).enumerate() {
        if i + 1 == cells.len() {
            let _ = write!(out, "  {cell}");
        } else {
            let _ = write!(out, "  {cell:<w$}");
        }
    }
    out.push('\n');
    out
}
