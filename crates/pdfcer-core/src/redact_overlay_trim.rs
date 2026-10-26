//! Keeps a redaction's overlay mark on the visible page.
//!
//! Table 192's overlay is drawn over the region it marks, and nothing stops
//! a region from reaching past the page box: an off-page clean-up
//! ([`crate::offpage::offpage_bands`]) is made only of such regions. A box
//! painted there is new content drawn outside the page, which is the very
//! thing the clean-up removed, and the next off-page scan reports it. So by
//! default the overlay is trimmed to the page box (`/CropBox`, §7.7.3.3
//! Table 30), and a region wholly outside it draws nothing.
//! [`crate::redact::RedactOptions::overlay_off_page`] keeps the old behaviour.
//! The content removal is not affected either way.

use crate::page_tree::Rect;
use crate::redact::{RedactionReport, RegionBox};

/// How many overlay regions were trimmed to the page box, and how many
/// were dropped because they lay wholly outside it.
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct TrimCounts {
    pub(crate) trimmed: u64,
    pub(crate) dropped: u64,
}

/// `regions` with each box trimmed to `page_box`, and those wholly outside
/// it (or touching it only along an edge) removed.
pub(crate) fn trim_to_page<T: Clone>(
    regions: &[(RegionBox, T)],
    page_box: Rect,
) -> (Vec<(RegionBox, T)>, TrimCounts) {
    let (px0, px1) = ordered(page_box.llx, page_box.urx);
    let (py0, py1) = ordered(page_box.lly, page_box.ury);
    let mut counts = TrimCounts::default();
    let mut out = Vec::with_capacity(regions.len());
    for (r, regime) in regions {
        let t = RegionBox {
            min_x: r.min_x.max(px0),
            min_y: r.min_y.max(py0),
            max_x: r.max_x.min(px1),
            max_y: r.max_y.min(py1),
        };
        if t.min_x >= t.max_x || t.min_y >= t.max_y {
            counts.dropped += 1;
            continue;
        }
        if t.min_x > r.min_x || t.min_y > r.min_y || t.max_x < r.max_x || t.max_y < r.max_y {
            counts.trimmed += 1;
        }
        out.push((t, regime.clone()));
    }
    (out, counts)
}

/// Record `counts` on `report`, with a note for each non-zero count.
pub(crate) fn disclose(report: &mut RedactionReport, counts: TrimCounts) {
    report.overlay_trimmed_to_page = counts.trimmed;
    report.overlay_off_page_skipped = counts.dropped;
    if counts.dropped > 0 {
        report.note(format!(
            "redaction: {} region(s) lay wholly outside the page box, so no overlay mark \
             was drawn for them; their content was removed",
            counts.dropped
        ));
    }
    if counts.trimmed > 0 {
        report.note(format!(
            "redaction: {} overlay mark(s) were trimmed to the page box; their content \
             was removed in full, including the part outside the page",
            counts.trimmed
        ));
    }
}

fn ordered(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn rb(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> RegionBox {
        RegionBox {
            min_x,
            min_y,
            max_x,
            max_y,
        }
    }

    const PAGE: Rect = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: 612.0,
        ury: 792.0,
    };

    #[test]
    fn inside_is_kept_overhang_is_trimmed_outside_is_dropped() {
        let regions = [
            (rb(10.0, 10.0, 20.0, 20.0), ()),
            (rb(600.0, 10.0, 700.0, 20.0), ()),
            (rb(612.0, 10.0, 900.0, 20.0), ()),
            (rb(-50.0, -50.0, -1.0, 900.0), ()),
        ];
        let (out, c) = trim_to_page(&regions, PAGE);
        assert_eq!(out.len(), 2);
        assert_eq!((c.trimmed, c.dropped), (1, 2));
        assert!((out[1].0.max_x - 612.0).abs() < 1e-9);
    }
}
