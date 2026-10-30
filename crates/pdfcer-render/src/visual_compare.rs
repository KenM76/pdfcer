//! Visual comparison of two documents (Pass 645.2): pages matched by how
//! they look, then each matched pair rendered and its changed pixels
//! grouped into tight regions, placed in the NEW page's user space.
//!
//! Every inference is reported: which pages were paired and how alike they
//! looked, which were taken as inserted, deleted or moved, and the
//! resolution, threshold and grouping the regions came from.

use pdfcer_core::document::Document;
use pdfcer_core::page_tree::{self, Page, PageTreeError, Rect};
use tiny_skia::Point;

use crate::visual_match::{self, Pairing};
use crate::visual_regions::changed_groups;
use crate::{ChangedRegion, RenderError, RenderOptions, page_device_geometry, render_page_with};

mod notes;
pub use notes::INSERTED_PAGE_NOTE;

/// How [`visual_compare`] matches pages and finds changed regions.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct VisualCompareOptions {
    /// Device pixels per point for the pair renders. Default `1.5` (108 dpi).
    pub scale: f32,
    /// A pixel is changed when R, G or B moved by more than this. Default
    /// 48: above antialiasing and re-encode noise, below a thin black line
    /// on white.
    pub threshold: u8,
    /// Changed pixels closer than this, in points, share a region.
    /// Default 6.
    pub merge_gap: f32,
    /// Regions with fewer changed pixels than this are dropped as noise.
    /// Default 6.
    pub min_pixels: u64,
    /// Two pages pair only when their similarity is at least this
    /// (`0.0..=1.0`). Default 0.5.
    pub match_threshold: f64,
    /// 0-based pages read from each side, `(old, new)`; `None` = all.
    pub pages: (Option<Vec<usize>>, Option<Vec<usize>>),
    /// Explicit `(old, new)` pairs (0-based, into the documents, not into
    /// [`Self::pages`]); when set, no automatic matching runs and every
    /// unlisted page of the read ranges is inserted or deleted.
    pub mapping: Option<Vec<(usize, usize)>>,
    /// Options for every render.
    pub render: RenderOptions,
}

impl Default for VisualCompareOptions {
    fn default() -> Self {
        Self {
            scale: 1.5,
            threshold: 48,
            merge_gap: 6.0,
            min_pixels: 6,
            match_threshold: 0.5,
            pages: (None, None),
            mapping: None,
            render: RenderOptions::default(),
        }
    }
}

impl VisualCompareOptions {
    /// Render the pairs at `scale` device pixels per point.
    #[must_use]
    pub fn with_scale(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    /// Count a pixel changed past `threshold` levels.
    #[must_use]
    pub fn with_threshold(mut self, threshold: u8) -> Self {
        self.threshold = threshold;
        self
    }

    /// Merge changed pixels within `points` of each other.
    #[must_use]
    pub fn with_merge_gap(mut self, points: f32) -> Self {
        self.merge_gap = points;
        self
    }

    /// Drop regions with fewer than `pixels` changed pixels.
    #[must_use]
    pub fn with_min_pixels(mut self, pixels: u64) -> Self {
        self.min_pixels = pixels;
        self
    }

    /// Pair pages only at or above `similarity`.
    #[must_use]
    pub fn with_match_threshold(mut self, similarity: f64) -> Self {
        self.match_threshold = similarity;
        self
    }

    /// Read only `old` and `new` (0-based, in that order).
    #[must_use]
    pub fn with_pages(mut self, old: Option<Vec<usize>>, new: Option<Vec<usize>>) -> Self {
        self.pages = (old, new);
        self
    }

    /// Pair pages as listed instead of matching them.
    #[must_use]
    pub fn with_mapping(mut self, pairs: Vec<(usize, usize)>) -> Self {
        self.mapping = Some(pairs);
        self
    }
}

/// How a page pair came about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PageMatch {
    /// Paired in document order.
    InOrder,
    /// Paired out of order: the page moved.
    Moved,
    /// Paired by [`VisualCompareOptions::mapping`].
    Mapped,
    /// Only in the new document.
    Inserted,
    /// Only in the old document.
    Deleted,
}

/// One changed region on a new page.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct VisualRegion {
    /// The region in render pixels, origin top-left of the new page.
    pub pixels: ChangedRegion,
    /// The region in the new page's default user space, points.
    pub rect: Rect,
    /// Changed pixels inside it.
    pub changed_pixels: u64,
}

/// One row of the comparison: a page pair, or a page only one side has.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PagePair {
    /// 0-based old page, when the old side has one.
    pub old: Option<usize>,
    /// 0-based new page, when the new side has one.
    pub new: Option<usize>,
    /// How the pair came about.
    pub how: PageMatch,
    /// How alike the two pages looked, `0.0..=1.0`; `None` for a page only
    /// one side has, or a mapped pair.
    pub similarity: Option<f64>,
    /// The two pages' displayed sizes differ; regions are found with both
    /// anchored top-left and white beyond the smaller page.
    pub size_differs: bool,
    /// Changed regions on the new page, largest first.
    pub regions: Vec<VisualRegion>,
    /// Why this pair was not compared (a render failed), when it was not.
    pub error: Option<String>,
}

/// What [`visual_compare`] found, with the settings it used.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct VisualCompareReport {
    /// In new-page order; a deleted page sits after the page it followed.
    pub pairs: Vec<PagePair>,
    /// The settings in force (echoed so the report states them).
    pub scale: f32,
    /// See [`VisualCompareOptions::threshold`].
    pub threshold: u8,
    /// See [`VisualCompareOptions::merge_gap`].
    pub merge_gap: f32,
    /// See [`VisualCompareOptions::min_pixels`].
    pub min_pixels: u64,
    /// See [`VisualCompareOptions::match_threshold`].
    pub match_threshold: f64,
    /// Whether pages were matched automatically (no mapping was given).
    pub matched_automatically: bool,
}

impl VisualCompareReport {
    /// Pairs compared and both read on each side but none paired: the
    /// matcher found nothing alike, and the inserted/deleted rows mean
    /// "no match", not "every page changed".
    #[must_use]
    pub fn nothing_matched(&self) -> bool {
        self.matched_automatically
            && self.pairs.iter().any(|p| p.old.is_some())
            && self.pairs.iter().any(|p| p.new.is_some())
            && self
                .pairs
                .iter()
                .all(|p| p.old.is_none() || p.new.is_none())
    }

    /// Changed regions across every pair.
    #[must_use]
    pub fn region_count(&self) -> usize {
        self.pairs.iter().map(|p| p.regions.len()).sum()
    }
}

/// Why the comparison could not run.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum VisualCompareError {
    /// A page tree could not be read.
    #[error("the {side} document's pages could not be read: {source}")]
    Pages {
        /// `"old"` or `"new"`.
        side: &'static str,
        /// The failure.
        #[source]
        source: PageTreeError,
    },
    /// A requested or mapped page does not exist.
    #[error("the {side} document has no page {} (it has {count})", .index + 1)]
    NoSuchPage {
        /// `"old"` or `"new"`.
        side: &'static str,
        /// The 0-based index asked for.
        index: usize,
        /// Pages in that document.
        count: usize,
    },
    /// A page could not be rendered for matching.
    #[error("{side} page {} could not be rendered for matching: {source}", .index + 1)]
    Signature {
        /// `"old"` or `"new"`.
        side: &'static str,
        /// 0-based page.
        index: usize,
        /// The failure.
        #[source]
        source: RenderError,
    },
}

/// Compare how `old` and `new` look, page by page.
///
/// # Errors
///
/// [`VisualCompareError`] when a page tree cannot be read, a requested page
/// does not exist, or a page cannot be rendered for matching. A pair whose
/// full render fails is reported in [`PagePair::error`] instead.
pub fn visual_compare(
    old: &Document,
    new: &Document,
    options: &VisualCompareOptions,
) -> Result<VisualCompareReport, VisualCompareError> {
    let po = pages_of(old, "old")?;
    let pn = pages_of(new, "new")?;
    let ro = selection(&options.pages.0, po.len(), "old")?;
    let rn = selection(&options.pages.1, pn.len(), "new")?;
    let pairings = match &options.mapping {
        Some(map) => mapped(map, &ro, &rn, po.len(), pn.len())?,
        None => {
            let so = signatures(old, &po, &ro, &options.render, "old")?;
            let sn = signatures(new, &pn, &rn, &options.render, "new")?;
            visual_match::align(&so, &sn, options.match_threshold)
                .into_iter()
                .map(|p| translate(p, &ro, &rn))
                .collect()
        }
    };
    let pairs = pairings
        .into_iter()
        .map(|(o, n, how, sim)| compare_pair(old, new, &po, &pn, (o, n), how, sim, options))
        .collect();
    Ok(VisualCompareReport {
        pairs,
        scale: options.scale,
        threshold: options.threshold,
        merge_gap: options.merge_gap,
        min_pixels: options.min_pixels,
        match_threshold: options.match_threshold,
        matched_automatically: options.mapping.is_none(),
    })
}

type Row = (Option<usize>, Option<usize>, PageMatch, Option<f64>);

fn pages_of(doc: &Document, side: &'static str) -> Result<Vec<Page>, VisualCompareError> {
    page_tree::pages(doc).map_err(|source| VisualCompareError::Pages { side, source })
}

fn selection(
    pick: &Option<Vec<usize>>,
    count: usize,
    side: &'static str,
) -> Result<Vec<usize>, VisualCompareError> {
    let v = pick.clone().unwrap_or_else(|| (0..count).collect());
    match v.iter().find(|i| **i >= count) {
        Some(&index) => Err(VisualCompareError::NoSuchPage { side, index, count }),
        None => Ok(v),
    }
}

fn signatures(
    doc: &Document,
    pages: &[Page],
    read: &[usize],
    render: &RenderOptions,
    side: &'static str,
) -> Result<Vec<visual_match::Signature>, VisualCompareError> {
    read.iter()
        .map(|&index| {
            let page = pages.get(index).ok_or(VisualCompareError::NoSuchPage {
                side,
                index,
                count: pages.len(),
            })?;
            visual_match::signature(doc, page, render).map_err(|source| {
                VisualCompareError::Signature {
                    side,
                    index,
                    source,
                }
            })
        })
        .collect()
}

/// A pairing over the read lists, as document page indices.
fn translate(p: Pairing, ro: &[usize], rn: &[usize]) -> Row {
    let o = |i: usize| ro.get(i).copied();
    let n = |i: usize| rn.get(i).copied();
    match p {
        Pairing::Matched {
            old,
            new,
            score,
            moved,
        } => (
            o(old),
            n(new),
            if moved {
                PageMatch::Moved
            } else {
                PageMatch::InOrder
            },
            Some(score),
        ),
        Pairing::Deleted { old } => (o(old), None, PageMatch::Deleted, None),
        Pairing::Inserted { new } => (None, n(new), PageMatch::Inserted, None),
    }
}

fn mapped(
    map: &[(usize, usize)],
    ro: &[usize],
    rn: &[usize],
    co: usize,
    cn: usize,
) -> Result<Vec<Row>, VisualCompareError> {
    for &(a, b) in map {
        if a >= co {
            return Err(VisualCompareError::NoSuchPage {
                side: "old",
                index: a,
                count: co,
            });
        }
        if b >= cn {
            return Err(VisualCompareError::NoSuchPage {
                side: "new",
                index: b,
                count: cn,
            });
        }
    }
    let mut rows: Vec<Row> = map
        .iter()
        .map(|&(a, b)| (Some(a), Some(b), PageMatch::Mapped, None))
        .collect();
    rows.extend(
        rn.iter()
            .filter(|b| !map.iter().any(|m| m.1 == **b))
            .map(|&b| (None, Some(b), PageMatch::Inserted, None)),
    );
    rows.extend(
        ro.iter()
            .filter(|a| !map.iter().any(|m| m.0 == **a))
            .map(|&a| (Some(a), None, PageMatch::Deleted, None)),
    );
    Ok(rows)
}

#[allow(clippy::too_many_arguments)] // one row's inputs; a struct would only rename them
fn compare_pair(
    old: &Document,
    new: &Document,
    po: &[Page],
    pn: &[Page],
    (o, n): (Option<usize>, Option<usize>),
    how: PageMatch,
    similarity: Option<f64>,
    options: &VisualCompareOptions,
) -> PagePair {
    let mut pair = PagePair {
        old: o,
        new: n,
        how,
        similarity,
        size_differs: false,
        regions: Vec::new(),
        error: None,
    };
    let (Some(pa), Some(pb)) = (o.and_then(|i| po.get(i)), n.and_then(|i| pn.get(i))) else {
        return pair;
    };
    let render = |doc, page, side: &str| {
        render_page_with(doc, page, options.scale, &options.render)
            .map_err(|e| format!("the {side} page could not be rendered: {e}"))
    };
    let (ra, rb) = match (render(old, pa, "old"), render(new, pb, "new")) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            pair.error = Some(e);
            return pair;
        }
    };
    pair.size_differs =
        (ra.pixmap.width(), ra.pixmap.height()) != (rb.pixmap.width(), rb.pixmap.height());
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // a small positive gap
    let gap = (options.merge_gap * options.scale).ceil().max(1.0) as u32;
    let (_, _, ctm) = page_device_geometry(pb, options.scale);
    let inverse = ctm.invert();
    pair.regions = changed_groups(
        &ra.pixmap,
        &rb.pixmap,
        options.threshold,
        gap,
        options.min_pixels,
    )
    .into_iter()
    .map(|g| VisualRegion {
        pixels: g.region,
        rect: inverse.map_or(pb.crop_box, |t| to_user(t, g.region, pb.crop_box)),
        changed_pixels: g.pixels,
    })
    .collect();
    pair
}

/// A pixel box mapped back to user space, clipped to the page's crop box.
fn to_user(inverse: tiny_skia::Transform, r: ChangedRegion, crop: Rect) -> Rect {
    #[allow(clippy::cast_precision_loss)] // pixel coordinates are far below 2^24
    let (x0, y0, x1, y1) = (
        r.x as f32,
        r.y as f32,
        (r.x + r.width) as f32,
        (r.y + r.height) as f32,
    );
    let mut pts = [
        Point::from_xy(x0, y0),
        Point::from_xy(x1, y0),
        Point::from_xy(x0, y1),
        Point::from_xy(x1, y1),
    ];
    inverse.map_points(&mut pts);
    let xs = pts.iter().map(|p| f64::from(p.x));
    let ys = pts.iter().map(|p| f64::from(p.y));
    Rect {
        llx: xs.clone().fold(f64::MAX, f64::min).max(crop.llx),
        lly: ys.clone().fold(f64::MAX, f64::min).max(crop.lly),
        urx: xs.fold(f64::MIN, f64::max).min(crop.urx),
        ury: ys.fold(f64::MIN, f64::max).min(crop.ury),
    }
}
