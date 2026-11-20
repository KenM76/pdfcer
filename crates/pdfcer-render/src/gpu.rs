//! GPU snapshot rasterising (decision 223): the display list turned into
//! GLSL ES 3.00 and buffers a host's graphics card draws, byte-identical to
//! the CPU replay. Behind the non-default `gpu` feature.
//!
//! Each part reproduces one stage of the CPU rasterizer exactly, with a CPU
//! reference of the same algorithm the shader runs, tested byte-for-byte
//! against `tiny-skia`.

pub(crate) mod bins;
pub(crate) mod blend;
pub(crate) mod block;
#[cfg(test)]
pub(crate) mod fill_coverage;
pub(crate) mod fill_edges;
pub(crate) mod glsl;
pub(crate) mod image;
pub(crate) mod layer;
pub(crate) mod prepare;
pub(crate) mod reference;
pub(crate) mod scene;
pub(crate) mod shade;
