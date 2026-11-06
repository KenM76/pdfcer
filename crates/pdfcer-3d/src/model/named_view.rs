//! Named viewing directions (front, top, iso, ...) for a model whose
//! vertical axis is chosen, so every shell frames "Front" the same way.

use std::fmt;
use std::str::FromStr;

/// The model axis that points up.
///
/// Mechanical CAD commonly models Y-up; architecture and PRC exports Z-up.
/// [`UpAxis::Z`] is the default, matching [`crate::DEFAULT_VIEW_UP`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum UpAxis {
    /// X up.
    X,
    /// Y up.
    Y,
    /// Z up.
    #[default]
    Z,
}

impl UpAxis {
    /// Every axis, in order.
    pub const ALL: [UpAxis; 3] = [UpAxis::X, UpAxis::Y, UpAxis::Z];

    /// The unit vector along this axis.
    ///
    /// ```
    /// assert_eq!(pdfcer_3d::UpAxis::Y.vector(), [0.0, 1.0, 0.0]);
    /// ```
    #[must_use]
    pub fn vector(self) -> [f64; 3] {
        self.orient([0.0, 0.0, 1.0])
    }

    /// Rotate a z-up direction so z maps onto this axis. A cyclic
    /// permutation of the axes, so a proper rotation: handedness is kept.
    #[must_use]
    pub fn orient(self, [a, b, c]: [f64; 3]) -> [f64; 3] {
        match self {
            UpAxis::Z => [a, b, c],
            UpAxis::Y => [a, c, -b],
            UpAxis::X => [c, a, b],
        }
    }

    /// The lower-case name: `x`, `y` or `z`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            UpAxis::X => "x",
            UpAxis::Y => "y",
            UpAxis::Z => "z",
        }
    }
}

/// A standard viewing direction.
///
/// Directions are defined for a Z-up model, where Front looks along +Y and
/// Right along -X, then rotated onto the chosen [`UpAxis`]. Top and Bottom
/// keep the model's +Y (in the Z-up frame) at the top of the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NamedView {
    /// From above the front-right corner: the direction of
    /// [`crate::DEFAULT_VIEW_DIRECTION`].
    #[default]
    Iso,
    /// From the front, looking toward the back.
    Front,
    /// From the back.
    Back,
    /// From the left side.
    Left,
    /// From the right side.
    Right,
    /// From above, looking down.
    Top,
    /// From below, looking up.
    Bottom,
}

impl NamedView {
    /// Every view, in menu order.
    pub const ALL: [NamedView; 7] = [
        NamedView::Iso,
        NamedView::Front,
        NamedView::Back,
        NamedView::Left,
        NamedView::Right,
        NamedView::Top,
        NamedView::Bottom,
    ];

    /// The look direction and the image's up direction, for a model whose
    /// vertical axis is `up`. Neither is normalised (Iso's direction has
    /// length √3).
    ///
    /// ```
    /// use pdfcer_3d::{NamedView, UpAxis};
    /// let (dir, up) = NamedView::Front.direction(UpAxis::Y);
    /// assert_eq!((dir, up), ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]));
    /// ```
    #[must_use]
    pub fn direction(self, up: UpAxis) -> ([f64; 3], [f64; 3]) {
        let (dir, image_up) = match self {
            NamedView::Iso => ([-1.0, 1.0, -1.0], [0.0, 0.0, 1.0]),
            NamedView::Front => ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
            NamedView::Back => ([0.0, -1.0, 0.0], [0.0, 0.0, 1.0]),
            NamedView::Left => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            NamedView::Right => ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
            NamedView::Top => ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            NamedView::Bottom => ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        };
        (up.orient(dir), up.orient(image_up))
    }

    /// The lower-case name (`iso`, `front`, ...), as the CLI spells it.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            NamedView::Iso => "iso",
            NamedView::Front => "front",
            NamedView::Back => "back",
            NamedView::Left => "left",
            NamedView::Right => "right",
            NamedView::Top => "top",
            NamedView::Bottom => "bottom",
        }
    }

    /// The label a reader shows for the view: `Iso`, `Front`, ...
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            NamedView::Iso => "Iso",
            NamedView::Front => "Front",
            NamedView::Back => "Back",
            NamedView::Left => "Left",
            NamedView::Right => "Right",
            NamedView::Top => "Top",
            NamedView::Bottom => "Bottom",
        }
    }
}

/// A name [`NamedView`] or [`UpAxis`] did not recognise.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("unknown {kind} `{name}`")]
pub struct UnknownName {
    kind: &'static str,
    name: String,
}

impl FromStr for NamedView {
    type Err = UnknownName;

    /// Case-insensitive [`NamedView::as_str`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        NamedView::ALL
            .into_iter()
            .find(|v| v.as_str().eq_ignore_ascii_case(s))
            .ok_or_else(|| UnknownName {
                kind: "view",
                name: s.to_owned(),
            })
    }
}

impl FromStr for UpAxis {
    type Err = UnknownName;

    /// Case-insensitive [`UpAxis::as_str`].
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        UpAxis::ALL
            .into_iter()
            .find(|v| v.as_str().eq_ignore_ascii_case(s))
            .ok_or_else(|| UnknownName {
                kind: "up axis",
                name: s.to_owned(),
            })
    }
}

impl fmt::Display for NamedView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Display for UpAxis {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn iso_is_the_default_view_direction() {
        let (dir, up) = NamedView::Iso.direction(UpAxis::Z);
        assert_eq!(dir, crate::DEFAULT_VIEW_DIRECTION);
        assert_eq!(up, crate::DEFAULT_VIEW_UP);
    }

    #[test]
    fn every_up_axis_keeps_handedness() {
        for axis in UpAxis::ALL {
            let [x, y, z] =
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(|v| axis.orient(v));
            let c = [
                x[1] * y[2] - x[2] * y[1],
                x[2] * y[0] - x[0] * y[2],
                x[0] * y[1] - x[1] * y[0],
            ];
            assert_eq!(c, z, "{axis}");
        }
    }

    #[test]
    fn names_round_trip() {
        for v in NamedView::ALL {
            assert_eq!(v.as_str().to_uppercase().parse::<NamedView>().unwrap(), v);
        }
        for a in UpAxis::ALL {
            assert_eq!(a.as_str().parse::<UpAxis>().unwrap(), a);
        }
        assert!("side".parse::<NamedView>().is_err());
    }
}
