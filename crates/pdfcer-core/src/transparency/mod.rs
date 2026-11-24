//! Where a page uses transparency (ISO 32000-2 §11), found by a resource
//! scan, and the types [`crate::edit::EditSession::flatten_transparency`]
//! takes and reports.
//!
//! The scan reads dictionaries only; it never decodes a content stream. A
//! graphics state that is declared but never selected is still reported,
//! so a clean result means "nothing on the page can paint transparently"
//! and a non-empty one means "something might". What counts, by clause:
//!
//! - `/CA` or `/ca` below 1 in an `ExtGState` (§11.6.4.4).
//! - A `/BM` other than `Normal` or `Compatible` (§11.3.5, Table 134).
//! - An `ExtGState` `/SMask` other than `/None` (§11.6.5.2).
//! - A form XObject or page whose `/Group` has `/S /Transparency`
//!   (§11.6.6, §11.4.7).
//! - An image with `/SMask`, or a JPX image with a nonzero `/SMaskInData`
//!   (§11.6.5.3, §11.6.4.3).
//! - An annotation with `/CA` below 1 or a non-`Normal` `/BM`
//!   (§12.5.2 Table 166), or whose normal appearance uses any of the above.
//!
//! Forms, tiling patterns, shading patterns' `/ExtGState` and Type 3 fonts'
//! `/Resources` are followed, to [`MAX_DEPTH`] levels, each object once.

mod flatten;

use std::collections::BTreeSet;

pub use flatten::{
    FlattenFailure, FlattenMode, FlattenOptions, FlattenTransparencyReport, FlattenedPage,
    PageFlattenReport, RasterPatch, RasterReason, RasterisedRegion, TransparencyFlattener,
};

use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::{Page, PageTreeError};
use crate::view::DocumentView;

/// How deep the scan follows resources inside resources.
pub const MAX_DEPTH: usize = 32;

/// One kind of transparency.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum TransparencyKind {
    /// Stroking constant alpha `/CA` below 1 (§11.6.4.4).
    StrokeAlpha(f64),
    /// Non-stroking constant alpha `/ca` below 1 (§11.6.4.4).
    FillAlpha(f64),
    /// A blend mode other than `Normal` or `Compatible`, by name (§11.3.5).
    BlendMode(Vec<u8>),
    /// A graphics-state soft mask (§11.6.5.2).
    SoftMask,
    /// A transparency group (§11.6.6; on the page itself, §11.4.7).
    Group,
    /// An image's own soft-mask image (§11.6.5.3).
    ImageSoftMask,
    /// A JPX image whose soft mask is in its data (§11.6.4.3).
    ImageSoftMaskInData,
}

/// The object that carries a [`TransparencyKind`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum TransparencyHolder {
    /// The page dictionary.
    Page,
    /// An `ExtGState` dictionary; `None` when it is direct.
    GraphicsState(Option<ObjId>),
    /// A form XObject.
    Form(ObjId),
    /// An image XObject.
    Image(ObjId),
    /// An annotation dictionary, or something its appearance uses.
    Annotation(ObjId),
}

/// One place a page uses transparency.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct TransparencySite {
    /// What was found.
    pub kind: TransparencyKind,
    /// The object that carries it.
    pub holder: TransparencyHolder,
    /// The resource name it is listed under, when it is a named resource.
    pub resource_name: Option<Vec<u8>>,
}

/// Everything [`has_transparency`] found on one page.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct TransparencyUse {
    /// Each place found, page content first, then annotations.
    pub sites: Vec<TransparencySite>,
    /// Resources the scan stopped at: deeper than [`MAX_DEPTH`].
    pub too_deep: usize,
}

impl TransparencyUse {
    /// True when nothing was found.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sites.is_empty()
    }

    /// True when the page's own content (not an annotation) uses
    /// transparency.
    #[must_use]
    pub fn in_page_content(&self) -> bool {
        self.sites
            .iter()
            .any(|s| !matches!(s.holder, TransparencyHolder::Annotation(_)))
    }

    /// The annotations that use transparency, each once, in `/Annots`
    /// order.
    #[must_use]
    pub fn annotations(&self) -> Vec<ObjId> {
        let mut out: Vec<ObjId> = Vec::new();
        for s in &self.sites {
            if let TransparencyHolder::Annotation(id) = s.holder
                && !out.contains(&id)
            {
                out.push(id);
            }
        }
        out
    }
}

/// Why [`has_transparency`] could not scan.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum TransparencyScanError {
    /// The page tree could not be read.
    #[error("the page tree could not be read: {0}")]
    PageTree(#[from] PageTreeError),
    /// `index` is past the last page.
    #[error("page index {index} is out of range (the document has {count} pages)")]
    PageOutOfRange {
        /// The 0-based index asked for.
        index: usize,
        /// The page count.
        count: usize,
    },
}

/// Find where page `page` (0-based) uses transparency.
///
/// # Errors
///
/// [`TransparencyScanError`] when the page tree cannot be read or `page`
/// is out of range.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::transparency::has_transparency;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec(),
/// )?;
/// assert!(has_transparency(&doc.view(), 0)?.is_empty());
/// # Ok(())
/// # }
/// ```
pub fn has_transparency(
    view: &DocumentView<'_>,
    page: usize,
) -> Result<TransparencyUse, TransparencyScanError> {
    let pages = crate::page_tree::pages_in(view.graph())?;
    let count = pages.len();
    let p = pages
        .get(page)
        .ok_or(TransparencyScanError::PageOutOfRange { index: page, count })?;
    Ok(scan_page(view.graph(), p))
}

/// [`has_transparency`] over an already-resolved page.
pub(crate) fn scan_page<G: ObjectGraph + ?Sized>(graph: &G, page: &Page) -> TransparencyUse {
    let mut scan = Scan {
        graph,
        seen: BTreeSet::new(),
        out: TransparencyUse::default(),
        holder: None,
    };
    if let Some(page_dict) = graph.resolved(page.id).as_dict()
        && is_transparency_group(graph, page_dict.get(b"Group"))
    {
        scan.push(TransparencyKind::Group, TransparencyHolder::Page, None);
    }
    scan.resources(&page.resources, 0);
    for annot in crate::annot::page_annotations(graph, page.id) {
        if let Some(id) = annot.id {
            scan.annotation(id);
        }
    }
    scan.out
}

/// Whether `group` is a group attributes dictionary with `/S /Transparency`.
pub(crate) fn is_transparency_group<G: ObjectGraph + ?Sized>(
    graph: &G,
    group: Option<&Object>,
) -> bool {
    group
        .map(|g| graph.resolve(g))
        .and_then(Object::as_dict)
        .and_then(|d| d.get(b"S"))
        .map(|s| graph.resolve(s))
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"Transparency")
}

/// The transparency an `ExtGState` dictionary sets, in key order
/// `CA`, `ca`, `BM`, `SMask`.
pub(crate) fn gstate_kinds<G: ObjectGraph + ?Sized>(graph: &G, gs: &Dict) -> Vec<TransparencyKind> {
    let mut out = Vec::new();
    let num = |k: &[u8]| gs.get(k).and_then(|o| graph.resolve(o).as_number());
    if let Some(a) = num(b"CA").filter(|a| *a < 1.0) {
        out.push(TransparencyKind::StrokeAlpha(a));
    }
    if let Some(a) = num(b"ca").filter(|a| *a < 1.0) {
        out.push(TransparencyKind::FillAlpha(a));
    }
    if let Some(bm) = gs.get(b"BM").and_then(|o| blend_name(graph, o)) {
        out.push(TransparencyKind::BlendMode(bm));
    }
    if gs.get(b"SMask").is_some_and(|m| is_soft_mask(graph, m)) {
        out.push(TransparencyKind::SoftMask);
    }
    out
}

/// The blend mode `bm` selects when it is not `Normal` or `Compatible`.
/// An array (§11.6.3, deprecated in PDF 2.0) is reported by its first
/// non-normal entry.
fn blend_name<G: ObjectGraph + ?Sized>(graph: &G, bm: &Object) -> Option<Vec<u8>> {
    let normal = |n: &[u8]| n == b"Normal" || n == b"Compatible";
    match graph.resolve(bm) {
        Object::Name(n) if !normal(n.as_bytes()) => Some(n.as_bytes().to_vec()),
        Object::Array(a) => a
            .iter()
            .filter_map(|o| graph.resolve(o).as_name())
            .find(|n| !normal(n.as_bytes()))
            .map(|n| n.as_bytes().to_vec()),
        _ => None,
    }
}

/// Whether an `/SMask` value is a mask rather than `/None`.
fn is_soft_mask<G: ObjectGraph + ?Sized>(graph: &G, mask: &Object) -> bool {
    match graph.resolve(mask) {
        Object::Name(n) => n.as_bytes() != b"None",
        Object::Null => false,
        _ => true,
    }
}

/// The scan's state: what it has visited, what it found, and which
/// annotation (if any) the current resources belong to.
struct Scan<'g, G: ObjectGraph + ?Sized> {
    graph: &'g G,
    seen: BTreeSet<ObjId>,
    out: TransparencyUse,
    holder: Option<ObjId>,
}

impl<G: ObjectGraph + ?Sized> Scan<'_, G> {
    /// Record one site, attributing it to the current annotation if any.
    fn push(&mut self, kind: TransparencyKind, holder: TransparencyHolder, name: Option<&[u8]>) {
        let holder = self.holder.map_or(holder, TransparencyHolder::Annotation);
        self.out.sites.push(TransparencySite {
            kind,
            holder,
            resource_name: name.map(<[u8]>::to_vec),
        });
    }

    /// The sub-dictionary `key` of `res`, resolved.
    fn sub(&self, res: &Dict, key: &[u8]) -> Vec<(Vec<u8>, Object)> {
        res.get(key)
            .map(|o| self.graph.resolve(o))
            .and_then(Object::as_dict)
            .map(|d| {
                d.iter()
                    .map(|(n, o)| (n.as_bytes().to_vec(), o.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// True the first time `obj` (when indirect) is reached.
    fn first_visit(&mut self, obj: &Object) -> bool {
        obj.as_reference().is_none_or(|id| self.seen.insert(id))
    }

    /// Scan a resource dictionary.
    fn resources(&mut self, res: &Dict, depth: usize) {
        if depth > MAX_DEPTH {
            self.out.too_deep += 1;
            return;
        }
        for (name, gs) in self.sub(res, b"ExtGState") {
            self.gstate(Some(&name), &gs);
        }
        for (name, x) in self.sub(res, b"XObject") {
            self.xobject(Some(&name), &x, depth);
        }
        for (_, pat) in self.sub(res, b"Pattern") {
            self.nested(&pat, depth, |d| d.get(b"ExtGState").cloned());
        }
        for (_, font) in self.sub(res, b"Font") {
            self.nested(&font, depth, |_| None);
        }
    }

    /// Scan one named `ExtGState`.
    fn gstate(&mut self, name: Option<&[u8]>, gs: &Object) {
        let id = gs.as_reference();
        let Some(dict) = self.graph.resolve(gs).as_dict() else {
            return;
        };
        for kind in gstate_kinds(self.graph, dict) {
            self.push(kind, TransparencyHolder::GraphicsState(id), name);
        }
    }

    /// Scan one XObject: an image's masks, or a form's group and resources.
    fn xobject(&mut self, name: Option<&[u8]>, x: &Object, depth: usize) {
        let Some(id) = x.as_reference() else {
            return;
        };
        if !self.seen.insert(id) {
            return;
        }
        let Some(dict) = self.graph.resolved(id).as_dict() else {
            return;
        };
        let subtype = dict.get(b"Subtype").and_then(Object::as_name);
        match subtype.map(|n| n.as_bytes()) {
            Some(b"Image") => {
                if dict
                    .get(b"SMask")
                    .is_some_and(|m| is_soft_mask(self.graph, m))
                {
                    self.push(
                        TransparencyKind::ImageSoftMask,
                        TransparencyHolder::Image(id),
                        name,
                    );
                }
                let in_data = dict
                    .get(b"SMaskInData")
                    .and_then(|o| self.graph.resolve(o).as_int());
                if in_data.is_some_and(|v| v != 0) {
                    self.push(
                        TransparencyKind::ImageSoftMaskInData,
                        TransparencyHolder::Image(id),
                        name,
                    );
                }
            }
            // An appearance stream may omit `/Subtype` (§12.5.5).
            Some(b"Form") | None => {
                if is_transparency_group(self.graph, dict.get(b"Group")) {
                    self.push(TransparencyKind::Group, TransparencyHolder::Form(id), name);
                }
                if let Some(res) = dict
                    .get(b"Resources")
                    .map(|o| self.graph.resolve(o))
                    .and_then(Object::as_dict)
                    .cloned()
                {
                    self.resources(&res, depth + 1);
                }
            }
            _ => {}
        }
    }

    /// Follow a pattern's or Type 3 font's `/Resources`, plus the extra
    /// `ExtGState` a shading pattern carries (`extra`).
    fn nested(&mut self, obj: &Object, depth: usize, extra: impl Fn(&Dict) -> Option<Object>) {
        if !self.first_visit(obj) {
            return;
        }
        let Some(dict) = self.graph.resolve(obj).as_dict().cloned() else {
            return;
        };
        if let Some(gs) = extra(&dict) {
            self.gstate(None, &gs);
        }
        if let Some(res) = dict
            .get(b"Resources")
            .map(|o| self.graph.resolve(o))
            .and_then(Object::as_dict)
            .cloned()
        {
            self.resources(&res, depth + 1);
        }
    }

    /// Scan one annotation: its own `/CA` and `/BM`, and each appearance
    /// stream of its `/AP` `/N`.
    fn annotation(&mut self, id: ObjId) {
        let Some(dict) = self.graph.resolved(id).as_dict().cloned() else {
            return;
        };
        self.holder = Some(id);
        let alpha = dict
            .get(b"CA")
            .and_then(|o| self.graph.resolve(o).as_number());
        if let Some(a) = alpha.filter(|a| *a < 1.0) {
            self.push(
                TransparencyKind::FillAlpha(a),
                TransparencyHolder::Page,
                None,
            );
        }
        if let Some(bm) = dict.get(b"BM").and_then(|o| blend_name(self.graph, o)) {
            self.push(
                TransparencyKind::BlendMode(bm),
                TransparencyHolder::Page,
                None,
            );
        }
        for ap in self.normal_appearances(&dict) {
            self.xobject(None, &Object::Reference(ap), 0);
        }
        self.holder = None;
    }

    /// The stream ids of an annotation's `/AP` `/N`: one stream, or each
    /// state of a sub-dictionary.
    fn normal_appearances(&self, annot: &Dict) -> Vec<ObjId> {
        let n = annot
            .get(b"AP")
            .map(|o| self.graph.resolve(o))
            .and_then(Object::as_dict)
            .and_then(|ap| ap.get(b"N"));
        match n {
            Some(Object::Reference(r)) if matches!(self.graph.resolved(*r), Object::Stream(_)) => {
                vec![*r]
            }
            Some(o) => self
                .graph
                .resolve(o)
                .as_dict()
                .map(|d| d.iter().filter_map(|(_, v)| v.as_reference()).collect())
                .unwrap_or_default(),
            None => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod verb_tests;
