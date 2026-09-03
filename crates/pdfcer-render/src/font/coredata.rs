//! # The `pdfcer-core` font-data seam
//!
//! `pdfcer-render` names every standard-14 metric, standard-14
//! descriptor and Annex D encoding lookup through **this** module
//! rather than through `pdfcer_core::fontdata` directly. The indirection
//! is one line of code and it buys a real property: the crate boundary
//! between "font *data*" and "font *programs*" stays visible at every
//! call site.
//!
//! ## Why the data lives in `pdfcer-core` and not here
//!
//! Decision 004 §7 splits the font problem in two:
//!
//! | Half | Owner | Needed by |
//! |---|---|---|
//! | code → glyph name → Unicode; standard-14 widths and descriptors | **`pdfcer-core`** (`fontdata`) | text **extraction**, `ToUnicode` synthesis, form-field appearance generation — none of which rasterize anything |
//! | glyph name / GID → outline | **`pdfcer-render`** ([`super::program`]) | rasterization only |
//!
//! Extraction needs the first half without the second, so putting the
//! tables in the renderer would force every text-extraction consumer to
//! link a rasterizer. And there must be exactly **one** Adobe Glyph
//! List in the binary (decision 004 §5.6 — which is also why
//! `read-fonts`' `agl` feature stays off): if extraction and rendering
//! resolved glyph names through different tables, a document could be
//! searched for text that is not what was painted.

pub use pdfcer_core::fontdata::*;
