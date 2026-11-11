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

#[cfg(feature = "render")]
mod camera;
mod error;
mod finish;
mod gltf;
#[cfg(feature = "render")]
mod gpu;
#[cfg(feature = "render")]
mod indirect;
#[cfg(feature = "render")]
mod lighting;
mod model;
mod prc;
#[cfg(feature = "render")]
mod render;
#[cfg(feature = "render")]
mod shadow;
mod texture;
/// Vector arithmetic on `[f64; 3]`: the `pdfcer-vec3` crate.
pub use pdfcer_vec3 as vec3;
mod write;

// Public only so the fuzz target can drive the bit reader directly.
#[doc(hidden)]
pub use prc::bits;

#[cfg(feature = "render")]
pub use camera::Camera;
#[cfg(feature = "render")]
pub use camera::bounds::Bounds;
#[cfg(feature = "render")]
pub use camera::lens::{FrameLens, LENS_LINE_PIECES, LensFace};
#[cfg(feature = "render")]
pub use camera::pick::{Hit, Ray, pick};
#[cfg(feature = "render")]
pub use camera::pick_index::PickIndex;
#[cfg(feature = "render")]
pub use camera::projection::{FisheyeMapping, Projection};
#[cfg(feature = "render")]
pub use camera::sphere::DEFAULT_FOV_Y;
pub use error::PrcError;
pub use finish::{MaterialFinish, ShininessScale};
pub use gltf::{
    GltfColours, GltfError, GltfImport, GltfLights, GltfOcclusion, GltfOptions, GltfUpAxis,
    read_gltf,
};
#[cfg(feature = "render")]
pub use gpu::ao::{
    FrameAmbientOcclusion, GLSL_ES_300_AO_COMPOSITE_FRAGMENT, GLSL_ES_300_AO_FRAGMENT,
    GLSL_ES_300_SCREEN_VERTEX,
};
#[cfg(feature = "render")]
pub use gpu::bounce::FrameBounce;
#[cfg(feature = "render")]
pub use gpu::finish::DrawFinish;
#[cfg(feature = "render")]
pub use gpu::frame::{
    FrameLights, LIGHT_TABLE_WIDTH, MAX_LAMP_SHADOWS, MAX_LIGHTS, MAX_SHADOW_CASTERS, ShadeMode,
};
#[cfg(feature = "render")]
pub use gpu::lamp_shadow::{
    FrameLampShadows, GLSL_ES_300_LAMP_SHADOW_FRAGMENT, GLSL_ES_300_LAMP_SHADOW_VERTEX,
};
#[cfg(feature = "render")]
pub use gpu::lens::{
    GLSL_ES_300_LENS_LINE_VERTEX, GLSL_ES_300_LENS_POINT_VERTEX, GLSL_ES_300_LENS_RESAMPLE_FRAGMENT,
};
#[cfg(feature = "render")]
pub use gpu::lines::{GLSL_ES_300_LINE_FRAGMENT, GLSL_ES_300_LINE_VERTEX, LINE_VERTEX_FLOATS};
#[cfg(feature = "render")]
pub use gpu::prepass::{GLSL_ES_300_PREPASS_FRAGMENT, GLSL_ES_300_PREPASS_VERTEX};
#[cfg(feature = "render")]
pub use gpu::shader::{GLSL_ES_300_FRAGMENT, GLSL_ES_300_VERTEX};
#[cfg(feature = "render")]
pub use gpu::shadow::{FrameShadow, GLSL_ES_300_SHADOW_FRAGMENT, GLSL_ES_300_SHADOW_VERTEX};
#[cfg(feature = "render")]
pub use gpu::shadow_set::{
    FrameExtraShadows, GLSL_ES_300_LAMP_SHADOW_ALPHA_FRAGMENT,
    GLSL_ES_300_LAMP_SHADOW_ALPHA_VERTEX, GLSL_ES_300_SHADOW_ALPHA_FRAGMENT,
    GLSL_ES_300_SHADOW_ALPHA_VERTEX, MAX_EXTRA_SHADOWS,
};
#[cfg(feature = "render")]
pub use gpu::wide_lines::GLSL_ES_300_WIDE_LINE_VERTEX;
#[cfg(feature = "render")]
pub use gpu::{Draw, DrawData, VERTEX_FLOATS};
#[cfg(feature = "render")]
pub use lighting::bounce::{Bounce, BounceSky, BounceStats, IndirectOptions, MAX_BOUNCE_STRENGTH};
#[cfg(feature = "render")]
pub use lighting::gain::{MAX_LIGHT_GAIN, MIN_LIGHT_GAIN};
#[cfg(feature = "render")]
pub use lighting::lamp_view::{LampCost, LampStats};
#[cfg(feature = "render")]
pub use lighting::shade::LampCut;
#[cfg(feature = "render")]
pub use lighting::sun::{MAX_SUN_KELVIN, MIN_SUN_KELVIN, SunColour, SunLight, kelvin_rgb};
#[cfg(feature = "render")]
pub use lighting::{Light, LightFrame, LightSet, LightingScheme, Lights};
pub use model::export::{to_obj, to_stl};
pub use model::light::{ModelLight, ModelLightKind, ViewLightUse, ViewLights};
pub use model::named_view::{NamedView, UnknownName, UpAxis};
pub use model::node_box::NodeBox;
pub use model::wire::ModelLine;
pub use model::{
    AssembleError, AssembleOptions, AssembledModel, DEFAULT_VIEW_DIRECTION, DEFAULT_VIEW_UP,
    MAX_PLACED_ATTRIBUTES, MAX_PLACED_TRIANGLES, MeshFit, StoredVisibility, assemble,
    assemble_with, assemble_with_options,
};
#[cfg(feature = "render")]
pub use model::{default_view_camera, render_default_view, render_model};
pub use prc::compressed::PartialMesh;
pub use prc::container::{
    FileStructure, MAX_FILE_STRUCTURES, MAX_INFLATED_BYTES, ModelUnits, PRC_READER_VERSION,
    PrcFile, PrcHeader, SectionKind, UniqueId,
};
pub use prc::elements::MAX_DECODED_ELEMENTS;
pub use prc::schema::Schema;
pub use prc::tess::{Tessellation, TriangleMesh, WireTessellation};
pub use prc::tree::{
    EntityOverrides, IDENTITY, Matrix, ModelNode, NameSource, PictureFiles, Placement, StyleAlpha,
    WrapBase, multiply, transform_point,
};
#[cfg(feature = "render")]
pub use render::error::RenderError;
#[cfg(feature = "render")]
pub use render::mode::{EdgeSet, FaceColour, RenderMode, RenderModeKind};
#[cfg(feature = "render")]
pub use render::prepared::PreparedModel;
#[cfg(feature = "render")]
pub use render::work::MAX_RENDER_PIXEL_TESTS;
#[cfg(feature = "render")]
pub use render::{Image, MAX_RENDER_PIXELS, RenderOptions, render, render_coloured};
#[cfg(feature = "render")]
pub use shadow::cast::{DEFAULT_SHADOW_CUTOFF, ShadowAlpha, ShadowCasters};
#[cfg(feature = "render")]
pub use shadow::lamp::{DEFAULT_LAMP_SHADOW_MAP_SIZE, MAX_LAMP_SHADOW_MAP_SIZE};
#[cfg(feature = "render")]
pub use shadow::{DEFAULT_SHADOW_MAP_SIZE, LightSense, MAX_SHADOW_MAP_SIZE, ShadowLight};
pub use texture::{MAX_TEXTURE_PIXELS, Texture, TextureFunction, TextureOrigin, TextureWrap};
pub use write::{
    MAX_DEPTH, MeshScene, PrcWriteError, PrcWriteReport, SceneMaterial, SceneMesh, SceneNode,
    ScenePrimitive, SceneTexture, write_prc,
};
