//! # Off-canvas content — finding what is drawn outside the page, and cutting it away
//!
//! ## What this answers
//!
//! *"Which of my drawings have marks outside the sheet?"* A PDF's page box
//! (`/CropBox`, falling back to `/MediaBox` — §7.7.3.3 Table 30) is what a
//! reader displays; a content stream may draw anywhere, including far outside
//! it. Those marks are **in the file** — they print on a larger sheet, they
//! survive a page-box change, they come back the moment somebody crops
//! differently, and text among them is extractable and searchable. So an
//! operator who thinks they deleted something by moving it off the sheet has
//! not.
//!
//! This module answers the question in two halves, which are the two things a
//! caller ever wants:
//!
//! 1. [`scan_page`] / [`scan_document`] — **which pages, and how much**. A
//!    read-only census, cheap enough to run over a folder tree.
//! 2. [`offpage_bands`] — **the geometry to remove**, expressed as the four
//!    rectangles that surround the page box, ready to become `/Redact` marks
//!    for [`crate::redact::apply_redactions`].
//!
//! ## ★★ Why the removal half is FOUR RECTANGLES and not new code
//!
//! The hard part of "delete what is off the page, keep what is on it" is the
//! *partial* object: a line that starts on the sheet and ends past its edge, a
//! text run half in the margin, an image overhanging the corner. Keeping the
//! on-page part means cutting geometry at the boundary — splitting paths,
//! dropping the glyphs that fall outside, overwriting the off-page samples of
//! an image.
//!
//! **pdfcer already does all of that**, in `redact_vector::cut_path`,
//! `redact`'s glyph surgery and `redact_image`'s sample grid — because it is
//! exactly what applying a redaction rectangle means (§12.5.6.23's *"remove
//! all traces of the specified content"*, which the same clause forbids
//! satisfying with a clip). The only thing missing was the observation that
//! **"outside the page" is a region like any other**, and the region's shape
//! is the four bands around the page box.
//!
//! So this module contributes geometry, not surgery. A partially-off path is
//! cut at the page edge by the same code that cuts it at the edge of an
//! operator's redaction box, and is therefore correct for the same reasons and
//! wrong in the same ways — one behaviour to reason about, not two.
//!
//! ## The bands are bounded by the DRAWN extent, not by infinity
//!
//! A redaction region is a quadrilateral with real coordinates, so the bands
//! have to stop somewhere. They stop at the union of the page box and every
//! object's bounding box, padded by [`BAND_MARGIN`]. Nothing can be drawn
//! outside that union by construction — it is the extent of what the page
//! draws — so a band that reaches it reaches everything.
//!
//! ## Tolerance, and why the default is not zero
//!
//! Real drawings touch their own edges. A border stroked exactly on the page
//! boundary has a bounding box that overhangs by half its line width; a hairline
//! rule at the trim edge overhangs by a fraction of a point. Reporting those as
//! "off canvas" would bury the real finding — a title block sitting entirely in
//! the margin — under thousands of edge-touching strokes.
//!
//! [`DEFAULT_TOLERANCE_PT`] is the width of the ignored fringe, in points, and
//! it is an **option rather than a constant** because the right value is a
//! property of the drawing, not of PDF: a 0.25 pt fringe is generous for a CAD
//! sheet and stingy for a poster. What it is NOT is a way to make the number
//! look better — a scan that is quiet because its tolerance is 12 pt has not
//! found nothing, it has been told not to look.

use crate::page_tree::{Page, Rect};
use crate::vector::decompose::{PageObjects, VectorObject, decompose_page};
use crate::vector::geometry::{Bounds, Matrix};
use crate::view::DocumentView;

/// The fringe, in points, inside which an overhang is not reported.
///
/// Chosen to sit just above the overhang a hairline border stroked on the page
/// boundary produces (half of a 0.5 pt line width), and well below anything an
/// operator would call "off the page".
pub const DEFAULT_TOLERANCE_PT: f64 = 0.25;

/// How far past the drawn extent the removal bands reach, in points.
///
/// Small and non-zero: the bands must cover every mark, and a mark exactly on
/// the outer edge of the drawn extent is covered only if the band goes past
/// it. Anything larger would be padding for its own sake — there is nothing
/// out there to catch.
pub const BAND_MARGIN: f64 = 1.0;

/// How an object sits relative to the page box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum OffPage {
    /// Entirely outside: the page box and the object's bounding box do not
    /// meet. **Nothing of this object is visible**, and removing it changes
    /// nothing a reader can see.
    Fully,
    /// Crossing the boundary: part of the object is on the page and part is
    /// not. Removing the off-page part is the case that needs cutting rather
    /// than deleting — see the module docs.
    Partial,
}

/// ★★★ A `Partial` IMAGE SURVIVES A CLEAN BY DESIGN, AND THIS SCAN STILL
/// REPORTS IT — measured 2026-09-12, recorded rather than fixed.
///
/// After `redact-offpage`, 12 objects across 7 of the operator's 174 drawings
/// are still reported, all `Partial`. They are **not a removal failure.**
///
/// `redact_image::covered_cells` snaps **outward** (`floor`/`ceil`), so an
/// image overhanging the page by 1 pt does have its off-page sample columns
/// cleared. What clearing cannot do is move the placement: the image is still
/// *drawn* extending past the page box, so its bounding box still crosses the
/// edge and this scan — which classifies by GEOMETRY — still counts it.
///
/// ⇒ It is the same shape as the empty text husk `Pass 294.2` fixed, one type
/// over: **the scan reporting its own output.** There the fix was to stop
/// counting runs that paint nothing; the analogous rule here is to stop
/// counting an image whose off-page cells carry no ink.
///
/// ★ That fix is deliberately NOT taken here, because it needs the samples,
/// and decoding every image during a scan is precisely what made the first
/// `redact-offpage` take ten minutes on one file (`Pass 294.1`). It wants a
/// measurement — how many placements, how much decode — not a guess at
/// 4 a.m. Until then the count is honest about the geometry and misleading
/// about the ink, and this paragraph is the disclosure.
///
/// One object that is not wholly on its page.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct OffPageObject {
    /// `path`, `text` or `image` — the object kind, as a stable token.
    pub kind: &'static str,
    /// Its page-space bounding box.
    pub bbox: Bounds,
    /// Fully outside, or crossing the edge.
    pub how: OffPage,
    /// The text the object shows, when it is a text object and the text could
    /// be recovered.
    ///
    /// ★ Carried because it is the disclosure that changes an operator's mind:
    /// *"there are 4 off-page objects"* invites a shrug, and *"one of them
    /// reads `SUPERSEDED — DO NOT BUILD`"* does not. Text drawn off the sheet
    /// is still extractable and still searchable.
    pub text: Option<String>,
}

/// What one page's scan found.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PageScan {
    /// 0-based page index.
    pub page_index: usize,
    /// The page box the scan measured against — `/CropBox`, or `/MediaBox`
    /// when there is no crop box (Table 30 makes the crop box default to the
    /// media box, and `Page::crop_box` has already applied that).
    pub page_box: Rect,
    /// The union of every drawn object's bounding box. Empty when the page
    /// draws nothing.
    pub drawn: Bounds,
    /// The objects that are not wholly on the page, in paint order.
    pub objects: Vec<OffPageObject>,
}

impl PageScan {
    /// How many objects are entirely off the page.
    #[must_use]
    pub fn fully_off(&self) -> usize {
        self.objects
            .iter()
            .filter(|o| o.how == OffPage::Fully)
            .count()
    }

    /// How many objects cross the page boundary.
    #[must_use]
    pub fn partial(&self) -> usize {
        self.objects
            .iter()
            .filter(|o| o.how == OffPage::Partial)
            .count()
    }

    /// Whether this page has anything off-canvas at all.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.objects.is_empty()
    }
}

/// Scan one page for content outside its page box.
///
/// `tolerance` is the ignored fringe in points — see the module docs on why it
/// is not zero.
///
/// # Errors
///
/// [`crate::content::ContentError`] when the page's content streams will not
/// decode or tokenize. A page pdfcer cannot read is **not** reported as clean:
/// the caller gets the error and can say so, because "no findings" and "I
/// could not look" must not print the same way.
pub fn scan_page(
    view: &DocumentView<'_>,
    page: &Page,
    page_index: usize,
    tolerance: f64,
) -> Result<PageScan, crate::content::ContentError> {
    let model = decompose_page(view, page, Matrix::IDENTITY)?;
    Ok(scan_model(&model, page, page_index, tolerance))
}

/// The classification half of [`scan_page`], split out so a caller that
/// already has a decomposition does not pay for a second one.
#[must_use]
pub fn scan_model(model: &PageObjects, page: &Page, page_index: usize, tolerance: f64) -> PageScan {
    let page_box = page.crop_box;
    let tol = tolerance.max(0.0);
    // The tolerant page box: an object must exceed THIS to be reported.
    let grown = Rect {
        llx: page_box.llx - tol,
        lly: page_box.lly - tol,
        urx: page_box.urx + tol,
        ury: page_box.ury + tol,
    };

    let mut objects = Vec::new();
    for obj in &model.objects {
        let bbox = obj.page_bbox();
        if !is_finite(&bbox) {
            // A degenerate or empty box says nothing about where the object
            // is. Counting it as off-page would be a claim about geometry
            // pdfcer could not measure.
            continue;
        }
        if !paints_anything(obj) {
            // ★ An object that DRAWS NOTHING is not "content drawn outside the
            // page", whatever its bounding box says (`Pass 294.2`).
            //
            // A text object's bbox comes from the text matrix, not from glyph
            // ink -- so after redaction removes every glyph, the positioned
            // but EMPTY `BT … ET` husk still measures the size the words used
            // to occupy. Scanning `redact-offpage`'s own output reported that
            // husk as a surviving off-page object, which reads as "the removal
            // did not work" when the removal worked exactly.
            //
            // The test is per-RUN ink, not recovered text: a run whose glyphs
            // have no `/ToUnicode` still paints, and calling it empty would
            // hide real content from a scan whose entire job is to find it.
            continue;
        }
        let how = if disjoint(&grown, &bbox) {
            OffPage::Fully
        } else if contained(&grown, &bbox) {
            continue;
        } else {
            OffPage::Partial
        };
        objects.push(OffPageObject {
            kind: kind_of(obj),
            bbox,
            how,
            text: text_of(obj),
        });
    }

    PageScan {
        page_index,
        page_box,
        drawn: model.page_bbox(),
        objects,
    }
}

/// One page that could not be read, and why — the second half of
/// [`scan_document`]'s answer.
///
/// A named type rather than a bare tuple because the pair IS the distinction
/// this module keeps making: the first element is a page number an operator
/// can go and look at, and the second is the reason pdfcer could not.
pub type UnreadablePage = (usize, String);

/// Scan every page of a document.
///
/// A page whose content will not decode is **skipped and reported** through
/// the second return value rather than silently dropped — the same
/// "no findings" / "could not look" distinction [`scan_page`] makes.
///
/// # Errors
///
/// [`crate::page_tree::PageTreeError`] when the page tree itself will not
/// walk, which is the one failure that is about the document rather than a
/// page.
pub fn scan_document(
    doc: &crate::document::Document,
    tolerance: f64,
) -> Result<(Vec<PageScan>, Vec<UnreadablePage>), crate::page_tree::PageTreeError> {
    let pages = crate::page_tree::pages(doc)?;
    let view = doc.view();
    let mut scans = Vec::new();
    let mut unreadable = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        match scan_page(&view, page, index, tolerance) {
            Ok(scan) => scans.push(scan),
            Err(err) => unreadable.push((index, err.to_string())),
        }
    }
    Ok((scans, unreadable))
}

/// The four rectangles that cover everything outside the page box, bounded by
/// the drawn extent.
///
/// Returns an empty vec when nothing is drawn outside — there is no region to
/// redact, and four zero-area marks would be four annotations that do nothing.
///
/// # The shape
///
/// ```text
///        +---------------------------+   <- outer = drawn ∪ page box, padded
///        |            TOP            |
///        +------+-------------+------+
///        | LEFT |  page box   | RIGHT|
///        +------+-------------+------+
///        |          BOTTOM           |
///        +---------------------------+
/// ```
///
/// The left and right bands stop at the page box's own top and bottom, so no
/// two bands overlap. Overlapping marks would be applied twice — harmless for
/// paths, wasteful for images, and confusing in the report.
#[must_use]
pub fn offpage_bands(scan: &PageScan, tolerance: f64) -> Vec<Rect> {
    if scan.objects.is_empty() || !is_finite(&scan.drawn) {
        return Vec::new();
    }
    // ★★ THE BANDS CARRY THE SAME TOLERANCE THE SCAN DOES, and leaving them
    // out was both a CORRECTNESS bug and the feature's whole performance
    // problem.
    //
    // Correctness: the scan ignores a fringe (a border stroked on the page
    // boundary overhangs by half its line width). Bands drawn at the exact
    // page box do NOT ignore it — so pdfcer would cut content the scan had
    // just reported as clean. Two answers to one question, from one feature.
    //
    // Performance, which is how it was found: a full-bleed scanned drawing
    // has an image reaching the page edge and a hair past it. Against exact
    // bands that image INTERSECTS a region, so `redact_image` decodes a
    // multi-megapixel scan, clears a sliver of cells and re-encodes it — per
    // image, per page. On a 6.9 MB 40-page drawing whose off-page content is
    // a handful of paths, that turned half a second into more than ten
    // minutes, and the operator watched it happen.
    //
    // Inset by the tolerance and the sliver is inside the kept area, so the
    // image is never touched: the work matches the finding.
    let tol = tolerance.max(0.0);
    let p = Rect {
        llx: scan.page_box.llx - tol,
        lly: scan.page_box.lly - tol,
        urx: scan.page_box.urx + tol,
        ury: scan.page_box.ury + tol,
    };
    let outer = Rect {
        llx: p.llx.min(scan.drawn.min.x) - BAND_MARGIN,
        lly: p.lly.min(scan.drawn.min.y) - BAND_MARGIN,
        urx: p.urx.max(scan.drawn.max.x) + BAND_MARGIN,
        ury: p.ury.max(scan.drawn.max.y) + BAND_MARGIN,
    };
    let mut bands = Vec::new();
    let mut push = |r: Rect| {
        if r.width() > 0.0 && r.height() > 0.0 {
            bands.push(r);
        }
    };
    // TOP: full width, above the page box.
    push(Rect {
        llx: outer.llx,
        lly: p.ury,
        urx: outer.urx,
        ury: outer.ury,
    });
    // BOTTOM: full width, below.
    push(Rect {
        llx: outer.llx,
        lly: outer.lly,
        urx: outer.urx,
        ury: p.lly,
    });
    // LEFT and RIGHT: only the page box's own vertical span, so the corners
    // belong to TOP and BOTTOM alone.
    push(Rect {
        llx: outer.llx,
        lly: p.lly,
        urx: p.llx,
        ury: p.ury,
    });
    push(Rect {
        llx: p.urx,
        lly: p.lly,
        urx: outer.urx,
        ury: p.ury,
    });
    bands
}

/// Whether this object puts ink on the page at all (`Pass 294.2`).
///
/// Paths and images are taken at their word — a path object exists because a
/// painting operator was seen, and an image because one was drawn. **Text is
/// the case that needs asking**: a `BT … ET` with every show operand emptied
/// (which is what redaction leaves behind) is a positioned husk that paints
/// nothing, while its object bbox — derived from the text matrix — still
/// measures the space the words used to fill.
///
/// A run counts as ink when its own laid-out bounds have positive width. That
/// is a statement about GLYPHS, not about recovered text: a run whose font has
/// no `/ToUnicode` previews as nothing and still paints, and treating it as
/// empty would hide exactly the content this module exists to find.
fn paints_anything(obj: &VectorObject) -> bool {
    let VectorObject::Text(t) = obj else {
        return true;
    };
    t.runs.iter().any(|r| {
        let b = r.bounds;
        b.min.x.is_finite() && b.max.x.is_finite() && (b.max.x - b.min.x).abs() > f64::EPSILON
    })
}

fn kind_of(obj: &VectorObject) -> &'static str {
    match obj {
        VectorObject::Path(_) => "path",
        VectorObject::Text(_) => "text",
        VectorObject::Image(_) => "image",
    }
}

fn text_of(obj: &VectorObject) -> Option<String> {
    let VectorObject::Text(t) = obj else {
        return None;
    };
    let mut out = String::new();
    for i in 0..t.runs.len() {
        if let Some(s) = t.run_text(i) {
            out.push_str(s);
        }
    }
    let trimmed = out.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.chars().take(120).collect())
    }
}

fn is_finite(b: &Bounds) -> bool {
    b.min.x.is_finite() && b.min.y.is_finite() && b.max.x.is_finite() && b.max.y.is_finite()
}

/// No overlap at all — the object is entirely outside `r`.
fn disjoint(r: &Rect, b: &Bounds) -> bool {
    b.max.x <= r.llx || b.min.x >= r.urx || b.max.y <= r.lly || b.min.y >= r.ury
}

/// Entirely inside `r`.
fn contained(r: &Rect, b: &Bounds) -> bool {
    b.min.x >= r.llx && b.min.y >= r.lly && b.max.x <= r.urx && b.max.y <= r.ury
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn page_box() -> Rect {
        Rect {
            llx: 0.0,
            lly: 0.0,
            urx: 100.0,
            ury: 100.0,
        }
    }

    fn bounds(x0: f64, y0: f64, x1: f64, y1: f64) -> Bounds {
        Bounds {
            min: crate::vector::geometry::Point { x: x0, y: y0 },
            max: crate::vector::geometry::Point { x: x1, y: y1 },
        }
    }

    #[test]
    fn a_box_wholly_outside_is_disjoint_and_one_crossing_is_not() {
        let p = page_box();
        assert!(disjoint(&p, &bounds(200.0, 200.0, 300.0, 300.0)));
        assert!(!disjoint(&p, &bounds(90.0, 90.0, 300.0, 300.0)));
        assert!(contained(&p, &bounds(10.0, 10.0, 20.0, 20.0)));
        assert!(!contained(&p, &bounds(10.0, 10.0, 120.0, 20.0)));
    }

    /// ★ The bands must not overlap: the corners belong to TOP and BOTTOM,
    /// and LEFT/RIGHT stop at the page box's own vertical span. Overlapping
    /// marks would redact the same area twice.
    #[test]
    fn the_four_bands_tile_the_outside_without_overlapping() {
        let scan = PageScan {
            page_index: 0,
            page_box: page_box(),
            drawn: bounds(-50.0, -50.0, 150.0, 150.0),
            objects: vec![OffPageObject {
                kind: "path",
                bbox: bounds(-50.0, -50.0, -10.0, -10.0),
                how: OffPage::Fully,
                text: None,
            }],
        };
        let bands = offpage_bands(&scan, DEFAULT_TOLERANCE_PT);
        assert_eq!(bands.len(), 4);
        for (i, a) in bands.iter().enumerate() {
            for b in bands.iter().skip(i + 1) {
                let overlap_x = a.llx.max(b.llx) < a.urx.min(b.urx);
                let overlap_y = a.lly.max(b.lly) < a.ury.min(b.ury);
                assert!(!(overlap_x && overlap_y), "bands {a:?} and {b:?} overlap");
            }
        }
        // And together they reach past the drawn extent in every direction.
        let outer_left = bands.iter().map(|r| r.llx).fold(f64::INFINITY, f64::min);
        let outer_right = bands
            .iter()
            .map(|r| r.urx)
            .fold(f64::NEG_INFINITY, f64::max);
        assert!(
            outer_left < -50.0,
            "left band reaches past the drawn extent"
        );
        assert!(outer_right > 150.0, "right band does too");
    }

    /// A page with nothing off-canvas gets NO bands — not four empty ones.
    #[test]
    fn a_clean_page_produces_no_bands() {
        let scan = PageScan {
            page_index: 0,
            page_box: page_box(),
            drawn: bounds(10.0, 10.0, 90.0, 90.0),
            objects: Vec::new(),
        };
        assert!(offpage_bands(&scan, DEFAULT_TOLERANCE_PT).is_empty());
        assert!(scan.is_clean());
    }
}
