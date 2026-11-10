//! Recognising a rectangular clip that cannot change the clip in force.
//!
//! A form XObject's `/BBox` clip (ISO 32000-2 §8.10.1) is usually the same
//! page-sized rectangle at every nesting level. Building its full-canvas
//! mask only to multiply it into a clip it already contains is pure cost,
//! and that cost scales with canvas area, once per form.

use tiny_skia::{Path, PathSegment, Point, Transform};

/// Device-space slack when matching a vertex to a rectangle corner.
const TOL: f32 = 1e-3;

/// `true` when intersecting the clip in force with `path` (filled under
/// `ctm`) is an exact identity, so no mask needs to be built.
///
/// `clipped` says whether a clip is in force; `bbox` is its device-space
/// bounds (`GraphicsState::clip_bbox`). That holds when `path` is one
/// axis-aligned rectangle in device space that fully covers every pixel
/// the current clip lets through: the whole canvas when nothing is
/// clipped, else `bbox` rounded out to whole pixels. A fully covered pixel
/// gets mask coverage 255, and `x × 255 / 255 == x`. A clip with no known
/// bounds is never skipped.
pub(super) fn rect_clip_is_identity(
    path: &Path,
    ctm: Transform,
    clipped: bool,
    bbox: Option<(f32, f32, f32, f32)>,
    canvas: (u32, u32),
) -> bool {
    #[allow(clippy::cast_precision_loss)] // Canvas sides are far below 2^24.
    let (w, h) = (canvas.0 as f32, canvas.1 as f32);
    let need = match (clipped, bbox) {
        (false, _) => (0.0, 0.0, w, h),
        (true, Some((l, t, r, b))) => (
            l.floor().max(0.0),
            t.floor().max(0.0),
            r.ceil().min(w),
            b.ceil().min(h),
        ),
        (true, None) => return false,
    };
    if need.2 <= need.0 || need.3 <= need.1 {
        // Nothing gets through the clip in force; intersecting keeps it so.
        return clipped;
    }
    device_rect(path, ctm).is_some_and(|(l, t, r, b)| {
        l <= need.0 + TOL && t <= need.1 + TOL && r >= need.2 - TOL && b >= need.3 - TOL
    })
}

/// The device bounds of `path` when it is one closed loop through the four
/// corners of an axis-aligned rectangle, each edge horizontal or vertical.
fn device_rect(path: &Path, ctm: Transform) -> Option<(f32, f32, f32, f32)> {
    let mut pts: Vec<Point> = Vec::with_capacity(5);
    for (i, seg) in path.segments().enumerate() {
        match seg {
            PathSegment::MoveTo(p) if i == 0 => pts.push(p),
            PathSegment::LineTo(p) if i > 0 => pts.push(p),
            PathSegment::Close => {}
            _ => return None,
        }
        if pts.len() > 5 {
            return None;
        }
    }
    ctm.map_points(&mut pts);
    let quad = match *pts.as_slice() {
        [a, b, c, d] => [a, b, c, d],
        [a, b, c, d, e] if near(a, e) => [a, b, c, d],
        _ => return None,
    };
    let b = tiny_skia::Rect::from_points(&quad)?;
    let (l, t, r, bt) = (b.left(), b.top(), b.right(), b.bottom());
    let corner = |p: Point| {
        let x = if (p.x - l).abs() <= TOL {
            0
        } else if (p.x - r).abs() <= TOL {
            1
        } else {
            return None;
        };
        let y = if (p.y - t).abs() <= TOL {
            0
        } else if (p.y - bt).abs() <= TOL {
            2
        } else {
            return None;
        };
        Some::<u8>(x | y)
    };
    let [a, b, c, d] = quad;
    let ids = [corner(a)?, corner(b)?, corner(c)?, corner(d)?];
    let all = ids.iter().fold(0u8, |m, &c| m | (1 << c)) == 0b1111;
    // Adjacent corners differ in exactly one of the two coordinate bits.
    let [i0, i1, i2, i3] = ids;
    let edges = [(i0, i1), (i1, i2), (i2, i3), (i3, i0)]
        .iter()
        .all(|(x, y)| (x ^ y).count_ones() == 1);
    (all && edges).then_some((l, t, r, bt))
}

fn near(a: Point, b: Point) -> bool {
    (a.x - b.x).abs() <= TOL && (a.y - b.y).abs() <= TOL
}

#[cfg(test)]
#[allow(clippy::expect_used)] // Tests fail loudly by design.
mod tests {
    use super::*;
    use tiny_skia::{FillRule, Mask, PathBuilder, Rect};

    fn rect(l: f32, t: f32, r: f32, b: f32) -> Path {
        PathBuilder::from_rect(Rect::from_ltrb(l, t, r, b).expect("valid rect"))
    }

    #[test]
    fn a_canvas_sized_rect_is_an_identity_when_nothing_is_clipped() {
        let p = rect(0.0, 0.0, 200.0, 200.0);
        let id = Transform::identity();
        assert!(rect_clip_is_identity(&p, id, false, None, (200, 200)));
        let scaled = Transform::from_scale(10.0, -10.0).post_translate(0.0, 2000.0);
        assert!(rect_clip_is_identity(&p, scaled, false, None, (2000, 2000)));
        assert!(!rect_clip_is_identity(&p, id, false, None, (201, 200)));
    }

    #[test]
    fn a_rect_must_cover_the_clip_rounded_out_to_whole_pixels() {
        let id = Transform::identity();
        let bbox = Some((10.5, 10.5, 50.5, 50.5));
        let outer = rect(10.0, 10.0, 51.0, 51.0);
        let same = rect(10.5, 10.5, 50.5, 50.5);
        assert!(rect_clip_is_identity(&outer, id, true, bbox, (100, 100)));
        // Same rectangle: its edge pixels are partly covered, so a second
        // application would darken them.
        assert!(!rect_clip_is_identity(&same, id, true, bbox, (100, 100)));
        assert!(!rect_clip_is_identity(&outer, id, true, None, (100, 100)));
    }

    #[test]
    fn rotated_or_non_rectangular_paths_are_never_skipped() {
        let p = rect(-50.0, -50.0, 250.0, 250.0);
        let rot = Transform::from_rotate_at(10.0, 100.0, 100.0);
        assert!(!rect_clip_is_identity(&p, rot, false, None, (200, 200)));
        let mut pb = PathBuilder::new();
        pb.move_to(0.0, 0.0);
        pb.line_to(200.0, 200.0);
        pb.line_to(200.0, 0.0);
        pb.line_to(0.0, 200.0);
        pb.close();
        let bowtie = pb.finish().expect("four points make a path");
        let id = Transform::identity();
        assert!(!rect_clip_is_identity(&bowtie, id, false, None, (200, 200)));
    }

    #[test]
    fn a_covering_rect_masks_every_pixel_at_full_coverage() {
        // The identity rests on this: AA leaves no interior pixel below 255.
        let mut m = Mask::new(64, 48).expect("mask");
        let p = rect(0.0, 0.0, 64.0, 48.0);
        m.fill_path(&p, FillRule::Winding, true, Transform::identity());
        assert!(m.data().iter().all(|&v| v == 255));
    }
}
