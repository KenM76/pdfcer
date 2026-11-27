//! The decision 228 gate: fixtures, the matrix runner, negative controls and
//! the generated matrix doc.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod fixes;
mod fixture;
mod matrix_doc;
mod negative;
mod rebake;
mod rebake_free_text;
mod runner;
