//! A coordinate system to read measured positions and offsets in.

use crate::vec3::{cross, dot, length, normalize, sub};

/// A right-handed coordinate system in model space: an origin and three
/// unit axes at right angles. Measured points and deltas are model
/// coordinates; this re-expresses them relative to a system the operator
/// chose, as a measure tool's "coordinate system" choice does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoordinateFrame {
    origin: [f64; 3],
    axes: [[f64; 3]; 3],
}

impl CoordinateFrame {
    /// The system at `origin` whose X axis points along `x_axis` and whose
    /// XY plane holds `y_toward` (any lengths): Y is the part of
    /// `y_toward` square to X, and Z is X × Y. `None` when a value is not
    /// finite, `x_axis` has no length, or `y_toward` is parallel to it.
    ///
    /// ```
    /// use pdfcer_3d::CoordinateFrame;
    /// let f = CoordinateFrame::new([10.0, 0.0, 0.0], [0.0, 2.0, 0.0], [-1.0, 1.0, 0.0]).unwrap();
    /// assert_eq!(f.point_to_local([10.0, 3.0, 0.0]), [3.0, 0.0, 0.0]);
    /// assert!(CoordinateFrame::new([0.0; 3], [1.0, 0.0, 0.0], [2.0, 0.0, 0.0]).is_none());
    /// ```
    #[must_use]
    pub fn new(origin: [f64; 3], x_axis: [f64; 3], y_toward: [f64; 3]) -> Option<Self> {
        let finite = |v: [f64; 3]| v.iter().all(|c| c.is_finite());
        if !(finite(origin) && finite(x_axis) && finite(y_toward)) {
            return None;
        }
        let x = normalize(x_axis)?;
        let z_dir = cross(x, y_toward);
        // Parallel within rounding: the plane is not determined.
        if length(z_dir) <= 1e-12 * length(y_toward) {
            return None;
        }
        let z = normalize(z_dir)?;
        let y = cross(z, x);
        Some(CoordinateFrame {
            origin,
            axes: [x, y, z],
        })
    }

    /// The origin, model coordinates.
    #[must_use]
    pub fn origin(&self) -> [f64; 3] {
        self.origin
    }

    /// The unit X, Y and Z axes, model coordinates.
    #[must_use]
    pub fn axes(&self) -> [[f64; 3]; 3] {
        self.axes
    }

    /// A model point's X, Y, Z in this system.
    #[must_use]
    pub fn point_to_local(&self, p: [f64; 3]) -> [f64; 3] {
        self.vector_to_local(sub(p, self.origin))
    }

    /// A model offset (a delta or a direction) in this system's axes; the
    /// origin plays no part.
    #[must_use]
    pub fn vector_to_local(&self, v: [f64; 3]) -> [f64; 3] {
        self.axes.map(|a| dot(a, v))
    }
}
