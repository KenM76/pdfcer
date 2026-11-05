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
#[cfg(feature = "render")]
mod ao;
mod arrays;
// Public only so the fuzz target can drive the bit reader directly.
#[doc(hidden)]
pub mod bits;
#[cfg(feature = "render")]
mod bounds;
mod compressed;
mod container;
#[cfg(feature = "render")]
mod edges;
mod elements;
mod error;
mod export;
mod finish;
#[cfg(feature = "render")]
mod frame;
#[cfg(feature = "render")]
mod gloss;
mod gltf;
#[cfg(feature = "render")]
mod gpu;
#[cfg(feature = "render")]
mod gpu_ao;
#[cfg(feature = "render")]
mod gpu_finish;
#[cfg(feature = "render")]
mod gpu_frame;
#[cfg(feature = "render")]
mod gpu_lamp_shadow;
#[cfg(feature = "render")]
mod gpu_lens;
#[cfg(feature = "render")]
mod gpu_lines;
#[cfg(feature = "render")]
mod gpu_prepass;
#[cfg(feature = "render")]
mod gpu_shader;
#[cfg(feature = "render")]
mod gpu_shadow;
#[cfg(feature = "render")]
mod gpu_shadow_set;
#[cfg(feature = "render")]
mod gpu_wide_lines;
#[cfg(feature = "render")]
mod lamp_shadow;
#[cfg(feature = "render")]
mod lamp_view;
#[cfg(all(test, feature = "render"))]
mod lamp_view_tests;
#[cfg(feature = "render")]
mod lens;
#[cfg(feature = "render")]
mod lens_frame;
#[cfg(all(test, feature = "render"))]
mod lens_tests;
#[cfg(feature = "render")]
mod light_gain;
#[cfg(feature = "render")]
mod lighting;
#[cfg(feature = "render")]
mod lines;
mod mesh_decode;
#[cfg(feature = "render")]
mod mode_frame;
#[cfg(feature = "render")]
mod mode_lines;
mod model;
mod model_light;
#[cfg(feature = "render")]
mod model_shade;
mod named_view;
mod node_box;
#[cfg(feature = "render")]
mod parallel;
#[cfg(feature = "render")]
mod pick;
#[cfg(feature = "render")]
mod pick_index;
#[cfg(feature = "render")]
mod prepared;
#[cfg(feature = "render")]
mod projection;
#[cfg(feature = "render")]
mod raster;
#[cfg(feature = "render")]
mod render;
#[cfg(feature = "render")]
mod render_error;
#[cfg(feature = "render")]
mod render_mode;
mod schema;
#[cfg(feature = "render")]
mod shadow;
#[cfg(feature = "render")]
mod shadow_cast;
#[cfg(feature = "render")]
mod shadow_set;
#[cfg(all(test, feature = "render"))]
mod shadow_tests;
#[cfg(feature = "render")]
mod sun;
mod tess;
mod tess_face;
mod tess_wire;
#[cfg(test)]
mod testw;
mod texture;
mod tree;
// Some helpers serve only the rasterizer.
#[cfg_attr(not(feature = "render"), allow(dead_code))]
mod vec3;
mod wire;
#[cfg(feature = "render")]
mod work;
mod write;

pub use compressed::PartialMesh;
pub use container::{
    FileStructure, MAX_FILE_STRUCTURES, MAX_INFLATED_BYTES, ModelUnits, PRC_READER_VERSION,
    PrcFile, PrcHeader, SectionKind, UniqueId,
};
pub use elements::MAX_DECODED_ELEMENTS;
pub use error::PrcError;
pub use export::{to_obj, to_stl};
pub use finish::{MaterialFinish, ShininessScale};
pub use gltf::{
    GltfColours, GltfError, GltfImport, GltfLights, GltfOcclusion, GltfOptions, GltfUpAxis,
    read_gltf,
};
#[cfg(feature = "render")]
pub use gpu::{Draw, DrawData, VERTEX_FLOATS};
#[cfg(feature = "render")]
pub use gpu_ao::{
    FrameAmbientOcclusion, GLSL_ES_300_AO_COMPOSITE_FRAGMENT, GLSL_ES_300_AO_FRAGMENT,
    GLSL_ES_300_SCREEN_VERTEX,
};
#[cfg(feature = "render")]
pub use gpu_finish::DrawFinish;
#[cfg(feature = "render")]
pub use gpu_frame::{FrameLights, LIGHT_TABLE_WIDTH, MAX_LIGHTS, MAX_SHADOW_CASTERS, ShadeMode};
#[cfg(feature = "render")]
pub use gpu_lamp_shadow::{
    FrameLampShadows, GLSL_ES_300_LAMP_SHADOW_FRAGMENT, GLSL_ES_300_LAMP_SHADOW_VERTEX,
};
#[cfg(feature = "render")]
pub use gpu_lens::{
    GLSL_ES_300_LENS_LINE_VERTEX, GLSL_ES_300_LENS_POINT_VERTEX, GLSL_ES_300_LENS_RESAMPLE_FRAGMENT,
};
#[cfg(feature = "render")]
pub use gpu_lines::{GLSL_ES_300_LINE_FRAGMENT, GLSL_ES_300_LINE_VERTEX, LINE_VERTEX_FLOATS};
#[cfg(feature = "render")]
pub use gpu_prepass::{GLSL_ES_300_PREPASS_FRAGMENT, GLSL_ES_300_PREPASS_VERTEX};
#[cfg(feature = "render")]
pub use gpu_shader::{GLSL_ES_300_FRAGMENT, GLSL_ES_300_VERTEX};
#[cfg(feature = "render")]
pub use gpu_shadow::{FrameShadow, GLSL_ES_300_SHADOW_FRAGMENT, GLSL_ES_300_SHADOW_VERTEX};
#[cfg(feature = "render")]
pub use gpu_shadow_set::{
    FrameExtraShadows, GLSL_ES_300_LAMP_SHADOW_ALPHA_FRAGMENT,
    GLSL_ES_300_LAMP_SHADOW_ALPHA_VERTEX, GLSL_ES_300_SHADOW_ALPHA_FRAGMENT,
    GLSL_ES_300_SHADOW_ALPHA_VERTEX, MAX_EXTRA_SHADOWS,
};
#[cfg(feature = "render")]
pub use gpu_wide_lines::GLSL_ES_300_WIDE_LINE_VERTEX;
#[cfg(feature = "render")]
pub use lamp_shadow::{DEFAULT_LAMP_SHADOW_MAP_SIZE, MAX_LAMP_SHADOW_MAP_SIZE};
#[cfg(feature = "render")]
pub use lamp_view::{LampCost, LampStats};
#[cfg(feature = "render")]
pub use lens::{FrameLens, LENS_LINE_PIECES, LensFace};
#[cfg(feature = "render")]
pub use light_gain::{MAX_LIGHT_GAIN, MIN_LIGHT_GAIN};
#[cfg(feature = "render")]
pub use lighting::{Light, LightFrame, LightSet, LightingScheme, Lights};
pub use model::{
    AssembleError, AssembleOptions, AssembledModel, DEFAULT_VIEW_DIRECTION, DEFAULT_VIEW_UP,
    MAX_PLACED_ATTRIBUTES, MAX_PLACED_TRIANGLES, MeshFit, StoredVisibility, assemble,
    assemble_with, assemble_with_options,
};
#[cfg(feature = "render")]
pub use model::{default_view_camera, render_default_view, render_model};
pub use model_light::{ModelLight, ModelLightKind, ViewLightUse, ViewLights};
#[cfg(feature = "render")]
pub use model_shade::LampCut;
pub use named_view::{NamedView, UnknownName, UpAxis};
pub use node_box::NodeBox;
#[cfg(feature = "render")]
pub use pick::{Hit, Ray, pick};
#[cfg(feature = "render")]
pub use pick_index::PickIndex;
#[cfg(feature = "render")]
pub use prepared::PreparedModel;
#[cfg(feature = "render")]
pub use render::{
    Bounds, Camera, FisheyeMapping, Image, MAX_RENDER_PIXELS, Projection, RenderError,
    RenderOptions, render, render_coloured,
};
#[cfg(feature = "render")]
pub use render_mode::{EdgeSet, FaceColour, RenderMode, RenderModeKind};
pub use schema::Schema;
#[cfg(feature = "render")]
pub use shadow::{DEFAULT_SHADOW_MAP_SIZE, LightSense, MAX_SHADOW_MAP_SIZE, ShadowLight};
#[cfg(feature = "render")]
pub use shadow_cast::{DEFAULT_SHADOW_CUTOFF, ShadowAlpha, ShadowCasters};
#[cfg(feature = "render")]
pub use sun::{MAX_SUN_KELVIN, MIN_SUN_KELVIN, SunColour, SunLight, kelvin_rgb};
pub use tess::{Tessellation, TriangleMesh, WireTessellation};
pub use texture::{MAX_TEXTURE_PIXELS, Texture, TextureFunction, TextureOrigin, TextureWrap};
pub use tree::{
    EntityOverrides, IDENTITY, Matrix, ModelNode, NameSource, PictureFiles, Placement, StyleAlpha,
    WrapBase, multiply, transform_point,
};
pub use wire::ModelLine;
#[cfg(feature = "render")]
pub use work::MAX_RENDER_PIXEL_TESTS;
pub use write::{
    MAX_DEPTH, MeshScene, PrcWriteError, PrcWriteReport, SceneMaterial, SceneMesh, SceneNode,
    ScenePrimitive, SceneTexture, write_prc,
};
