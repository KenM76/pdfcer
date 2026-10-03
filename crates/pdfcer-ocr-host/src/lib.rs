//! OCR hosting for pdfcer's shells: run any discovered OCR model on a page
//! raster, whatever its kind.
//!
//! `pdfcer-core` discovers models (`pdfcer_core::ocr::addons`) and holds the
//! in-process engines, but never starts a process, so it keeps compiling for
//! `wasm32`. A *program* add-on (decision 184) carries an engine executable,
//! and this crate is where it is run: [`ProgramEngine`] re-verifies every
//! hashed file before each run and starts exactly the named program with
//! `std::process::Command`, never a shell.
//!
//! [`OcrRunner::load`] is the one call a shell needs: a discovered
//! [`OcrModel`](pdfcer_core::ocr::addons::OcrModel) plus [`RunOptions`] in,
//! a recogniser out, whichever engine the model is for.
//! [`check_runnable`] answers "can this model run here, and if not why" for
//! a model list or a drop-down without loading anything.
//!
//! Supported program protocols: `tesseract` (PGM on stdin, TSV on stdout).
//!
//! # Example
//!
//! ```no_run
//! use pdfcer_core::ocr::addons::discover_ocr_models;
//! use pdfcer_ocr_host::{OcrRunner, RunOptions, check_runnable};
//!
//! let found = discover_ocr_models(&["models".into()]);
//! let options = RunOptions::new("eng", 300.0);
//! for model in &found.models {
//!     match check_runnable(model, options.policy) {
//!         Ok(()) => println!("{} ({})", model.name, model.kind().as_str()),
//!         Err(why) => println!("{}: {why}", model.name),
//!     }
//! }
//! let model = found.by_name("tesseract-eng").ok_or("not installed")?;
//! let runner = OcrRunner::load(model, &options)?;
//! let words = runner.recognize(2, 1, &[255, 0])?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```

#![forbid(unsafe_code)]
// Panic-free: it reads manifests and child output written by others.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod disclosure;
mod program;
mod runner;
pub mod tesseract;

#[cfg(feature = "paddle")]
pub use disclosure::paddle_disclosure;
#[cfg(feature = "ocr-vl")]
pub use disclosure::paddle_vl_disclosure;
pub use program::{
    PROGRAM_ENGINES, ProgramEngine, ProgramError, ProgramPolicy, ProgramRefusal, ProgramSource,
    program_status,
};
pub use runner::{OcrRunner, RunOptions, RunnerError, check_runnable};
