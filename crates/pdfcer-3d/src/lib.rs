//! pdfcer 3D models: the PRC format (Adobe PRC 8137, the text ISO 14739-1
//! standardised), read and drawn; a triangle-mesh scene written as PRC
//! ([`write_prc`]); and glTF 2.0 / GLB imported as such a scene
//! ([`read_gltf`]).
//!
//! [`PrcFile::parse`] walks the container: the uncompressed file header,
//! every file structure's header and its five zlib sections, and the model
//! file section, each inflated under [`MAX_INFLATED_BYTES`].
//!
//! Sources, cited per item as `[WD clause]` (ISO/TC171/SC2 N570, the PRC 8137
//! working draft) and `[PRCRS file]` (the MIT `prc-rs` reader, a permissive
//! code inference); the register is the spec RAG's
//! `threed/prc__8137__sources_provenance.md`, and decision 169 governs their
//! use. No GUI, network or thread dependency; builds for
//! `wasm32-unknown-unknown`.

#![forbid(unsafe_code)]
// Panic-free: this crate reads untrusted input, so a reachable panic is a
// denial-of-service bug. Tests opt out per module.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

mod acof;
mod arrays;
// Public only so the fuzz target can drive the bit reader directly.
#[doc(hidden)]
pub mod bits;
mod compressed;
mod container;
mod error;
mod export;
mod gltf;
mod model;
mod named_view;
#[cfg(feature = "render")]
mod pick;
#[cfg(feature = "render")]
mod raster;
#[cfg(feature = "render")]
mod render;
mod schema;
mod tess;
#[cfg(test)]
mod testw;
mod texture;
mod tree;
// Some helpers serve only the rasterizer.
#[cfg_attr(not(feature = "render"), allow(dead_code))]
mod vec3;
mod write;

pub use compressed::PartialMesh;
pub use container::{
    FileStructure, MAX_FILE_STRUCTURES, MAX_INFLATED_BYTES, PRC_READER_VERSION, PrcFile, PrcHeader,
    SectionKind, UniqueId,
};
pub use error::PrcError;
pub use export::{to_obj, to_stl};
pub use gltf::{GltfColours, GltfError, GltfImport, GltfOptions, GltfUpAxis, read_gltf};
pub use model::{
    AssembleError, AssembleOptions, AssembledModel, DEFAULT_VIEW_DIRECTION, DEFAULT_VIEW_UP,
    MeshFit, StoredVisibility, assemble, assemble_with, assemble_with_options,
};
#[cfg(feature = "render")]
pub use model::{default_view_camera, render_default_view, render_model};
pub use named_view::{NamedView, UnknownName, UpAxis};
#[cfg(feature = "render")]
pub use pick::{Hit, Ray, pick};
#[cfg(feature = "render")]
pub use render::{
    Bounds, Camera, Image, MAX_RENDER_PIXELS, Projection, RenderError, RenderOptions, render,
    render_coloured,
};
pub use schema::Schema;
pub use tess::{Tessellation, TriangleMesh};
pub use texture::{MAX_TEXTURE_PIXELS, Texture, TextureFunction, TextureOrigin, TextureWrap};
pub use tree::{
    EntityOverrides, IDENTITY, Matrix, ModelNode, NameSource, PictureFiles, Placement, StyleAlpha,
    WrapBase, multiply, transform_point,
};
pub use write::{
    MAX_DEPTH, MeshScene, PrcWriteError, PrcWriteReport, SceneMaterial, SceneMesh, SceneNode,
    ScenePrimitive, SceneTexture, write_prc,
};
