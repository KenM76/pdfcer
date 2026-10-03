//! Export a rectangle of one page as a one-page PDF that looks the way the
//! viewer showed it: annotation visibility and per-layer visibility applied,
//! everything outside the rectangle gone.
//!
//! # Pipeline
//!
//! 1. [`assemble()`] copies the page out of the view (session edits included),
//!    dropping the outline.
//! 2. Optional content is resolved to the requested visibility: every layer
//!    the default configuration registers is deleted, hidden ones with their
//!    content (`EditSession::delete_layer`).
//! 3. With annotations on, fields then annotations are flattened into page
//!    content; anything that cannot be flattened is removed and named.
//! 4. Form XObjects wholly outside the rectangle are dropped; ones crossing
//!    its edge are inlined (§8.10.1 Do procedure: `q`, `/Matrix cm`,
//!    `/BBox` clip, content, `Q`) so step 6 can cut inside them.
//! 5. The page is framed: `/MediaBox` = `/CropBox` = the rectangle, the other
//!    §14.11.2 boxes clamped to it, `/Annots`, `/Thumb`, `/B` and `/AA`
//!    removed.
//! 6. The four bands around the rectangle are redacted (§12.5.6.23, no
//!    `/IC`, so nothing is painted) out to everything the page draws, which
//!    destroys the outside content rather than hiding it.
//! 7. The result is copied once more so unreachable objects are not written.
//!
//! # What happens at the edge
//!
//! Redaction semantics, as `pdfcer redact` applies them: a glyph whose box
//! crosses the edge is removed whole, its advance kept so the rest of the
//! line does not move; a stroked path is cut back about one stroke width
//! inside the edge; a fill is cut exactly at the edge; image samples outside
//! are destroyed. Content wholly inside keeps its exact position; its streams
//! are re-serialized, annotations are burned into content, and a clipping
//! path that crosses the edge is kept whole ([`RegionReport::clips_kept`]).

mod frame;
mod inline;

use std::collections::BTreeSet;

use crate::document::{DocError, Document};
use crate::edit::{EditError, EditSession, LayerContentPolicy};
use crate::layers::{LayerScan, read_layers_with};
use crate::object::{ObjId, Object};
use crate::page_tree::Rect;
use crate::redact::{
    RedactError, RedactOptions, RedactionReport, ResidualScope, apply_redactions_with,
};
use crate::view::DocumentView;
use crate::writer::{SaveOptions, WriteError};

use super::{AssembleOptions, OutlinePolicy, PageOpError, assemble};

/// How the exported region should look: the viewer state to reproduce.
///
/// Mirrors `pdfcer_render::RenderOptions::annotations` and
/// `RenderOptions::layers`, so a shell passes the same state it renders with.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RegionExport {
    /// Whether annotations and form fields appear. `true` flattens them into
    /// page content; `false` removes them. Default `true`.
    pub annotations: bool,
    /// The complete set of hidden layers (optional content groups, by id in
    /// the source view), replacing the default configuration's `/D` state.
    /// `None` obeys `/D`.
    pub hidden_layers: Option<BTreeSet<ObjId>>,
}

impl Default for RegionExport {
    fn default() -> Self {
        Self {
            annotations: true,
            hidden_layers: None,
        }
    }
}

impl RegionExport {
    /// Annotations shown, layers as the document's default configuration
    /// sets them.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Show (`true`) or remove (`false`) annotations and form fields.
    #[must_use]
    pub fn with_annotations(mut self, show: bool) -> Self {
        self.annotations = show;
        self
    }

    /// Hide exactly these layers and show every other one, whatever `/D`
    /// says. An empty set shows every layer.
    #[must_use]
    pub fn with_hidden_layers(mut self, hidden: impl IntoIterator<Item = ObjId>) -> Self {
        self.hidden_layers = Some(hidden.into_iter().collect());
        self
    }
}

/// Why a region export was refused.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum RegionError {
    /// The rectangle is not finite or has no area.
    #[error("the region must be a finite rectangle with positive width and height")]
    InvalidRect,
    /// The page could not be copied out of the document.
    #[error(transparent)]
    PageOp(#[from] PageOpError),
    /// Layer resolution or flattening was refused.
    #[error(transparent)]
    Edit(#[from] EditError),
    /// A content stream on the page could not be parsed.
    #[error(transparent)]
    Content(#[from] crate::content::ContentError),
    /// Removing the content outside the region was refused.
    #[error(transparent)]
    Redact(#[from] RedactError),
    /// An intermediate or final revision could not be written.
    #[error(transparent)]
    Write(#[from] WriteError),
    /// Every object number is in use, so rewritten page content has nowhere
    /// to go.
    #[error("the document has no free object number for the rewritten page content")]
    NoObjectNumber,
    /// An intermediate revision pdfcer wrote could not be read back.
    #[error("an intermediate revision could not be re-read: {0}")]
    Reload(#[from] DocError),
    /// Raster samples outside the region could not be destroyed, which
    /// leaves every mark covering them unapplied; exporting would keep
    /// content the region excludes.
    #[error(
        "{marks} band(s) around the region cover image samples pdfcer cannot destroy; \
         the export was refused rather than keep content outside the region"
    )]
    ImageNotCut {
        /// How many of the four bands were left unapplied.
        marks: u64,
    },
}

/// What [`extract_region`] did. Every count is for the one exported page.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RegionReport {
    /// The region, normalized; it is the output page's `/MediaBox`.
    pub rect: Rect,
    /// Layers hidden in the output: their content was removed.
    pub layers_hidden: usize,
    /// Layers shown in the output: their content kept, no longer on a layer.
    pub layers_kept: usize,
    /// Marked-content sections, XObject calls and annotations removed
    /// because they belonged to a hidden layer.
    pub layer_content_removed: usize,
    /// Form fields flattened.
    pub fields_flattened: usize,
    /// Widget appearances burned into content by field flattening.
    pub widgets_flattened: usize,
    /// Annotations burned into content (ce dimensions included).
    pub annotations_flattened: usize,
    /// ce dimensions among [`Self::annotations_flattened`].
    pub ce_dimensions_flattened: usize,
    /// Annotations removed: all of them with annotations off, otherwise the
    /// ones that could not be flattened (each named in [`Self::notes`]).
    pub annotations_removed: usize,
    /// Form XObjects crossing the edge, inlined so they could be cut.
    pub forms_inlined: usize,
    /// Form XObjects wholly outside the region, dropped.
    pub forms_dropped: usize,
    /// Form XObjects crossing the edge that could not be inlined; their
    /// outside content survives (a residual, named in [`Self::notes`]).
    pub forms_kept_straddling: usize,
    /// Glyphs removed, including every glyph crossing the edge.
    pub glyphs_removed: u64,
    /// Paths cut at the edge.
    pub paths_cut: u64,
    /// Paths wholly outside, removed.
    pub paths_dropped: u64,
    /// Paths crossing the edge that could not be cut (a residual).
    pub paths_uncut: u64,
    /// Clipping paths crossing the edge, kept whole (a residual of geometry
    /// only: a clip paints nothing).
    pub clips_kept: u64,
    /// Images whose outside samples were blanked.
    pub images_cleared: u64,
    /// Images wholly outside, removed.
    pub images_removed: u64,
    /// Shadings cut at the edge.
    pub shadings_cut: u64,
    /// Shadings crossing the edge that could not be cut (a residual).
    pub shadings_uncut: u64,
    /// `/TrimBox`, `/BleedBox` or `/ArtBox` entries clamped to the region or
    /// removed because they lay outside it.
    pub boxes_clamped: usize,
    /// Everything else worth saying, one sentence each.
    pub notes: Vec<String>,
}

impl RegionReport {
    fn new(rect: Rect) -> Self {
        Self {
            rect,
            layers_hidden: 0,
            layers_kept: 0,
            layer_content_removed: 0,
            fields_flattened: 0,
            widgets_flattened: 0,
            annotations_flattened: 0,
            ce_dimensions_flattened: 0,
            annotations_removed: 0,
            forms_inlined: 0,
            forms_dropped: 0,
            forms_kept_straddling: 0,
            glyphs_removed: 0,
            paths_cut: 0,
            paths_dropped: 0,
            paths_uncut: 0,
            clips_kept: 0,
            images_cleared: 0,
            images_removed: 0,
            shadings_cut: 0,
            shadings_uncut: 0,
            boxes_clamped: 0,
            notes: Vec::new(),
        }
    }

    /// Whether anything outside the region may survive in the output.
    #[must_use]
    pub fn has_residuals(&self) -> bool {
        self.paths_uncut > 0 || self.shadings_uncut > 0 || self.forms_kept_straddling > 0
    }
}

/// Export `rect` of page `page` (0-based) as a one-page PDF showing what the
/// viewer showed under `state`.
///
/// `view` is read, never changed; pass `EditSession::view` to export with
/// unsaved edits. The output is a full rewrite with `/MediaBox` =
/// `/CropBox` = `rect` (default user space, before `/Rotate`), and no other
/// page box extending past it. Content outside `rect` is removed, not
/// clipped, so it cannot be recovered from the file. See the module docs for
/// what happens to content crossing the edge.
///
/// `export_svg_view` / `export_emf_view` on the result draw the same page.
///
/// # Errors
///
/// [`RegionError::InvalidRect`]; [`RegionError::PageOp`] for a bad page
/// index; [`RegionError::Edit`] when flattening or layer removal is refused
/// (a certified or encrypted document); [`RegionError::ImageNotCut`] when
/// image samples outside could not be destroyed; the remaining variants when
/// an intermediate revision could not be written or parsed.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::document::Document;
/// use pdfcer_core::page_tree::Rect;
/// use pdfcer_core::pageops::region::{RegionExport, extract_region};
/// # fn run() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::load(std::path::Path::new("drawing.pdf"))?;
/// let rect = Rect::from_corners(100.0, 100.0, 300.0, 250.0);
/// let state = RegionExport::new().with_annotations(false);
/// let (bytes, report) = extract_region(&doc.view(), 0, rect, &state)?;
/// std::fs::write("detail.pdf", bytes)?;
/// println!("{} glyphs removed at the edge or outside", report.glyphs_removed);
/// # Ok(()) }
/// ```
pub fn extract_region(
    view: &DocumentView<'_>,
    page: usize,
    rect: Rect,
    state: &RegionExport,
) -> Result<(Vec<u8>, RegionReport), RegionError> {
    let rect = checked(rect)?;
    let mut report = RegionReport::new(rect);
    let (copied, _) = assemble(std::slice::from_ref(view), &[(0, page)], &copy_options())?;
    let prepared = prepare(Document::from_bytes(copied)?, view, state, &mut report)?;
    let inlined = inline::inline_forms(Document::from_bytes(prepared)?, rect, &mut report)?;
    let framed = frame::frame(&Document::from_bytes(inlined)?, rect, &mut report)?;
    let marked = frame::mark_bands(Document::from_bytes(framed)?, rect)?;
    let options = RedactOptions::with_residual_scope(ResidualScope::MarkedOnly);
    let (cut, redaction) = apply_redactions_with(
        &Document::from_bytes(marked)?,
        &SaveOptions::identity(),
        &options,
    )?;
    absorb(&mut report, &redaction)?;
    let finished = frame::strip_annots(Document::from_bytes(cut)?)?;
    let doc = Document::from_bytes(finished)?;
    let (bytes, _) = assemble(&[doc.view()], &[(0, 0)], &copy_options())?;
    Ok((bytes, report))
}

fn checked(rect: Rect) -> Result<Rect, RegionError> {
    let finite = [rect.llx, rect.lly, rect.urx, rect.ury]
        .iter()
        .all(|v| v.is_finite());
    let rect = Rect::from_corners(rect.llx, rect.lly, rect.urx, rect.ury);
    if !finite || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return Err(RegionError::InvalidRect);
    }
    Ok(rect)
}

fn copy_options() -> AssembleOptions {
    AssembleOptions {
        outline: OutlinePolicy::Drop,
        ..AssembleOptions::default()
    }
}

/// Steps 2 and 3: layers, then fields and annotations, in a session over the
/// copied page.
fn prepare(
    doc: Document,
    source: &DocumentView<'_>,
    state: &RegionExport,
    report: &mut RegionReport,
) -> Result<Vec<u8>, RegionError> {
    let mut session = EditSession::new(doc);
    resolve_layers(&mut session, source, state, report)?;
    if state.annotations {
        flatten(&mut session, report)?;
    }
    Ok(session.to_full_bytes(&SaveOptions::identity())?.0)
}

fn resolve_layers(
    session: &mut EditSession,
    source: &DocumentView<'_>,
    state: &RegionExport,
    report: &mut RegionReport,
) -> Result<(), RegionError> {
    let copy = read_layers_with(&session.graph(), LayerScan::CatalogOnly);
    let plan = layer_plan(&copy.layers, source, state, report);
    for (id, hide) in plan {
        let policy = if hide {
            LayerContentPolicy::RemoveContent
        } else {
            LayerContentPolicy::KeepUnlayered
        };
        let done = session.delete_layer(id, policy)?;
        if hide {
            report.layers_hidden += 1;
            report.layer_content_removed += done.sections + done.xobject_calls + done.annotations;
        } else {
            report.layers_kept += 1;
        }
    }
    Ok(())
}

/// `(copied layer id, hide?)` for each layer the default configuration
/// registers. An override names source ids; copied layers are matched to
/// them by position and name, since the copy deep-copies `/OCProperties` in
/// order.
fn layer_plan(
    copied: &[crate::layers::Layer],
    source: &DocumentView<'_>,
    state: &RegionExport,
    report: &mut RegionReport,
) -> Vec<(ObjId, bool)> {
    let Some(hidden) = &state.hidden_layers else {
        return copied
            .iter()
            .filter(|l| l.in_default_config)
            .map(|l| (l.id, !l.visible_by_default))
            .collect();
    };
    let original = read_layers_with(source.graph(), LayerScan::CatalogOnly).layers;
    let paired = original.len() == copied.len()
        && original.iter().zip(copied).all(|(a, b)| a.name == b.name);
    if !paired {
        report.notes.push(
            "the layer override could not be matched to the copied page's layers; \
             the default configuration's visibility was used"
                .to_string(),
        );
    }
    let unknown = hidden
        .iter()
        .filter(|id| !original.iter().any(|l| l.id == **id))
        .count();
    if unknown > 0 {
        report.notes.push(format!(
            "{unknown} hidden-layer id(s) name no layer in the document and were ignored"
        ));
    }
    copied
        .iter()
        .zip(&original)
        .filter(|(c, _)| c.in_default_config)
        .map(|(c, o)| {
            let hide = if paired {
                hidden.contains(&o.id)
            } else {
                !c.visible_by_default
            };
            (c.id, hide)
        })
        .collect()
}

/// Step 3: fields first (their widgets are annotations too), then every
/// other annotation on the page.
fn flatten(session: &mut EditSession, report: &mut RegionReport) -> Result<(), RegionError> {
    match session.flatten_fields(None) {
        Ok(done) => {
            report.fields_flattened = done.fields_flattened;
            report.widgets_flattened = done.widgets_burned;
        }
        Err(EditError::NoInteractiveForm) => {}
        Err(err) => return Err(err.into()),
    }
    let dimensions = ce_dimensions(session)?;
    let done = session.flatten_annotations(0, None)?;
    report.annotations_flattened = done.flattened;
    report.ce_dimensions_flattened = dimensions
        .iter()
        .filter(|id| !done.skipped.iter().any(|s| s.id == Some(**id)))
        .count();
    for skipped in &done.skipped {
        report.notes.push(format!(
            "a /{} annotation could not be flattened ({}) and was removed",
            String::from_utf8_lossy(&skipped.subtype),
            skipped.reason
        ));
    }
    Ok(())
}

/// The ce dimensions on the copied page: `/Line` annotations with
/// `/IT /LineDimension`.
fn ce_dimensions(session: &EditSession) -> Result<Vec<ObjId>, RegionError> {
    let graph = session.graph();
    let slots = session.page_slots().map_err(PageOpError::from)?;
    let Some(slot) = slots.first() else {
        return Ok(Vec::new());
    };
    let ids = crate::annot::page_annotations(&graph, slot.id)
        .into_iter()
        .filter(|a| a.subtype == b"Line")
        .filter_map(|a| a.id)
        .filter(|id| {
            crate::graph::ObjectGraph::value(&graph, *id)
                .and_then(Object::as_dict)
                .and_then(|d| d.get(b"IT"))
                .and_then(Object::as_name)
                .is_some_and(|n| n.as_bytes() == b"LineDimension")
        })
        .collect();
    Ok(ids)
}

/// Step 6's report, folded into the region's.
fn absorb(report: &mut RegionReport, r: &RedactionReport) -> Result<(), RegionError> {
    if r.marks_retained > 0 {
        return Err(RegionError::ImageNotCut {
            marks: r.marks_retained,
        });
    }
    report.glyphs_removed = r.glyphs_removed;
    report.paths_cut = r.vector_paths_cut;
    report.paths_dropped = r.vector_paths_dropped;
    report.paths_uncut = r.vector_paths_intersecting;
    report.clips_kept = r.vector_clips_kept;
    report.images_cleared = r.images_cleared;
    report.images_removed = r.images_removed;
    report.shadings_cut = r.shadings_cut;
    report.shadings_uncut = r.shadings_intersecting;
    let straddling = report.forms_kept_straddling > 0;
    report.notes.extend(
        r.notes
            .iter()
            .filter(|n| straddling || !n.contains("form XObject overlaps"))
            .cloned(),
    );
    Ok(())
}
