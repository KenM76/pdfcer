//! Steps 5 and 6: frame the page to the region, then mark the four bands
//! around it for redaction.
//!
//! §14.11.2: a reader intersects every page box with the media box, so a
//! `/TrimBox`, `/BleedBox` or `/ArtBox` reaching past the new media box is
//! clamped to it here rather than left for each reader to clamp.

use crate::annot_author::{Quad, RedactSpec};
use crate::document::Document;
use crate::edit::EditSession;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, Object};
use crate::offpage::{BAND_MARGIN, scan_page};
use crate::page_tree::{self, Rect};
use crate::vartext::Quadding;
use crate::writer::{DirtySet, SaveOptions, save_full};

use super::{PageOpError, RegionError, RegionReport};

/// Page entries that describe the page outside the region or act on it:
/// `/Annots` (everything left after flattening), `/Thumb` (a picture of the
/// whole page), `/B` (article beads) and `/AA` (page actions).
const DROPPED: [&[u8]; 4] = [b"Annots", b"Thumb", b"B", b"AA"];

/// Set the boxes and drop [`DROPPED`] on the copied page.
pub(super) fn frame(
    doc: &Document,
    rect: Rect,
    report: &mut RegionReport,
) -> Result<Vec<u8>, RegionError> {
    let (id, mut page) = first_page(doc)?;
    report.annotations_removed += page
        .get(b"Annots")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)
        .map_or(0, <[Object]>::len);
    for key in DROPPED {
        page.remove(key);
    }
    for key in [&b"TrimBox"[..], b"BleedBox", b"ArtBox"] {
        let Some(value) = page.get(key) else {
            continue;
        };
        let current = page_tree::parse_rect(doc, value, "box").ok();
        match current.and_then(|r| r.intersection(&rect)) {
            Some(clamped) if Some(clamped) == current => {}
            Some(clamped) => {
                page.insert(Name::from(key), rect_object(clamped));
                report.boxes_clamped += 1;
            }
            None => {
                page.remove(key);
                report.boxes_clamped += 1;
            }
        }
    }
    page.insert(Name::from(b"MediaBox"), rect_object(rect));
    page.insert(Name::from(b"CropBox"), rect_object(rect));
    save_page(doc, id, page)
}

/// Mark everything outside `rect` for redaction, out to the union of the
/// region, the page's media box and every object the page draws.
pub(super) fn mark_bands(doc: Document, rect: Rect) -> Result<Vec<u8>, RegionError> {
    let pages = page_tree::pages(&doc).map_err(PageOpError::from)?;
    let page = pages.first().ok_or(PageOpError::NoPages)?;
    let scan = scan_page(&doc.view(), page, 0, 0.0)?;
    let mut outer = Rect {
        llx: rect.llx.min(page.media_box.llx),
        lly: rect.lly.min(page.media_box.lly),
        urx: rect.urx.max(page.media_box.urx),
        ury: rect.ury.max(page.media_box.ury),
    };
    let d = scan.drawn;
    if [d.min.x, d.min.y, d.max.x, d.max.y]
        .iter()
        .all(|v| v.is_finite())
    {
        outer.llx = outer.llx.min(d.min.x);
        outer.lly = outer.lly.min(d.min.y);
        outer.urx = outer.urx.max(d.max.x);
        outer.ury = outer.ury.max(d.max.y);
    }
    let mut session = EditSession::new(doc);
    for band in bands(rect, outer) {
        let spec = RedactSpec {
            quads: vec![Quad::from_rect(band)],
            // No `/IC`: Table 192 leaves the region transparent on apply, so
            // the bands paint nothing.
            fill: None,
            overlay_text: None,
            quadding: Quadding::Left,
        };
        session.add_redaction(0, &spec)?;
    }
    Ok(session.to_full_bytes(&SaveOptions::identity())?.0)
}

/// The four non-overlapping bands between `rect` and `outer` grown by
/// [`BAND_MARGIN`]: full-width above and below, region-height left and
/// right.
fn bands(rect: Rect, outer: Rect) -> Vec<Rect> {
    let (x0, y0) = (outer.llx - BAND_MARGIN, outer.lly - BAND_MARGIN);
    let (x1, y1) = (outer.urx + BAND_MARGIN, outer.ury + BAND_MARGIN);
    [
        Rect::from_corners(x0, rect.ury, x1, y1),
        Rect::from_corners(x0, y0, x1, rect.lly),
        Rect::from_corners(x0, rect.lly, rect.llx, rect.ury),
        Rect::from_corners(rect.urx, rect.lly, x1, rect.ury),
    ]
    .into_iter()
    .filter(|b| b.width() > 0.0 && b.height() > 0.0)
    .collect()
}

/// Remove whatever `/Annots` survived redaction (its own marks included).
pub(super) fn strip_annots(doc: Document) -> Result<Vec<u8>, RegionError> {
    let (id, mut page) = first_page(&doc)?;
    if page.remove(b"Annots").is_none() {
        return Ok(doc.bytes().to_vec());
    }
    save_page(&doc, id, page)
}

fn first_page(doc: &Document) -> Result<(crate::object::ObjId, Dict), RegionError> {
    let pages = page_tree::pages(doc).map_err(PageOpError::from)?;
    let page = pages.first().ok_or(PageOpError::NoPages)?;
    let dict = doc
        .value(page.id)
        .and_then(Object::as_dict)
        .cloned()
        .ok_or(PageOpError::NoPages)?;
    Ok((page.id, dict))
}

fn save_page(doc: &Document, id: crate::object::ObjId, page: Dict) -> Result<Vec<u8>, RegionError> {
    // bypass-exempt: writes a scratch revision of a one-page copy that only
    // `extract_region` reads; no editing session or operator file is involved.
    let mut dirty = DirtySet::empty();
    dirty.replace(id, Object::Dict(page));
    // bypass-exempt: as above.
    Ok(save_full(doc, &dirty, &SaveOptions::identity())?.0)
}

fn rect_object(r: Rect) -> Object {
    Object::Array(
        [r.llx, r.lly, r.urx, r.ury]
            .into_iter()
            .map(|v| {
                if v.fract() == 0.0 && v.abs() < 1e15 {
                    #[allow(clippy::cast_possible_truncation)] // integral and in range
                    Object::Integer(v as i64)
                } else {
                    Object::Real(v)
                }
            })
            .collect(),
    )
}
