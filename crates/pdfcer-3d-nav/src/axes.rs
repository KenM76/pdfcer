//! Which of the model's axes points up and which way the Front view looks.
//! Every named view is measured in this pair; a file's own opening view keeps
//! its own axes.

use pdfcer_3d::UpAxis;

/// One signed model axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Axis {
    /// `+x`.
    PosX,
    /// `+y`.
    PosY,
    /// `+z`.
    PosZ,
    /// `-x`.
    NegX,
    /// `-y`.
    NegY,
    /// `-z`.
    NegZ,
}

impl Axis {
    /// Every axis, in the order a choice lists them: `+x +y +z -x -y -z`.
    pub const ALL: [Axis; 6] = [
        Axis::PosX,
        Axis::PosY,
        Axis::PosZ,
        Axis::NegX,
        Axis::NegY,
        Axis::NegZ,
    ];

    /// Its place in [`Axis::ALL`], 0 to 5.
    pub fn index(self) -> usize {
        match self {
            Axis::PosX => 0,
            Axis::PosY => 1,
            Axis::PosZ => 2,
            Axis::NegX => 3,
            Axis::NegY => 4,
            Axis::NegZ => 5,
        }
    }

    /// The unit vector it names.
    pub fn vector(self) -> [f64; 3] {
        let v = self.up_axis().vector();
        if self.negative() { v.map(|c| -c) } else { v }
    }

    /// Its token: `+x` … `-z`.
    pub fn token(self) -> &'static str {
        match self {
            Axis::PosX => "+x",
            Axis::PosY => "+y",
            Axis::PosZ => "+z",
            Axis::NegX => "-x",
            Axis::NegY => "-y",
            Axis::NegZ => "-z",
        }
    }

    /// The engine's unsigned axis along this one.
    pub fn up_axis(self) -> UpAxis {
        match self {
            Axis::PosX | Axis::NegX => UpAxis::X,
            Axis::PosY | Axis::NegY => UpAxis::Y,
            Axis::PosZ | Axis::NegZ => UpAxis::Z,
        }
    }

    /// Whether this axis points along the negative half of
    /// [`Axis::up_axis`].
    pub fn negative(self) -> bool {
        matches!(self, Axis::NegX | Axis::NegY | Axis::NegZ)
    }

    /// Whether `other` is a different line through the origin.
    pub fn perpendicular(self, other: Self) -> bool {
        self.up_axis() != other.up_axis()
    }

    /// The Front view's look when this axis is up: along +y for z or x up
    /// (the CAD convention), along -z for y up (the convention of y-up
    /// exporters).
    fn default_front(self) -> Self {
        match self.up_axis() {
            UpAxis::Y => Self::NegZ,
            UpAxis::X | UpAxis::Z => Self::PosY,
        }
    }
}

/// The up axis and the Front view's look direction; perpendicular whenever
/// built by [`Upright::default`], [`Upright::with_up`] and
/// [`Upright::with_front`]. Built directly with two axes along one line, it
/// still turns (`Axes::upright` falls back to the default pair).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Upright {
    /// The axis that appears upward.
    pub up: Axis,
    /// The direction the Front view looks along.
    pub front: Axis,
}

impl Default for Upright {
    /// z up, Front looking along +y.
    fn default() -> Self {
        Self {
            up: Axis::PosZ,
            front: Axis::PosY,
        }
    }
}

impl Upright {
    /// `up` as the up axis, keeping the Front look when it is still
    /// perpendicular, else that axis's conventional one.
    pub fn with_up(self, up: Axis) -> Self {
        let front = if up.perpendicular(self.front) {
            self.front
        } else {
            up.default_front()
        };
        Self { up, front }
    }

    /// `front` as the Front view's look, or unchanged when it lies along up.
    pub fn with_front(self, front: Axis) -> Self {
        if self.up.perpendicular(front) {
            Self { front, ..self }
        } else {
            self
        }
    }

    /// The Front looks a choice offers: the four perpendicular to up.
    pub fn fronts(self) -> impl Iterator<Item = Axis> {
        Axis::ALL
            .into_iter()
            .filter(move |a| self.up.perpendicular(*a))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_up_keeps_a_front_that_still_fits_and_replaces_one_that_does_not() {
        let start = Upright::default();
        assert_eq!(start.with_up(Axis::PosX).front, Axis::PosY);
        assert_eq!(start.with_up(Axis::PosY).front, Axis::NegZ);
        assert_eq!(start.with_front(Axis::NegZ), start);
        assert_eq!(start.fronts().count(), 4);
    }

    #[test]
    fn every_axis_is_a_signed_unit_vector() {
        assert_eq!(Axis::NegY.vector(), [0.0, -1.0, 0.0]);
        assert_eq!(Axis::PosZ.vector(), [0.0, 0.0, 1.0]);
        for (i, axis) in Axis::ALL.into_iter().enumerate() {
            assert_eq!(axis.index(), i);
            let v = axis.vector();
            assert_eq!(v.iter().map(|c| c.abs()).sum::<f64>(), 1.0);
            assert_eq!(v.iter().sum::<f64>() < 0.0, axis.negative());
        }
    }
}
