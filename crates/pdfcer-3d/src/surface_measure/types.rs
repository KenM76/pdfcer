//! What a surface measurement is asked with and answers with.

/// The parallel tolerance a measurement starts from, in degrees: a
/// judgement call, small enough that two faces a designer drew at an angle
/// are never called parallel.
pub const DEFAULT_PARALLEL_DEGREES: f64 = 0.5;

/// The flat tolerance a surface starts from, in degrees.
pub const DEFAULT_FLAT_DEGREES: f64 = 0.5;

/// When two surfaces count as parallel, and what counts as one surface.
///
/// Start from [`SurfaceTolerance::default`] and assign.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct SurfaceTolerance {
    /// Surfaces whose planes meet at this angle or less, in degrees (0 to
    /// 90), are parallel and report a distance; any wider pair reports its
    /// angle.
    pub parallel_degrees: f64,
    /// Report a distance whatever the angle. The distance is still measured
    /// (from the second pick to the first surface's plane), and
    /// [`SurfaceRelation::Parallel::forced`] says the pair was not within
    /// the tolerance.
    pub force_parallel: bool,
    /// A triangle belongs to the picked surface when it faces within this
    /// angle, in degrees (0 to 90), of the picked triangle and lies in its
    /// plane.
    pub flat_degrees: f64,
}

impl Default for SurfaceTolerance {
    fn default() -> Self {
        SurfaceTolerance {
            parallel_degrees: DEFAULT_PARALLEL_DEGREES,
            force_parallel: false,
            flat_degrees: DEFAULT_FLAT_DEGREES,
        }
    }
}

/// A picked place on a model: [`crate::Hit`] converts into it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfacePick {
    /// The index of the mesh in the slice measured.
    pub mesh: usize,
    /// The index of the triangle in that mesh.
    pub triangle: usize,
    /// The picked point, model coordinates.
    pub point: [f64; 3],
}

/// The flat surface found round a pick, and how it was found.
///
/// The plane is fitted from the triangles, not read from the file: show
/// [`Self::triangles`], [`Self::whole_face`] and [`Self::flatness_degrees`]
/// beside any figure that rests on it.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct SurfacePatch {
    /// The mesh the surface is on.
    pub mesh: usize,
    /// The face of the mesh the pick is on; `None` when the file separates
    /// no faces, and the surface was then sought over the whole mesh.
    pub face: Option<usize>,
    /// The surface is every triangle of [`Self::face`]. `false` when the
    /// face is curved or stepped and only the flat part round the pick was
    /// used, and when the file separates no faces.
    pub whole_face: bool,
    /// Triangles the plane was fitted from.
    pub triangles: usize,
    /// Their area, in model units squared.
    pub area: f64,
    /// Their area-weighted centre: a point on the fitted plane.
    pub centroid: [f64; 3],
    /// The fitted plane's unit normal, pointing the way the triangles wind
    /// (out of a closed solid).
    pub normal: [f64; 3],
    /// The widest angle, in degrees, between a fitted triangle and the
    /// fitted plane: 0 for a truly flat surface.
    pub flatness_degrees: f64,
}

/// How two surfaces lie to each other.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum SurfaceRelation {
    /// Parallel within the tolerance, or forced.
    Parallel {
        /// From the second pick to the first surface's plane, in model
        /// units; 0 when both lie in one plane.
        distance: f64,
        /// The pair was wider than the tolerance and
        /// [`SurfaceTolerance::force_parallel`] was set.
        forced: bool,
    },
    /// Wider than the tolerance.
    Angled {
        /// The angle between the planes, 0 to 90.
        degrees: f64,
    },
}

/// A measurement between two picked surfaces.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct SurfaceMeasure {
    /// The first surface.
    pub a: SurfacePatch,
    /// The second surface.
    pub b: SurfacePatch,
    /// The angle between the two planes, 0 to 90 degrees, whatever
    /// [`Self::relation`] says: how far off parallel the pair is.
    pub angle_degrees: f64,
    /// The angle between the two outward normals, 0 to 180 degrees: 180 for
    /// the two sides of a plate, 0 for two steps facing the same way.
    pub normals_degrees: f64,
    /// A distance or an angle, by the tolerance.
    pub relation: SurfaceRelation,
    /// The straight distance between the two picked points, model units.
    pub pick_distance: f64,
    /// The second picked point less the first, along X, Y and Z.
    pub delta: [f64; 3],
}

/// Why a surface measurement was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SurfaceMeasureError {
    /// The pick names a mesh or triangle the model does not have.
    #[error("the model has no triangle {triangle} in mesh {mesh}")]
    NoSuchTriangle {
        /// The mesh asked for.
        mesh: usize,
        /// The triangle asked for.
        triangle: usize,
    },
    /// The picked triangle has no area, so it has no plane.
    #[error("the picked triangle has no area")]
    Degenerate,
    /// The named tolerance is not within 0 to 90 degrees.
    #[error("the {0} must be from 0 to 90 degrees")]
    BadTolerance(&'static str),
    /// A picked point is not finite.
    #[error("a picked point is not usable")]
    BadPoint,
}
