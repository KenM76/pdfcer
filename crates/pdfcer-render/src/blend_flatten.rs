//! Bake blend modes out of a vector export whose consumer cannot blend.
//!
//! EMF has no blend modes and Word ignores SVG's `mix-blend-mode`, so a
//! `/Multiply` highlight written as itself paints opaque yellow over the
//! text it marks. Each top-level blended op is replaced by one of two
//! source-over equivalents, judged against the backdrop it blends with
//! (§11.3.5: everything recorded before it):
//!
//! 1. **Underlay** (vector). The op, made source-over, is moved beneath
//!    the whole page. A Multiply mark over white paper and black ink looks
//!    the same either way (`B·C` against `ink over C`), so the highlight
//!    stays an editable shape. Taken only when re-rendering the page that
//!    way matches the true blend within [`UNDERLAY_TOLERANCE`] on every
//!    channel of every pixel the op covers; coloured ink under a coloured
//!    mark fails the check.
//! 2. **Patch** (raster). A pixel patch `P` chosen so `P` drawn source-over
//!    the backdrop reproduces the blend: `αP = αop`,
//!    `P = after − (1 − αop)·before` per premultiplied channel, clamped to
//!    `[0, αP]`.
//!
//! Only the TOP level is flattened. Inside a transparency group the
//! contents blend against the group's own transparent backdrop (§11.4.7),
//! which the writers already reproduce by rasterising the group.

use std::sync::Arc;

use tiny_skia::{FillRule, Pixmap, PremultipliedColorU8, Transform};

use crate::canvas::BrushSpec;
use crate::display_list::{ClipDef, DeviceBounds, MaskBuilder, Op, replay_ops};
use tiny_skia::BlendMode;

/// Largest per-channel difference (0-255, premultiplied) at which an
/// underlay counts as looking the same as the blend. Covers rounding in the
/// two compositing orders; a visible colour shift is far larger.
pub(crate) const UNDERLAY_TOLERANCE: u8 = 2;

/// How the blended ops were baked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Flattened {
    /// Moved beneath the page as source-over vectors.
    pub underlaid: usize,
    /// Replaced by a pixel patch (or by nothing, for an op that painted
    /// nothing).
    pub rasterised: usize,
}

impl Flattened {
    /// Every blended op baked, by either route.
    pub fn total(self) -> usize {
        self.underlaid + self.rasterised
    }
}

/// Replace every top-level blended op in `ops` with a source-over
/// equivalent: an underlay where `underlay` is set and it looks the same,
/// a pixel patch otherwise.
pub fn flatten_blends(
    ops: &mut Vec<Op>,
    clips: &[ClipDef],
    width: u32,
    height: u32,
    underlay: bool,
) -> Flattened {
    let mut out = Flattened::default();
    let Some(last) = ops.iter().rposition(is_blended) else {
        return out;
    };
    let Some(mut backdrop) = Pixmap::new(width, height) else {
        return out;
    };
    let region = full_region(width, height);
    let mut masks = MaskBuilder::new(clips, width, height, 0.0, 0.0);
    let mut underlays: Vec<Op> = Vec::new();
    for i in 0..=last {
        let Some(op) = ops.get(i).cloned() else {
            break;
        };
        if !is_blended(&op) {
            replay_ops(std::slice::from_ref(&op), &mut backdrop, &mut masks, region);
            continue;
        }
        let Some(mut alone) = Pixmap::new(width, height) else {
            continue;
        };
        replay_ops(std::slice::from_ref(&op), &mut alone, &mut masks, region);
        let Some(bbox) = painted_box(&alone) else {
            set(ops, i, empty_op());
            out.rasterised += 1;
            continue;
        };
        let before = copy_box(&backdrop, bbox);
        replay_ops(std::slice::from_ref(&op), &mut backdrop, &mut masks, region);
        if underlay && let Some(under) = source_over(&op) {
            let mut probe = underlays.clone();
            probe.push(under);
            let matches = ops.get(..i).is_some_and(|prior| {
                underlay_matches(&probe, prior, &backdrop, bbox, &mut masks, region)
            });
            if matches {
                underlays = probe;
                set(ops, i, empty_op());
                out.underlaid += 1;
                continue;
            }
        }
        if let Some(p) = patch(&alone, &before, &backdrop, bbox) {
            set(ops, i, p);
            out.rasterised += 1;
        }
    }
    if !underlays.is_empty() {
        ops.splice(0..0, underlays);
    }
    out
}

fn set(ops: &mut [Op], i: usize, op: Op) {
    if let Some(slot) = ops.get_mut(i) {
        *slot = op;
    }
}

fn full_region(width: u32, height: u32) -> DeviceBounds {
    #[allow(clippy::cast_precision_loss)]
    DeviceBounds {
        left: 0.0,
        top: 0.0,
        right: width as f32,
        bottom: height as f32,
    }
}

/// `op` with its blend made source-over, or `None` when it cannot be an
/// underlay (text, or a non-separable group blend).
fn source_over(op: &Op) -> Option<Op> {
    let mut op = op.clone();
    match &mut op {
        Op::Fill { brush, .. } | Op::Stroke { brush, .. } => brush.blend = BlendMode::SourceOver,
        Op::Layer { paint, .. } if paint.nonseparable.is_none() => {
            paint.blend = BlendMode::SourceOver;
        }
        _ => return None,
    }
    Some(op)
}

/// Whether drawing `under` beneath `prior` reproduces `truth` inside `bbox`.
fn underlay_matches(
    under: &[Op],
    prior: &[Op],
    truth: &Pixmap,
    bbox: PxBox,
    masks: &mut MaskBuilder<'_>,
    region: DeviceBounds,
) -> bool {
    let Some(mut probe) = Pixmap::new(truth.width(), truth.height()) else {
        return false;
    };
    replay_ops(under, &mut probe, masks, region);
    replay_ops(prior, &mut probe, masks, region);
    let near = |a: u8, b: u8| a.abs_diff(b) <= UNDERLAY_TOLERANCE;
    copy_box(&probe, bbox)
        .iter()
        .zip(copy_box(truth, bbox).iter())
        .all(|(p, t)| {
            near(p.red(), t.red())
                && near(p.green(), t.green())
                && near(p.blue(), t.blue())
                && near(p.alpha(), t.alpha())
        })
}

/// Whether `op` composites against the page with a mode other than normal.
fn is_blended(op: &Op) -> bool {
    match op {
        Op::Fill { brush, .. } | Op::Stroke { brush, .. } => brush.blend != BlendMode::SourceOver,
        Op::Layer { paint, .. } => {
            paint.blend != BlendMode::SourceOver || paint.nonseparable.is_some()
        }
        Op::Text { ops, .. } => ops.iter().any(is_blended),
    }
}

/// `(x0, y0, width, height)` of the pixels with any coverage.
type PxBox = (u32, u32, u32, u32);

fn painted_box(p: &Pixmap) -> Option<PxBox> {
    let w = p.width();
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0u32, 0u32);
    for (i, px) in p.pixels().iter().enumerate() {
        if px.alpha() == 0 {
            continue;
        }
        #[allow(clippy::cast_possible_truncation)]
        let (x, y) = ((i as u32) % w, (i as u32) / w);
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x);
        y1 = y1.max(y);
    }
    (x0 != u32::MAX).then(|| (x0, y0, x1 - x0 + 1, y1 - y0 + 1))
}

fn copy_box(p: &Pixmap, (x0, y0, bw, bh): PxBox) -> Vec<PremultipliedColorU8> {
    let w = p.width();
    let mut out = Vec::with_capacity((bw * bh) as usize);
    for row in 0..bh {
        let s0 = ((y0 + row) * w + x0) as usize;
        out.extend_from_slice(p.pixels().get(s0..s0 + bw as usize).unwrap_or_default());
    }
    out
}

/// The source-over patch over `bbox` (module docs for the formula).
fn patch(
    alone: &Pixmap,
    before: &[PremultipliedColorU8],
    after: &Pixmap,
    bbox: PxBox,
) -> Option<Op> {
    let (x0, y0, bw, bh) = bbox;
    let mut texels = Pixmap::new(bw, bh)?;
    let a_box = copy_box(alone, bbox);
    let r_box = copy_box(after, bbox);
    for (((dst, a), b), r) in texels
        .pixels_mut()
        .iter_mut()
        .zip(&a_box)
        .zip(before)
        .zip(&r_box)
    {
        let alpha = a.alpha();
        let keep = 255 - u16::from(alpha);
        let ch = |r: u8, b: u8| {
            let under = (u16::from(b) * keep + 127) / 255;
            #[allow(clippy::cast_possible_truncation)]
            let v = u16::from(r).saturating_sub(under).min(u16::from(alpha)) as u8;
            v
        };
        *dst = PremultipliedColorU8::from_rgba(
            ch(r.red(), b.red()),
            ch(r.green(), b.green()),
            ch(r.blue(), b.blue()),
            alpha,
        )
        .unwrap_or(PremultipliedColorU8::TRANSPARENT);
    }
    #[allow(clippy::cast_precision_loss)]
    let (fx, fy, fw, fh) = (x0 as f32, y0 as f32, bw as f32, bh as f32);
    let rect = tiny_skia::Rect::from_xywh(fx, fy, fw, fh)?;
    Some(Op::Fill {
        path: Arc::new(tiny_skia::PathBuilder::from_rect(rect)),
        brush: BrushSpec::raster(Arc::new(texels), Transform::from_translate(fx, fy)),
        rule: FillRule::Winding,
        // Device space already: the patch IS the page grid.
        ctm: Transform::identity(),
        clip: None,
        bounds: Some(DeviceBounds {
            left: fx,
            top: fy,
            right: fx + fw,
            bottom: fy + fh,
        }),
    })
}

/// An op that paints nothing, for a blended op that painted nothing.
fn empty_op() -> Op {
    Op::Layer {
        paint: crate::canvas::LayerPaint {
            opacity: 1.0,
            blend: BlendMode::SourceOver,
            nonseparable: None,
        },
        ops: Vec::new(),
        mask: None,
    }
}
