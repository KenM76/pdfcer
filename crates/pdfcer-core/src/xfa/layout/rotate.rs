//! A draw's or field's `rotate` (XFA 3.3 p.55, p.286, p.685; gaps
//! G-L28-G-L34): counter-clockwise about the anchor point, in multiples of
//! 90 degrees, with the parent reserving the rotated extent.

use super::OddRotate;
use crate::xfa::{ContainerKind, XfaNode};

/// `node`'s rotation, counter-clockwise: 0, 90, 180 or 270. Only draws and
/// fields rotate (G-L30); a negative value turns clockwise (G-L28); an
/// unreadable one is 0; one not a multiple of 90 follows `odd` (G-L29).
pub(super) fn degrees(node: &XfaNode, kind: ContainerKind, odd: OddRotate) -> u16 {
    if !matches!(kind, ContainerKind::Draw | ContainerKind::Field) {
        return 0;
    }
    let Some(v) = node
        .attr("rotate")
        .and_then(|v| v.trim().parse::<f64>().ok())
        .filter(|v| v.is_finite())
    else {
        return 0;
    };
    let v = v.rem_euclid(360.0);
    let quarter = (v / 90.0).round();
    if (v - quarter * 90.0).abs() > 1e-9 && odd == OddRotate::Ignore {
        return 0;
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0..=4
    let q = quarter as u16 % 4;
    q * 90
}

/// The rotated extent of a `w` x `h` box whose anchor sits at fraction
/// `(ax, ay)` of it: offset of its top-left from the anchor, and its size.
/// Y runs down, so a counter-clockwise quarter turn maps (dx, dy) to
/// (dy, -dx).
pub(super) fn extent(rot: u16, (ax, ay): (f64, f64), (w, h): (f64, f64)) -> (f64, f64, f64, f64) {
    let turn = |(dx, dy): (f64, f64)| match rot {
        90 => (dy, -dx),
        180 => (-dx, -dy),
        270 => (-dy, dx),
        _ => (dx, dy),
    };
    let (x0, y0) = turn((-ax * w, -ay * h));
    let (x1, y1) = turn(((1.0 - ax) * w, (1.0 - ay) * h));
    (x0.min(x1), y0.min(y1), (x1 - x0).abs(), (y1 - y0).abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_turn_about_the_top_left_rises_above_it() {
        assert_eq!(
            extent(90, (0.0, 0.0), (40.0, 10.0)),
            (0.0, -40.0, 10.0, 40.0)
        );
        assert_eq!(
            extent(180, (0.0, 0.0), (40.0, 10.0)),
            (-40.0, -10.0, 40.0, 10.0)
        );
        assert_eq!(
            extent(270, (0.0, 0.0), (40.0, 10.0)),
            (-10.0, 0.0, 10.0, 40.0)
        );
        assert_eq!(
            extent(90, (0.5, 0.5), (40.0, 10.0)),
            (-5.0, -20.0, 10.0, 40.0)
        );
    }
}
