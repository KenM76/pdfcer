//! Spot colorants on an ink page as the flattener's spot planes (ISO
//! 32000-1 §11.7.3).
//!
//! The raster gives a colorant a plane in each buffer that names it and
//! merges a group's planes into its parent's by name, so while the page
//! names at most [`SPOT_PLANES`] colorants every one is planed everywhere
//! and one page-wide plane per colorant reproduces it. The order planes are
//! taken in does not matter: planes composite independently and fold into
//! the colour by multiplication. `/All` (§8.6.6.4) sets the planes its
//! buffer holds at that paint, which is tracked per group. A page naming
//! more colorants, or an `/All` inside a non-isolated group (whose content
//! the raster runs over two different rosters), stays a patch.

use std::collections::HashMap;

use pdfcer_flatten::{SPOT_PLANES, Spots};
use pdfcer_render::writer_seam::{BrushSpec, Op, SpotInk};

use crate::vector_census::Cause;

const _: () = assert!(SPOT_PLANES == pdfcer_render::compositor::MAX_SPOTS);

/// The page's spot colorants, one plane each in the order they are first
/// named, and the planes each `/All` paint sets.
pub(super) struct Roster {
    inks: Vec<SpotInk>,
    /// Per `/All` brush, by address: the planes its buffer holds, or `None`
    /// inside a non-isolated group.
    all: HashMap<*const BrushSpec, Option<[bool; SPOT_PLANES]>>,
    overflow: bool,
    /// Non-isolated, non-knockout groups enclosing the walk.
    twice: usize,
}

impl Roster {
    /// The roster of the ops of a page, in paint order.
    pub(super) fn of(ops: &[Op]) -> Self {
        let mut r = Self {
            inks: Vec::new(),
            all: HashMap::new(),
            overflow: false,
            twice: 0,
        };
        r.walk(ops, &mut Vec::new());
        r
    }

    /// Each colorant's ink, by plane.
    pub(super) fn inks(&self) -> &[SpotInk] {
        &self.inks
    }

    /// The tint `brush` states on each plane, `None` where it names none.
    pub(super) fn named(&self, brush: &BrushSpec) -> Result<[Option<f32>; SPOT_PLANES], Cause> {
        let mut out = [None; SPOT_PLANES];
        if brush.spots.is_empty() {
            return Ok(out);
        }
        if self.overflow {
            return Err(Cause::SpotRoster);
        }
        for ink in &brush.spots {
            let slot = self
                .plane(&ink.colorant)
                .and_then(|p| out.get_mut(p))
                .ok_or(Cause::SpotRoster)?;
            *slot = Some(ink.tint);
        }
        Ok(out)
    }

    /// The planes an `/All` paint at `tint` sets to that tint.
    pub(super) fn all(&self, brush: &BrushSpec, tint: f32) -> Result<Spots, Cause> {
        if self.overflow {
            return Err(Cause::SpotRoster);
        }
        let held = self
            .all
            .get(&std::ptr::from_ref(brush))
            .ok_or(Cause::SpotRoster)?
            .ok_or(Cause::SpotAllGroup)?;
        Ok(held.map(|h| if h { tint } else { 0.0 }))
    }

    fn plane(&self, colorant: &[u8]) -> Option<usize> {
        self.inks.iter().position(|i| *i.colorant == *colorant)
    }

    /// The plane of `ink`'s colorant, taken on first sight.
    fn take(&mut self, ink: &SpotInk) -> Option<usize> {
        if let Some(p) = self.plane(&ink.colorant) {
            return Some(p);
        }
        if self.inks.len() >= SPOT_PLANES {
            self.overflow = true;
            return None;
        }
        self.inks.push(ink.clone());
        Some(self.inks.len() - 1)
    }

    /// `buffer` holds the planes the current group's buffer has taken.
    fn walk(&mut self, ops: &[Op], buffer: &mut Vec<usize>) {
        for op in ops {
            match op {
                Op::Fill { brush, .. } | Op::Stroke { brush, .. } => self.paint(brush, buffer),
                Op::Text { ops, .. } => self.walk(ops, buffer),
                Op::Layer {
                    ops,
                    isolated,
                    knockout,
                    ..
                } => {
                    // A group's buffer starts with no planes when isolated,
                    // with its parent's otherwise; its planes join the
                    // parent's when it composites.
                    let mut child = if *isolated {
                        Vec::new()
                    } else {
                        buffer.clone()
                    };
                    let twice = !*isolated && !*knockout;
                    self.twice += usize::from(twice);
                    self.walk(ops, &mut child);
                    self.twice -= usize::from(twice);
                    for p in child {
                        if !buffer.contains(&p) {
                            buffer.push(p);
                        }
                    }
                }
            }
        }
    }

    fn paint(&mut self, brush: &BrushSpec, buffer: &mut Vec<usize>) {
        if brush.all_colorants.is_some() {
            let held = (self.twice == 0).then(|| {
                let mut h = [false; SPOT_PLANES];
                for p in buffer.iter() {
                    if let Some(slot) = h.get_mut(*p) {
                        *slot = true;
                    }
                }
                h
            });
            self.all.insert(std::ptr::from_ref(brush), held);
        }
        for ink in &brush.spots {
            if let Some(p) = self.take(ink)
                && !buffer.contains(&p)
            {
                buffer.push(p);
            }
        }
    }
}

/// `rgb` with each spot plane's ink multiplied in where it holds a tint
/// (ISO 32000-2 §10.8.3 step (c)), as the raster's colorant buffer folds it.
pub(super) fn fold(inks: &[SpotInk], spots: Spots, mut rgb: [f32; 3]) -> [f32; 3] {
    for (ink, tint) in inks.iter().zip(spots) {
        if tint <= 0.0 {
            continue;
        }
        let a = ink.appearance(tint);
        for (c, f) in rgb.iter_mut().zip(a) {
            *c *= f;
        }
    }
    rgb
}
