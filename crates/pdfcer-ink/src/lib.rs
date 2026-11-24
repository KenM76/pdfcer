//! # pdfcer-ink — paints composited into ink, and the coverage rules over it
//!
//! A press sheet is a set of colorant planes: four process inks and any spot
//! inks, each holding a tint in `[0, 1]` where `0` is no ink (the medium).
//! [`InkCanvas`] composites paints and transparency groups into those planes
//! in the subtractive space (ISO 32000-1 §11.3.4) and reads them back as ink
//! on the sheet (ISO 32000-2 §11.4.7); [`CoverageWarning`] flags pixels
//! where that ink passes a total-area-coverage limit or forms a rich black.
//!
//! ## Model
//!
//! - A [`Paint`] marks the page through a [`Coverage`] (a mask or spans),
//!   sets some colorants to a tint and leaves the others, and has a constant
//!   alpha, a [`Blend`] and an [`Overprint`] setting.
//! - Overprint follows Table 149 (`CompatibleOverprint`, §11.7.4.3): a
//!   colorant the paint does not set keeps the backdrop under overprint and
//!   is erased without it; under [`Overprint::OnNonzero`] a zero process
//!   tint keeps the backdrop too (§8.6.7). [`ComponentRule`] is that
//!   per-component decision.
//! - A [`Group`] is isolated or not and knockout or not (§11.4.5, §11.4.6).
//!   Closing one applies §11.4.4's backdrop removal and composites the
//!   result into its parent with the group's alpha and blend.
//! - Spot planes take only separable, white-preserving blend modes; others
//!   are `Normal` there (§11.7.4.2). `K` is selected, not blended, under the
//!   non-separable modes (§11.3.5.3).
//!
//! The compositing functions ([`pixel`], [`blend`], [`nonsep`]) are public
//! so a rasteriser with its own pixel storage can share the arithmetic.
//!
//! ## Failure modes
//!
//! [`InkError`]: a page or group layer over [`Limits::max_bytes`], or more
//! colorants than [`Limits::max_colorants`] or four process plus
//! [`MAX_SPOTS`] spots, is refused before anything is allocated.
//!
//! ## Example
//!
//! ```
//! use pdfcer_ink::{Blend, Coverage, InkCanvas, Limits, Overprint, Paint};
//!
//! let mut canvas = InkCanvas::new(2, 1, &[], Limits::new(1 << 20, 4))?;
//! let full = [255, 255];
//! canvas.paint(&Paint {
//!     coverage: Coverage::Mask(&full),
//!     tints: &[Some(1.0)],
//!     alpha: 1.0,
//!     blend: Blend::Normal,
//!     overprint: Overprint::Off,
//! })?;
//! assert_eq!(canvas.landed(0), Some(vec![1.0, 1.0]));
//! # Ok::<(), pdfcer_ink::InkError>(())
//! ```

#![forbid(unsafe_code)]
// Panic-free: the input is built from untrusted pages. Tests opt out per
// module.
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

pub mod blend;
mod canvas;
pub mod coverage;
pub mod nonsep;
pub mod overprint;
pub mod pixel;

pub use blend::Blend;
pub use canvas::{Coverage, Group, InkCanvas, InkError, Limits, Overprint, PROCESS, Paint, Span};
pub use coverage::{CoverageMask, CoverageWarning};
pub use overprint::ComponentRule;
pub use pixel::{MAX_SPOTS, PixelCmyk};
