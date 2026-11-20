//! A shading on the cache recorder: kept as [`Op::Shading`] for replay to
//! evaluate, where a direct render paints it into the destination.
//!
//! [`Op::Shading`]: crate::display_list::Op::Shading

use std::sync::Arc;

use tiny_skia::Transform;

use super::Interpreter;
use crate::canvas::Canvas;
use crate::display_list::{ShadingArea, ShadingOp};
use crate::shading::Shading;

/// What a shading site hands the recorder besides the shading itself.
pub(super) struct ShadingPaint<'p> {
    /// Shading space → device.
    pub(super) to_device: Transform,
    /// The device pixels the painter would walk.
    pub(super) region: (i32, i32, i32, i32),
    /// Constant alpha.
    pub(super) alpha: f32,
    /// A pattern fill's path, rule and CTM; `None` for `sh`.
    pub(super) area: Option<(&'p tiny_skia::Path, tiny_skia::FillRule, Transform)>,
}

impl Interpreter<'_> {
    /// Record `shading` on the cache recorder ([`Canvas::caching`]);
    /// whether a direct render would have painted it.
    pub(super) fn record_shading(
        &mut self,
        shading: Shading,
        paint: ShadingPaint<'_>,
        canvas: &mut Canvas<'_>,
    ) -> bool {
        let paintable = shading.is_paintable();
        let canvas_pixels = u64::from(canvas.width()) * u64::from(canvas.height());
        let area = paint.area.map(|(path, rule, ctm)| ShadingArea {
            path: Arc::new(path.clone()),
            rule,
            ctm,
            anti_alias: self.edge_aa,
        });
        canvas.push_shading(ShadingOp {
            shading: Arc::new(shading),
            to_device: paint.to_device,
            region: paint.region,
            clip: self.gs.current.clip_ref().id,
            alpha: paint.alpha,
            area,
            canvas_pixels,
        });
        paintable
    }
}
