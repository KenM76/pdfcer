//! The OCR "sandwich" writer — turning recognised words into an **invisible,
//! selectable text layer** over page content that is left completely untouched.
//!
//! # What this module is
//!
//! [`super`] defines the engine-independent *types* — [`RecognizedWord`],
//! [`OcrPage`], the `OcrEngine` trait, and the y-flip.
//! **This module is what those types were for**: it takes an [`OcrPage`] whose
//! words are already in PDF user space and writes them into a document.
//!
//! # The mechanism, and its one spec citation
//!
//! ISO 32000-1 **§9.3.6, Table 106, mode 3**: *"Neither fill nor stroke text
//! (invisible)."* The PDF-spec corpus names this by name as the mechanism for
//! OCR text layers (`iso32000__s__9.3.md`), and notes the converse obligation
//! on the renderer — a rasteriser that does **not** honour mode 3 draws the
//! layer as visible garbage across the scan.
//!
//! So the emitted stream is, per page:
//!
//! ```text
//! q                    <- isolate: Tf/Tr/Tz are GRAPHICS state, so Q restores them
//!   3 Tr 0 Tc 0 Tw 0 Ts  <- invisible; spacing zeroed (Tw acts on the space glyph)
//!   BT                 <- one text object per block
//!     /OCR0 <size> Tf  <- per line: spans every word's fitted glyph box
//!     <tz> Tz          <- per line: summed box widths / summed natural widths
//!     1 0 0 1 <x> <y> Tm
//!     [(The ) -12 (quick ) 3 (b) -1 (r) ...] TJ  <- one show per line
//!     ...              <- repeated per line
//!   ET
//! Q
//! ```
//!
//! One show operator per line makes the text model read a recognised line as
//! one run, so a shell selects and edits lines and paragraphs, not words
//! (G201). Words are joined by the font's space glyph; the `TJ` number before
//! each word lands its start on its box, and small numbers between a word's
//! letters land its end there too, bounded well inside the extractor's word-
//! and line-break thresholds so no word splits. `Tm` is absolute, so line
//! order in the stream affects extraction order and nothing else.
//!
//! ## Why `q … Q`, and why it is not optional
//!
//! §8.4.2 requires `q`/`Q` to balance within a content stream, and a
//! `/Contents` **array** is defined as the concatenation of its members — so
//! an appended stream inherits whatever graphics state the preceding streams
//! left set. `Tf`, `Tr` and `Tz` are graphics-state parameters (only `Tm` and
//! `Tlm` are reset by `BT`), which means an OCR layer that set `3 Tr` without
//! wrapping would leave **every subsequent stream's text invisible**. The
//! wrapper is the same convention [`crate::text_edit::add_text`] already
//! documents at its §8.4.2 note; this module follows it deliberately rather
//! than by imitation.
//!
//! # Why the scan itself is never re-encoded
//!
//! An OCR layer is purely **additive**: one new content stream appended to
//! `/Contents`, one new font dict, one rewritten page dict. The image object is
//! not in the dirty set, so under an incremental save it is not re-emitted at
//! all (project rule 3). The second reason is the one that matters: **a scan is
//! usually the record of something** — a signed contract, a survey, a stamped
//! drawing — and pushing its JPEG through a decode/re-encode cycle to "help"
//! costs generation loss on an image whose provenance the operator may need to
//! defend. OCR makes a document findable. It does not get to modify it.
//!
//! # Geometry: how a bounding box becomes a font size and a baseline
//!
//! An engine reports an ink bounding box. A PDF viewer computes a selection
//! highlight from the font's ascent/descent times the size, positioned at the
//! baseline. So to make selection land on the ink, the glyph box has to be
//! fitted to the reported box in both axes — and the two axes are fitted by
//! **different** mechanisms, which is the part worth stating plainly:
//!
//! | axis | fitted by | why |
//! |---|---|---|
//! | vertical | the **font size** (`HELVETICA_ASCENT_FRAC` + `HELVETICA_DESCENT_FRAC`) | size is the only vertical control; there is no vertical-scaling operator short of a full `Tm` |
//! | horizontal | **`Tz`** (horizontal scaling, §9.3.4) | the size is already spent on the vertical fit, so width must come from somewhere else |
//!
//! Vertical: `size = height / (HELVETICA_ASCENT_FRAC + HELVETICA_DESCENT_FRAC)` and the baseline
//! sits at `lly + HELVETICA_DESCENT_FRAC × size`, so the glyph box's top lands at
//! `lly + height` — the reported box top — by construction.
//!
//! "Vertical" and "horizontal" are the DISPLAYED axes: on a page with
//! `/Rotate` the `Tm` turns with the page, so on a quarter turn the size
//! fits the box's user-space width and `Tz` its height
//! ([`build_layer_content_on`](crate::ocr::layer::build_layer_content_on)).
//!
//! Horizontal: the word's natural width at that size is measured through the
//! same Standard-14 metric tables the rest of the crate uses, and
//! `Tz = 100 × target ÷ natural`. `Tz` is a **percentage** and `Th` is the
//! ratio (§9.3.4) — `100 Tz` means `Th = 1.0`, and the corpus flags treating
//! the operand as the ratio as a 100× error, so the percentage is emitted here
//! and the conversion is left where the spec puts it.
//!
//! **This is an approximation and is documented as one.** Helvetica's metrics
//! are not the scanned face's metrics; the fit is exact at the word box's
//! edges and drifts within it. That is the accepted trade in every sandwich
//! implementation, because the alternative — per-glyph positioning derived
//! from per-glyph boxes — needs data most engines do not report.
//!
//! # Rule 4 (decision 059): every word here is a guess
//!
//! - **The page looks normal the instant the command completes**: mode 3
//!   adds nothing visible, and low-confidence words are never marked on the
//!   page (a second rendering path for the same content).
//! - **The disclosure is [`OcrLayerReport`](crate::ocr::layer::OcrLayerReport), off-canvas**: mean confidence,
//!   words needing review, substituted and skipped words. A shell shows it;
//!   the CLI prints it.
//! - **`confidence_available == false` is its own disclosed fact**, never
//!   flattened into "no low-confidence words".
//!
//! # Where this differs from `add_text`
//!
//! [`crate::text_edit::add_text`] refuses (`R71`) a character its face cannot
//! encode, because the operator typed that string. OCR output is bulk machine
//! text nobody typed: refusing a page's layer over one non-WinAnsi word would
//! fail exactly the documents that need it, so the word is substituted and
//! counted (`OcrLayerReport::words_substituted`) instead.
//!
//! **The limit this leaves is real and is named**: a Standard-14 WinAnsi face
//! cannot represent CJK, Cyrillic, Greek or Arabic at all. Recognising those
//! scripts needs an embedded composite font, which is its own slice — see
//! `OcrLayerReport::words_substituted` for how a caller detects that it has
//! landed in that case rather than discovering it from a page of `?`.

use super::marker::{LayerStrip, contents_without, page_ocr_layers, plan_strip};
use crate::crypto::PermissionBit;
use crate::document::Document;
use crate::fontdata::Std14;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree;
use crate::span::ByteSpan;
use crate::text_edit::addtext::pick_font_name;
use crate::text_edit::edit::make_raw_stream;
use crate::vartext::standard14_font_dict;
use crate::writer::{DirtySet, SaveOptions, save_incremental};

use super::OcrPage;
use super::editable::{ImageTarget, ImageWrite, OcrOutput};
use super::layer_content::{OcSectionNames, layer_content};
pub use super::layer_content::{build_layer_content, build_layer_content_on};
pub use super::layer_report::OcrLayerReport;
use super::layout::RegionGroup;
use std::collections::BTreeMap;

/// Ascent as a fraction of the font size, for the vertical fit.
///
/// Helvetica's `Ascender` is 718/1000 em (and its `CapHeight` is the same
/// value, which is why an all-caps word and a mixed-case word fit the same
/// way). Paired with [`HELVETICA_DESCENT_FRAC`] this defines the glyph box the
/// vertical fit solves against.
///
/// # Why the name carries the face, and is not just `ASCENT_FRAC`
///
/// `pdfcer-core` already contains `ASCENT_FRAC` / `DESCENT_FRAC` — twice, in
/// `text_edit::addtext` and `text_edit::reflow` — holding **0.75 / 0.25**.
/// Those are the *block model's* nominal figures, deliberately shared between
/// the two so a new run's box and a reflowed line's box agree with each other.
/// These are the *real AFM metrics of one specific face*, used because this
/// module is solving a fit against a font it chose itself.
///
/// **Both are correct, and a third module adding a fourth `ASCENT_FRAC` would
/// also be correct.** That is exactly the problem: under the bare name, a grep
/// for the identifier returns the wrong constant about half the time, and the
/// two differ by 0.043 em — small enough to look like a rounding artefact
/// rather than a different quantity. The 0.558 pt residual in this module's
/// integration test is that difference, measured. Naming the face is what makes
/// the collision impossible to have by accident.
pub const HELVETICA_ASCENT_FRAC: f64 = 0.718;

/// Descent as a fraction of the font size, for the vertical fit.
///
/// Helvetica's `Descender` is −207/1000 em, taken here as a positive
/// magnitude. This is what lifts the baseline off the bottom of the reported
/// box: without it, a word with a descender would have its tail hang below the
/// ink the engine actually saw, and every selection would sit low.
pub const HELVETICA_DESCENT_FRAC: f64 = 0.207;

/// The smallest horizontal scaling emitted, as a `Tz` percentage.
///
/// A clamp floor, not a preference. It exists because a degenerate box (a word
/// box one point wide holding a ten-character word) would otherwise produce a
/// scaling near zero, and a zero-width text run is unselectable — the layer
/// would silently contain a word nobody can reach. Clamping is counted and
/// disclosed ([`OcrLayerReport::words_scale_clamped`]).
pub const MIN_TZ: f64 = 1.0;

/// The largest horizontal scaling emitted, as a `Tz` percentage.
///
/// The mirror of [`MIN_TZ`]: a one-character word inside a very wide box
/// (a common artefact when an engine merges a rule line into a word box)
/// would otherwise stretch a single glyph across the page and swallow every
/// selection near it.
pub const MAX_TZ: f64 = 10_000.0;

/// Options for building an OCR text layer.
///
/// `#[non_exhaustive]`: the layer's shape is expected to grow (an embedded
/// composite face for non-Latin scripts is the known next axis), and a struct
/// literal at a call site outside the crate would break when it does.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct OcrLayerOptions {
    /// The Standard-14 face the invisible text is written in.
    ///
    /// It is never seen, so this is not an aesthetic choice — it selects the
    /// **metric table** the horizontal fit measures against, and therefore how
    /// closely a selection highlight tracks the ink. Helvetica is the default
    /// because its widths are the closest of the fourteen to the proportional
    /// sans faces most scanned business documents are set in.
    pub font: Std14,
    /// The `/Engine` recorded in the layer's marker ([`super::marker`]), so a
    /// later [`super::marker::OcrLayerRef`] can say what wrote it.
    pub engine: Option<String>,
    /// What to do when the page already carries a pdfcer OCR layer.
    pub existing: ExistingLayers,
    /// The optional-content group (a layer in a Layers panel) the text is
    /// written on; see [`Self::on_layer`].
    pub optional_content: Option<ObjId>,
    /// Per region group, the optional-content group that group's blocks are
    /// written on; see [`Self::on_region_layer`].
    pub region_layers: Vec<(RegionGroup, ObjId)>,
    /// What the run writes: invisible text over the scan (the default) or
    /// editable text with the glyphs erased; see [`Self::with_output`].
    pub output: OcrOutput,
}

pub use super::layer_removal::{OcrLayerRemoval, RegionLayerRemoval};

/// What an OCR write does when the page already carries a layer pdfcer wrote
/// (found by its marker, [`super::marker`]).
///
/// Layers written by other software carry no marker and are never found, so
/// no policy here touches them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum ExistingLayers {
    /// Remove the page's pdfcer layers in the same write, then add the new
    /// one. The default: re-running OCR means *redo it*, and two stacked
    /// layers double every extracted, searched and copied word. Disclosed as
    /// [`OcrLayerReport::layers_replaced`].
    #[default]
    Replace,
    /// Refuse with [`OcrLayerError::LayerPresent`].
    Refuse,
    /// Add the new layer beside the old ones.
    Stack,
}

impl Default for OcrLayerOptions {
    fn default() -> Self {
        Self {
            font: Std14::Helvetica,
            engine: None,
            existing: ExistingLayers::Replace,
            optional_content: None,
            region_layers: Vec::new(),
            output: OcrOutput::SearchableImage,
        }
    }
}

impl OcrLayerOptions {
    /// A fresh set of options (Helvetica).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Write the layer in a different Standard-14 face.
    #[must_use]
    pub fn with_font(mut self, font: Std14) -> Self {
        self.font = font;
        self
    }

    /// Record `engine` in the layer's marker.
    #[must_use]
    pub fn with_engine(mut self, engine: impl Into<String>) -> Self {
        self.engine = Some(engine.into());
        self
    }

    /// Choose what happens to a layer pdfcer already wrote on the page.
    #[must_use]
    pub fn with_existing(mut self, existing: ExistingLayers) -> Self {
        self.existing = existing;
        self
    }

    /// Write the text on optional-content group `group` (one registered in
    /// `/OCProperties /OCGs`, e.g. from `EditSession::add_layer`), so a
    /// Layers panel lists it and can hide it. The group's `/OC` section
    /// (ISO 32000-1 §8.11.3.2) nests inside the OCR marker, so the layer is
    /// still found and removed whole; the page's `/Properties` gains a name
    /// for the group when it has none.
    #[must_use]
    pub fn on_layer(mut self, group: ObjId) -> Self {
        self.optional_content = Some(group);
        self
    }

    /// Write the blocks read from `region`'s layout regions
    /// ([`super::OcrBlock::region`]) on optional-content group `group`, each
    /// block in its own `/OC` section; a second call for the same region
    /// group replaces the first. Blocks with no region, or a region with no
    /// group, are written as before. With [`Self::on_layer`] as well, a
    /// region's section nests inside the whole layer's, so it shows only when
    /// both groups are on (§8.11.3.2).
    #[must_use]
    pub fn on_region_layer(mut self, region: RegionGroup, group: ObjId) -> Self {
        self.region_layers.retain(|(r, _)| *r != region);
        self.region_layers.push((region, group));
        self
    }
}

pub use super::layer_error::OcrLayerError;

/// The saved bytes plus the disclosure report.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct OcrLayerOutcome {
    /// The incrementally-saved document.
    pub bytes: Vec<u8>,
    /// What was written and what was inferred.
    pub report: OcrLayerReport,
}

/// Everything one page's OCR layer needs, resolved against a graph, with
/// **nothing allocated and nothing written**.
///
/// # WHY THE PLANNING IS SPLIT FROM THE WRITING
///
/// Because there are now two writers with genuinely different allocation
/// models, and the *decisions* between them must not be made twice:
///
/// - [`add_ocr_layer`] is a one-shot on an immutable [`Document`]: it takes
///   object numbers from `next_object_number()` and stages content bytes at
///   `doc.bytes().len()`.
/// - [`crate::edit::EditSession::add_ocr_layer`] is a session command: it
///   takes numbers from the session's allocator and stages bytes through the
///   session's own R45 buffer, and it does this for **several pages under one
///   undo entry**.
///
/// Everything before that fork — which font name avoids a collision, which
/// words could be placed, what the `/Resources` merge has to preserve, what
/// the content stream says — is identical, and is exactly the part that is
/// expensive to get right. This mirrors `text_edit::addtext::plan_add_text`
/// and `AddTextPrep`, deliberately and structurally, because that pair solved
/// the same problem for the same three-object append.
///
/// The prep holds **no object numbers**. That is the whole point: a plan
/// that had already allocated could not be reused by a caller with a different
/// allocator, and a plan that allocated *per page* could not be collected into
/// one command.
pub(crate) struct OcrLayerPrep {
    /// The page object being modified.
    pub(crate) page_id: ObjId,
    /// The page dict as it currently stands — base, or the session overlay.
    page_dict: Dict,
    /// How the layer is appended to the page's current `/Contents`.
    pub(crate) append: page_tree::OverlayAppend,
    /// The page's **effective** `/Resources` minus `/Font`, references intact.
    resources_base: Dict,
    /// The existing `/Font` entries the new font merges into.
    font_subdict_base: Dict,
    /// The collision-free `/Font` name for the OCR font.
    font_name: Vec<u8>,
    /// `/Properties` names to bind to the layer's groups the page has no
    /// name for yet.
    new_bindings: Vec<(Name, ObjId)>,
    /// The page's resolved `/Properties`, which a new binding merges into.
    properties_base: Dict,
    /// The `BT … ET` content-stream bytes for the invisible layer.
    pub(crate) content_data: Vec<u8>,
    /// The Standard-14 font dictionary object.
    pub(crate) font_dict: Object,
    /// What was written and what was inferred, minus the two object numbers
    /// the caller fills in once it has allocated them.
    pub(crate) report: OcrLayerReport,
    /// The page's resolved `/XObject`, which a cloned image's binding
    /// merges into.
    xobject_base: Dict,
    /// Editable output: the images to rewrite.
    pub(crate) images: Vec<ImageWrite>,
}

impl OcrLayerPrep {
    /// The rewritten page dictionary, given the new `/Contents` value
    /// (from [`Self::append`]), the font number the caller allocated and
    /// the object each cloned image was given, in [`Self::images`] order.
    pub(crate) fn build_page_dict(
        &self,
        contents: Object,
        font_id: ObjId,
        clones: &[ObjId],
    ) -> Dict {
        let mut new_page = self.page_dict.clone();
        new_page.insert(Name::from(b"Contents"), contents);
        let mut font_subdict = self.font_subdict_base.clone();
        font_subdict.insert(Name(self.font_name.clone()), Object::Reference(font_id));
        let mut resources = self.resources_base.clone();
        resources.insert(Name::from(b"Font"), Object::Dict(font_subdict));
        let cloned = self
            .images
            .iter()
            .filter(|i| i.target == ImageTarget::Clone)
            .zip(clones);
        if !clones.is_empty() {
            let mut xobjects = self.xobject_base.clone();
            for (image, id) in cloned {
                xobjects.insert(Name(image.name.clone()), Object::Reference(*id));
            }
            resources.insert(Name::from(b"XObject"), Object::Dict(xobjects));
        }
        if !self.new_bindings.is_empty() {
            let mut props = self.properties_base.clone();
            for (name, group) in &self.new_bindings {
                props.insert(name.clone(), Object::Reference(*group));
            }
            resources.insert(Name::from(b"Properties"), Object::Dict(props));
        }
        new_page.insert(Name::from(b"Resources"), Object::Dict(resources));
        new_page
    }
}

/// Apply [`OcrLayerOptions::existing`] to one page: the layers to take off
/// before writing, or the refusal.
///
/// # Errors
///
/// [`OcrLayerError::LayerPresent`] under [`ExistingLayers::Refuse`] when the
/// page carries a pdfcer layer.
pub(crate) fn existing_layer_strip(
    view: &crate::view::DocumentView<'_>,
    page: &crate::page_tree::Page,
    page_index: usize,
    opts: &OcrLayerOptions,
) -> Result<LayerStrip, OcrLayerError> {
    let found = page_ocr_layers(view, page, page_index);
    if found.iter().any(|l| l.editable) {
        return Err(OcrLayerError::EditableLayerPresent { page_index });
    }
    if found.is_empty() || opts.existing == ExistingLayers::Stack {
        return Ok(LayerStrip::default());
    }
    if opts.existing == ExistingLayers::Refuse {
        return Err(OcrLayerError::LayerPresent {
            page_index,
            count: found.len(),
        });
    }
    Ok(plan_strip(view, page, &found))
}

/// The `/Properties` names the layer's `/OC` sections use, and the
/// bindings the page still needs.
fn section_names(
    graph: &crate::view::DocumentView<'_>,
    page: &crate::page_tree::Page,
    opts: &OcrLayerOptions,
) -> Result<(OcSectionNames, Vec<(Name, ObjId)>), OcrLayerError> {
    let groups: Vec<ObjId> = opts
        .optional_content
        .into_iter()
        .chain(opts.region_layers.iter().map(|(_, g)| *g))
        .collect();
    if let Some(&id) = groups
        .iter()
        .find(|&&g| !super::group::is_registered(graph, g))
    {
        return Err(OcrLayerError::NotALayerGroup { id });
    }
    let named = super::group::property_names(graph, page, &groups);
    let new_bindings = named
        .iter()
        .zip(&groups)
        .filter(|((_, new), _)| *new)
        .map(|((n, _), g)| (n.clone(), *g))
        .collect();
    let mut it = named.into_iter().map(|(n, _)| n);
    let whole = opts.optional_content.and_then(|_| it.next());
    let regions = opts.region_layers.iter().map(|(r, _)| *r).zip(it).collect();
    Ok((OcSectionNames { whole, regions }, new_bindings))
}

/// Plan one page's OCR layer against `graph`, allocating nothing.
///
/// `page` must come from the same graph the caller will write through — the
/// session's overlay for a session command, the base for the one-shot — so
/// that a page edited earlier in the session contributes **its edited**
/// `/Contents` and `/Resources` to the append rather than the base revision's.
/// That divergence is the exact trap the consuming shell reported working
/// around with a refusal, and planning against the caller's own graph is what
/// removes it at the root.
///
/// The words in `ocr_page` must already be in **PDF default user space, y-up**
/// — see [`add_ocr_layer`]. Nothing here flips anything.
///
/// # Errors
///
/// [`OcrLayerError::Unsupported`] if the page object is not a dictionary, and
/// [`OcrLayerError::NothingToWrite`] if every word proved unplaceable. Both
/// happen before the caller allocates anything.
pub(crate) fn plan_ocr_layer(
    page: &crate::page_tree::Page,
    ocr_page: &OcrPage,
    opts: &OcrLayerOptions,
    strip: &LayerStrip,
    graph: &crate::view::DocumentView<'_>,
    uses: &BTreeMap<ObjId, usize>,
) -> Result<OcrLayerPrep, OcrLayerError> {
    let page_dict = graph.resolved(page.id).as_dict().cloned().ok_or_else(|| {
        OcrLayerError::Unsupported("the page object is not a dictionary".to_owned())
    })?;
    let contents_before = if strip.is_empty() {
        page_dict.get(b"Contents").cloned()
    } else {
        contents_without(graph, page_dict.get(b"Contents"), &strip.contents)
    };
    let append = page_tree::plan_overlay_append(graph, contents_before.as_ref());

    // The §7.7.3.4 inheritance-safe recipe, identical to add-text's: take the
    // page's EFFECTIVE resources (own-or-inherited, already resolved by the
    // page-tree walk), strip /Font, and re-add it merged. Writing an own
    // /Resources holding only the new font would shadow an inherited one and
    // silently break every other resource the page uses.
    let mut font_subdict_base: Dict = match page.resources.get(b"Font") {
        Some(o) => graph.resolve(o).as_dict().cloned().unwrap_or_default(),
        None => Dict::new(),
    };
    for name in &strip.font_names {
        font_subdict_base.remove(name);
    }
    let font_name = pick_font_name(&font_subdict_base);
    let mut resources_base = page.resources.clone();
    resources_base.remove(b"Font");

    let (names, new_bindings) = section_names(graph, page, opts)?;
    let editable = match opts.output {
        OcrOutput::EditableText(eo) => Some(eo),
        _ => None,
    };
    let analysis =
        editable.map(|eo| super::editable::analyse(graph, page, ocr_page, opts, &eo, uses));
    let styles = analysis.as_ref().map_or(&[][..], |a| a.styles.as_slice());
    let fitted = analysis
        .as_ref()
        .and_then(|a| super::editable_fit::fitted(a, ocr_page, page.rotate));
    let placed = fitted.as_ref().unwrap_or(ocr_page);
    let (content_data, mut report, visible) =
        layer_content(placed, &font_name, &names, opts, page.rotate, styles);
    if report.words_written == 0 {
        return Err(OcrLayerError::NothingToWrite);
    }
    report.layers_replaced = strip.contents.len();
    let images = match (analysis, editable) {
        (Some(a), Some(eo)) => {
            super::editable::apply(a, &visible, ocr_page, eo.image_codec, &mut report)
                .ok_or(OcrLayerError::ImageEncodeFailed)?
        }
        _ => Vec::new(),
    };

    Ok(OcrLayerPrep {
        page_id: page.id,
        page_dict,
        append,
        resources_base,
        font_subdict_base,
        font_name,
        new_bindings,
        properties_base: super::group::properties(graph, &page.resources),
        content_data,
        font_dict: Object::Dict(standard14_font_dict(opts.font)),
        report,
        xobject_base: super::editable::xobject_base(graph, page),
        images,
    })
}

/// Add an invisible OCR text layer to `page_index` of `doc` and return the
/// incrementally-saved bytes plus the disclosure report.
///
/// The words in `ocr_page` must already be in **PDF default user space,
/// y-up** — run [`words_to_page_space`](super::words_to_page_space) on raw
/// engine output first. This function does not flip anything, deliberately: a
/// second place that could flip is a second place that could flip twice.
///
/// # What it writes
///
/// Three objects' worth of change, all additive: a new content stream
/// (appended to the page's `/Contents`), a new Standard-14 font dictionary,
/// and the rewritten page dictionary. The scanned image object is untouched
/// and, under the incremental save, not re-emitted at all (project rule 3).
///
/// # Errors
///
/// [`OcrLayerError`] — an out-of-range page, an encrypted document, a page
/// whose words all proved unplaceable ([`OcrLayerError::NothingToWrite`]), a
/// non-dictionary page object, exhausted object numbers, or a save failure.
/// Every refusal happens **before** any object is allocated.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::ocr::{OcrPage, layer};
///
/// let doc = Document::load(std::path::Path::new("scan.pdf"))?;
/// let recognised = OcrPage::default(); // ...from an engine, in page space
/// let out = layer::add_ocr_layer(&doc, 0, &recognised, &layer::OcrLayerOptions::new())?;
/// std::fs::write("searchable.pdf", &out.bytes)?;
/// // Rule 4 (decision 059): render normally, report separately. Both.
/// for line in out.report.disclosures() {
///     eprintln!("{line}");
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn add_ocr_layer(
    doc: &Document,
    page_index: usize,
    ocr_page: &OcrPage,
    opts: &OcrLayerOptions,
) -> Result<OcrLayerOutcome, OcrLayerError> {
    one_shot_guards(doc)?;
    let pages = page_tree::pages(doc)?;
    let page = pages
        .get(page_index)
        .ok_or(OcrLayerError::PageIndex(page_index))?;
    let uses = match opts.output {
        OcrOutput::EditableText(_) => crate::redact_image::image_use_census(doc, &pages),
        _ => BTreeMap::new(),
    };
    // The plan is shared with `EditSession::add_ocr_layer`; only where
    // numbers and staged bytes come from differs.
    let strip = existing_layer_strip(&doc.view(), page, page_index, opts)?;
    let prep = plan_ocr_layer(page, ocr_page, opts, &strip, &doc.view(), &uses)?;
    let mut report = prep.report.clone();

    let exhausted = || OcrLayerError::ObjectNumbersExhausted;
    let content_num = doc.next_object_number().ok_or_else(exhausted)?;
    let font_num = content_num.checked_add(1).ok_or_else(exhausted)?;
    let content_id = ObjId::new(content_num, 0);
    let font_id = ObjId::new(font_num, 0);

    // A sanctioned writer bypass, exception 7's shape (see
    // `tools/check-bypass-paths.sh`): a one-shot on a `Document` outside any
    // session, so no undo stack to join. `one_shot_guards` refuses encrypted
    // and enforced-certified documents, which is what makes the exemption
    // honest; do not copy this marker to a writer without both refusals.
    // Bytes are staged at `base.len() + local` (R45); the image (unless
    // rewritten) and the original content are not re-emitted (rule 3).
    let start = doc.bytes().len();
    let mut staging = prep.content_data.clone();
    let mut dirty = DirtySet::empty();
    // bypass-exempt: the note above; tokens sit within the gate's window.
    dirty.replace(
        content_id,
        make_raw_stream(ByteSpan::new(start, staging.len()), staging.len()),
    );
    dirty.replace(font_id, prep.font_dict.clone());
    let mut next = font_num.checked_add(1);
    let mut clones = Vec::new();
    for (k, image) in prep.images.iter().enumerate() {
        let id = match image.target {
            ImageTarget::InPlace(id) => id,
            ImageTarget::Clone => {
                let id = ObjId::new(next.ok_or_else(exhausted)?, 0);
                next = id.num.checked_add(1);
                clones.push(id);
                id
            }
        };
        if let Some(r) = report.images_rewritten.get_mut(k) {
            r.image = id.num;
        }
        // bypass-exempt: the image rewrite of the same one-shot write.
        dirty.replace(id, image.staged(start + staging.len()));
        staging.extend_from_slice(&image.data);
    }
    let contents = crate::text_edit::addtext::one_shot_contents(
        &prep.append,
        content_id,
        next,
        start,
        &mut staging,
        &mut dirty,
    )
    .ok_or_else(exhausted)?;
    dirty.replace(
        prep.page_id,
        Object::Dict(prep.build_page_dict(contents, font_id, &clones)),
    );
    // bypass-exempt: the second token covers `save_incremental` below.
    dirty.set_staging(staging);
    let (bytes, _) = save_incremental(doc, &dirty, &SaveOptions::identity())?;

    report.content_object = content_num;
    report.font_object = font_num;
    Ok(OcrLayerOutcome { bytes, report })
}

/// The one-shot's refusals, before any work: encryption, then an enforced
/// certification (§12.8.4 Table 258, `/P` absent means 2 per Table 254),
/// by the same machinery as `add_text`.
fn one_shot_guards(doc: &Document) -> Result<(), OcrLayerError> {
    if crate::encryption_gate::forbids(doc, &[PermissionBit::ModifyContents]) {
        return Err(OcrLayerError::Encrypted);
    }
    let census = crate::signature::census(doc);
    if census.forbids_structural_change() {
        return Err(OcrLayerError::CertificationForbidsChange {
            permission: census.certification_permission.unwrap_or(2),
        });
    }
    Ok(())
}
