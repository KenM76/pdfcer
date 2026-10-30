//! The words and marks a front end shows for a [`VisualCompareReport`], so
//! the CLI and the GUI produce the same summary note and the same marks.

use std::fmt::Write as _;

use pdfcer_core::compare::RegionMark;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Rect};

use super::{PageMatch, PagePair, VisualCompareReport};

/// The text an inserted page's frame carries.
pub const INSERTED_PAGE_NOTE: &str = "Inserted page: no page of the old document looks like it.";

fn page(p: Option<usize>) -> String {
    p.map_or_else(|| "-".to_owned(), |i| (i + 1).to_string())
}

impl PagePair {
    /// How this row was paired, 1-based: `old p2 moved to new p3`,
    /// `new p4 inserted (no matching old page)`, `old p1 = new p1`.
    #[must_use]
    pub fn match_text(&self) -> String {
        match self.how {
            PageMatch::Inserted => {
                format!("new p{} inserted (no matching old page)", page(self.new))
            }
            PageMatch::Deleted => format!("old p{} deleted (no matching new page)", page(self.old)),
            PageMatch::Moved => format!("old p{} moved to new p{}", page(self.old), page(self.new)),
            PageMatch::Mapped => {
                format!("old p{} = new p{} (mapped)", page(self.old), page(self.new))
            }
            _ => format!("old p{} = new p{}", page(self.old), page(self.new)),
        }
    }
}

impl VisualCompareReport {
    /// The settings in force, as one clause: resolution, tolerance, merge
    /// gap, minimum region and how pages were paired.
    #[must_use]
    pub fn settings_text(&self) -> String {
        let pairing = if self.matched_automatically {
            format!(
                "pages matched by appearance (paired at similarity {} or more)",
                self.match_threshold
            )
        } else {
            "pages paired as given".to_owned()
        };
        format!(
            "{:.0} dpi, tolerance {}, regions merged within {} pt and under {} px dropped; {pairing}",
            self.scale * 72.0,
            self.threshold,
            self.merge_gap,
            self.min_pixels,
        )
    }

    /// The paragraph for the marked result's summary page: the count, what
    /// the marks look like, [`settings_text`](Self::settings_text), then one
    /// line per row not paired in order.
    #[must_use]
    pub fn summary_note(&self) -> String {
        let mut s = format!(
            "Visual comparison: {} changed area(s), each an orange cloud with a note; a blue \
             frame marks a page with no match in the old document. {}.\n",
            self.region_count(),
            self.settings_text()
        );
        for p in self.pairs.iter().filter(|p| p.how != PageMatch::InOrder) {
            let _ = writeln!(s, "{}", p.match_text());
        }
        if self.nothing_matched() {
            s.push_str("No page of one document looked like any page of the other.\n");
        }
        s
    }

    /// The marks for the result, in row order, for
    /// [`pdfcer_core::compare::mark_regions`] on `new` (the document this
    /// report's `new` pages index): a frame on each inserted page's crop
    /// box, and a cloud on each changed region of a paired page. A deleted
    /// page has nowhere on `new` to go and gets none; an inserted page
    /// index past `new`'s last page is skipped.
    #[must_use]
    pub fn region_marks(&self, new: &Document) -> Vec<RegionMark> {
        let boxes: Vec<Rect> = page_tree::pages(new)
            .map(|p| p.iter().map(|p| p.crop_box).collect())
            .unwrap_or_default();
        let mut out = Vec::new();
        for p in &self.pairs {
            let Some(n) = p.new else { continue };
            if p.old.is_none() {
                if let Some(b) = boxes.get(n) {
                    out.push(RegionMark::inserted_page(n, *b, INSERTED_PAGE_NOTE));
                }
                continue;
            }
            let from = page(p.old);
            out.extend(p.regions.iter().map(|g| {
                RegionMark::changed(
                    n,
                    g.rect,
                    format!(
                        "Changed appearance (compared with old page {from}; {} px)",
                        g.changed_pixels
                    ),
                )
            }));
        }
        out
    }
}

#[cfg(test)]
mod tests;
