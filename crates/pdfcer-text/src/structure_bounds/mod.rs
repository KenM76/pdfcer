//! Painted bounds for structure elements: the area each element's content
//! actually paints — text, filled and stroked paths, images and shadings,
//! clipped — as opposed to [`StructureTree::element_bbox`]'s glyph hulls.
//!
//! # Contract
//!
//! - [`read_painted_bounds`] walks the pages a [`StructureTree`] extracted
//!   and records, per marked-content sequence keyed by (page, content
//!   stream, MCID), the union of what it paints. A form XObject with its
//!   own `BDC`s keys them by the form stream (§14.7.4.2 `/Stm`); a form
//!   without them paints into the invoking sequence (§14.7.4.4).
//! - Nested `BDC`s: paint counts for the innermost sequence that carries an
//!   `/MCID`. ISO 32000-1 §14.7.4 forbids nesting one content item inside
//!   another ("may not"; 2.0 "shall not") and gives no reader rule, so
//!   this is pdfcer's, and matches `text_extract`'s run attribution.
//! - Text contributes the element's run boxes (as `element_bbox` does,
//!   invisible text included), confined to the union of the clips the
//!   sequence's text was shown under.
//! - `/OBJR` to an annotation (or anything else with a `/Rect`) gives that
//!   `/Rect`; to an XObject, where it is placed on its `/Pg` page — an
//!   image's unit square or a form's `/BBox` under the CTM, clipped.
//! - Elements whose content resolves to no bounds are named with a
//!   [`NoBoundsReason`] by [`StructureTree::elements_without_painted_bounds`].
//! - Bounded: the structure walk is the tree's (pre-order indices, visited
//!   set); forms are guarded by object-number cycle detection and
//!   [`MAX_FORM_DEPTH`](pdfcer_model::content::MAX_FORM_DEPTH); each page
//!   executes at most 4,000,000 operations.

mod walk;

use std::collections::HashMap;
use std::fmt;

use pdfcer_model::content::ContentStream;
use pdfcer_model::object::ObjId;
use pdfcer_model::page_tree::{self, Rect};
use pdfcer_model::view::DocumentView;

use crate::structure_tree::{StructKid, StructureTree};
use crate::text_extract::{ContentStreamRef, ExtractError};
use walk::{Clip, PageBounds, union};

/// What [`read_painted_bounds`] found, per page walked.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct PaintedBounds {
    pages: HashMap<usize, PageBounds>,
    /// What the walk could not do.
    pub diagnostics: PaintedDiagnostics,
}

/// Counters from the paint walk.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct PaintedDiagnostics {
    /// Pages walked.
    pub pages_walked: usize,
    /// Pages whose content could not be decoded or tokenized: their
    /// sequences have no non-text bounds.
    pub pages_unreadable: usize,
    /// Pages whose walk stopped at the operation budget.
    pub pages_truncated: usize,
    /// Form invocations skipped at the nesting limit.
    pub form_depth_overflows: usize,
    /// Form invocations skipped because the form was already executing.
    pub form_cycles: usize,
}

/// Why an element has no painted bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum NoBoundsReason {
    /// A content item has no resolvable page (no `/Pg` on it, its element
    /// or any ancestor).
    PageUnresolved,
    /// A marked-content item names an MCID no `BDC` on its page declares.
    McidNotFound,
    /// The content was found but paints nothing (an empty sequence, paint
    /// wholly clipped away, or an `/OBJR` target never placed on its page).
    NothingPainted,
    /// The element and its descendants have no content items at all.
    NoContent,
}

impl fmt::Display for NoBoundsReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::PageUnresolved => "page unresolved",
            Self::McidNotFound => "MCID not found",
            Self::NothingPainted => "nothing painted",
            Self::NoContent => "no content",
        })
    }
}

/// One element with no painted bounds, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct UnboundedElement {
    /// Index into [`StructureTree::elements`].
    pub element: usize,
    /// The most specific reason found among its content items:
    /// `PageUnresolved`, then `McidNotFound`, then `NothingPainted`.
    pub reason: NoBoundsReason,
}

/// Walk the pages `tree` extracted and record what each marked-content
/// sequence paints. `view` must be the view `tree` was read from.
///
/// # Errors
///
/// [`ExtractError::PageTree`] when the page tree cannot be walked. A page
/// whose content cannot be read is counted in
/// [`PaintedDiagnostics::pages_unreadable`], not an error.
///
/// # Examples
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use pdfcer_model::document::Document;
/// use pdfcer_text::structure_bounds::read_painted_bounds;
/// use pdfcer_text::structure_tree::read_structure_tree;
/// use pdfcer_text::text_extract::ExtractOptions;
///
/// let doc = Document::from_bytes(std::fs::read("tagged.pdf")?)?;
/// let tree = read_structure_tree(&doc.view(), &ExtractOptions::default())?;
/// let painted = read_painted_bounds(&doc.view(), &tree)?;
/// for (i, e) in tree.elements.iter().enumerate() {
///     println!("{} {:?}", e.resolved_type, tree.element_painted_bounds(i, &painted));
/// }
/// for u in tree.elements_without_painted_bounds(&painted) {
///     eprintln!("element {}: {}", u.element, u.reason);
/// }
/// # Ok(())
/// # }
/// ```
pub fn read_painted_bounds(
    view: &DocumentView<'_>,
    tree: &StructureTree,
) -> Result<PaintedBounds, ExtractError> {
    let pages = page_tree::pages_in(view)?;
    let mut out = PaintedBounds::default();
    for index in tree.text.pages.iter().map(|p| p.page_index) {
        let Some(page) = pages.get(index) else {
            continue;
        };
        if out.pages.contains_key(&index) {
            continue;
        }
        out.diagnostics.pages_walked += 1;
        let found = match ContentStream::from_page(view, page) {
            Ok(content) => walk::walk_page(view, page, &content),
            Err(_) => {
                out.diagnostics.pages_unreadable += 1;
                PageBounds::default()
            }
        };
        let d = &mut out.diagnostics;
        d.pages_truncated += usize::from(found.budget_exhausted);
        d.form_depth_overflows += found.form_depth_overflows;
        d.form_cycles += found.form_cycles;
        out.pages.insert(index, found);
    }
    Ok(out)
}

impl PaintedBounds {
    /// The non-text paint of one marked-content sequence, clipped; `None`
    /// when it painted none or its page was not walked.
    #[must_use]
    pub fn sequence(&self, page: usize, stream: ContentStreamRef, mcid: u32) -> Option<Rect> {
        self.pages
            .get(&page)?
            .sequences
            .get(&(stream, mcid))
            .copied()
    }

    /// Where the XObject `object` is placed on `page`, clipped (the union
    /// of every placement); `None` when it is not painted there.
    #[must_use]
    pub fn placement(&self, page: usize, object: ObjId) -> Option<Rect> {
        self.pages.get(&page)?.placements.get(&object).copied()
    }
}

/// One content item's outcome.
enum Resolved {
    Bounds(usize, Rect),
    Missing(NoBoundsReason),
    /// On a page the walk did not cover (a page selection).
    Elsewhere,
}

impl StructureTree {
    /// The element's painted extent per page: the union of what its (and
    /// its descendants') content items paint, in page order. Text-run
    /// boxes, paths, images and shadings all count; `/OBJR` gives an
    /// annotation's `/Rect` or an XObject's placement.
    /// [`Self::element_bbox`] is the glyph-hull form.
    #[must_use]
    pub fn element_painted_bounds(
        &self,
        index: usize,
        painted: &PaintedBounds,
    ) -> Vec<(usize, Rect)> {
        let mut by_page: Vec<(usize, Rect)> = Vec::new();
        let mut stack = vec![index];
        while let Some(i) = stack.pop() {
            let Some(e) = self.elements.get(i) else {
                continue;
            };
            for kid in &e.kids {
                match kid {
                    StructKid::Element(c) if *c > i => stack.push(*c),
                    StructKid::Element(_) => {}
                    kid => {
                        if let Resolved::Bounds(p, r) = self.resolve_kid(kid, painted) {
                            add_to_page(&mut by_page, p, r);
                        }
                    }
                }
            }
        }
        by_page.sort_by_key(|(p, _)| *p);
        by_page
    }

    /// Every element whose content resolves to no painted bounds, in
    /// element order. Elements whose content lies only on pages outside a
    /// [`read_structure_tree_in_pages`](crate::structure_tree::read_structure_tree_in_pages)
    /// selection are not listed.
    #[must_use]
    pub fn elements_without_painted_bounds(
        &self,
        painted: &PaintedBounds,
    ) -> Vec<UnboundedElement> {
        // Kids always have larger indices, so a reverse pass sees every
        // child before its parent.
        let mut acc: Vec<Summary> = vec![Summary::default(); self.elements.len()];
        for i in (0..self.elements.len()).rev() {
            let mut s = Summary::default();
            for kid in self.elements.get(i).map_or(&[][..], |e| &e.kids[..]) {
                match kid {
                    StructKid::Element(c) if *c > i => {
                        if let Some(child) = acc.get(*c) {
                            s.merge(*child);
                        }
                    }
                    StructKid::Element(_) => {}
                    kid => s.add(self.resolve_kid(kid, painted)),
                }
            }
            if let Some(slot) = acc.get_mut(i) {
                *slot = s;
            }
        }
        acc.iter()
            .enumerate()
            .filter_map(|(element, s)| {
                s.reason()
                    .map(|reason| UnboundedElement { element, reason })
            })
            .collect()
    }

    fn resolve_kid(&self, kid: &StructKid, painted: &PaintedBounds) -> Resolved {
        match kid {
            StructKid::MarkedContent {
                page_index: Some(p),
                stream,
                mcid,
                runs,
                declared,
            } => {
                let Some(found) = painted.pages.get(p) else {
                    return Resolved::Elsewhere;
                };
                let key = (*stream, *mcid);
                let clip = found
                    .text_clips
                    .get(&key)
                    .copied()
                    .unwrap_or(Clip::Unbounded);
                let text = self
                    .text
                    .pages
                    .iter()
                    .find(|pg| pg.page_index == *p)
                    .into_iter()
                    .flat_map(|page| runs.iter().filter_map(|&r| page.runs.get(r)))
                    .filter_map(|r| r.bbox)
                    .reduce(union)
                    .and_then(|r| clip.apply(r));
                let paint = found.sequences.get(&key).copied();
                match (text, paint) {
                    (Some(a), Some(b)) => Resolved::Bounds(*p, union(a, b)),
                    (Some(r), None) | (None, Some(r)) => Resolved::Bounds(*p, r),
                    (None, None) if !*declared && !found.declared.contains(&key) => {
                        Resolved::Missing(NoBoundsReason::McidNotFound)
                    }
                    (None, None) => Resolved::Missing(NoBoundsReason::NothingPainted),
                }
            }
            StructKid::Object {
                page_index: Some(p),
                object,
                rect,
                ..
            } => match rect {
                Some(r) => Resolved::Bounds(*p, *r),
                None if !painted.pages.contains_key(p) => Resolved::Elsewhere,
                None => painted
                    .placement(*p, *object)
                    .map_or(Resolved::Missing(NoBoundsReason::NothingPainted), |r| {
                        Resolved::Bounds(*p, r)
                    }),
            },
            _ => Resolved::Missing(NoBoundsReason::PageUnresolved),
        }
    }
}

fn add_to_page(by_page: &mut Vec<(usize, Rect)>, p: usize, r: Rect) {
    match by_page.iter_mut().find(|(q, _)| *q == p) {
        Some((_, u)) => *u = union(*u, r),
        None => by_page.push((p, r)),
    }
}

/// A subtree's outcome, for [`StructureTree::elements_without_painted_bounds`].
#[derive(Debug, Clone, Copy, Default)]
struct Summary {
    bounded: bool,
    content: bool,
    page_unresolved: bool,
    mcid_not_found: bool,
    nothing_painted: bool,
}

impl Summary {
    fn add(&mut self, r: Resolved) {
        self.content = true;
        match r {
            Resolved::Bounds(..) | Resolved::Elsewhere => self.bounded = true,
            Resolved::Missing(NoBoundsReason::PageUnresolved) => self.page_unresolved = true,
            Resolved::Missing(NoBoundsReason::McidNotFound) => self.mcid_not_found = true,
            Resolved::Missing(_) => self.nothing_painted = true,
        }
    }

    fn merge(&mut self, o: Self) {
        self.bounded |= o.bounded;
        self.content |= o.content;
        self.page_unresolved |= o.page_unresolved;
        self.mcid_not_found |= o.mcid_not_found;
        self.nothing_painted |= o.nothing_painted;
    }

    fn reason(self) -> Option<NoBoundsReason> {
        if self.bounded {
            return None;
        }
        Some(if self.page_unresolved {
            NoBoundsReason::PageUnresolved
        } else if self.mcid_not_found {
            NoBoundsReason::McidNotFound
        } else if self.nothing_painted {
            NoBoundsReason::NothingPainted
        } else {
            NoBoundsReason::NoContent
        })
    }
}
