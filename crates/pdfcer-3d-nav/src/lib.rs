//! Navigation for an interactive 3D model viewer: the orbit camera
//! ([`Orbit`]), first-person walking ([`Walk`]) over the model's surface
//! ([`Solids`]), the lens a picture is drawn through ([`Lens`]), the
//! operator's up axis ([`Upright`]), a file's own saved view ([`SavedAim`],
//! [`ViewFrame`]), and the model's size for fitting ([`reach`]).
//!
//! Every function is pure state over [`pdfcer_3d`]'s [`Camera`](pdfcer_3d::Camera),
//! with no windowing, input or GPU dependency, so a desktop shell and a
//! wasm32 one drive the same logic; the shell owns input mapping, drawing
//! and the clock. Every camera produced is finite for any finite model
//! (lengths are scaled norms, eyes come from
//! [`Camera::looking_at`](pdfcer_3d::Camera::looking_at)), or the call
//! returns an error.
//!
//! Not published to crates.io: its one dependency, `pdfcer-3d`, is not.

#![forbid(unsafe_code)]
#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::unreachable,
    clippy::todo,
    clippy::unimplemented
)]

mod axes;
mod lens;
mod orbit;
mod reach;
mod saved;
mod solids;
mod stick;
mod walk;

pub use axes::{Axis, Upright};
pub use lens::{AUTO_RANGE, CURVES, FISHEYE_WIDEST, FLAT_WIDEST, Lens, LensKind, is_fisheye};
pub use orbit::{Anchor, Axes, FIT_MARGIN, Orbit, PITCH_LIMIT, ZOOM_RANGE};
pub use reach::{radius, reach};
pub use saved::{NotInside, SavedAim, ViewFrame};
pub use solids::Solids;
pub use stick::shaped;
pub use walk::{CLIMB, Intent, Rules, Walk};
