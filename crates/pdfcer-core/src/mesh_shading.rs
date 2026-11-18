//! Mesh shadings (types 4–7) as data: decode one to its **authored**
//! colour components, tessellate its patches, re-express it in another
//! colour space, and write a free-form triangle mesh (type 4) back out.
//!
//! ISO 32000-1 §8.7.4.5.5–.8; clause tags (`MSH…`) are the spec RAG's
//! (`iso32000__s__8.7.4.5__mesh.md`).
//!
//! The renderer has its own mesh parser (`pdfcer-render`'s `mesh`), which
//! converts each vertex into its paint spaces as it reads. This module keeps
//! the components as authored, which is what a writer and a colour
//! conversion need; the two decoders are held together by an agreement
//! test in `pdfcer-render`.
//!
//! Everything here is panic-free on hostile input: every width, count and
//! `/Decode` entry is validated before use, arithmetic is checked, and the
//! triangle count is capped. Past a cap the answer is a typed refusal, never
//! a partial shading.
//!
//! # Example
//!
//! ```
//! use pdfcer_core::mesh_shading::{write_free_form_mesh, MeshVertex, MeshWriteOptions};
//! use pdfcer_core::object::{Name, Object};
//!
//! let v = |x: f64, y: f64, g: f64| MeshVertex { xy: [x, y], color: vec![g] };
//! let triangles = vec![[v(0.0, 0.0, 0.0), v(100.0, 0.0, 1.0), v(0.0, 100.0, 0.5)]];
//! let opts = MeshWriteOptions::new(Object::Name(Name::from(b"DeviceGray")), 1);
//! let written = write_free_form_mesh(&triangles, &opts)?;
//! assert_eq!(written.report.triangles, 1);
//! # Ok::<(), pdfcer_core::mesh_shading::MeshError>(())
//! ```

mod decode;
mod reexpress;
mod tessellate;
mod write;

pub use decode::{MeshStreamFormat, decode_mesh};
pub use reexpress::{Reexpress, ReexpressReport, reexpress_mesh};
pub use tessellate::{Tessellation, tessellate_patches};
pub use write::{MeshWriteOptions, MeshWriteReport, WrittenMesh, write_free_form_mesh};

/// The most triangles any function here produces or accepts by default.
///
/// The renderer's own mesh ceiling, so a mesh this module writes is one the
/// renderer will paint whole.
pub const MAX_TRIANGLES: usize = 4_000_000;

/// One mesh vertex: a point in the shading's target space and its colour.
///
/// `color` holds one entry per colour-space component, or a single
/// parametric `t` when the shading has a `/Function` (`MSH14`).
#[derive(Debug, Clone, PartialEq)]
pub struct MeshVertex {
    /// `[x, y]` in the shading's target coordinate space (`MSH2`).
    pub xy: [f64; 2],
    /// The colour fields, as authored.
    pub color: Vec<f64>,
}

/// A triangle of three vertices, in stream (paint) order.
pub type MeshTriangle = [MeshVertex; 3];

/// One type 6 or type 7 patch, always as sixteen tensor control points.
///
/// `p[i][j]` is column `i` (`u`), row `j` (`v`) (`MSH28`). A Coons patch's
/// four internal points are derived on decode (`MSH30`).
#[derive(Debug, Clone, PartialEq)]
pub struct MeshPatch {
    /// The control points, `p[i][j]` as `[x, y]`.
    pub p: [[[f64; 2]; 4]; 4],
    /// Corner colours in walk-around order: `(u,v) = (0,0)`, `(0,1)`,
    /// `(1,1)`, `(1,0)` (`MSH24`).
    pub corner: [Vec<f64>; 4],
}

/// The geometry of a decoded mesh.
#[derive(Debug, Clone, PartialEq)]
pub enum MeshGeometry {
    /// Types 4 and 5, resolved to a triangle list.
    Triangles(Vec<MeshTriangle>),
    /// Types 6 and 7, in stream order, which is paint order (`MSH33`).
    Patches(Vec<MeshPatch>),
}

/// A mesh shading decoded to authored colour components.
#[derive(Debug, Clone, PartialEq)]
pub struct DecodedMesh {
    /// 4, 5, 6 or 7.
    pub shading_type: u8,
    /// Colour fields per vertex: the colour space's components, or 1 when
    /// [`Self::parametric`].
    pub components: usize,
    /// Whether the colour field is a `/Function` input `t`.
    pub parametric: bool,
    /// The triangles or patches.
    pub geometry: MeshGeometry,
    /// The stream ended part-way through a record, or a type 5 stream had a
    /// partial row; the complete records were kept (`MSH-N2`).
    pub truncated: bool,
}

/// Why a mesh could not be decoded, re-expressed or written.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum MeshError {
    /// `ShadingType` is not 4–7.
    #[error("shading type {0} is not a mesh (4-7)")]
    NotAMesh(i64),
    /// `BitsPerCoordinate` outside `{1,2,4,8,12,16,24,32}` (`MSH4`).
    #[error("BitsPerCoordinate {0} is not one of 1, 2, 4, 8, 12, 16, 24, 32")]
    BadBitsPerCoordinate(i64),
    /// `BitsPerComponent` outside `{1,2,4,8,12,16}` (`MSH5`).
    #[error("BitsPerComponent {0} is not one of 1, 2, 4, 8, 12, 16")]
    BadBitsPerComponent(i64),
    /// `BitsPerFlag` outside `{2,4,8}` on a type that has a flag (`MSH6`).
    #[error("BitsPerFlag {0} is not one of 2, 4, 8")]
    BadBitsPerFlag(i64),
    /// `/Decode` absent, short, or holding a non-finite number (`MSH10`).
    #[error("Decode has {found} usable numbers; this mesh needs {needed}")]
    BadDecode {
        /// Numbers required: `4 + 2n`.
        needed: usize,
        /// Finite numbers present.
        found: usize,
    },
    /// Type 5 `VerticesPerRow` absent or below 2.
    #[error("type 5 VerticesPerRow is absent or below 2")]
    BadVerticesPerRow,
    /// Zero colour components, or more than 32 (the DeviceN limit).
    #[error("{0} colour components per vertex is outside 1-32")]
    BadComponentCount(usize),
    /// The first type 6/7 patch carried a nonzero edge flag.
    #[error("the first patch has a nonzero edge flag, so nothing to continue from")]
    FirstPatchNotNew,
    /// Not one complete triangle or patch in the input.
    #[error("the mesh has no complete triangle or patch")]
    Empty,
    /// More triangles than the cap allows. Nothing is returned.
    #[error("the mesh needs more than {cap} triangles")]
    TooManyTriangles {
        /// The cap in force.
        cap: usize,
    },
    /// A coordinate or colour value is NaN or infinite.
    #[error("triangle {triangle} has a non-finite coordinate or colour")]
    NonFinite {
        /// Index of the offending triangle in the input.
        triangle: usize,
    },
    /// A vertex's colour has the wrong number of entries.
    #[error("triangle {triangle} has a vertex with {found} colour values; expected {expected}")]
    ComponentMismatch {
        /// Index of the offending triangle.
        triangle: usize,
        /// The count the mesh declares.
        expected: usize,
        /// The count found.
        found: usize,
    },
    /// The colour space names a component count that disagrees with the
    /// vertex data, or is `Indexed`/`Pattern`, which a mesh cannot use.
    #[error("the colour space does not fit {components}-component vertex data")]
    ColorSpaceMismatch {
        /// Components per vertex in the data.
        components: usize,
    },
    /// A coordinate or colour span (`max − min`) overflows `f64`.
    #[error("a coordinate or colour range is too wide to encode")]
    RangeTooWide,
    /// `Background` has the wrong length or a non-finite entry.
    #[error("Background does not have one finite value per component")]
    BadBackground,
    /// The requested output `BitsPerComponent` is not 8 or 16.
    #[error("output BitsPerComponent {0} is not 8 or 16")]
    BadOutputBits(u8),
    /// The colour conversion declined a vertex colour.
    #[error("the colour conversion declined a colour")]
    ConversionDeclined,
    /// The colour conversion returned the wrong number of components.
    #[error("the colour conversion returned {found} values; expected {expected}")]
    ConversionShape {
        /// Destination components expected.
        expected: usize,
        /// Values returned.
        found: usize,
    },
}
