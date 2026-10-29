//! An isolated group whose declared blending colour space differs from its
//! parent's (ISO 32000-2 §11.3.4, §11.4.7 Table 147 `CS`): its contents
//! blend in its own space, and only its result crosses into the parent's.
//!
//! A non-isolated group ignores `CS` and inherits (§11.6.6), so it never
//! reaches here with a space of its own.

use tiny_skia::{Mask, Pixmap};

use super::{Canvas, GroupOutcome, LayerPaint};
use crate::cmyk_buffer::CmykBuffer;
use crate::compositor::{Blend, BlendSpace};
use crate::display_list::PoisonReason;

/// The parameters a fresh colorant buffer needs on a page that has none.
#[derive(Debug, Clone, Copy)]
pub(crate) struct InkSetup {
    pub intent: pdfcer_core::settings::CmykIntent,
    pub max_bytes: Option<usize>,
}

/// What the group's interior is composited with: §11.4.5 applies the outer
/// state to the group's result, so the interior starts neutral.
const NEUTRAL: LayerPaint = LayerPaint {
    opacity: 1.0,
    blend: tiny_skia::BlendMode::SourceOver,
    nonseparable: None,
};

impl Canvas<'_> {
    /// [`Canvas::group`] for a group that may declare its own blending
    /// space. `declared` is the group's resolved `CS`, ignored unless
    /// `isolated`; absent, or naming the space this canvas already
    /// composites in, this is exactly [`Canvas::group`].
    ///
    /// Otherwise the interior runs in a buffer of the declared space — a
    /// colorant buffer for a subtractive group on an sRGB canvas, a pixmap
    /// for an additive group on a colorant canvas — and that buffer,
    /// converted, is the isolated group's result, which the parent then
    /// composites under the outer state and mask as for any group.
    ///
    /// A recording cannot hold ink, so a subtractive group refuses it with
    /// [`PoisonReason::ColorantBuffer`] and composites in sRGB. A colorant
    /// buffer that cannot be allocated does the same, and the interior's
    /// blends are then counted by `blends_in_wrong_space`.
    #[allow(clippy::too_many_arguments)] // `group`'s seven plus the space and its buffer setup.
    pub(crate) fn group_in_space<R>(
        &mut self,
        declared: Option<BlendSpace>,
        ink: InkSetup,
        paint: LayerPaint,
        isolated: bool,
        knockout: bool,
        mask: Option<&Mask>,
        bounds: Option<tiny_skia::IntRect>,
        mut f: impl FnMut(&mut Canvas<'_>) -> (R, bool),
    ) -> Option<GroupOutcome<R>> {
        let Some(space) = declared.filter(|_| isolated) else {
            return self.group(paint, isolated, knockout, mask, bounds, f);
        };
        let on_ink = matches!(self, Self::Cmyk(_));
        if space.is_subtractive() == on_ink {
            return self.group(paint, true, knockout, mask, bounds, f);
        }
        if matches!(self, Self::Record(_)) {
            self.refuse(PoisonReason::ColorantBuffer);
            return self.group(paint, true, knockout, mask, bounds, f);
        }
        let (w, h) = (self.width(), self.height());
        if on_ink {
            let outer = self.group(paint, true, false, mask, bounds, |sub| {
                (additive_interior(sub, w, h, knockout, &mut f), false)
            })?;
            return outer.result;
        }
        let Some(mut buffer) = CmykBuffer::new(w, h, ink.intent, ink.max_bytes) else {
            return self.group(paint, true, knockout, mask, bounds, f);
        };
        let outer = self.group(paint, true, false, mask, bounds, |sub| {
            (ink_interior(sub, &mut buffer, knockout, &mut f), false)
        })?;
        outer.result
    }
}

/// Run the interior of an isolated group into `target`, a fresh
/// transparent buffer: directly, or as a knockout group over it.
fn run_isolated<R>(
    target: &mut Canvas<'_>,
    knockout: bool,
    f: &mut impl FnMut(&mut Canvas<'_>) -> (R, bool),
) -> Option<GroupOutcome<R>> {
    if knockout {
        return target.group(NEUTRAL, true, true, None, None, f);
    }
    let (result, _) = f(target);
    Some(GroupOutcome {
        result,
        backdrop_rerun: false,
        knockout_approximated: 0,
    })
}

/// A subtractive group on an sRGB canvas: blend in ink, then hand the
/// parent the result converted to sRGB with its alpha kept, through the
/// same intent and spot fold the page's own colorant buffer uses.
fn ink_interior<R>(
    sub: &mut Canvas<'_>,
    buffer: &mut CmykBuffer,
    knockout: bool,
    f: &mut impl FnMut(&mut Canvas<'_>) -> (R, bool),
) -> Option<GroupOutcome<R>> {
    let outcome = run_isolated(&mut Canvas::Cmyk(buffer), knockout, f)?;
    if let (Canvas::Paint(p), Some(result)) = (sub, buffer.to_srgb_transparent()) {
        p.data_mut().copy_from_slice(result.data());
    }
    Some(outcome)
}

/// An additive group on a colorant canvas: blend in sRGB, then composite
/// the result into the (transparent) child colorant buffer.
fn additive_interior<R>(
    sub: &mut Canvas<'_>,
    w: u32,
    h: u32,
    knockout: bool,
    f: &mut impl FnMut(&mut Canvas<'_>) -> (R, bool),
) -> Option<GroupOutcome<R>> {
    let mut pixmap = Pixmap::new(w, h)?;
    let outcome = run_isolated(&mut Canvas::Paint(&mut pixmap), knockout, f)?;
    if let Some(b) = sub.cmyk_mut() {
        b.composite_srgb(&pixmap, (0, 0, w, h), 1.0, Blend::Normal);
    }
    Some(outcome)
}
