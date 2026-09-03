//! # `text_edit` — the editable text model (READ-ONLY first slice)
//!
//! Pass 14.0 of pdfcer (`docs/decisions/014-acrobat-text-editing.md` §5.2's
//! 13.0 slice): the first, **read-only** slice of the Acrobat-style
//! in-place text-editing subsystem. It builds a reviewable
//! **Run → Line → Column → Block** hierarchy on top of Pass 4's extraction
//! ([`crate::text_extract`]) and offers a hit-test and caret/selection
//! resolver over it — the substrate a later Pass turns into real editing.
//!
//! ## What this Pass is, and is emphatically NOT
//!
//! It is the **model + navigation** half of decision 014, and nothing
//! more. Concretely:
//!
//! - **No write.** Zero content-stream mutation happens anywhere in this
//!   module. The block model is a derived *view*; editing (the
//!   advance-preserving content-stream surgery that extends Pass 8.0's
//!   redaction machinery) is decision 014's Pass 14.1, deliberately not
//!   here.
//! - **No reflow, no formatting, no UI.** Those are later slices (14.1+)
//!   and the fast-follow ladder (`014` §5.3).
//! - **No new dependency.** Everything is a second pass over data Pass 4
//!   already produced; the one font parser (R21) and the existing crate
//!   graph are untouched.
//!
//! ## The two things it produces
//!
//! 1. **[`EditableTextModel`]** — the recognized structure of one
//!    extracted page: [`Block`]s (paragraphs), [`Line`]s, and column
//!    bands, every inference **counted** in [`BlockDiagnostics`] and the
//!    **sourced-only** [`crate::text_extract::PageText`] always retrievable
//!    via [`EditableTextModel::sourced_view`]. This is the "fuzzy, never
//!    sneaky" contract (rule 4) made structural: the guesses are made
//!    visibly, and the honest lower bound is one call away.
//! 2. **Provenance linkage** — when the page was extracted with
//!    [`ExtractOptions::capture_provenance`](crate::text_extract::ExtractOptions),
//!    each glyph carries a
//!    [`GlyphProvenance`](crate::text_extract::GlyphProvenance) (its show
//!    operator's byte span + stream, governing font resource and `Tf`
//!    size, fill colour, and text/CTM matrices), reachable through
//!    [`EditableTextModel::provenance`]. That is the substrate the Pass
//!    14.1 surgery needs to locate and re-encode a run in place.
//!
//! ## Why it derives, and cites the spec for doing so
//!
//! Words, lines, paragraphs, columns and reading order **do not exist** in
//! an untagged content stream — ISO 32000-1 §14.8's sourced negative
//! results **S1–S9**, the same ones `text_extract/layout.rs` documents. An
//! editor needs those concepts anyway, so the only honest path (decision
//! 014 §4.1) is to derive them, count them, and present them as a
//! reviewable hint the operator corrects — never a silent, authoritative
//! re-layout. See [`model`]'s documentation for the full pipeline and the
//! per-stage spec citations.
//!
//! ## GUI-core separation (load-bearing)
//!
//! This module lives in `pdfcer-core` and introduces **no** GUI/windowing
//! type: hit-testing takes and returns plain page-space coordinates and
//! indices ([`TextPosition`], [`GlyphRef`]). That is what lets the future
//! `pdfce-gui` canvas edit tool and the `pdfcer inspect --text-blocks`
//! command share exactly one recognizer (`ARCHITECTURE.md` §3).

pub mod addtext;
pub mod edit;
pub mod encoding;
pub mod format;
pub mod forms;
pub mod model;
pub mod reflow;
pub mod reflow_apply;
pub mod synth;

pub use addtext::{
    AddTextError, AddTextOutcome, AddTextReport, AddTextRequest, AddTextWrapPreview,
    FontProvenance, NewTextColor, WrapPreviewLine, add_text, preview_wrap,
};
pub use edit::{
    EditError, EditGlyphSource, EditOptions, EditOutcome, EditReport, EditRequest, EditTarget,
    FollowerDisposition, edit_text,
};
// `CompositeEncoding` sits beside `InverseEncoding` deliberately: they are the
// two halves of ONE seam (`plan_edit` picks between them on `font.is_simple()`
// and both answer the same two questions — per-code values for the §9.4.4
// advance sum, and bytes for the show string). Pass 29.0 made composite fonts
// editable but left its types out of this list, so the simple-font half was
// public API and the composite half was reachable only by module path — an
// asymmetry with no reason behind it.
pub use encoding::{
    CharEncoding, CompositeEncodeResult, CompositeEncoding, EncodeResult, InverseEncoding,
    RInvTrigger, Refusal,
};
pub use format::{
    FillModel, FontAcceptance, FontPreflight, FontResourceEntry, FontSelector, FontSibling,
    FormatError, FormatOptions, FormatOutcome, FormatReport, FormatRequest, MetricSpec, NewFill,
    SUBSCRIPT, SUPERSCRIPT, ScriptMetrics, ScriptPosition, StyleOutcome, StyleResolution,
    set_format,
};
pub use forms::{
    FormRef, FormScan, InvocationSet, InvocationSite, MAX_FORM_DEPTH, ResourceTier,
    form_objects_on_page, invocation_set, scan_page_forms,
};
pub use model::{
    Block, BlockDiagnostics, BlockKind, BlockRecognitionOptions, EditableTextModel, GlyphRef, Line,
    TextPosition,
};
pub use reflow::{
    AlignmentSource, BlockAlignment, DetectedAlignment, PageOverflow, ReflowDiagnostics,
    ReflowEngine, ReflowError, ReflowLine, ReflowPreview, ReflowRequest,
    reflow_recognition_options,
};
pub use reflow_apply::{ReflowApplyError, ReflowApplyReport, ReflowOutcome, apply_reflow};
pub use synth::{
    BOLD_STROKE_RATIO, OBLIQUE_TAN, StyleSynthesis, SynthesisOffer, SynthesisPath,
    bold_stroke_width, detect as detect_style_synthesis,
};
