//! Where on the page an export recording approximated something
//! (`Pass 700.12`).
//!
//! The export recorder counts each approximation it makes in
//! [`crate::display_list::ExportTally`]. A writer that can replace an
//! approximation with the true pixels also needs to know WHERE it was: the
//! device box of the op the approximation was made for. This log keeps it.
//! It never changes the recording itself.

use crate::display_list::{DeviceBounds, Op, PoisonReason};

/// What a site approximated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteCause {
    /// A paint under overprint drawn as a knockout.
    Overprint,
    /// A non-isolated group recorded as isolated (§11.4.4).
    NonIsolatedGroup,
    /// A blend drawn `Normal`.
    Blend,
    /// Compositing done in RGB that the raster does in a subtractive
    /// blending space (§11.7.2).
    BlendingSpace,
}

/// One approximated region, in device pixels at the recording scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Site {
    pub cause: SiteCause,
    pub bounds: DeviceBounds,
}

/// A cause whose op has not been pushed yet.
#[derive(Debug, Clone, Copy)]
struct Pending {
    cause: SiteCause,
    /// Frame depth the poison was raised at.
    depth: usize,
    /// How many painted boxes preceded it (overprint only reads these).
    painted_before: usize,
}

/// The recorder's site log. Export mode only.
#[derive(Debug, Default)]
pub(crate) struct SiteLog {
    /// Resolved sites, in recording order.
    pub sites: Vec<Site>,
    pending: Vec<Pending>,
    /// Boxes of the leaf paints recorded so far.
    painted: Vec<DeviceBounds>,
    /// The page group's blending space is subtractive: every compositing
    /// op on the page is a [`SiteCause::BlendingSpace`] site, which the
    /// writer decides from the finished op list.
    pub subtractive_page: bool,
    /// The page-level colorant refusal is still to come and must not open
    /// a per-op site.
    skip_page_colorant: bool,
}

impl SiteLog {
    /// Mark the page subtractive, before the walk refuses it once.
    pub(crate) fn mark_subtractive_page(&mut self) {
        self.subtractive_page = true;
        self.skip_page_colorant = true;
    }

    /// A poison raised at frame `depth`. `last` is the box of the op most
    /// recently pushed into the innermost frame (a non-isolated group is
    /// poisoned after its layer is pushed).
    pub(crate) fn poisoned(
        &mut self,
        reason: PoisonReason,
        depth: usize,
        last: Option<Option<DeviceBounds>>,
    ) {
        let cause = match reason {
            PoisonReason::Overprint => SiteCause::Overprint,
            PoisonReason::NonSeparableBlend => SiteCause::Blend,
            PoisonReason::NonIsolatedGroup => {
                if let Some(bounds) = last {
                    self.sites.push(Site {
                        cause: SiteCause::NonIsolatedGroup,
                        bounds: bounds.unwrap_or(UNKNOWN),
                    });
                }
                return;
            }
            PoisonReason::ColorantBuffer if self.skip_page_colorant => {
                self.skip_page_colorant = false;
                return;
            }
            PoisonReason::ColorantBuffer => SiteCause::BlendingSpace,
            _ => return,
        };
        self.pending.push(Pending {
            cause,
            depth,
            painted_before: self.painted.len(),
        });
    }

    /// An op with device box `bounds` was recorded at frame `depth`:
    /// resolve every cause raised at that depth or deeper. `leaf` adds the
    /// box to the painted set.
    pub(crate) fn pushed(&mut self, depth: usize, bounds: Option<DeviceBounds>, leaf: bool) {
        let bounds = bounds.unwrap_or(UNKNOWN);
        let mut i = 0;
        while let Some(p) = self.pending.get(i).copied() {
            if p.depth < depth {
                i += 1;
                continue;
            }
            self.pending.remove(i);
            let region = if p.cause == SiteCause::Overprint {
                self.beneath(bounds, p.painted_before)
            } else {
                Some(bounds)
            };
            if let Some(bounds) = region {
                self.sites.push(Site {
                    cause: p.cause,
                    bounds,
                });
            }
        }
        if leaf {
            self.painted.push(bounds);
        }
    }

    /// End of the walk: a cause whose op never arrived is placed over the
    /// whole page rather than lost. An overprint that recorded no paint
    /// approximated nothing.
    pub(crate) fn finish(&mut self) {
        for p in std::mem::take(&mut self.pending) {
            if p.cause == SiteCause::Overprint {
                continue;
            }
            self.sites.push(Site {
                cause: p.cause,
                bounds: UNKNOWN,
            });
        }
    }

    /// The part of `op` lying over anything painted before it, as one box,
    /// or `None` over bare paper — where an overprint paints exactly what a
    /// knockout paints.
    fn beneath(&self, op: DeviceBounds, before: usize) -> Option<DeviceBounds> {
        let mut out: Option<DeviceBounds> = None;
        for b in self.painted.get(..before).unwrap_or_default() {
            let Some(hit) = intersect(op, *b) else {
                continue;
            };
            out = Some(out.map_or(hit, |o| union(o, hit)));
            if out == Some(op) {
                break;
            }
        }
        out
    }
}

/// An op of unknown extent: the whole page, once clamped.
pub const UNKNOWN: DeviceBounds = DeviceBounds {
    left: f32::NEG_INFINITY,
    top: f32::NEG_INFINITY,
    right: f32::INFINITY,
    bottom: f32::INFINITY,
};

/// The overlap of two boxes, `None` when they do not overlap.
pub(crate) fn intersect(a: DeviceBounds, b: DeviceBounds) -> Option<DeviceBounds> {
    let r = DeviceBounds {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    };
    (r.left < r.right && r.top < r.bottom).then_some(r)
}

/// The smallest box holding both.
pub(crate) fn union(a: DeviceBounds, b: DeviceBounds) -> DeviceBounds {
    DeviceBounds {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

/// The device box `op` can paint into: its own bounds, or the union of
/// its children's. `None` when unknown or when it holds nothing.
pub fn op_box(op: &Op) -> Option<DeviceBounds> {
    match op {
        Op::Fill { bounds, .. } | Op::Stroke { bounds, .. } => Some(bounds.unwrap_or(UNKNOWN)),
        Op::Layer { ops, .. } | Op::Text { ops, .. } => ops.iter().filter_map(op_box).reduce(union),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn b(left: f32, top: f32, right: f32, bottom: f32) -> DeviceBounds {
        DeviceBounds {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn an_overprint_over_paper_opens_no_site() {
        let mut log = SiteLog::default();
        log.pushed(1, Some(b(0.0, 0.0, 10.0, 10.0)), true);
        log.poisoned(PoisonReason::Overprint, 1, None);
        log.pushed(1, Some(b(20.0, 0.0, 30.0, 10.0)), true);
        assert!(log.sites.is_empty());
    }

    #[test]
    fn an_overprint_site_is_the_overlap_with_what_lies_beneath() {
        let mut log = SiteLog::default();
        log.pushed(1, Some(b(0.0, 0.0, 10.0, 10.0)), true);
        log.poisoned(PoisonReason::Overprint, 1, None);
        log.pushed(1, Some(b(5.0, 5.0, 15.0, 15.0)), true);
        assert_eq!(log.sites.len(), 1);
        assert_eq!(log.sites.first().unwrap().bounds, b(5.0, 5.0, 10.0, 10.0));
    }

    #[test]
    fn a_cause_waits_for_the_op_at_its_own_depth() {
        let mut log = SiteLog::default();
        log.poisoned(PoisonReason::ColorantBuffer, 1, None);
        log.pushed(2, Some(b(0.0, 0.0, 1.0, 1.0)), true);
        assert!(log.sites.is_empty());
        log.pushed(1, Some(b(0.0, 0.0, 4.0, 4.0)), false);
        assert_eq!(log.sites.first().unwrap().cause, SiteCause::BlendingSpace);
        assert_eq!(log.sites.first().unwrap().bounds, b(0.0, 0.0, 4.0, 4.0));
    }

    #[test]
    fn the_page_level_colorant_refusal_opens_no_site() {
        let mut log = SiteLog::default();
        log.mark_subtractive_page();
        log.poisoned(PoisonReason::ColorantBuffer, 1, None);
        log.pushed(1, Some(b(0.0, 0.0, 4.0, 4.0)), true);
        assert!(log.sites.is_empty());
        assert!(log.subtractive_page);
    }
}
