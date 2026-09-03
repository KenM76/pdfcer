//! # Annotation appearance placement + painting (ISO 32000-1 §12.5.5)
//!
//! The **paint half** of Pass 6.0 (docs/decisions/008). [`pdfcer_core::annot`]
//! walks a page's `/Annots`, decodes flags, and *selects* each
//! annotation's normal (`/AP` `/N`) appearance stream; this module
//! computes the §12.5.5 placement and paints the selected stream over the
//! page content through the **existing** §8.10.1 form-execution path
//! ([`crate::interpret::run_form_at`]). Nothing here synthesises an
//! appearance (R43): an annotation with no usable `/AP` is counted, not
//! drawn.
//!
//! ## The §12.5.5 placement algorithm (implemented verbatim, cited)
//!
//! Given the appearance form XObject's `/BBox` (required) and `/Matrix`
//! (default identity) and the annotation's `/Rect`:
//!
//! - **step a** — transform the four corners of `/BBox` by `/Matrix` to a
//!   quadrilateral, and take the **smallest upright rectangle** enclosing
//!   it (the *transformed appearance box*). A rotating `/Matrix` grows
//!   this box to the axis-aligned bounds of the rotated `/BBox`.
//! - **step b** — compute a matrix **A** that maps the transformed box's
//!   lower-left→`/Rect` lower-left and upper-right→`/Rect` upper-right,
//!   **independently in x and y**. This is an **anisotropic** scale:
//!   aspect ratio is *not* preserved — a square stamp in a wide `/Rect`
//!   is stretched wide. That is **normative**, not a bug (§12.5.5 RAG).
//! - **step c** — the effective transform is **AA = Matrix × A**.
//!
//! ## How the placement is applied without re-implementing §8.10.1
//!
//! `AA = Matrix × A`, so painting the raw appearance stream under
//! `AA × base_device_ctm` is identical to painting it under the ordinary
//! §8.10.1 `Do` procedure (which *itself* concatenates `/Matrix`) if the
//! interpreter's incoming CTM is **`A × base_device_ctm`**. So this module
//! computes only **A**, sets the initial CTM to `A × base`, and hands the
//! stream to [`crate::interpret::run_form_at`], which applies `/Matrix`,
//! clips to `/BBox`, and runs the content — inheriting the resource
//! scoping (X8), cycle guard, depth bound, and font cache the page's own
//! forms use. `/Matrix` is therefore applied **exactly once**; folding it
//! into `A` here would double-apply it (the §12.5.5 RAG's named trap).
//!
//! ## Negative results, all named and counted (R20/R27/R43/R50)
//!
//! - **`/Popup`** (§12.5.6.14): a reader UI window, **never** page
//!   content. Skipped before flags or appearance — a structural rule
//!   stronger than R43 (risk X4). Counted in the total, never painted.
//! - **Hidden / NoView** (§12.5.3): not painted on screen; **counted**
//!   (R50). NoView still prints on the future print path if Print is set;
//!   Pass 6.0 is the screen path, so both suppress here.
//! - **Degenerate transformed box** (zero width/height ⇒ step-b matrix
//!   singular): painted as **nothing**, counted, named — never a
//!   divide-by-zero, never a fabricated placement (risk X2). Likewise a
//!   missing `/Rect` or `/BBox`.
//! - **NoZoom / NoRotate** (§12.5.3): the special post-`AA` transform
//!   about the `/Rect` upper-left corner is a **documented Pass-6.0
//!   deferral** — the base `AA` placement is used and the deviation is
//!   counted+named. These flags appear almost exclusively on icon
//!   subtypes that carry no `/AP` (so are named-not-painted anyway), and
//!   no acceptance fixture exercises them; a wrong post-transform would be
//!   worse than a disclosed omission (fuzzy-never-sneaky). See the Pass
//!   6.0 report / ROADMAP residuals.

use pdfcer_core::annot::{Annotation, Appearance};
// decision 018: read paths take a `DocumentView` (graph + byte source), so
// the same code renders a loaded file or an editing session's unsaved state.
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::page_tree::{Page, Rect};
use pdfcer_core::view::DocumentView;
use tiny_skia::{Point, Transform};

use crate::canvas::Canvas;

use crate::font::{FontEnvironment, RenderPolicy};
use crate::gstate::GraphicsState;
use crate::interpret::{self, Diagnostics};

/// A transformed appearance box that is thinner than this on either axis
/// is treated as degenerate: the §12.5.5 step-b fit matrix would divide
/// by (near) zero, so `A` is singular and there is no honest placement.
///
/// A small positive epsilon rather than exact zero, because a `/Matrix`
/// that collapses `/BBox` to a sliver is degenerate for placement purposes
/// well before the extent is bit-exactly `0.0`, and dividing by `1e-9`
/// produces a placement no one can see anyway.
const MIN_BOX_EXTENT: f32 = 1e-6;

/// Which of the four §12.5.6 classes an annotation `/Subtype` belongs to,
/// **for the purpose of deciding whether a given [`AnnotationScope`] paints
/// it**.
///
/// # Why a class and not a subtype list at every call site
///
/// The scope question ("does this render want stamps?") is asked once per
/// annotation, and the answer depends on a *partition* of Table 169, not on
/// the individual subtype. Naming the partition once means the markup list
/// — the part that is sourced from the standard and therefore the part that
/// can be got wrong — lives in exactly one place
/// ([`AnnotationClass::of_subtype`]) rather than being re-derived by each
/// scope.
///
/// # The partition, and where it comes from (ISO 32000-1 Table 169's
/// "Markup" column)
///
/// Table 169 (§12.5.6.1) lists all 26 standard `/Subtype` values with a
/// per-subtype `Markup` Yes/No column, and the partition it gives is
/// **total** — 17 Yes, 9 No, no blank cell and no conditional value.
/// That two-way split plus two named subtypes — `/Stamp` (§12.5.6.12) and
/// `/Widget` (§12.5.6.19) — is exactly what Acrobat's four-way print scope
/// needs, so the class here is four-valued rather than two.
///
/// `/Stamp` and `/Widget` are **not** exceptions to the markup rule: a
/// `/Stamp` *is* a markup annotation (Table 169 `Markup` = Yes) and a
/// `/Widget` *is* not (Table 169 `Markup` = No). They are broken out here
/// only because two of the four scopes name them individually.
///
/// # ★ Table 169, NOT §12.5.6.2's prose — the prose is wrong (erratum T169-E1)
///
/// §12.5.6.2 also states the split in words, and its parenthetical names
/// **five** non-markup subtypes: "*For all other annotation types (`Link`,
/// `Movie`, `Widget`, `PrinterMark`, and `TrapNet`) …*". Table 169 marks
/// **nine** as `Markup` = No. The three the prose omits — `/Screen` (PDF
/// 1.5), `/Watermark` (1.6) and `/3D` (1.6) — are precisely the
/// non-markup subtypes added *after* PDF 1.4; the sentence was never
/// updated when they landed. This is a real defect in ISO 32000-1, not a
/// transcription slip, and the PDF Association's public ISO 32000-2 errata
/// does not correct it (it edits §12.5.6.2 twice, but not this bullet).
///
/// Consequence, and the reason it is written here rather than in a
/// changelog: **an implementation that derives the partition from the
/// prose classifies `/Screen`, `/Watermark` and `/3D` as markup** — so a
/// "Document" print scope would silently drop a page's watermark, which is
/// exactly the kind of content loss that looks like a rendering bug six
/// months later. The `Markup` arm below is transcribed from the table.
///
/// Sourced 2026-08-10 from `_sources\PDF32000_2008.pdf` physical page 398
/// (printed 390) by two independent extractors agreeing on all 26 rows;
/// recorded in the project spec RAG at
/// `iso32000\iso32000__s__12.5.6.md`, "PRINT/PAINT-SCOPE PARTITION AXIS".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum AnnotationClass {
    /// A markup annotation **other than** `/Stamp` — Table 169 `Markup`
    /// = Yes (§12.5.6.2). Comments, highlights, ink, free text, sticky
    /// notes, callouts, measurement geometry: everything an operator would
    /// call "a markup".
    Markup,
    /// `/Stamp` (§12.5.6.12). A markup annotation, split out because
    /// Acrobat's "Document and Stamps" scope admits stamps **and no other
    /// markup type** — a narrower slice than "Document and Markups", not a
    /// synonym for it.
    Stamp,
    /// `/Widget` (§12.5.6.19) — the form-field annotation, Table 169
    /// `Markup` = No. The only subtype "Form fields only" paints.
    ///
    /// # The test is `/Subtype`, and it must not be "does it have `/FT`"
    ///
    /// §12.5.6.19: interactive forms "use **widget annotations** (PDF 1.2)
    /// to represent the appearance of fields", and `/Widget` is the sole
    /// form-field subtype — which is what makes "Form fields only" and the
    /// markup scopes disjoint on subtype alone.
    ///
    /// A widget may carry field keys inline, because §12.5.6.19 permits a
    /// single-widget field to merge the field and annotation dictionaries;
    /// that does not change its class (it is an annotation first, R49).
    /// The trap is the *converse*: an **unmerged** widget sitting in a
    /// field's `/Kids` array has no `/FT` of its own — it inherits the
    /// field type through `/Parent` — so a "is this a form field?" test
    /// written as "does the dictionary have `/FT`?" would miss exactly the
    /// multi-widget fields (radio groups, repeated fields) that a
    /// print-the-fields-only job most needs.
    FormField,
    /// Everything else — Table 169 `Markup` = No and not a `/Widget`.
    /// The complete list, from Table 169's own column: `/Link`, `/Popup`,
    /// `/Movie`, `/Screen`, `/PrinterMark`, `/TrapNet`, `/Watermark`,
    /// `/3D`. Plus any subtype this build does not recognise.
    ///
    /// # Why an unrecognised subtype lands here and not in [`Self::Markup`]
    ///
    /// A `/Subtype` pdfcer has never heard of is, by construction, not one
    /// of the markup types the standard enumerates, so `Other` is the
    /// *true* answer rather than a fallback. It is also the safe one for
    /// the scope that matters: "Document" is the scope an operator picks to
    /// keep review comments off a printed page, and classifying an unknown
    /// subtype as `Markup` would let a future extension type silently
    /// disappear from a "Document" render, which is the direction that
    /// loses content rather than the direction that shows too much.
    ///
    /// `/Popup` is listed here for completeness only. It never reaches a
    /// scope decision: [`survey_page_annotations`] refuses it structurally
    /// first (§12.5.6.14 — a pop-up is a reader window, never page
    /// content), which is a stronger rule than any scope and must stay
    /// ahead of one.
    Other,
}

impl AnnotationClass {
    /// Classify a raw `/Subtype` name (the bytes as they appear in the
    /// file, without the leading solidus).
    ///
    /// # The markup list is the sourced part
    ///
    /// The `Stamp` and `Markup` arms below are, between them, ISO 32000-1
    /// Table 169's complete `Markup` = Yes set — all **seventeen**:
    /// `Text`, `FreeText`, `Line`, `Square`, `Circle`, `Polygon`,
    /// `PolyLine`, `Highlight`, `Underline`, `Squiggly`, `StrikeOut`,
    /// `Stamp`, `Caret`, `Ink`, `FileAttachment`, `Sound`, `Redact`.
    ///
    /// Two of those are worth pointing at, because both are easy to get
    /// wrong from an intuition about what "markup" means:
    ///
    /// - **`Sound` is markup** (Table 169 Yes, corroborated by its own
    ///   bullet inside §12.5.6.2's positive grouping) even though it is the
    ///   one markup subtype with **no pop-up window**. "Has a pop-up" is
    ///   not a test for "is markup".
    /// - **`Redact` is markup** (Table 169 Yes, PDF 1.7 §12.5.6.23). A
    ///   redaction *mark* is a review artefact and belongs off a "Document"
    ///   print exactly like a comment does — which is a happy alignment,
    ///   not a coincidence: unapplied redaction marks are the last thing
    ///   that should print onto a document handed to someone.
    ///
    /// It is deliberately a match on *names*, not a lookup keyed off
    /// anything pdfcer infers: the partition is normative, so a subtype
    /// either appears in the standard's table or it does not.
    ///
    /// # Case and encoding
    ///
    /// Name objects are case-sensitive (§7.3.5), so the comparison is
    /// exact. A file writing `/stamp` has not written a stamp annotation,
    /// and pdfcer does not repair it into one — it classifies as
    /// [`Self::Other`] and the operator sees it under whatever scope admits
    /// non-markup annotations, rather than pdfcer quietly deciding what the
    /// producer meant.
    #[must_use]
    pub fn of_subtype(subtype: &[u8]) -> Self {
        match subtype {
            // §12.5.6.19 — the form-field annotation. Checked first because
            // it is ~88 % of organic annotations (decision-008 census), so
            // the common case exits the match immediately.
            b"Widget" => Self::FormField,
            // §12.5.6.12 — a markup annotation, named separately because
            // "Document and Stamps" admits it alone.
            b"Stamp" => Self::Stamp,
            // Table 169 `Markup` = Yes, minus `/Stamp` (above).
            b"Text" | b"FreeText" | b"Line" | b"Square" | b"Circle" | b"Polygon" | b"PolyLine"
            | b"Highlight" | b"Underline" | b"Squiggly" | b"StrikeOut" | b"Caret" | b"Ink"
            | b"FileAttachment" | b"Sound" | b"Redact" => Self::Markup,
            // PDF 2.0 (§12.5.6.24), which ISO 32000-1's Table 169 predates:
            // "A projection annotation (PDF 2.0) is a **markup annotation
            // subtype**". Classified as markup so a 2.0 file's projection
            // markup is withheld by a "Document" scope like every other
            // markup, rather than printing because pdfcer had never heard of
            // it.
            //
            // EVIDENCE TIER: secondary. The quotation is from the PDF
            // Association's public ISO 32000-2 errata, not from a held copy
            // of 32000-2 (paywalled, not in `_sources`). It is recorded as
            // a named, weaker-sourced arm rather than folded silently into
            // the list above, so a later primary-source check knows exactly
            // which line to confirm.
            b"Projection" => Self::Markup,
            // Table 169 `Markup` = No — the complete set is Link, Popup,
            // Movie, Screen, PrinterMark, TrapNet, Watermark, 3D (Widget,
            // the ninth, is handled above) — plus anything this build does
            // not recognise. See [`Self::Other`] for why an unknown name
            // belongs here rather than under `Markup`.
            _ => Self::Other,
        }
    }
}

/// Which classes of annotation — and whether the page content itself — a
/// render paints.
///
/// # Why this exists (Acrobat parity, and what pdfcer could not say before)
///
/// Until this type, a render's annotation control was a single
/// `RenderOptions::annotations: bool`: paint every annotation, or paint
/// none. Acrobat's own print dialog offers **four** scopes, catalogued in
/// the project's Acrobat feature RAG
/// (`printing__annotation_and_form_printing.md`, verified 2026-08-10):
///
/// | Acrobat's name | Variant here | What paints |
/// |---|---|---|
/// | Document | [`Self::Document`] | page content + non-markup annotations (form fields, links) |
/// | Document and Markups | [`Self::DocumentAndMarkups`] | page content + **every** annotation |
/// | Document and Stamps | [`Self::DocumentAndStamps`] | page content + non-markup + `/Stamp` **only** |
/// | Form fields only | [`Self::FormFieldsOnly`] | `/Widget` appearances, **no page content at all** |
///
/// pdfcer could express exactly one of those four ("Document and Markups",
/// as `annotations: true`) plus a fifth of its own that Acrobat has no name
/// for — content with no annotations whatsoever, which is
/// [`Self::ContentOnly`] and is what `annotations: false` has always meant.
/// So the enum is five-valued: the four Acrobat scopes, and pdfcer's
/// content-only raster.
///
/// # "Document and Stamps" is narrower than "Document and Markups"
///
/// It is not a synonym. It admits `/Stamp` and **no other markup type** —
/// sticky notes, ink, highlights and free text stay excluded. The Acrobat
/// RAG names this trap directly: a two-option implementation that collapsed
/// the two would over-include every non-stamp markup, and would do it
/// silently, since the result still looks like "a page with annotations
/// on it".
///
/// # The default is [`Self::DocumentAndMarkups`], and that is a
/// compatibility decision, not a parity one
///
/// The Acrobat RAG records that free Reader defaults to "Document"
/// (markups excluded) while Acrobat Pro defaults to "Document and
/// Markups", and recommends Reader's default for a print path. This type's
/// [`Default`] is `DocumentAndMarkups` regardless, for a reason that has
/// nothing to do with which product to imitate: it is what
/// `RenderOptions::default()` has always done, every existing caller relies
/// on it, and a *rendering* default that started hiding a document's markup
/// would be a silent content loss in the GUI page view. A print path that
/// wants Reader's default sets it explicitly — which is the right shape
/// anyway, because "what a print job defaults to" is a decision for the
/// print path to own, not one to inherit from the rasterizer.
///
/// # The scope is a PRODUCT construct over a NORMATIVE partition
///
/// Worth stating plainly, because the two halves have very different
/// standing. ISO 32000-1 defines the markup/non-markup partition
/// normatively (Table 169) but attaches **no print-scope semantics to it
/// whatsoever** — it nowhere says markup annotations are the suppressible
/// review layer. The four-way scope is Acrobat's product design, sourced
/// from the Acrobat RAG; the partition it selects over is the standard's.
/// So a disagreement about *which subtypes are markup* is a spec question
/// with a right answer, and a disagreement about *what a scope should do
/// with them* is a parity question with a sourced-behaviour answer. Do not
/// resolve one by appealing to the other.
///
/// # Interaction with the per-annotation `/F` Print flag
///
/// This scope and §12.5.3's flags compose as **AND**, never OR — the
/// Acrobat RAG states the same requirement, and the spec side agrees: a
/// scope selects a *candidate set*, then §12.5.3 decides whether a
/// candidate paints. An annotation whose flags suppress it is not painted
/// no matter which scope is selected, and a scope that excludes an
/// annotation's class withholds it even when its flags say it should show.
/// Neither mechanism can override the other, and both are counted (see
/// [`survey_page_annotations`]).
///
/// `Hash` is derived (beyond the house style for a small settings enum)
/// because a shell that caches rasters keys them on the render inputs, and
/// the annotation scope is one — `pdfce-gui`'s raster cache already keys on
/// the `bool` this replaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum AnnotationScope {
    /// Page content only — **no** annotations of any class.
    ///
    /// Not one of Acrobat's four; it is pdfcer's own, and it is what
    /// `RenderOptions::annotations = false` has always meant. The
    /// content-only raster is load-bearing for more than a user preference:
    /// it is the pre-Pass-6.0 baseline the round-trip raster oracle and any
    /// A/B comparison reproduce, so it must stay exactly reproducible.
    ///
    /// Annotations are still **surveyed and counted** under this scope —
    /// only the painting stops. A suppressed render that could not say how
    /// many annotations it was not showing would be sneaky (R27/R50).
    ContentOnly,
    /// Page content + every annotation whose Table 169 `Markup` column is
    /// **No** — form-field widgets and links. Acrobat's "**Document**".
    ///
    /// # Why non-markup, non-widget subtypes paint here
    ///
    /// The dialog's own wording is about comments and forms, and the
    /// partition it expresses is markup-vs-not. `/Link`, `/Screen`,
    /// `/PrinterMark`, `/Watermark` and friends are all Table 169
    /// `Markup` = No, so they are on the same side of the split as the
    /// widgets and paint here. This is a **reasoned reading** of a
    /// dialog-level control against the standard's own partition, not a
    /// separately-sourced Acrobat behaviour: no source in the Acrobat RAG
    /// says what Acrobat does with a `/Link` appearance under "Document",
    /// and in practice almost no link carries an `/AP` at all. Recorded
    /// here so the next reader does not mistake it for a measured fact.
    Document,
    /// Page content + **every** annotation. Acrobat's "**Document and
    /// Markups**", and the [`Default`] — see the type docs for why the
    /// default is this rather than Reader's.
    ///
    /// Behaviourally identical to the pre-existing
    /// `RenderOptions::annotations = true`, which is what keeps every
    /// existing caller's raster byte-identical.
    #[default]
    DocumentAndMarkups,
    /// Page content + non-markup annotations + `/Stamp`, and **no other
    /// markup type**. Acrobat's "**Document and Stamps**".
    DocumentAndStamps,
    /// `/Widget` appearances **only** — no other annotation class, and
    /// **no page content**. Acrobat's "**Form fields only**".
    ///
    /// # The page-content suppression is the whole point
    ///
    /// The sourced real-world workflow is printing onto a **pre-printed
    /// paper form**: the page background already exists as physical paper,
    /// so painting it again would double-print the form's own rules and
    /// boxes over themselves. This is therefore the one scope that changes
    /// what the *page* does, not merely what its annotations do, and the
    /// suppression is disclosed in the diagnostics
    /// ([`crate::Diagnostics::page_content_suppressed`]) so a caller
    /// handed a nearly-blank raster can always tell "the form fields are
    /// empty" from "pdfcer was told not to draw the page".
    FormFieldsOnly,
}

impl AnnotationScope {
    /// Whether this scope paints an annotation of `class`.
    ///
    /// The single decision table for the whole feature — every "is this
    /// annotation wanted?" question in the crate routes through here, so
    /// the four Acrobat scopes cannot come to disagree between the
    /// annotation walk, a future print path, and a shell's preview.
    ///
    /// This answers the **class** question only. It is ANDed with the
    /// §12.5.3 flag check and the §8.11.3.3 optional-content check by
    /// [`survey_page_annotations`]; a `true` here is permission to
    /// consider the annotation, not permission to paint it.
    #[must_use]
    pub const fn paints_class(self, class: AnnotationClass) -> bool {
        match self {
            // Nothing, ever — the content-only raster.
            Self::ContentOnly => false,
            // Table 169 `Markup` = No only: widgets and the rest.
            Self::Document => matches!(class, AnnotationClass::FormField | AnnotationClass::Other),
            // Everything.
            Self::DocumentAndMarkups => true,
            // "Document" plus stamps — and stamps alone of the markup
            // types. `AnnotationClass::Markup` is deliberately absent from
            // this arm; adding it would silently turn this scope into
            // `DocumentAndMarkups`.
            Self::DocumentAndStamps => matches!(
                class,
                AnnotationClass::FormField | AnnotationClass::Other | AnnotationClass::Stamp
            ),
            // Widgets alone.
            Self::FormFieldsOnly => matches!(class, AnnotationClass::FormField),
        }
    }

    /// Whether this scope paints the **page's own content streams**.
    ///
    /// `false` for [`Self::FormFieldsOnly`] and `true` for every other
    /// scope. Split out as its own predicate rather than inlined at the
    /// render entry point because it is the one place a scope reaches
    /// outside the annotation walk, and a caller (or a future print path)
    /// must be able to ask the question without re-deriving which variant
    /// it was.
    #[must_use]
    pub const fn paints_page_content(self) -> bool {
        !matches!(self, Self::FormFieldsOnly)
    }
}

/// Survey every annotation on `page`, updating `diag`'s annotation
/// counters, and paint the appearance of each annotation that is both
/// visible and admitted by `scope` over the already-rendered page content
/// (ISO 32000-1 §12.5).
///
/// Called by [`crate::render_page_with`] **after** the page content is
/// interpreted, so appearances composite on top (their natural z-order).
/// `base_ctm` is the page's device CTM (CropBox → origin, y-flip, scale,
/// `/Rotate`) — the same transform the page content was drawn under, so an
/// annotation rotates with the page by default (unless NoRotate, deferred).
///
/// The **counting is unconditional**; only the *painting* is gated. So a
/// `render-page --no-annotations` (or the GUI toggle off, or a "Document"
/// print scope) still discloses how many annotations the page carries, how
/// many are hidden, how many its scope withheld, and how many have no
/// appearance — a suppressed render is honest about what it is *not*
/// showing (R50/R27), and the pre-6.0 content-only raster is reproduced
/// exactly because no appearance pixels are laid down.
///
/// # The three independent gates, and why none of them short-circuits
///
/// An annotation must clear all three to paint, and they are ANDed:
///
/// 1. **Structure** — a `/Popup` is never page content (§12.5.6.14). This
///    is checked *before* everything else and `continue`s, because it is
///    not a preference at all: no scope, flag or layer state can make a
///    pop-up window into page content, and a scope check ahead of it could
///    only ever weaken that guarantee.
/// 2. **Scope** — does the caller want this *class* of annotation
///    ([`AnnotationScope::paints_class`])? Counted in
///    [`crate::Diagnostics::annotations_out_of_scope`], but deliberately
///    **not** a `continue`: the survey goes on, so `annotations_hidden` and
///    `annotations_without_ap` keep counting under a restricted scope
///    exactly as they do under the default one. That is what makes
///    `annotations = false` (⇒ [`AnnotationScope::ContentOnly`]) report the
///    same counters it always has, instead of quietly emptying the R43
///    named-not-painted census the moment a caller narrows the scope.
/// 3. **Document state** — §12.5.3's Hidden/NoView flags and §8.11.3.3's
///    optional-content state. These *do* `continue`, as they always have:
///    an annotation the document itself hides has no appearance decision
///    left to make.
///
/// A single annotation can legitimately be counted under both gate 2 and
/// gate 3 (an out-of-scope annotation that is also Hidden). Both facts are
/// true and independent, so both are reported rather than one masking the
/// other.
///
/// Over clippy's argument bound by one, since 2026-08-07's cancellation
/// parameter. Same `#[allow]` and same reasoning as
/// [`crate::interpret::run_nested`]: this is one link in the renderer's
/// argument-threading chain, and a params struct here would only move
/// the list somewhere less visible.
#[allow(clippy::too_many_arguments)]
pub(crate) fn survey_page_annotations(
    doc: &DocumentView<'_>,
    page: &Page,
    base_ctm: Transform,
    fonts: &FontEnvironment,
    scope: AnnotationScope,
    diag: &mut Diagnostics,
    canvas: &mut Canvas<'_>,
    cancel: Option<&crate::cancel::RenderCancel>,
    policy: RenderPolicy,
) {
    // Pass 12.M2 (§8.11.3.3): the set of optional-content groups the catalog
    // /OCProperties /D config leaves OFF by default. An annotation whose /OC
    // resolves to an OFF group is not painted (authored-layer /OC honouring;
    // full content-stream BDC/EMC /OC stays deferred — decision 011 §2.4).
    // Computed once; empty (⇒ nothing hidden) when the file has no optional
    // content, so this is a no-op on every pre-12.M2 file.
    // Annotation `/OC` (§8.11.3.3) answers to the same layer state as
    // content-stream `/OC`, and must read it from the same place: an
    // operator who hides a layer expects the dimension annotations ON
    // that layer to go with it. Splitting the two sources is how a
    // toggle ends up half-working.
    let oc_off = match policy.layers {
        // An operator override replaces everything, including usage
        // application — §8.11.4.5: "Manual changes shall override the
        // states that were set automatically… and shall not be
        // readjusted based on usage application dictionaries."
        Some(v) => v.hidden_set().clone(),
        None => {
            let mut off = pdfcer_core::annot::optional_content_default_off(doc);
            if let Some(magnification) = policy.view_magnification {
                // The notes are dropped HERE on purpose: this is the
                // render path, and it has no channel to report them on.
                // The shells surface them from their own call.
                let _ = pdfcer_core::annot::apply_view_usage(doc, &mut off, magnification);
            }
            off
        }
    };

    // `AS-A1` (R169): what to show for a multi-entry /AP /N subdictionary
    // that carries no /AS. §12.5.5 makes /AS Required there and states no
    // recovery, so the direction is the operator's — and it is decided
    // HERE, at appearance SELECTION, not at paint time, which is why the
    // policy goes into `page_annotations_with` rather than being consulted
    // below. The default paints nothing and the annotation is counted as
    // state-unresolved either way.
    for annot in &pdfcer_core::annot::page_annotations_with(doc, page.id, policy.missing_as) {
        diag.annotations_total += 1;
        if annot.is_widget() {
            diag.annotations_widget += 1;
        }

        // §12.5.6.14 (risk X4): a /Popup is a reader window, never page
        // content — checked before flags/appearance. Counted in the
        // total, provably never painted.
        if annot.is_popup {
            continue;
        }

        // Gate 2 (see the function docs): does this render's scope want
        // this CLASS of annotation? Decided from the raw `/Subtype` bytes
        // against ISO 32000-1 Table 169's markup partition. Counted, and
        // then the survey continues — the counters below must mean the
        // same thing under every scope.
        let in_scope = scope.paints_class(AnnotationClass::of_subtype(&annot.subtype));
        if !in_scope {
            diag.annotations_out_of_scope += 1;
        }

        // §12.5.3 Table 165 (R50): Hidden (screen+print) and NoView
        // (screen) suppress on-screen painting — honoured AND counted.
        if annot.flags.suppressed_on_screen() {
            diag.annotations_hidden += 1;
            continue;
        }
        // §8.11.3.3: annotation visibility = (flags permit) AND (OC state
        // visible). An /OC pointing at an OFF group hides the annotation,
        // counted alongside the flag-hidden ones (Pass 12.M2).
        if let Some(oc) = annot.oc
            && pdfcer_core::annot::oc_is_hidden(doc, oc, &oc_off)
        {
            diag.annotations_hidden += 1;
            continue;
        }

        match &annot.appearance {
            Appearance::Normal { stream_id } => {
                // `annotations_painted` and the placement counters only
                // mean something when painting is enabled; when suppressed
                // the annotation is disclosed by `annotations_total` and
                // `annotations_out_of_scope` alone.
                if in_scope {
                    paint_appearance(
                        doc, page, base_ctm, fonts, annot, *stream_id, diag, canvas, cancel, policy,
                    );
                }
            }
            // R43 named-not-painted, counted by subtype — the measured
            // demand signal for the later generation Passes.
            Appearance::None => {
                *diag
                    .annotations_without_ap
                    .entry(annot.subtype_label())
                    .or_insert(0) += 1;
            }
            // §12.5.5 NOTE 3: an /AS that could not be resolved — display
            // nothing, counted separately (the annotation HAS appearances;
            // only selection failed).
            Appearance::StateUnresolved => {
                diag.annotations_appearance_state_missing += 1;
            }
        }
    }
}

/// Place and paint one annotation's selected normal appearance
/// (§12.5.5), or refuse it by a named, counted diagnostic.
#[allow(clippy::too_many_arguments)] // every argument is placement input.
fn paint_appearance(
    doc: &DocumentView<'_>,
    page: &Page,
    base_ctm: Transform,
    fonts: &FontEnvironment,
    annot: &Annotation,
    stream_id: Option<ObjId>,
    diag: &mut Diagnostics,
    canvas: &mut Canvas<'_>,
    cancel: Option<&crate::cancel::RenderCancel>,
    policy: RenderPolicy,
) {
    // /Rect is Required (Table 164) and is the §12.5.5 placement target.
    let Some(rect) = annot.rect else {
        diag.annotations_placement_degenerate += 1;
        diag.note_annotation("annotation /AP present but /Rect is missing - not placed");
        return;
    };
    // Streams are indirect (§7.3.8.1), so a well-formed /N carries an id.
    let Some(id) = stream_id else {
        diag.annotations_placement_degenerate += 1;
        diag.note_annotation("annotation /AP /N stream not reachable by reference - not placed");
        return;
    };
    let Object::Stream(stream) = doc.resolved(id) else {
        // Selection said this resolved to a stream; a disagreement here is
        // a race no read-only path can produce, but it is refused not
        // panicked.
        diag.annotations_placement_degenerate += 1;
        diag.note_annotation("annotation /AP /N did not resolve to a stream - not placed");
        return;
    };

    // §12.5.5 step a needs /BBox (Table 95, Required for a form XObject).
    let Some(bbox) = read_rect_numbers(doc, &stream.dict, b"BBox") else {
        diag.annotations_placement_degenerate += 1;
        diag.note_annotation("annotation appearance has no /BBox - cannot place");
        return;
    };
    let matrix = read_matrix(doc, &stream.dict);

    // step a: transform /BBox by /Matrix, take the upright bounding box.
    let Some(tbox) = transformed_appearance_box(bbox, matrix) else {
        // Degenerate transformed box ⇒ step-b matrix singular (risk X2).
        diag.annotations_placement_degenerate += 1;
        diag.note_annotation(
            "annotation appearance box is degenerate (zero width or height) - not placed",
        );
        return;
    };

    // step b: A maps the transformed box to /Rect (anisotropic).
    let a = fit_matrix(tbox, rect);
    // AA = Matrix × A applied to the page CTM: initial = A × base, and
    // `run_form_at`'s `do_form` concatenates /Matrix on top (module docs).
    let placement = a.post_concat(base_ctm);
    let initial = GraphicsState::default_with_ctm(placement);

    // ★ §12.5.2 /CA -- the annotation's CONSTANT OPACITY, applied to the
    // annotation AS COMPOSITED onto the page.
    //
    // Until 2026-08-14 this was ignored entirely, and the consequence was not
    // limited to markup pdfcer authors: EVERY IMPORTED ANNOTATION CARRYING /CA
    // RENDERED SOLID. Reduced opacity is the house style for a shaded area or
    // a fill over a drawing -- precisely because the drawing underneath has to
    // stay readable -- so a Bluebeam or Acrobat cloud at 50% covered the thing
    // the operator opened the file to read. Reported by the `pdfcer-gui` session,
    // which correctly ranked it as a FIDELITY DEFECT IN THE CURRENT PRODUCT
    // rather than a prerequisite of a future authoring control.
    //
    // Absent means opaque (§12.5.2), so the common path is unchanged and pays
    // nothing: no scratch allocation, no composite.
    let alpha = annot.constant_alpha.unwrap_or(1.0);
    #[allow(clippy::float_cmp)]
    let sub = if alpha >= 1.0 {
        interpret::run_form_at_on(
            doc,
            stream,
            Some(id),
            &page.resources,
            fonts,
            initial,
            canvas,
            cancel,
            policy,
        )
    } else {
        // The appearance must be composited as ONE object at `alpha`, not
        // drawn with each operator at `alpha`. Those differ wherever the
        // appearance overlaps itself -- a cloud's arc chain, a polygon's
        // border meeting its fill -- where per-operator alpha would darken
        // every seam and the correct result is uniform.
        //
        // That is precisely `Canvas::layer`, and it is precisely what
        // §11.4.5's transparency-group composite does in `do_form`. The two
        // were separate implementations of one operation until Pass 75.0;
        // the scratch buffer, its TRANSPARENT initial state and the
        // no-mask-on-the-composite rule now live in one place.
        //
        // `PixmapPaint::default()`'s blend mode is `SourceOver`, which is
        // what this composite has always used -- stated rather than
        // defaulted, because `LayerPaint` makes it an argument.
        #[allow(clippy::cast_possible_truncation)]
        let paint = crate::canvas::LayerPaint {
            opacity: alpha as f32,
            blend: tiny_skia::BlendMode::SourceOver,
            // An annotation's `/CA` composite is a constant-alpha composite,
            // not a blend-mode one -- §12.5.5 has no `/BM` at this level.
            nonseparable: None,
        };
        let painted = canvas.layer(paint, |sub_canvas| {
            interpret::run_form_at_on(
                doc,
                stream,
                Some(id),
                &page.resources,
                fonts,
                initial,
                sub_canvas,
                cancel,
                policy,
            )
        });
        let Some(sub) = painted else {
            diag.annotations_placement_degenerate += 1;
            diag.note_annotation(
                "annotation /CA compositing buffer could not be allocated - painted opaque",
            );
            return;
        };
        sub
    };
    diag.merge(sub);
    diag.annotations_painted += 1;

    // NoZoom/NoRotate special placement is a documented Pass-6.0 deferral
    // (module docs): the base AA placement is used and the deviation is
    // disclosed rather than approximated wrongly.
    if annot.flags.no_zoom() || annot.flags.no_rotate() {
        diag.note_annotation(
            "annotation NoZoom/NoRotate placement adjustment deferred (base AA placement used)",
        );
    }
}

/// §12.5.5 step a: transform the corners of `bbox` (normalised
/// `[minx, miny, maxx, maxy]`) by `matrix` and return the smallest upright
/// rectangle enclosing the resulting quadrilateral as
/// `[minx, miny, maxx, maxy]`.
///
/// Returns `None` when that box is degenerate (either extent
/// ≤ [`MIN_BOX_EXTENT`]) — the step-b fit matrix is then singular
/// (division by zero on the collapsed axis), and §12.5.5 specifies no
/// handling, so the caller paints nothing and names it rather than
/// fabricating a placement (risk X2 / §12.5.5 RAG negative result).
fn transformed_appearance_box(bbox: [f64; 4], matrix: Transform) -> Option<[f32; 4]> {
    let [minx, miny, maxx, maxy] = bbox;
    let mut corners = [
        Point::from_xy(minx as f32, miny as f32),
        Point::from_xy(maxx as f32, miny as f32),
        Point::from_xy(maxx as f32, maxy as f32),
        Point::from_xy(minx as f32, maxy as f32),
    ];
    matrix.map_points(&mut corners);

    let (mut tminx, mut tminy) = (f32::INFINITY, f32::INFINITY);
    let (mut tmaxx, mut tmaxy) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in corners {
        tminx = tminx.min(p.x);
        tminy = tminy.min(p.y);
        tmaxx = tmaxx.max(p.x);
        tmaxy = tmaxy.max(p.y);
    }
    // NaN/inf guard (a hostile /Matrix could produce them) and degeneracy.
    if !(tminx.is_finite() && tminy.is_finite() && tmaxx.is_finite() && tmaxy.is_finite()) {
        return None;
    }
    if (tmaxx - tminx) <= MIN_BOX_EXTENT || (tmaxy - tminy) <= MIN_BOX_EXTENT {
        return None;
    }
    Some([tminx, tminy, tmaxx, tmaxy])
}

/// §12.5.5 step b: the scale-and-translate matrix **A** mapping the
/// transformed appearance box `tbox` (`[minx, miny, maxx, maxy]`) onto the
/// annotation `/Rect`, **independently in x and y** (anisotropic — aspect
/// ratio is not preserved; normative).
///
/// A maps `tbox` lower-left → `/Rect` lower-left and `tbox` upper-right →
/// `/Rect` upper-right, so
/// `sx = Rect.width / tbox.width`, `sy = Rect.height / tbox.height`,
/// `tx = Rect.llx − sx·tbox.minx`, `ty = Rect.lly − sy·tbox.miny`.
/// `tbox`'s extents are guaranteed positive by
/// [`transformed_appearance_box`], so the divisions are safe here.
fn fit_matrix(tbox: [f32; 4], rect: Rect) -> Transform {
    let [tminx, tminy, tmaxx, tmaxy] = tbox;
    let sx = (rect.width() as f32) / (tmaxx - tminx);
    let sy = (rect.height() as f32) / (tmaxy - tminy);
    let tx = rect.llx as f32 - sx * tminx;
    let ty = rect.lly as f32 - sy * tminy;
    Transform::from_row(sx, 0.0, 0.0, sy, tx, ty)
}

/// Read a `/Matrix` array (Table 95) as a [`Transform`], defaulting to the
/// identity when absent or malformed (Table 95's documented default).
fn read_matrix(doc: &DocumentView<'_>, dict: &Dict) -> Transform {
    let Some(items) = dict
        .get(b"Matrix")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_array)
    else {
        return Transform::identity();
    };
    let n: Vec<f32> = items
        .iter()
        .filter_map(|o| doc.resolve(o).as_number().map(|v| v as f32))
        .collect();
    match n.as_slice() {
        &[a, b, c, d, e, f] => Transform::from_row(a, b, c, d, e, f),
        _ => Transform::identity(),
    }
}

/// Read a four-number rectangle entry (each element possibly indirect,
/// §7.3.10) as `[minx, miny, maxx, maxy]`, normalising corners per §7.9.5.
///
/// Returns `None` when the value is not an array of four resolvable
/// numbers — a malformed `/BBox`, which the caller reports as a placement
/// refusal rather than repairs.
fn read_rect_numbers(doc: &DocumentView<'_>, dict: &Dict, key: &[u8]) -> Option<[f64; 4]> {
    let items = doc.resolve(dict.get(key)?).as_array()?;
    let n: Vec<f64> = items
        .iter()
        .filter_map(|o| doc.resolve(o).as_number())
        .collect();
    match n.as_slice() {
        &[x0, y0, x1, y1] => Some([x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)]),
        _ => None,
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;
    // `Document` is a test-only name here since decision 018 moved the
    // module's own parameter type to `DocumentView`: the fixtures build a
    // real parsed file and then render it through the `&Document`
    // back-compat wrappers, which is exactly the coverage those wrappers
    // need.
    use crate::{AnnotationClass, AnnotationScope, RenderOptions, render_page, render_page_with};
    use pdfcer_core::document::Document;

    /// Assemble a classic-xref PDF from numbered object bodies (raw bytes,
    /// for stream objects). Non-contiguous numbering is tolerated (gaps
    /// become free entries), so annotation fixtures can skip ids.
    fn build_pdf(objects: &[(u32, Vec<u8>)]) -> (Document, Page) {
        let mut buf = b"%PDF-1.7\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, body) in objects {
            offsets.push((*num, buf.len()));
            buf.extend_from_slice(format!("{num} 0 obj\n").as_bytes());
            buf.extend_from_slice(body);
            buf.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = buf.len();
        let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
        let size = max_num + 1;
        buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f\r\n").as_bytes());
        for num in 1..=max_num {
            match offsets.iter().find(|(n, _)| *n == num) {
                Some((_, off)) => {
                    buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
                }
                None => buf.extend_from_slice(b"0000000000 65535 f\r\n"),
            }
        }
        buf.extend_from_slice(
            format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
                .as_bytes(),
        );
        let doc = Document::from_bytes(buf).unwrap();
        let page = pdfcer_core::page_tree::pages(&doc).unwrap().remove(0);
        (doc, page)
    }

    fn stream_object(dict_extra: &str, data: &[u8]) -> Vec<u8> {
        let mut out = format!("<< {dict_extra} /Length {} >>\nstream\n", data.len()).into_bytes();
        out.extend_from_slice(data);
        out.extend_from_slice(b"\nendstream");
        out
    }

    /// A 100×100-MediaBox one-page document carrying the given `/Annots`
    /// array text plus the given extra objects (numbered from 5). The page
    /// is object 3.
    fn doc_with_annots(annots: &str, extra: &[(u32, Vec<u8>)]) -> (Document, Page) {
        let mut objects: Vec<(u32, Vec<u8>)> = vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] \
                  /Resources << >> >>"
                    .to_vec(),
            ),
            (
                3,
                format!("<< /Type /Page /Parent 2 0 R /Annots {annots} >>").into_bytes(),
            ),
        ];
        objects.extend_from_slice(extra);
        build_pdf(&objects)
    }

    /// An appearance form XObject that fills its whole `/BBox` black, with
    /// the given extra dict entries (a `/BBox`, optionally a `/Matrix`).
    fn black_fill_ap(dict_extra: &str, bbox: &str) -> Vec<u8> {
        // Fill a rectangle exactly covering the declared BBox so placement
        // is visible across the whole /Rect.
        let (x0, y0, x1, y1) = parse_bbox(bbox);
        let body = format!("0 0 0 rg {} {} {} {} re f", x0, y0, x1 - x0, y1 - y0);
        stream_object(
            &format!("/Type /XObject /Subtype /Form /BBox {bbox} {dict_extra}"),
            body.as_bytes(),
        )
    }

    fn parse_bbox(bbox: &str) -> (f32, f32, f32, f32) {
        let n: Vec<f32> = bbox
            .trim_matches(|c| c == '[' || c == ']')
            .split_whitespace()
            .map(|t| t.parse().unwrap())
            .collect();
        (n[0], n[1], n[2], n[3])
    }

    use tiny_skia::Pixmap;

    fn pixel(pm: &Pixmap, x: u32, y: u32) -> (u8, u8, u8) {
        let p = pm.pixel(x, y).unwrap();
        (p.red(), p.green(), p.blue())
    }

    fn ink_bbox(pm: &Pixmap) -> Option<(u32, u32, u32, u32)> {
        let mut bbox: Option<(u32, u32, u32, u32)> = None;
        for y in 0..pm.height() {
            for x in 0..pm.width() {
                if pixel(pm, x, y) != (255, 255, 255) {
                    bbox = Some(match bbox {
                        None => (x, y, x, y),
                        Some((x0, y0, x1, y1)) => (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
                    });
                }
            }
        }
        bbox
    }

    // -----------------------------------------------------------------
    // §12.5.5 placement — pinned from both directions (acceptance crit 4)
    // -----------------------------------------------------------------

    #[test]
    fn identity_bbox_maps_one_to_one_into_rect() {
        // /BBox [0 0 20 20], identity /Matrix, /Rect [40 30 60 50]: the
        // black fill lands exactly in that 20×20 rect. Device y-down: user
        // y 30..50 → device y 50..70.
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Stamp /Rect [40 30 60 50] /AP << /N 6 0 R >> >>".to_vec(),
                ),
                (6, black_fill_ap("/Resources << >>", "[0 0 20 20]")),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.annotations_painted, 1);
        // Centre of the rect (user 50,40 → device 50,60): black.
        assert_eq!(pixel(&out.pixmap, 50, 60), (0, 0, 0));
        // Outside the rect: paper white.
        assert_eq!(pixel(&out.pixmap, 10, 10), (255, 255, 255));
        // Ink is confined to the rect: device x 40..60, y 50..70.
        let (x0, y0, x1, y1) = ink_bbox(&out.pixmap).unwrap();
        assert!(x0 >= 39 && x1 <= 61, "x extent {x0}..{x1}");
        assert!(y0 >= 49 && y1 <= 71, "y extent {y0}..{y1}");
    }

    // -----------------------------------------------------------------
    // §8.11.3.3 authored-layer /OC visibility (Pass 12.M2)
    // -----------------------------------------------------------------

    /// A one-page doc whose catalog carries `/OCProperties` and whose only
    /// annotation sits on OCG object 10, with the given `/D` config body.
    fn doc_with_oc_annot(d_config: &str) -> (Document, Page) {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (
                1,
                format!(
                    "<< /Type /Catalog /Pages 2 0 R /OCProperties \
                     << /OCGs [10 0 R] /D << {d_config} >> >> >>"
                )
                .into_bytes(),
            ),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] \
                  /Resources << >> >>"
                    .to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /Annots [5 0 R] >>".to_vec(),
            ),
            (
                5,
                b"<< /Subtype /Stamp /Rect [40 30 60 50] /OC 10 0 R /AP << /N 6 0 R >> >>".to_vec(),
            ),
            (6, black_fill_ap("/Resources << >>", "[0 0 20 20]")),
            (10, b"<< /Type /OCG /Name (Dimensions) >>".to_vec()),
        ];
        build_pdf(&objects)
    }

    #[test]
    fn an_annotation_on_an_off_layer_is_not_painted() {
        // The OCG is registered and placed in /D /OFF ⇒ hidden by default.
        let (doc, page) = doc_with_oc_annot("/Order [10 0 R] /OFF [10 0 R]");
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(
            out.diagnostics.annotations_painted, 0,
            "an /OC annotation on an OFF layer must not paint"
        );
        assert_eq!(out.diagnostics.annotations_hidden, 1);
        // No ink at all: the layer is hidden.
        assert!(ink_bbox(&out.pixmap).is_none(), "the page must be blank");
    }

    #[test]
    fn an_annotation_on_an_on_layer_is_painted() {
        // Same OCG, but NOT in /OFF ⇒ ON by default (BaseState default ON).
        let (doc, page) = doc_with_oc_annot("/Order [10 0 R]");
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(
            out.diagnostics.annotations_painted, 1,
            "an /OC annotation on an ON layer paints normally"
        );
        assert_eq!(pixel(&out.pixmap, 50, 60), (0, 0, 0));
    }

    #[test]
    fn non_origin_bbox_is_translated_to_rect() {
        // /BBox [100 100 120 120] (far from origin) must still fill the
        // /Rect exactly — step b translates the transformed box onto Rect.
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Stamp /Rect [0 0 40 40] /AP << /N 6 0 R >> >>".to_vec(),
                ),
                (6, black_fill_ap("/Resources << >>", "[100 100 120 120]")),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.annotations_painted, 1);
        // Rect [0 0 40 40] → device y 60..100. Centre user (20,20) →
        // device (20,80): black.
        assert_eq!(pixel(&out.pixmap, 20, 80), (0, 0, 0));
        let (x0, y0, x1, y1) = ink_bbox(&out.pixmap).unwrap();
        assert!(x0 <= 1 && (39..=41).contains(&x1), "x extent {x0}..{x1}");
        assert!(y0 >= 59 && y1 >= 99, "y extent {y0}..{y1}");
    }

    #[test]
    fn bbox_larger_than_rect_scales_down() {
        // /BBox [0 0 80 80] into /Rect [10 10 30 30] (20×20): scaled DOWN
        // to fit. Ink confined to the small rect.
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Stamp /Rect [10 10 30 30] /AP << /N 6 0 R >> >>".to_vec(),
                ),
                (6, black_fill_ap("/Resources << >>", "[0 0 80 80]")),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        let (x0, y0, x1, y1) = ink_bbox(&out.pixmap).unwrap();
        // Rect x 10..30, device y 70..90.
        assert!(x0 >= 9 && x1 <= 31, "x {x0}..{x1} not confined to Rect");
        assert!(y0 >= 69 && y1 <= 91, "y {y0}..{y1} not confined to Rect");
    }

    #[test]
    fn bbox_smaller_than_rect_scales_up() {
        // /BBox [0 0 5 5] into /Rect [0 0 100 100]: scaled UP to fill the
        // whole page.
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Stamp /Rect [0 0 100 100] /AP << /N 6 0 R >> >>".to_vec(),
                ),
                (6, black_fill_ap("/Resources << >>", "[0 0 5 5]")),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        let (x0, y0, x1, y1) = ink_bbox(&out.pixmap).unwrap();
        assert!(
            x0 <= 1 && y0 <= 1 && x1 >= 98 && y1 >= 98,
            "should fill page: {x0},{y0}..{x1},{y1}"
        );
    }

    #[test]
    fn scaling_matrix_grows_the_transformed_box() {
        // /BBox [0 0 10 10] with /Matrix [2 0 0 2 0 0] → transformed box
        // 20×20; then fit to /Rect. The whole thing still fills /Rect (the
        // fit absorbs the Matrix scale), which proves Matrix is applied
        // once (not twice) — a double-apply would misplace/clip the fill.
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Stamp /Rect [0 0 40 40] /AP << /N 6 0 R >> >>".to_vec(),
                ),
                (
                    6,
                    black_fill_ap("/Matrix [2 0 0 2 0 0] /Resources << >>", "[0 0 10 10]"),
                ),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.annotations_painted, 1);
        // Centre of Rect user (20,20) → device (20,80): black.
        assert_eq!(pixel(&out.pixmap, 20, 80), (0, 0, 0));
        let (x0, y0, x1, y1) = ink_bbox(&out.pixmap).unwrap();
        assert!(
            x0 <= 1 && x1 >= 39 && y0 >= 59,
            "Matrix double-applied? {x0},{y0}..{x1},{y1}"
        );
    }

    #[test]
    fn rotating_matrix_places_within_rect() {
        // A 90° /Matrix rotates /BBox; step a takes the axis-aligned
        // bounds, step b fits them to /Rect. The fill must stay inside
        // /Rect (no spill), which is the placement property under rotation.
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Stamp /Rect [20 20 60 60] /AP << /N 6 0 R >> >>".to_vec(),
                ),
                (
                    6,
                    // Matrix [0 1 -1 0 20 0] rotates 90° and translates so the
                    // box stays in positive space; fill covers the BBox.
                    black_fill_ap("/Matrix [0 1 -1 0 20 0] /Resources << >>", "[0 0 20 20]"),
                ),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.annotations_painted, 1);
        let (x0, y0, x1, y1) = ink_bbox(&out.pixmap).unwrap();
        // Rect [20 20 60 60] → device x 20..60, y 40..80. Ink stays inside.
        assert!(
            x0 >= 19 && x1 <= 61 && y0 >= 39 && y1 <= 81,
            "spilled: {x0},{y0}..{x1},{y1}"
        );
    }

    #[test]
    fn inverted_rect_corners_are_normalized() {
        // /Rect [60 50 40 30] (corners reversed, §7.9.5) is the same target
        // box as [40 30 60 50]: identical placement, no divide-by-negative.
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Stamp /Rect [60 50 40 30] /AP << /N 6 0 R >> >>".to_vec(),
                ),
                (6, black_fill_ap("/Resources << >>", "[0 0 20 20]")),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.annotations_painted, 1);
        assert_eq!(pixel(&out.pixmap, 50, 60), (0, 0, 0), "normalized Rect");
    }

    #[test]
    fn degenerate_bbox_is_named_not_placed() {
        // /BBox [10 10 10 90] has zero width ⇒ transformed box degenerate ⇒
        // step-b matrix singular. Paint NOTHING, count + name — never a
        // divide-by-zero (risk X2).
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Stamp /Rect [0 0 40 40] /AP << /N 6 0 R >> >>".to_vec(),
                ),
                (
                    6,
                    stream_object(
                        "/Type /XObject /Subtype /Form /BBox [10 10 10 90] /Resources << >>",
                        b"0 0 0 rg 0 0 100 100 re f",
                    ),
                ),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(ink_bbox(&out.pixmap), None, "degenerate box painted");
        assert_eq!(out.diagnostics.annotations_painted, 0);
        assert_eq!(out.diagnostics.annotations_placement_degenerate, 1);
        assert!(
            out.diagnostics
                .annotation_notes
                .iter()
                .any(|s| s.contains("degenerate")),
            "must name the degenerate refusal: {:?}",
            out.diagnostics.annotation_notes
        );
    }

    // -----------------------------------------------------------------
    // Suppression + non-goals (acceptance criteria 5, 6)
    // -----------------------------------------------------------------

    #[test]
    fn hidden_annotation_is_not_painted_but_counted() {
        // /F 2 = Hidden. A fill that would cover the whole page must NOT
        // appear, and the suppression is counted (R50).
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Stamp /Rect [0 0 100 100] /F 2 /AP << /N 6 0 R >> >>".to_vec(),
                ),
                (6, black_fill_ap("/Resources << >>", "[0 0 100 100]")),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(ink_bbox(&out.pixmap), None, "Hidden annotation painted");
        assert_eq!(out.diagnostics.annotations_hidden, 1);
        assert_eq!(out.diagnostics.annotations_painted, 0);
        assert_eq!(out.diagnostics.annotations_total, 1);
    }

    #[test]
    fn noview_annotation_is_not_painted_on_screen_but_counted() {
        // /F 32 = NoView: screen-suppressed (this is the screen path).
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Stamp /Rect [0 0 100 100] /F 32 /AP << /N 6 0 R >> >>".to_vec(),
                ),
                (6, black_fill_ap("/Resources << >>", "[0 0 100 100]")),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(
            ink_bbox(&out.pixmap),
            None,
            "NoView annotation painted on screen"
        );
        assert_eq!(out.diagnostics.annotations_hidden, 1);
    }

    #[test]
    fn popup_is_never_painted_as_page_content() {
        // Even with a (malformed) /AP, a /Popup must never paint (X4).
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Popup /Rect [0 0 100 100] /Open true /AP << /N 6 0 R >> >>"
                        .to_vec(),
                ),
                (6, black_fill_ap("/Resources << >>", "[0 0 100 100]")),
            ],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(
            ink_bbox(&out.pixmap),
            None,
            "/Popup painted as page content"
        );
        assert_eq!(out.diagnostics.annotations_painted, 0);
        assert_eq!(out.diagnostics.annotations_total, 1);
    }

    #[test]
    fn no_ap_annotation_is_counted_by_subtype() {
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[(
                5,
                b"<< /Subtype /Circle /Rect [0 0 40 40] /IC [1 0 0] >>".to_vec(),
            )],
        );
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(
            ink_bbox(&out.pixmap),
            None,
            "R43: nothing synthesised from /IC"
        );
        assert_eq!(
            out.diagnostics.annotations_without_ap.get("Circle"),
            Some(&1)
        );
    }

    // -----------------------------------------------------------------
    // X8 — appearance resource scoping (the named-once correctness bug)
    // -----------------------------------------------------------------

    #[test]
    fn appearance_uses_its_own_resources_not_the_page_font() {
        // Page /Resources and the appearance both define /F1, but as
        // DIFFERENT fonts. The appearance text must resolve /F1 against the
        // APPEARANCE's own /Resources (X8), which run_form_at inherits from
        // do_form. We prove it via the substitution diagnostic: the
        // appearance names a font the page does not, so the substituted
        // set must include the appearance's font, not (only) the page's.
        //
        // Page /F1 = Helvetica; appearance /F1 = Times-Roman. If the wrong
        // resources were used, the appearance's text would resolve to
        // Helvetica.
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] \
                  /Resources << /Font << /F1 8 0 R >> >> >>"
                    .to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                  /Resources << /Font << /F1 8 0 R >> >> /Annots [5 0 R] >>"
                    .to_vec(),
            ),
            // Page content draws nothing text-wise (keep the page's own
            // render clean so the appearance's font is what we measure).
            (4, stream_object("", b"")),
            (
                5,
                b"<< /Subtype /Stamp /Rect [0 0 100 100] /AP << /N 6 0 R >> >>".to_vec(),
            ),
            (
                6,
                stream_object(
                    "/Type /XObject /Subtype /Form /BBox [0 0 100 100] \
                     /Resources << /Font << /F1 7 0 R >> >>",
                    b"BT /F1 20 Tf 5 40 Td (T) Tj ET",
                ),
            ),
            // Appearance /F1 = Times-Roman.
            (
                7,
                b"<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman >>".to_vec(),
            ),
            // Page /F1 = Helvetica.
            (
                8,
                b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec(),
            ),
        ];
        let (doc, page) = build_pdf(&objects);
        let out = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(out.diagnostics.annotations_painted, 1);
        // The appearance's glyph painted from ITS /F1 (Times-Roman), which
        // is the X8 correctness proof: the page's /F1 is Helvetica.
        assert!(
            out.diagnostics
                .substituted_fonts
                .iter()
                .any(|f| f == "Times-Roman"),
            "appearance resolved /F1 against the wrong resources: {:?}",
            out.diagnostics.substituted_fonts
        );
    }

    // -----------------------------------------------------------------
    // The suppression flag (acceptance: pre-6.0 raster reproducible)
    // -----------------------------------------------------------------

    #[test]
    fn no_annotations_option_reproduces_content_only_raster() {
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Stamp /Rect [0 0 100 100] /AP << /N 6 0 R >> >>".to_vec(),
                ),
                (6, black_fill_ap("/Resources << >>", "[0 0 100 100]")),
            ],
        );
        let opts = RenderOptions::default().with_annotations(false);
        let out = render_page_with(&doc, &page, 1.0, &opts).unwrap();
        // Nothing painted (the appearance is suppressed), and — crucially —
        // the annotation counters are STILL recorded so a suppressed
        // render discloses how many annotations exist (R50/R27).
        assert_eq!(ink_bbox(&out.pixmap), None);
        assert_eq!(out.diagnostics.annotations_painted, 0);
        assert_eq!(
            out.diagnostics.annotations_total, 1,
            "suppressed but disclosed"
        );
    }
    // -----------------------------------------------------------------
    // The four-way annotation scope (Acrobat's Comments & Forms selector)
    //
    // Fixture shape, shared by every scope test below: ONE page carrying
    // four annotations whose /Rects do not overlap, each with a black
    // appearance filling its own /Rect, plus page content of its own. The
    // rects are chosen so a single `ink_bbox` cannot distinguish them —
    // each is probed by a pixel at its own centre, and the presence and
    // ABSENCE of every one is asserted in every scope. Asserting only what
    // paints would pass a scope that painted everything.
    // -----------------------------------------------------------------

    /// Device-space centre of each fixture annotation's `/Rect`, for the
    /// 100×100 page at scale 1.0 (user y → device 100 − y).
    const HIGHLIGHT_PROBE: (u32, u32) = (15, 85); // /Rect [10 5 20 15]
    const STAMP_PROBE: (u32, u32) = (35, 85); // /Rect [30 5 40 15]
    const WIDGET_PROBE: (u32, u32) = (55, 85); // /Rect [50 5 60 15]
    const POPUP_PROBE: (u32, u32) = (75, 85); // /Rect [70 5 80 15]
    /// A pixel inside the page's own content rectangle (user [5 50 95 90]
    /// → device y 10..50), far from every annotation.
    const CONTENT_PROBE: (u32, u32) = (50, 30);

    /// A page with its own content plus one markup (`/Highlight`), one
    /// `/Stamp`, one `/Widget` and one `/Popup` — the four classes the
    /// scope has to tell apart. Every annotation carries a usable `/AP`,
    /// including the `/Popup` (which must still never paint).
    fn doc_with_one_of_each_class() -> (Document, Page) {
        let objects: Vec<(u32, Vec<u8>)> = vec![
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] \
                  /Resources << >> >>"
                    .to_vec(),
            ),
            (
                3,
                b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R \
                  /Annots [5 0 R 6 0 R 7 0 R 8 0 R] >>"
                    .to_vec(),
            ),
            // Page content: a black band across the upper half of the page,
            // well clear of every annotation /Rect.
            (4, stream_object("", b"0 0 0 rg 5 50 90 40 re f")),
            (
                5,
                b"<< /Subtype /Highlight /Rect [10 5 20 15] /AP << /N 9 0 R >> >>".to_vec(),
            ),
            (
                6,
                b"<< /Subtype /Stamp /Rect [30 5 40 15] /AP << /N 9 0 R >> >>".to_vec(),
            ),
            (
                7,
                b"<< /Subtype /Widget /FT /Btn /Rect [50 5 60 15] /AP << /N 9 0 R >> >>".to_vec(),
            ),
            (
                8,
                b"<< /Subtype /Popup /Rect [70 5 80 15] /Open true /AP << /N 9 0 R >> >>".to_vec(),
            ),
            // One shared appearance: a 10×10 black fill, stretched into
            // whichever /Rect refers to it (§12.5.5 step b).
            (9, black_fill_ap("/Resources << >>", "[0 0 10 10]")),
        ];
        build_pdf(&objects)
    }

    /// Render the four-class fixture under `scope` and report, in order,
    /// whether the markup, stamp, widget, popup and page content painted.
    fn painted_under(scope: AnnotationScope) -> ([bool; 4], bool, Diagnostics) {
        let (doc, page) = doc_with_one_of_each_class();
        let options = RenderOptions::default().with_annotation_scope(scope);
        let out = render_page_with(&doc, &page, 1.0, &options).unwrap();
        let black = |(x, y): (u32, u32)| pixel(&out.pixmap, x, y) == (0, 0, 0);
        (
            [
                black(HIGHLIGHT_PROBE),
                black(STAMP_PROBE),
                black(WIDGET_PROBE),
                black(POPUP_PROBE),
            ],
            black(CONTENT_PROBE),
            out.diagnostics,
        )
    }

    /// **A narrowed scope must not become "paint everything anyway".**
    ///
    /// The default is the pre-existing behaviour — every class paints —
    /// and it is pinned first so the other four tests are read as
    /// deviations from a known-good baseline rather than as isolated
    /// assertions. A regression that ignored the scope field entirely
    /// would pass this test and fail every other one, which is exactly the
    /// diagnosis this ordering is meant to give.
    #[test]
    fn document_and_markups_paints_every_class_and_the_page() {
        let (painted, content, diag) = painted_under(AnnotationScope::DocumentAndMarkups);
        assert_eq!(
            painted,
            [true, true, true, false],
            "markup/stamp/widget must paint, /Popup must not"
        );
        assert!(content, "page content must paint");
        assert_eq!(diag.annotations_total, 4);
        assert_eq!(diag.annotations_painted, 3, "the /Popup is never painted");
        assert_eq!(
            diag.annotations_out_of_scope, 0,
            "the default scope withholds nothing"
        );
        assert!(!diag.page_content_suppressed);
    }

    /// **"Document" must exclude markups without also excluding widgets.**
    ///
    /// The defect this catches is a scope implemented as a single
    /// "annotations off" bool: it would blank the widget too, and the page
    /// would lose its form-field appearances — which is the *other* half of
    /// Acrobat's "Document", not an acceptable approximation of it.
    #[test]
    fn document_scope_paints_form_fields_but_no_markup_or_stamp() {
        let (painted, content, diag) = painted_under(AnnotationScope::Document);
        assert_eq!(
            painted,
            [false, false, true, false],
            "only the /Widget may paint under Document"
        );
        assert!(content, "page content must still paint");
        assert_eq!(diag.annotations_painted, 1);
        // The /Highlight and the /Stamp; the /Popup is refused structurally
        // before the scope is consulted, so it is NOT counted here.
        assert_eq!(
            diag.annotations_out_of_scope, 2,
            "both markups must be disclosed as withheld"
        );
        assert_eq!(
            diag.annotations_total, 4,
            "a narrowed scope still discloses the full census"
        );
    }

    /// **"Document and Stamps" is NARROWER than "Document and Markups" —
    /// it must not admit non-stamp markups.**
    ///
    /// Named directly in the Acrobat source as the trap: an implementation
    /// that collapsed the two options would over-include every sticky
    /// note, ink stroke and highlight, and would do it invisibly, because
    /// the result still looks like "a page with annotations on it". The
    /// `/Highlight` probe is the whole test.
    #[test]
    fn document_and_stamps_admits_the_stamp_and_no_other_markup() {
        let (painted, content, diag) = painted_under(AnnotationScope::DocumentAndStamps);
        assert_eq!(
            painted,
            [false, true, true, false],
            "the /Stamp and /Widget paint; the /Highlight must not"
        );
        assert!(content);
        assert_eq!(diag.annotations_painted, 2);
        assert_eq!(
            diag.annotations_out_of_scope, 1,
            "only the non-stamp markup is withheld"
        );
    }

    /// **"Form fields only" must suppress the PAGE, not just the other
    /// annotations.**
    ///
    /// The sourced workflow is printing onto a pre-printed paper form, so
    /// a version of this scope that painted the page content would
    /// double-print the form's own rules over the physical paper — the
    /// feature would be worse than useless while looking implemented. The
    /// content probe is what makes this test different from "Document with
    /// markups off".
    #[test]
    fn form_fields_only_paints_widgets_and_suppresses_page_content() {
        let (painted, content, diag) = painted_under(AnnotationScope::FormFieldsOnly);
        assert_eq!(
            painted,
            [false, false, true, false],
            "only the /Widget may paint"
        );
        assert!(!content, "page content must NOT paint under FormFieldsOnly");
        assert_eq!(diag.annotations_painted, 1);
        assert_eq!(diag.annotations_out_of_scope, 2);
        assert!(
            diag.page_content_suppressed,
            "the suppression must be disclosed, not silent"
        );
        assert_eq!(
            diag.contents_streams_unresolved, 0,
            "pdfcer did not read the content streams, so it reports nothing about them"
        );
    }

    /// **`ContentOnly` must keep meaning exactly what
    /// `with_annotations(false)` has always meant.**
    ///
    /// Two things are pinned. The raster: page content paints, no
    /// annotation does — the pre-Pass-6.0 baseline the round-trip oracle
    /// compares against. And the census: `annotations_total` still counts,
    /// because a suppressed render that could not say what it was hiding
    /// would be sneaky (R27/R50).
    #[test]
    fn content_only_paints_the_page_and_no_annotation_at_all() {
        let (painted, content, diag) = painted_under(AnnotationScope::ContentOnly);
        assert_eq!(painted, [false; 4], "no annotation class may paint");
        assert!(content, "page content must still paint");
        assert_eq!(diag.annotations_painted, 0);
        assert_eq!(diag.annotations_total, 4);
        assert!(!diag.page_content_suppressed);
    }

    /// **The legacy `bool` and the new scope must not disagree — the bool
    /// can only ever subtract.**
    ///
    /// `with_annotations(false)` is a live caller contract (`pdfcer`'s
    /// `--no-annotations`, the GUI's visibility toggle) and it predates the
    /// scope. If the two fields were read independently anywhere, a caller
    /// that set both would get one honoured and one dropped — silently,
    /// because a dropped setting looks exactly like a setting nobody
    /// changed. This asserts the master gate wins even against the widest
    /// scope, and that it does NOT drag the page content down with it.
    #[test]
    fn the_annotations_bool_overrides_any_scope_but_never_the_page() {
        let (doc, page) = doc_with_one_of_each_class();
        let options = RenderOptions::default()
            .with_annotation_scope(AnnotationScope::DocumentAndMarkups)
            .with_annotations(false);
        assert_eq!(
            options.effective_annotation_scope(),
            AnnotationScope::ContentOnly
        );
        let out = render_page_with(&doc, &page, 1.0, &options).unwrap();
        assert_eq!(out.diagnostics.annotations_painted, 0);
        assert_eq!(
            pixel(&out.pixmap, CONTENT_PROBE.0, CONTENT_PROBE.1),
            (0, 0, 0),
            "the annotation gate must never suppress page content"
        );

        // And the reverse composition: the gate cannot RESURRECT a class
        // the scope excludes, which is the other way two knobs could
        // contradict each other.
        let options = RenderOptions::default()
            .with_annotations(true)
            .with_annotation_scope(AnnotationScope::FormFieldsOnly);
        let out = render_page_with(&doc, &page, 1.0, &options).unwrap();
        assert_eq!(out.diagnostics.annotations_painted, 1);
    }

    /// **A `/Popup` must stay unpaintable under every scope, including the
    /// one that names form fields.**
    ///
    /// §12.5.6.14 makes this structural: a pop-up is a reader window, never
    /// page content, and no caller preference may promote it. The risk is
    /// ordering — a scope check placed ahead of the pop-up refusal could
    /// let a `/Popup` through some future permissive scope. This walks all
    /// five scopes so the guarantee is not pinned only where it was last
    /// convenient to test.
    #[test]
    fn popup_never_paints_under_any_scope() {
        for scope in [
            AnnotationScope::ContentOnly,
            AnnotationScope::Document,
            AnnotationScope::DocumentAndMarkups,
            AnnotationScope::DocumentAndStamps,
            AnnotationScope::FormFieldsOnly,
        ] {
            let (painted, _, diag) = painted_under(scope);
            assert!(
                !painted[3],
                "/Popup painted as page content under {scope:?}"
            );
            // Nor is it ever counted as merely "out of scope" — it is
            // refused before the scope is consulted at all.
            assert!(
                diag.annotations_out_of_scope <= 3,
                "the /Popup must not be counted as a scope exclusion under {scope:?}"
            );
        }
    }

    /// **The §12.5.3 flag gate and the scope gate compose as AND, and both
    /// are disclosed.**
    ///
    /// The Acrobat source is explicit that the two mechanisms compose as
    /// AND rather than OR. The subtler half is the *reporting*: folding an
    /// out-of-scope annotation into `annotations_hidden` would tell an
    /// operator "two annotations are not shown" while destroying the only
    /// fact that says which of them they can bring back by changing an
    /// option.
    #[test]
    fn scope_and_flags_are_counted_independently() {
        // A Hidden markup: excluded by the Document scope AND by /F 2.
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[
                (
                    5,
                    b"<< /Subtype /Highlight /Rect [0 0 100 100] /F 2 /AP << /N 6 0 R >> >>"
                        .to_vec(),
                ),
                (6, black_fill_ap("/Resources << >>", "[0 0 100 100]")),
            ],
        );
        let options = RenderOptions::default().with_annotation_scope(AnnotationScope::Document);
        let out = render_page_with(&doc, &page, 1.0, &options).unwrap();
        assert_eq!(ink_bbox(&out.pixmap), None);
        assert_eq!(
            out.diagnostics.annotations_out_of_scope, 1,
            "the scope exclusion is a fact about the caller's request"
        );
        assert_eq!(
            out.diagnostics.annotations_hidden, 1,
            "the flag suppression is a fact about the document"
        );
    }

    /// **A narrowed scope must not empty the R43 named-not-painted
    /// census.**
    ///
    /// The scope gate deliberately does not `continue`. If it did, an
    /// out-of-scope annotation with no `/AP` would stop being counted in
    /// `annotations_without_ap` — and that map is the measured demand
    /// signal driving which appearance-generation Pass gets built first.
    /// Losing it would be invisible: the numbers would simply be smaller.
    #[test]
    fn an_out_of_scope_annotation_is_still_counted_by_subtype() {
        let (doc, page) = doc_with_annots(
            "[5 0 R]",
            &[(
                5,
                b"<< /Subtype /Circle /Rect [0 0 40 40] /IC [1 0 0] >>".to_vec(),
            )],
        );
        let options = RenderOptions::default().with_annotation_scope(AnnotationScope::Document);
        let out = render_page_with(&doc, &page, 1.0, &options).unwrap();
        assert_eq!(
            out.diagnostics.annotations_without_ap.get("Circle"),
            Some(&1),
            "the R43 census must survive a narrowed scope"
        );
        assert_eq!(out.diagnostics.annotations_out_of_scope, 1);
    }

    // -----------------------------------------------------------------
    // The Table 169 partition itself (ISO 32000-1 §12.5.6.1)
    // -----------------------------------------------------------------

    /// **The markup partition must match Table 169's column, not
    /// §12.5.6.2's prose.**
    ///
    /// The prose names only five non-markup subtypes; the table marks nine
    /// (erratum T169-E1). The three the prose omits — `/Screen`,
    /// `/Watermark`, `/3D` — are the whole point of this test: an
    /// implementation derived from the sentence would classify them as
    /// markup, and a "Document" print would then silently drop a page's
    /// watermark. All 26 standard subtypes are named so a future edit
    /// cannot quietly move one across the line.
    #[test]
    fn table_169_markup_partition_is_transcribed_exactly() {
        // Table 169 `Markup` = Yes (17), minus /Stamp which has its own
        // class because "Document and Stamps" names it alone.
        for subtype in [
            &b"Text"[..],
            b"FreeText",
            b"Line",
            b"Square",
            b"Circle",
            b"Polygon",
            b"PolyLine",
            b"Highlight",
            b"Underline",
            b"Squiggly",
            b"StrikeOut",
            b"Caret",
            b"Ink",
            b"FileAttachment",
            b"Sound",
            b"Redact",
        ] {
            assert_eq!(
                AnnotationClass::of_subtype(subtype),
                AnnotationClass::Markup,
                "/{} is Table 169 Markup=Yes",
                String::from_utf8_lossy(subtype)
            );
        }
        assert_eq!(
            AnnotationClass::of_subtype(b"Stamp"),
            AnnotationClass::Stamp
        );
        assert_eq!(
            AnnotationClass::of_subtype(b"Widget"),
            AnnotationClass::FormField
        );
        // Table 169 `Markup` = No, minus /Widget (8 of the 9). /Screen,
        // /Watermark and /3D are the three §12.5.6.2's prose forgets.
        for subtype in [
            &b"Link"[..],
            b"Popup",
            b"Movie",
            b"Screen",
            b"PrinterMark",
            b"TrapNet",
            b"Watermark",
            b"3D",
        ] {
            assert_eq!(
                AnnotationClass::of_subtype(subtype),
                AnnotationClass::Other,
                "/{} is Table 169 Markup=No",
                String::from_utf8_lossy(subtype)
            );
        }
    }

    /// **An unrecognised or mis-cased `/Subtype` must not be guessed into
    /// the markup bucket.**
    ///
    /// Name objects are case-sensitive (§7.3.5), so `/stamp` is not a
    /// stamp, and a private extension subtype is not a markup annotation
    /// merely because pdfcer has not heard of it. Classifying an unknown
    /// name as markup would make it vanish from a "Document" render — the
    /// direction that loses content — and would do so on the strength of a
    /// guess.
    #[test]
    fn an_unknown_or_miscased_subtype_classifies_as_other() {
        assert_eq!(
            AnnotationClass::of_subtype(b"stamp"),
            AnnotationClass::Other
        );
        assert_eq!(
            AnnotationClass::of_subtype(b"HIGHLIGHT"),
            AnnotationClass::Other
        );
        assert_eq!(AnnotationClass::of_subtype(b""), AnnotationClass::Other);
        assert_eq!(
            AnnotationClass::of_subtype(b"AcmeSquiggle"),
            AnnotationClass::Other
        );
    }

    /// **The override reaches ANNOTATIONS too, not only page content.**
    ///
    /// pdfcer's own authored dimensions live on annotation `/OC`
    /// (§8.11.3.3), and an operator who hides that layer means the
    /// dimensions. Two code paths read layer state — the interpreter and
    /// the annotation walk — and a toggle that reached only one of them
    /// would look like it half-worked.
    #[test]
    fn an_override_reaches_annotation_oc_as_well_as_page_content() {
        let (doc, page) = doc_with_oc_annot("/Order [10 0 R]");
        let shown = render_page(&doc, &page, 1.0).unwrap();
        assert_eq!(shown.diagnostics.annotations_painted, 1);

        let options = RenderOptions::default()
            .with_layers(crate::LayerVisibility::hiding([ObjId::new(10, 0)]));
        let hidden = render_page_with(&doc, &page, 1.0, &options).unwrap();
        assert_eq!(
            hidden.diagnostics.annotations_painted, 0,
            "hiding a layer must hide the annotations on it"
        );
        assert_eq!(hidden.diagnostics.annotations_hidden, 1);
    }
}
