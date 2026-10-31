//! Painting a rotated draw or field (XFA 3.3 p.55, p.685; gaps G-L28-G-L34).

use crate::xfa::ContainerKind;

use super::{LayoutItem, LayoutRect, PaintNoteKind, Painter, XfaNode};

impl Painter<'_> {
    /// Paint `item` turned `item.rotate` degrees counter-clockwise: its
    /// unrotated box shares the centre of the rotated extent `item.rect`.
    pub(super) fn turned(&mut self, item: &LayoutItem, node: &XfaNode) {
        self.rotate_note(item, node);
        let (cos, sin) = match item.rotate {
            90 => (0.0, 1.0),
            180 => (-1.0, 0.0),
            270 => (0.0, -1.0),
            _ => return self.item(item, node),
        };
        let r = item.rect;
        let (w, h) = if item.rotate == 180 {
            (r.w, r.h)
        } else {
            (r.h, r.w)
        };
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let mut upright = item.clone();
        upright.rect = LayoutRect {
            x: cx - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
        };
        // About the centre, in PDF's y-up space.
        let py = self.height - cy;
        let dashed = self.dashed;
        self.b.save_state();
        self.b.concat_matrix(
            cos,
            sin,
            -sin,
            cos,
            cx - cos * cx + sin * py,
            py - sin * cx - cos * py,
        );
        self.item(&upright, node);
        self.b.restore_state();
        self.dashed = dashed;
    }

    fn rotate_note(&mut self, item: &LayoutItem, node: &XfaNode) {
        let Some(v) = node.attr("rotate") else {
            return;
        };
        let disclosed = match v.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => {
                let odd = (n - (n / 90.0).round() * 90.0).abs() > 1e-9;
                let turns = matches!(item.kind, ContainerKind::Draw | ContainerKind::Field);
                odd || (!turns && n.rem_euclid(360.0) != 0.0)
            }
            _ => true,
        };
        if disclosed {
            self.note(item, PaintNoteKind::Rotate);
        }
    }
}
