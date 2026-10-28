//! Flowing placement cursor (XFA 3.3 Ch.8 pp.279-282).

use crate::xfa::LayoutStrategy;

/// Whether `s` is a flow this pass lays (`tb`, `lr-tb`, `rl-tb`). Table
/// strategies are Pass 647.7.
pub(super) const fn is_flow(s: LayoutStrategy) -> bool {
    matches!(
        s,
        LayoutStrategy::Tb | LayoutStrategy::LrTb | LayoutStrategy::RlTb
    )
}

/// Where the next flowed child goes in a content region whose top-left is
/// the origin. x, y and anchorType of a flowed child are ignored (p.279).
#[derive(Debug, Clone, Copy)]
pub(super) struct Flow {
    strategy: LayoutStrategy,
    /// Width children wrap at (`lr-tb`/`rl-tb`); `None` never wraps.
    width: Option<f64>,
    x: f64,
    y: f64,
    row_h: f64,
}

impl Flow {
    pub(super) const fn new(strategy: LayoutStrategy, width: Option<f64>) -> Self {
        Self {
            strategy,
            width,
            x: 0.0,
            y: 0.0,
            row_h: 0.0,
        }
    }

    fn tiles(&self) -> bool {
        self.strategy != LayoutStrategy::Tb
    }

    /// A tiled child that does not fit the rest of the row starts a new
    /// one, unless the row is empty.
    fn wraps(&self, w: f64) -> bool {
        self.tiles() && self.x > 0.0 && self.width.is_some_and(|max| self.x + w > max)
    }

    /// Top-left a `w`-wide child would take.
    pub(super) fn slot(&self, w: f64) -> (f64, f64) {
        if !self.tiles() {
            return (0.0, self.y);
        }
        let (x, y) = if self.wraps(w) {
            (0.0, self.y + self.row_h)
        } else {
            (self.x, self.y)
        };
        match (self.strategy, self.width) {
            (LayoutStrategy::RlTb, Some(max)) => (max - x - w, y),
            _ => (x, y),
        }
    }

    /// Take the slot for a `w` × `h` child.
    pub(super) fn commit(&mut self, w: f64, h: f64) {
        if !self.tiles() {
            self.y += h;
            return;
        }
        if self.wraps(w) {
            self.y += self.row_h;
            self.x = 0.0;
            self.row_h = 0.0;
        }
        self.x += w;
        self.row_h = self.row_h.max(h);
    }
}
