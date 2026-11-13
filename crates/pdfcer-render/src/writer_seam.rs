//! The export recording and the helpers an out-of-crate writer of it needs.
//!
//! Its one consumer is `pdfcer-emf`, the Enhanced Metafile writer, which
//! walks the same recording [`crate::svg`] does. The module is
//! `#[doc(hidden)]` and outside the semver surface: everything here is the
//! renderer's internal display-list model and changes whenever the
//! interpreter does. A writer built on it is released at this crate's exact
//! version.
//!
//! Why a seam and not a public display-list API: the writers need every
//! field of every recorded op (brush, spot inks, overprint tints, clip
//! table, soft masks, text runs) and mutate the op list (blend flattening,
//! patch planning). A stable model of that is a second interpreter's worth
//! of contract; re-exporting the internals keeps one model and one place
//! it changes.

pub use crate::blend_flatten::{Flattened, flatten_blends};
pub use crate::canvas::{Brush, BrushSpec, LayerPaint, SpotInk, apply_mask};
pub use crate::display_list::{
    ClipDef, ClipId, DeviceBounds, ExportRecording, ExportTally, MaskBuilder, Op, TextGlyph,
    TextRunInfo, record_page_for_export, record_page_for_plates, replay_ops,
};
pub use crate::export_sites::{Site, SiteCause, UNKNOWN, op_box};
pub use crate::gouraud::{
    BAND_STEP, Bands, Decline, FlatBand, Gouraud, ShadingSource, band, band_gradient, tessellate,
    tessellate_gradient,
};
pub use crate::mask_source::MaskSource;
pub use crate::plates::{plate_pixmap, probe_inks};
pub use crate::svg_text::{family_of, is_sfnt, single_char, uniform_paint};
