//! Form XObject surgery for redaction (ISO 32000-1 §8.10.1).
//!
//! A `Do` of a form whose clipped extent (`/BBox` × `/Matrix` × CTM) meets
//! a region is redacted by running the surgeon over the form's own content,
//! in the graphics state in effect at the `Do`, and binding the result as a
//! fresh copy under a new resource name. Copy-on-write keeps every other
//! placement of a shared form unchanged; [`retire_originals`] then empties
//! an original whose every counted use was rewritten, because the full
//! rewrite keeps unreferenced objects and the original still holds the
//! removed content.
//!
//! Image placements inside a form go through the same sample surgery as
//! the page's (`redact_image::plan_page`) against the form's own content
//! and resources; their clones are bound in the copy's `/XObject`
//! dictionary. The pre-surgery census descends too ([`NestedImages`]), so
//! an undestroyable image in a form retains its mark and a shared image's
//! copy-on-write decision counts its form placements.
//!
//! Not handled, and disclosed rather than claimed: a form nested deeper
//! than [`MAX_FORM_DEPTH`], one painted inside itself, and one whose
//! stream cannot be decoded.

use super::{
    Dict, Document, Edit, Mat, Name, ObjId, Object, RedactionReport, RegionBox, Stream, Surgeon,
    aabb, alloc, splice,
};
use crate::content::ContentStream;
use crate::redact::CarrierAction;
use crate::redact_image::{self, Allocator, DecodeCache, ImageHit, ImageOutcome};
use crate::view::DocumentView;
use crate::writer::DirtySet;
use std::collections::BTreeMap;

/// Deepest form nesting the surgery descends into.
pub(super) const MAX_FORM_DEPTH: usize = crate::text_edit::forms::MAX_FORM_DEPTH;

/// A redacted form placement: the copy to create and bind under `name`.
pub(super) struct FormRewrite {
    name: Vec<u8>,
    source: ObjId,
    /// The form's decoded content and the text/vector edits against it.
    buf: Vec<u8>,
    edits: Vec<Edit>,
    /// Image placements in this form (not its children) that cover a cell.
    image_hits: Vec<ImageHit>,
    /// The resources the form's content was read against (its own, or
    /// the page's for a form with none).
    resources: Dict,
    children: Vec<FormRewrite>,
}

/// Per-surgeon form state and tallies.
#[derive(Default)]
pub(super) struct FormScope {
    /// Descend into forms. Off for the image census, which needs only the
    /// page's own placements.
    pub(super) descend: bool,
    depth: usize,
    active: Vec<u32>,
    pub(super) rewrites: Vec<FormRewrite>,
    pub(super) tally: FormTally,
    /// Image placements found inside forms at any depth, for the census.
    pub(super) images: Vec<NestedImages>,
}

/// Image placements inside one form placement, with what the blocker test
/// reads them against.
pub(super) struct NestedImages {
    pub(super) resources: Dict,
    pub(super) buf: Vec<u8>,
    pub(super) hits: Vec<ImageHit>,
}

/// What image surgery inside forms reads and accumulates for one page.
pub(super) struct ImagePass<'r, 'v> {
    pub(super) view: &'r DocumentView<'v>,
    pub(super) page_number: usize,
    pub(super) regions: &'r [RegionBox],
    pub(super) uses: &'r BTreeMap<ObjId, usize>,
    pub(super) covered: &'r BTreeMap<ObjId, usize>,
    pub(super) tombstoned: &'r mut BTreeMap<ObjId, ()>,
    pub(super) cache: &'r mut DecodeCache,
    /// Counters and notes for the report (edits, objects and bindings are
    /// consumed per form).
    pub(super) out: ImageOutcome,
}

/// Form placements over a region, by outcome.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct FormTally {
    /// Placements whose content was redacted inside a copy.
    pub(super) redacted: u64,
    /// Placements whose content could not be read or reached.
    pub(super) unreached: u64,
}

impl FormTally {
    /// Add another page's tally to this document-wide one.
    pub(super) fn absorb(&mut self, o: Self) {
        self.redacted += o.redacted;
        self.unreached += o.unreached;
    }
}

/// The form's `/Matrix`, identity when absent or malformed.
fn form_matrix(doc: &Document, dict: &Dict) -> Mat {
    let nums: Vec<f64> = doc
        .resolve(dict.get(b"Matrix").unwrap_or(&Object::Null))
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|o| doc.resolve(o).as_number())
                .collect()
        })
        .unwrap_or_default();
    match nums.as_slice() {
        [a, b, c, d, e, f] => Mat {
            a: *a,
            b: *b,
            c: *c,
            d: *d,
            e: *e,
            f: *f,
        },
        _ => Mat::IDENTITY,
    }
}

/// The form's `/BBox` mapped to page space through `placement`, or `None`
/// when it is absent or malformed (treated as unbounded).
fn page_extent(doc: &Document, dict: &Dict, placement: Mat) -> Option<(f64, f64, f64, f64)> {
    let nums: Vec<f64> = doc
        .resolve(dict.get(b"BBox")?)
        .as_array()?
        .iter()
        .filter_map(|o| doc.resolve(o).as_number())
        .collect();
    let [x0, y0, x1, y1] = nums.as_slice() else {
        return None;
    };
    let corners = [
        placement.apply(*x0, *y0),
        placement.apply(*x1, *y0),
        placement.apply(*x0, *y1),
        placement.apply(*x1, *y1),
    ];
    Some(aabb(&corners))
}

type BoxF = (f64, f64, f64, f64);

/// The painted area: the extent within the clip. `Some(None)` is
/// unbounded; `None` paints nothing.
fn visible(clip: Option<BoxF>, extent: Option<BoxF>) -> Option<Option<BoxF>> {
    let b = match (clip, extent) {
        (None, None) => return Some(None),
        (Some(b), None) | (None, Some(b)) => b,
        (Some(a), Some(b)) => (a.0.max(b.0), a.1.max(b.1), a.2.min(b.2), a.3.min(b.3)),
    };
    (b.0 <= b.2 && b.1 <= b.3).then_some(Some(b))
}

fn meets_any(regions: &[RegionBox], area: Option<BoxF>) -> bool {
    match area {
        None => !regions.is_empty(),
        Some((x0, y0, x1, y1)) => regions.iter().any(|r| r.intersects(x0, y0, x1, y1)),
    }
}

impl<'a> Surgeon<'a> {
    /// A `Do` of a form (object `id`, stream `form`) at `span`.
    pub(super) fn form_placement(
        &mut self,
        id: Option<ObjId>,
        form: &Stream,
        span: (usize, usize),
    ) {
        let placement = form_matrix(self.doc, &form.dict).mul(self.ctm);
        let extent = page_extent(self.doc, &form.dict, placement);
        let Some(area) = visible(self.clip_bbox, extent) else {
            return;
        };
        if !self.forms.descend || !meets_any(self.regions, area) {
            return;
        }
        let Some(id) = id
            .filter(|id| self.forms.depth < MAX_FORM_DEPTH && !self.forms.active.contains(&id.num))
        else {
            self.forms.tally.unreached += 1;
            return;
        };
        let Ok(stream) = ContentStream::from_form(&self.doc.view(), id) else {
            self.forms.tally.unreached += 1;
            return;
        };
        // §8.10.1 / Table 95: the form's own resources; the page's for a
        // form with none.
        let resources = self
            .doc
            .resolve(form.dict.get(b"Resources").unwrap_or(&Object::Null))
            .as_dict()
            .filter(|d| !d.is_empty())
            .cloned()
            .unwrap_or_else(|| self.page_resources.clone());
        let mut child = Surgeon::new(self.doc, &resources, self.regions);
        child.inherit(self, placement, area, id);
        for op in stream.operations() {
            child.operation(&op, &stream.buf);
        }
        child.marked_close_all();
        let view = self.doc.view();
        let image_hits: Vec<ImageHit> = child
            .image_hits
            .iter()
            .filter(|h| redact_image::placement_covers_cells(&view, h, self.regions))
            .cloned()
            .collect();
        let (edits, children) = self.absorb_child(child, &resources, &stream.buf);
        if edits.is_empty() && children.is_empty() && image_hits.is_empty() {
            return;
        }
        let fresh = self.fresh_form_name(id);
        let mut bytes = Vec::with_capacity(fresh.len() + 4);
        bytes.push(b'/');
        bytes.extend_from_slice(&fresh);
        bytes.extend_from_slice(b" Do");
        self.edits.push(Edit {
            start: span.0,
            end: span.1,
            bytes,
        });
        self.forms.tally.redacted += 1;
        self.forms.rewrites.push(FormRewrite {
            name: fresh,
            source: id,
            buf: stream.buf,
            edits,
            image_hits,
            resources,
            children,
        });
    }

    /// Start a form's surgeon in the state in effect at the `Do` (§8.10.1:
    /// the form inherits the graphics state, text state included).
    fn inherit<'p: 'a>(
        &mut self,
        parent: &Surgeon<'p>,
        placement: Mat,
        area: Option<BoxF>,
        id: ObjId,
    ) {
        self.page_resources = parent.page_resources;
        self.ctm = placement;
        self.clip_bbox = area;
        self.line_width = parent.line_width;
        self.font = parent.font.clone();
        self.tf_size = parent.tf_size;
        self.tc = parent.tc;
        self.tw = parent.tw;
        self.th = parent.th;
        self.trise = parent.trise;
        self.tl = parent.tl;
        self.forms.descend = true;
        self.forms.depth = parent.forms.depth + 1;
        self.forms.active = parent.forms.active.clone();
        self.forms.active.push(id.num);
    }

    /// Fold a finished form surgeon's results into this one, and return
    /// its edits and nested rewrites. `resources` and `buf` are what the
    /// child read, recorded with its image placements for the census.
    fn absorb_child(
        &mut self,
        mut child: Surgeon<'_>,
        resources: &Dict,
        buf: &[u8],
    ) -> (Vec<Edit>, Vec<FormRewrite>) {
        self.glyphs_removed += child.glyphs_removed;
        self.replacement_text_stripped += child.replacement_text_stripped;
        self.ops_edited += child.ops_edited;
        for (region, text) in child.removed_by_region {
            self.removed_by_region
                .entry(region)
                .or_default()
                .push_str(&text);
        }
        self.vector_paths_intersecting += child.vector_paths_intersecting;
        self.vector_paths_cut += child.vector_paths_cut;
        self.vector_paths_dropped += child.vector_paths_dropped;
        self.vector_clips_kept += child.vector_clips_kept;
        self.shadings_intersecting += child.shadings_intersecting;
        self.shadings_cut += child.shadings_cut;
        self.estimated_fonts.extend(child.estimated_fonts);
        self.forms.tally.absorb(child.forms.tally);
        self.forms.images.append(&mut child.forms.images);
        if !child.image_hits.is_empty() {
            self.forms.images.push(NestedImages {
                resources: resources.clone(),
                buf: buf.to_vec(),
                hits: std::mem::take(&mut child.image_hits),
            });
        }
        (child.edits, child.forms.rewrites)
    }

    /// A resource name unused by this content's `/XObject` dictionary and
    /// by the copies already bound beside it.
    fn fresh_form_name(&self, id: ObjId) -> Vec<u8> {
        let taken = self
            .resources
            .get(b"XObject")
            .map(|o| self.doc.resolve(o))
            .and_then(Object::as_dict);
        let mut k = self.forms.rewrites.len();
        loop {
            k += 1;
            let name = format!("pdfceRdF{}_{k}", id.num).into_bytes();
            let clash = taken.is_some_and(|d| d.get(&name).is_some())
                || self.forms.rewrites.iter().any(|r| r.name == name);
            if !clash {
                return name;
            }
        }
    }
}

/// Create the copies for `rewrites` (children first) and return the
/// name → reference bindings for the enclosing resources. `made` counts
/// copies per original, for [`retire_originals`]; `img` runs the image
/// surgery for each copy's own placements.
pub(super) fn materialise(
    doc: &Document,
    rewrites: Vec<FormRewrite>,
    alloc_: &mut Allocator<'_>,
    dirty: &mut DirtySet,
    made: &mut BTreeMap<ObjId, usize>,
    img: &mut ImagePass<'_, '_>,
) -> Dict {
    let view = doc.view();
    let mut bindings = Dict::new();
    for rw in rewrites {
        let mut children = materialise(doc, rw.children, alloc_, dirty, made, img);
        let Some(Object::Stream(original)) = view.graph().value(rw.source) else {
            continue;
        };
        let mut found = ImageOutcome::default();
        if !rw.image_hits.is_empty() {
            redact_image::plan_page(
                doc,
                img.view,
                img.page_number,
                &rw.resources,
                &rw.buf,
                &rw.image_hits,
                img.regions,
                img.uses,
                img.covered,
                img.tombstoned,
                img.cache,
                alloc_,
                &mut found,
            );
        }
        let mut edits = rw.edits;
        edits.extend(
            found
                .edits
                .drain(..)
                .map(|(start, end, bytes)| Edit { start, end, bytes }),
        );
        for (id, obj) in found.objects.drain(..) {
            dirty.replace(id, obj);
        }
        for (n, id) in found.bindings.drain(..) {
            children.insert(Name::from(n.as_slice()), Object::Reference(id));
        }
        absorb_outcome(&mut img.out, found);
        let content = splice(&rw.buf, edits);
        let mut dict = stripped(&original.dict);
        if !children.is_empty() || dict.get(b"Resources").is_none() {
            let mut res = rw.resources;
            if !children.is_empty() {
                let mut xobjects = res
                    .get(b"XObject")
                    .map(|o| doc.resolve(o))
                    .and_then(Object::as_dict)
                    .cloned()
                    .unwrap_or_default();
                for (n, v) in children.iter() {
                    xobjects.insert(n.clone(), v.clone());
                }
                res.insert(Name::from(b"XObject"), Object::Dict(xobjects));
            }
            dict.insert(Name::from(b"Resources"), Object::Dict(res));
        }
        let obj = alloc_.flate_stream(dict, &content);
        let id = ObjId::new(alloc(alloc_.next_num), 0);
        dirty.replace(id, obj);
        *made.entry(rw.source).or_insert(0) += 1;
        bindings.insert(Name::from(rw.name.as_slice()), Object::Reference(id));
    }
    bindings
}

/// Add `o`'s counters and notes to `into`.
fn absorb_outcome(into: &mut ImageOutcome, o: ImageOutcome) {
    into.cleared += o.cleared;
    into.removed += o.removed;
    into.cloned_shared += o.cloned_shared;
    into.rotated_overcovered += o.rotated_overcovered;
    into.notes.extend(o.notes);
}

/// A form dictionary without its encoding entries, for re-emission.
fn stripped(dict: &Dict) -> Dict {
    let mut d = dict.clone();
    for key in [&b"Filter"[..], b"DecodeParms", b"DL", b"Length"] {
        d.remove(key);
    }
    d
}

/// Empty every original form whose counted uses (`uses`, from
/// `redact_image::image_use_census`) were all replaced by copies. The
/// census over-counts toward "shared", so a miss keeps an original that an
/// unmarked placement may still paint.
pub(super) fn retire_originals(
    doc: &Document,
    made: &BTreeMap<ObjId, usize>,
    uses: &BTreeMap<ObjId, usize>,
    alloc_: &mut Allocator<'_>,
    dirty: &mut DirtySet,
) -> u64 {
    let view = doc.view();
    let mut retired = 0;
    for (id, n) in made {
        if *n < uses.get(id).copied().unwrap_or(0) {
            continue;
        }
        let Some(Object::Stream(original)) = view.graph().value(*id) else {
            continue;
        };
        dirty.replace(*id, alloc_.flate_stream(stripped(&original.dict), &[]));
        retired += 1;
    }
    retired
}

/// Report what happened to form placements over a region. The residual
/// note keeps the phrase "form XObject overlaps", which the page-region
/// operations match on.
pub(super) fn disclose(tally: FormTally, report: &mut RedactionReport) {
    let FormTally {
        redacted,
        unreached,
    } = tally;
    if redacted > 0 {
        report.note(format!(
            "redaction: content inside {redacted} form XObject placement(s) was redacted; \
             each got its own copy of the form, so other placements are unchanged"
        ));
    }
    if unreached > 0 {
        report.note(format!(
            "redaction: {unreached} form XObject overlaps a redaction region but its content \
             was NOT redacted (nested deeper than {MAX_FORM_DEPTH}, painted inside itself, or \
             an unreadable stream); verify the region by eye or flatten the form first"
        ));
    }
    let action = match (redacted, unreached) {
        (_, r) if r > 0 => CarrierAction::DisclosedNotScrubbed,
        (r, _) if r > 0 => CarrierAction::Scrubbed,
        _ => CarrierAction::Absent,
    };
    report.add_carrier("form_xobjects", redacted + unreached > 0, action);
}
