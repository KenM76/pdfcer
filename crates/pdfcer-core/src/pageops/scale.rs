//! Scale page contents to a target sheet size (`Pass 364.0`).
//!
//! The geometry and the dictionary rewrites behind
//! [`EditSession::scale_pages`](crate::edit::EditSession::scale_pages). Every
//! function here is pure: it takes values and returns changed copies, and the
//! verb decides which objects to write.
//!
//! The page's visible region (the crop box clipped to the media box, Table 30
//! and §14.11.2.1) is mapped uniformly into a `[0 0 W H]` sheet by
//! `x' = s·x + tx`, `y' = s·y + ty`. The content streams are wrapped, not
//! rewritten: `q s 0 0 s tx ty cm <clip> re W n` before them and `Q` after
//! (§8.4.2, §8.5.4). The clip keeps content that sat outside the old crop box
//! hidden when `Fit` pads the sheet.
//!
//! Annotation geometry moves with the content (§12.5.2 `/Rect`, Tables 175,
//! 179, 181, 182, 183), an appearance stream follows its `/Rect` through
//! §12.5.5's BBox→Rect mapping with no change of its own, and destinations
//! that name a scaled page are transformed (§12.3.2.2 Table 149).

use std::collections::HashMap;

use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::Rect;

/// How content meets a target whose aspect ratio differs from the page's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ScaleMode {
    /// Scale to fit inside the target; the remaining margin is left blank,
    /// split evenly on both sides.
    #[default]
    Fit,
    /// Scale to cover the target; the overflow falls outside the new page
    /// boxes and is not displayed (it is not removed from the file).
    Fill,
}

/// How a target size is oriented against each page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum OrientationPolicy {
    /// Swap the target's width and height when that makes it match the
    /// page's displayed orientation (portrait stays portrait). A square page
    /// or a square target is never swapped.
    #[default]
    Match,
    /// Use the target exactly as given, in displayed orientation.
    Exact,
}

/// What [`EditSession::scale_pages`](crate::edit::EditSession::scale_pages)
/// is asked to do. `width` × `height` is the target sheet as the page is
/// **displayed** (after `/Rotate`), in points (1/72 inch).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScaleRequest {
    /// Displayed sheet width, in points.
    pub width: f64,
    /// Displayed sheet height, in points.
    pub height: f64,
    /// Fit or fill.
    pub mode: ScaleMode,
    /// Match each page's orientation, or use the size exactly.
    pub orientation: OrientationPolicy,
}

impl ScaleRequest {
    /// A `Fit`, `Match` request for a `width` × `height` sheet.
    #[must_use]
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            mode: ScaleMode::default(),
            orientation: OrientationPolicy::default(),
        }
    }

    /// The same request with another mode.
    #[must_use]
    pub fn with_mode(self, mode: ScaleMode) -> Self {
        Self { mode, ..self }
    }

    /// The same request with another orientation policy.
    #[must_use]
    pub fn with_orientation(self, orientation: OrientationPolicy) -> Self {
        Self {
            orientation,
            ..self
        }
    }
}

/// Where one page's content lands: `x' = scale·x + offset_x`,
/// `y' = scale·y + offset_y`, in the page's unrotated user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PagePlacement {
    /// The new sheet, `[0 0 w h]` in unrotated user space; every page box
    /// present is set to it.
    pub target: Rect,
    /// The uniform scale factor (below 1 shrinks).
    pub scale: f64,
    /// Horizontal translation, user-space units.
    pub offset_x: f64,
    /// Vertical translation, user-space units.
    pub offset_y: f64,
    /// `true` when [`OrientationPolicy::Match`] swapped the target's width
    /// and height for this page.
    pub orientation_flipped: bool,
}

impl PagePlacement {
    /// Map an x coordinate.
    #[must_use]
    pub fn x(&self, x: f64) -> f64 {
        self.scale * x + self.offset_x
    }

    /// Map a y coordinate.
    #[must_use]
    pub fn y(&self, y: f64) -> f64 {
        self.scale * y + self.offset_y
    }
}

/// One scaled page, as reported (rule 4: the scale, offset and mode used).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PageScaled {
    /// 0-based page index.
    pub page_index: usize,
    /// The region that was scaled: the crop box clipped to the media box.
    pub source: Rect,
    /// Where it went.
    pub placement: PagePlacement,
    /// The mode used.
    pub mode: ScaleMode,
    /// Annotations on this page whose geometry was transformed.
    pub annotations: usize,
    /// `/Measure` dictionaries whose conversion factors were rescaled so
    /// measurements still read true (§12.9, Table 266).
    pub measures: usize,
}

/// What [`EditSession::scale_pages`](crate::edit::EditSession::scale_pages)
/// did.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct ScaleReport {
    /// One entry per requested page, ascending.
    pub pages: Vec<PageScaled>,
    /// Destinations (outline items, links, named destinations, actions)
    /// naming a scaled page whose coordinates were transformed.
    pub destinations: usize,
    /// Geospatial `/Measure` dictionaries (`/Subtype /GEO`) left unchanged:
    /// their mapping is a point-to-coordinate registration, not a linear
    /// factor, and rescaling it is not attempted.
    pub geo_measures_unchanged: usize,
}

/// The visible region of a page: its crop box clipped to its media box
/// (§14.11.2.1). `None` when the two do not overlap.
#[must_use]
pub fn visible_region(media: Rect, crop: Rect) -> Option<Rect> {
    let r = Rect {
        llx: media.llx.max(crop.llx),
        lly: media.lly.max(crop.lly),
        urx: media.urx.min(crop.urx),
        ury: media.ury.min(crop.ury),
    };
    (r.width() > 0.0 && r.height() > 0.0).then_some(r)
}

/// Plan where `source` lands for `request`, on a page displayed at
/// `rotate` degrees (0/90/180/270). `None` for a non-finite or non-positive
/// size on either side.
#[must_use]
pub fn plan_placement(source: Rect, rotate: u16, request: &ScaleRequest) -> Option<PagePlacement> {
    let (sw, sh) = (source.width(), source.height());
    let (mut tw, mut th) = (request.width, request.height);
    let sane = |v: f64| v.is_finite() && v > 0.0;
    if !(sane(sw) && sane(sh) && sane(tw) && sane(th)) {
        return None;
    }
    let quarter = rotate % 180 == 90;
    let (dw, dh) = if quarter { (sh, sw) } else { (sw, sh) };
    let mut flipped = false;
    if request.orientation == OrientationPolicy::Match
        && dw != dh
        && tw != th
        && (dw > dh) != (tw > th)
    {
        std::mem::swap(&mut tw, &mut th);
        flipped = true;
    }
    // Back into unrotated user space: a quarter turn swaps the axes.
    let (uw, uh) = if quarter { (th, tw) } else { (tw, th) };
    let (fx, fy) = (uw / sw, uh / sh);
    let scale = match request.mode {
        ScaleMode::Fit => fx.min(fy),
        ScaleMode::Fill => fx.max(fy),
    };
    Some(PagePlacement {
        target: Rect {
            llx: 0.0,
            lly: 0.0,
            urx: uw,
            ury: uh,
        },
        scale,
        offset_x: (uw - scale * sw) / 2.0 - scale * source.llx,
        offset_y: (uh - scale * sh) / 2.0 - scale * source.lly,
        orientation_flipped: flipped,
    })
}

/// A content-stream number: at most six decimals, no trailing zeros.
fn num(v: f64) -> String {
    let s = format!("{v:.6}");
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_owned()
    } else {
        s.to_owned()
    }
}

/// The stream placed before the page's own content: save state, apply the
/// placement (§8.4.4 `cm`), clip to the old visible region (§8.5.4).
#[must_use]
pub(crate) fn content_prefix(source: Rect, p: &PagePlacement) -> Vec<u8> {
    format!(
        "q\n{} 0 0 {} {} {} cm\n{} {} {} {} re W n\n",
        num(p.scale),
        num(p.scale),
        num(p.offset_x),
        num(p.offset_y),
        num(source.llx),
        num(source.lly),
        num(source.width()),
        num(source.height()),
    )
    .into_bytes()
}

/// The stream placed after the page's own content. The leading newline
/// keeps a final token of the previous stream from fusing with `Q`.
pub(crate) const CONTENT_SUFFIX: &[u8] = b"\nQ\n";

fn number_of(resolve: &dyn Fn(&Object) -> Object, o: &Object) -> Option<f64> {
    resolve(o).as_number()
}

/// Map a flat `[x1 y1 x2 y2 …]` array. `None` if any element is not a
/// number or the length is odd.
fn map_pairs(
    resolve: &dyn Fn(&Object) -> Object,
    items: &[Object],
    p: &PagePlacement,
) -> Option<Vec<Object>> {
    if !items.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(items.len());
    for pair in items.chunks_exact(2) {
        let (x, y) = match pair {
            [x, y] => (number_of(resolve, x)?, number_of(resolve, y)?),
            _ => return None,
        };
        out.push(Object::Real(p.x(x)));
        out.push(Object::Real(p.y(y)));
    }
    Some(out)
}

fn rect_of(resolve: &dyn Fn(&Object) -> Object, o: &Object) -> Option<Rect> {
    let arr = resolve(o);
    match arr.as_array()? {
        [a, b, c, d] => Some(Rect::from_corners(
            number_of(resolve, a)?,
            number_of(resolve, b)?,
            number_of(resolve, c)?,
            number_of(resolve, d)?,
        )),
        _ => None,
    }
}

fn rect_object(r: Rect) -> Object {
    Object::Array(vec![
        Object::Real(r.llx),
        Object::Real(r.lly),
        Object::Real(r.urx),
        Object::Real(r.ury),
    ])
}

/// Map a rectangle; the result is re-normalized (§7.9.5).
#[must_use]
pub(crate) fn map_rect(r: Rect, p: &PagePlacement) -> Rect {
    Rect::from_corners(p.x(r.llx), p.y(r.lly), p.x(r.urx), p.y(r.ury))
}

/// `/F` bit 4, NoZoom (§12.5.3 Table 165): the annotation keeps its size
/// and its upper-left corner stays fixed to the page.
fn no_zoom(resolve: &dyn Fn(&Object) -> Object, annot: &Dict) -> bool {
    annot
        .get(b"F")
        .and_then(|o| resolve(o).as_int())
        .is_some_and(|f| f & 8 != 0)
}

/// Transform one annotation's geometry. Returns the changed dictionary, or
/// `None` when it carries no geometry this recognises.
///
/// Rewritten: `/Rect` (NoZoom: moved, not resized), `/QuadPoints`,
/// `/Vertices`, `/L`, `/CL` (point arrays), `/InkList` and `/Path` (arrays
/// of point arrays), `/RD` (margins, × scale) and the line leader lengths
/// `/LL`, `/LLE`, `/LLO` (× scale). `/Measure` is the caller's, because it
/// may be a shared indirect object.
pub(crate) fn transform_annotation(
    resolve: &dyn Fn(&Object) -> Object,
    annot: &Dict,
    p: &PagePlacement,
) -> Option<Dict> {
    let mut out = annot.clone();
    let mut changed = false;
    let fixed_size = no_zoom(resolve, annot);

    if let Some(r) = annot.get(b"Rect").and_then(|o| rect_of(resolve, o)) {
        let mapped = if fixed_size {
            let (x, y) = (p.x(r.llx), p.y(r.ury));
            Rect::from_corners(x, y - r.height(), x + r.width(), y)
        } else {
            map_rect(r, p)
        };
        out.insert(Name::from(b"Rect"), rect_object(mapped));
        changed = true;
    }
    if fixed_size {
        return changed.then_some(out);
    }
    for key in [&b"QuadPoints"[..], b"Vertices", b"L", b"CL"] {
        let Some(items) = annot.get(key).map(resolve) else {
            continue;
        };
        if let Some(mapped) = items.as_array().and_then(|a| map_pairs(resolve, a, p)) {
            out.insert(Name::from(key), Object::Array(mapped));
            changed = true;
        }
    }
    for key in [&b"InkList"[..], b"Path"] {
        let Some(outer) = annot.get(key).map(resolve) else {
            continue;
        };
        let Some(strokes) = outer.as_array() else {
            continue;
        };
        let mapped: Option<Vec<Object>> = strokes
            .iter()
            .map(|s| {
                let s = resolve(s);
                s.as_array()
                    .and_then(|a| map_pairs(resolve, a, p))
                    .map(Object::Array)
            })
            .collect();
        if let Some(mapped) = mapped {
            out.insert(Name::from(key), Object::Array(mapped));
            changed = true;
        }
    }
    if let Some(rd) = annot.get(b"RD").map(resolve)
        && let Some(items) = rd.as_array()
    {
        let scaled: Option<Vec<Object>> = items
            .iter()
            .map(|o| number_of(resolve, o).map(|v| Object::Real(v * p.scale)))
            .collect();
        if let Some(scaled) = scaled {
            out.insert(Name::from(b"RD"), Object::Array(scaled));
            changed = true;
        }
    }
    for key in [&b"LL"[..], b"LLE", b"LLO"] {
        if let Some(v) = annot.get(key).and_then(|o| number_of(resolve, o)) {
            out.insert(Name::from(key), Object::Real(v * p.scale));
            changed = true;
        }
    }
    changed.then_some(out)
}

/// Rescale a `/Measure` dictionary's user-space conversion factors so a
/// measurement taken on the scaled page reads the same value (§12.9.1
/// Table 266, Table 267).
///
/// Only element 0 of each number-format array converts FROM user space;
/// later elements convert between units and are unchanged. Linear arrays
/// (`/X`, `/Y`, `/D`) are divided by the scale, `/A` (area) by its square;
/// `/T` (angle) and `/S` (slope) are scale-free.
///
/// Returns `Err(())` for `/Subtype /GEO`, which this does not rescale, and
/// `Ok(None)` when nothing changed.
#[allow(clippy::result_unit_err)] // crate-internal; the unit error IS the GEO answer
pub(crate) fn scale_measure(
    resolve: &dyn Fn(&Object) -> Object,
    measure: &Dict,
    scale: f64,
) -> Result<Option<Dict>, ()> {
    let is_geo = measure
        .get(b"Subtype")
        .map(resolve)
        .and_then(|o| o.as_name().map(|n| n.as_bytes() == b"GEO"))
        .unwrap_or(false);
    if is_geo {
        return Err(());
    }
    let mut out = measure.clone();
    let mut changed = false;
    for (key, power) in [(&b"X"[..], 1), (b"Y", 1), (b"D", 1), (b"A", 2)] {
        let Some(arr) = measure.get(key).map(resolve) else {
            continue;
        };
        let Some(items) = arr.as_array() else {
            continue;
        };
        let Some(first) = items.first().map(resolve) else {
            continue;
        };
        let Some(format) = first.as_dict() else {
            continue;
        };
        let Some(c) = format.get(b"C").and_then(|o| number_of(resolve, o)) else {
            continue;
        };
        let mut format = format.clone();
        format.insert(Name::from(b"C"), Object::Real(c / scale.powi(power)));
        let mut items = items.to_vec();
        if let Some(slot) = items.first_mut() {
            *slot = Object::Dict(format);
        }
        out.insert(Name::from(key), Object::Array(items));
        changed = true;
    }
    Ok(changed.then_some(out))
}

/// Transform a viewport (§12.9.1 Table 265): its `/BBox`. Its `/Measure` is
/// the caller's.
pub(crate) fn transform_viewport(
    resolve: &dyn Fn(&Object) -> Object,
    vp: &Dict,
    p: &PagePlacement,
) -> Option<Dict> {
    let r = vp.get(b"BBox").and_then(|o| rect_of(resolve, o))?;
    let mut out = vp.clone();
    out.insert(Name::from(b"BBox"), rect_object(map_rect(r, p)));
    Some(out)
}

/// Transform an article bead (§12.4.3 Table 162): its `/R`.
pub(crate) fn transform_bead(
    resolve: &dyn Fn(&Object) -> Object,
    bead: &Dict,
    p: &PagePlacement,
) -> Option<Dict> {
    let r = bead.get(b"R").and_then(|o| rect_of(resolve, o))?;
    let mut out = bead.clone();
    out.insert(Name::from(b"R"), rect_object(map_rect(r, p)));
    Some(out)
}

/// Transform an explicit destination array naming a scaled page
/// (§12.3.2.2 Table 149). `/XYZ`'s zoom and every `null` are kept; `/Fit`
/// and `/FitB` carry no coordinates. `None` when nothing changes or the
/// array is not a coordinate-bearing destination.
fn transform_destination(items: &[Object], p: &PagePlacement) -> Option<Vec<Object>> {
    let kind = items.get(1)?.as_name()?.as_bytes();
    // For each operand slot after the kind: which axis it lies on.
    let axes: &[u8] = match kind {
        b"XYZ" => b"xy",
        b"FitH" | b"FitBH" => b"y",
        b"FitV" | b"FitBV" => b"x",
        b"FitR" => b"xyxy",
        _ => return None,
    };
    let mut out = items.to_vec();
    let mut changed = false;
    for (i, axis) in axes.iter().enumerate() {
        let Some(slot) = out.get_mut(i + 2) else {
            break;
        };
        let Some(v) = slot.as_number() else {
            continue; // null: "unchanged" (Table 149)
        };
        *slot = Object::Real(if *axis == b'x' { p.x(v) } else { p.y(v) });
        changed = true;
    }
    changed.then_some(out)
}

/// Maximum nesting walked looking for destinations inside one object.
const MAX_DEST_DEPTH: usize = 32;

/// Rewrite every explicit destination inside `obj` that names a page in
/// `pages`, returning the changed value and how many were rewritten.
///
/// A destination array is recognised by shape — element 0 a reference to a
/// scaled page, element 1 a destination-kind name — wherever it sits
/// (outline item, `/Dest`, `/GoTo` `/D`, `/OpenAction`, name-tree leaf,
/// catalog `/Dests`). Streams are not entered.
pub(crate) fn rewrite_destinations(
    obj: &Object,
    pages: &HashMap<ObjId, PagePlacement>,
) -> Option<(Object, usize)> {
    fn walk(
        obj: &Object,
        pages: &HashMap<ObjId, PagePlacement>,
        depth: usize,
        count: &mut usize,
    ) -> Option<Object> {
        if depth > MAX_DEST_DEPTH {
            return None;
        }
        match obj {
            Object::Array(items) => {
                if let Some(p) = items
                    .first()
                    .and_then(Object::as_reference)
                    .and_then(|id| pages.get(&id))
                    && let Some(out) = transform_destination(items, p)
                {
                    *count += 1;
                    return Some(Object::Array(out));
                }
                let mut out: Option<Vec<Object>> = None;
                for (i, item) in items.iter().enumerate() {
                    if let Some(new) = walk(item, pages, depth + 1, count) {
                        let v = out.get_or_insert_with(|| items.clone());
                        if let Some(slot) = v.get_mut(i) {
                            *slot = new;
                        }
                    }
                }
                out.map(Object::Array)
            }
            Object::Dict(dict) => {
                let mut out: Option<Dict> = None;
                for (key, value) in dict.iter() {
                    if let Some(new) = walk(value, pages, depth + 1, count) {
                        out.get_or_insert_with(|| dict.clone())
                            .insert(key.clone(), new);
                    }
                }
                out.map(Object::Dict)
            }
            _ => None,
        }
    }
    let mut count = 0;
    walk(obj, pages, 0, &mut count).map(|o| (o, count))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
mod tests {
    use super::*;

    fn rect(llx: f64, lly: f64, urx: f64, ury: f64) -> Rect {
        Rect { llx, lly, urx, ury }
    }

    const LETTER: Rect = Rect {
        llx: 0.0,
        lly: 0.0,
        urx: 612.0,
        ury: 792.0,
    };

    #[test]
    fn letter_to_a4_fit_scales_by_the_tighter_axis_and_centres() {
        let p = plan_placement(LETTER, 0, &ScaleRequest::new(595.276, 841.89)).unwrap();
        let s = 595.276 / 612.0;
        assert!((p.scale - s).abs() < 1e-12);
        assert_eq!(p.offset_x, 0.0);
        assert!((p.offset_y - (841.89 - s * 792.0) / 2.0).abs() < 1e-9);
        assert!(!p.orientation_flipped);
    }

    #[test]
    fn fill_scales_by_the_looser_axis() {
        let r = ScaleRequest::new(595.276, 841.89).with_mode(ScaleMode::Fill);
        let p = plan_placement(LETTER, 0, &r).unwrap();
        assert!((p.scale - 841.89 / 792.0).abs() < 1e-12);
        assert!(p.offset_x < 0.0, "overflow is split both sides");
    }

    #[test]
    fn a_downscale_maps_the_corners_inside_the_target() {
        let a1 = rect(0.0, 0.0, 2383.94, 1683.78);
        let p = plan_placement(a1, 0, &ScaleRequest::new(612.0, 792.0)).unwrap();
        // Landscape page, portrait target, Match: target flipped.
        assert!(p.orientation_flipped);
        assert_eq!(p.target, rect(0.0, 0.0, 792.0, 612.0));
        let m = map_rect(a1, &p);
        assert!(m.llx >= -1e-9 && m.urx <= 792.0 + 1e-9);
        assert!(m.lly >= -1e-9 && m.ury <= 612.0 + 1e-9);
        assert!(p.scale < 0.34);
    }

    #[test]
    fn exact_keeps_the_target_as_given() {
        let wide = rect(0.0, 0.0, 800.0, 400.0);
        let r = ScaleRequest::new(400.0, 800.0).with_orientation(OrientationPolicy::Exact);
        let p = plan_placement(wide, 0, &r).unwrap();
        assert!(!p.orientation_flipped);
        assert_eq!(p.target, rect(0.0, 0.0, 400.0, 800.0));
        assert_eq!(p.scale, 0.5);
    }

    #[test]
    fn a_quarter_turned_page_is_matched_in_its_displayed_orientation() {
        // Unrotated portrait, /Rotate 90 → displayed landscape.
        let p = plan_placement(LETTER, 90, &ScaleRequest::new(1190.55, 841.89)).unwrap();
        // Displayed landscape meets a landscape target: no flip, and the
        // unrotated sheet is portrait.
        assert!(!p.orientation_flipped);
        assert_eq!(p.target, rect(0.0, 0.0, 841.89, 1190.55));
    }

    #[test]
    fn a_non_zero_origin_is_translated_away() {
        let src = rect(100.0, 200.0, 712.0, 992.0);
        let p = plan_placement(src, 0, &ScaleRequest::new(612.0, 792.0)).unwrap();
        assert_eq!(p.scale, 1.0);
        assert_eq!((p.x(100.0), p.y(200.0)), (0.0, 0.0));
    }

    #[test]
    fn degenerate_sizes_are_refused() {
        assert!(plan_placement(LETTER, 0, &ScaleRequest::new(0.0, 10.0)).is_none());
        assert!(plan_placement(LETTER, 0, &ScaleRequest::new(f64::NAN, 10.0)).is_none());
        assert!(
            plan_placement(rect(0.0, 0.0, 0.0, 5.0), 0, &ScaleRequest::new(9.0, 9.0)).is_none()
        );
    }

    #[test]
    fn the_prefix_is_a_cm_and_a_clip() {
        let p = plan_placement(LETTER, 0, &ScaleRequest::new(306.0, 396.0)).unwrap();
        let s = String::from_utf8(content_prefix(LETTER, &p)).unwrap();
        assert_eq!(s, "q\n0.5 0 0 0.5 0 0 cm\n0 0 612 792 re W n\n");
    }

    fn id_resolve(o: &Object) -> Object {
        o.clone()
    }

    fn nums(o: &Object) -> Vec<f64> {
        o.as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_number().unwrap())
            .collect()
    }

    fn half() -> PagePlacement {
        plan_placement(LETTER, 0, &ScaleRequest::new(306.0, 396.0)).unwrap()
    }

    fn arr(v: &[f64]) -> Object {
        Object::Array(v.iter().map(|x| Object::Real(*x)).collect())
    }

    #[test]
    fn annotation_geometry_follows_the_content() {
        let mut d = Dict::new();
        d.insert(Name::from(b"Rect"), arr(&[100.0, 100.0, 200.0, 150.0]));
        d.insert(
            Name::from(b"QuadPoints"),
            arr(&[100.0, 150.0, 200.0, 150.0, 100.0, 100.0, 200.0, 100.0]),
        );
        d.insert(
            Name::from(b"InkList"),
            Object::Array(vec![arr(&[10.0, 20.0, 30.0, 40.0])]),
        );
        d.insert(Name::from(b"RD"), arr(&[2.0, 2.0, 2.0, 2.0]));
        d.insert(Name::from(b"LL"), Object::Real(10.0));
        let out = transform_annotation(&id_resolve, &d, &half()).unwrap();
        assert_eq!(nums(out.get(b"Rect").unwrap()), [50.0, 50.0, 100.0, 75.0]);
        assert_eq!(nums(out.get(b"QuadPoints").unwrap())[..2], [50.0, 75.0]);
        let ink = out.get(b"InkList").unwrap().as_array().unwrap();
        assert_eq!(nums(&ink[0]), [5.0, 10.0, 15.0, 20.0]);
        assert_eq!(nums(out.get(b"RD").unwrap()), [1.0; 4]);
        assert_eq!(out.get(b"LL").unwrap().as_number(), Some(5.0));
    }

    #[test]
    fn a_no_zoom_annotation_moves_but_keeps_its_size() {
        let mut d = Dict::new();
        d.insert(Name::from(b"Rect"), arr(&[100.0, 100.0, 120.0, 120.0]));
        d.insert(Name::from(b"F"), Object::Integer(8));
        let out = transform_annotation(&id_resolve, &d, &half()).unwrap();
        // Upper-left (100,120) → (50,60); size 20×20 kept.
        assert_eq!(nums(out.get(b"Rect").unwrap()), [50.0, 40.0, 70.0, 60.0]);
    }

    fn format(c: f64) -> Object {
        let mut f = Dict::new();
        f.insert(Name::from(b"U"), Object::String(b"mm".to_vec()));
        f.insert(Name::from(b"C"), Object::Real(c));
        Object::Dict(f)
    }

    #[test]
    fn a_measure_still_reads_true_after_scaling() {
        let mut m = Dict::new();
        m.insert(
            Name::from(b"X"),
            Object::Array(vec![format(0.5), format(10.0)]),
        );
        m.insert(Name::from(b"A"), Object::Array(vec![format(0.25)]));
        m.insert(Name::from(b"T"), Object::Array(vec![format(1.0)]));
        let out = scale_measure(&id_resolve, &m, 0.5).unwrap().unwrap();
        let c = |k: &[u8], i: usize| {
            out.get(k).unwrap().as_array().unwrap()[i]
                .as_dict()
                .unwrap()
                .get(b"C")
                .unwrap()
                .as_number()
                .unwrap()
        };
        assert_eq!(c(b"X", 0), 1.0);
        assert_eq!(c(b"X", 1), 10.0, "unit-to-unit factors are scale-free");
        assert_eq!(c(b"A", 0), 1.0);
        assert_eq!(c(b"T", 0), 1.0);
    }

    #[test]
    fn a_geo_measure_is_reported_not_rescaled() {
        let mut m = Dict::new();
        m.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"GEO")));
        assert!(scale_measure(&id_resolve, &m, 0.5).is_err());
    }

    #[test]
    fn destinations_naming_a_scaled_page_are_transformed_wherever_they_sit() {
        let page = ObjId::new(3, 0);
        let other = ObjId::new(4, 0);
        let pages = HashMap::from([(page, half())]);
        let dest = |id, kind: &[u8], ops: Vec<Object>| {
            let mut v = vec![Object::Reference(id), Object::Name(Name::from(kind))];
            v.extend(ops);
            Object::Array(v)
        };
        let mut action = Dict::new();
        action.insert(
            Name::from(b"D"),
            dest(
                page,
                b"XYZ",
                vec![Object::Real(100.0), Object::Null, Object::Real(2.0)],
            ),
        );
        let mut holder = Dict::new();
        holder.insert(Name::from(b"A"), Object::Dict(action));
        holder.insert(
            Name::from(b"Names"),
            Object::Array(vec![
                Object::String(b"a".to_vec()),
                dest(
                    page,
                    b"FitR",
                    vec![
                        Object::Real(0.0),
                        Object::Real(0.0),
                        Object::Real(100.0),
                        Object::Real(200.0),
                    ],
                ),
                Object::String(b"b".to_vec()),
                dest(
                    other,
                    b"XYZ",
                    vec![Object::Real(1.0), Object::Real(1.0), Object::Null],
                ),
                Object::String(b"c".to_vec()),
                dest(page, b"Fit", vec![]),
            ]),
        );
        let (out, n) = rewrite_destinations(&Object::Dict(holder), &pages).unwrap();
        assert_eq!(n, 2);
        let out = out.as_dict().unwrap();
        let xyz = out.get(b"A").unwrap().as_dict().unwrap().get(b"D").unwrap();
        let xyz = xyz.as_array().unwrap();
        assert_eq!(xyz[2].as_number(), Some(50.0));
        assert_eq!(xyz[3], Object::Null);
        assert_eq!(xyz[4].as_number(), Some(2.0), "zoom is kept");
        let names = out.get(b"Names").unwrap().as_array().unwrap();
        assert_eq!(
            nums(&Object::Array(names[1].as_array().unwrap()[2..].to_vec())),
            [0.0, 0.0, 50.0, 100.0]
        );
        assert_eq!(
            names[3].as_array().unwrap()[2].as_number(),
            Some(1.0),
            "another page is untouched"
        );
    }

    #[test]
    fn an_object_with_no_destination_is_not_rewritten() {
        let pages = HashMap::from([(ObjId::new(3, 0), half())]);
        let o = Object::Array(vec![
            Object::Reference(ObjId::new(3, 0)),
            Object::Reference(ObjId::new(5, 0)),
        ]);
        assert!(rewrite_destinations(&o, &pages).is_none());
    }
}
