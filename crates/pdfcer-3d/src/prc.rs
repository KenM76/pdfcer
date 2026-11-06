//! The PRC format (ISO 14739-1): the container, its sections and the
//! entity readers, the compressed tessellation and the assembly tree.

pub(crate) mod acof;
pub(crate) mod arrays;
#[doc(hidden)]
pub mod bits;
pub(crate) mod compressed;
pub(crate) mod container;
pub(crate) mod elements;
pub(crate) mod mesh;
pub(crate) mod schema;
pub(crate) mod tess;
#[cfg(test)]
pub(crate) mod testw;
pub(crate) mod tree;
