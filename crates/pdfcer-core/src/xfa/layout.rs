//! XFA layout of template content onto `pageSet` pages (XFA 3.3 Ch.7-8;
//! decision 204, Passes 647.1-647.2).
//!
//! [`layout_template`] is a pure function of the template: it computes where
//! each container lands and on which page, and writes nothing to the
//! document. The result is inferred content (rule 4): a caller that renders
//! or saves it discloses that, and every [`LayoutWarning`] is part of that
//! disclosure, [`LayoutWarning::ContentDiscarded`] above all (gap G-L6).
//!
//! Scope: positioned and flowed (`tb`, `lr-tb`, `rl-tb`) placement, the
//! template's empty merge (`occur@initial` instances; the data merge is Pass
//! 647.3), and ordered-occurrence pagination with `breakBefore`/`breakAfter`.
//! A flowed root moves an object that does not fit the rest of its content
//! area to the next one; a `tb` root splits a `tb` subform across areas
//! between the objects it holds. Fields and draws are not split (their
//! content is not sized yet): one taller than an area overflows it
//! ([`LayoutWarning::ContentOverflow`]). Table strategies are Pass 647.7:
//! such a container is laid as if positioned and reported as
//! [`LayoutWarning::FlowedLayoutPending`].

mod flow;
mod geometry;
mod pager;
mod split;

use flow::{Flow, is_flow};
use geometry::{Ctx, Placed, instances, is_placeable, lay, shifted};
use pager::{Area, Pager};
use split::split;

use super::{ContainerKind, LayoutStrategy, Template, XfaNode};

/// Pages one layout may produce; past it the rest is discarded and
/// [`LayoutWarning::PageLimit`] reported (`ARCHITECTURE.md` §10).
pub const MAX_LAYOUT_PAGES: usize = 10_000;

/// Containers one layout may place; past it [`LayoutWarning::ItemLimit`].
pub const MAX_LAYOUT_ITEMS: usize = 200_000;

/// Letter, the `medium@stock` default (Ch.17 p.780), in points.
const LETTER: (f64, f64) = (612.0, 792.0);

/// What to do with a negative resolved coordinate, which XFA leaves
/// unspecified (gap G-L1). Reported as [`LayoutWarning::NegativeCoordinate`]
/// either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum NegativeCoordinates {
    /// Keep the template's geometry, even if it lands off the page.
    #[default]
    Keep,
    /// Clamp the coordinate to zero.
    Clamp,
}

/// Settings for [`layout_template`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct LayoutOptions {
    /// Gap G-L1.
    pub negative_coordinates: NegativeCoordinates,
}

impl LayoutOptions {
    /// The defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set [`Self::negative_coordinates`].
    #[must_use]
    pub const fn with_negative_coordinates(mut self, v: NegativeCoordinates) -> Self {
        self.negative_coordinates = v;
        self
    }
}

/// A rectangle in points, origin at the page's top-left, `y` downward (the
/// XFA coordinate system, Ch.2 p.36).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayoutRect {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub w: f64,
    /// Height.
    pub h: f64,
}

/// One placed container.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LayoutItem {
    /// What it is.
    pub kind: ContainerKind,
    /// Its `name`, if any.
    pub name: Option<String>,
    /// Child indices from [`Template::element`] to its template node.
    pub path: Vec<usize>,
    /// Its nominal extent on the page.
    pub rect: LayoutRect,
    /// Page background (direct content of the `pageArea`), drawn under the
    /// foreground.
    pub background: bool,
    /// `false` for `presence="invisible"`: it takes space but does not draw.
    pub visible: bool,
    /// Index into [`LayoutPage::content_areas`] for foreground content.
    pub content_area: Option<usize>,
}

/// One laid-out page.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct LayoutPage {
    /// Path from [`Template::element`] to the `pageArea` it instantiates.
    pub page_area: Vec<usize>,
    /// The `pageArea`'s `name`, if any.
    pub name: Option<String>,
    /// Page width in points, from `medium`.
    pub width: f64,
    /// Page height in points.
    pub height: f64,
    /// The `contentArea` rectangles, in document order.
    pub content_areas: Vec<LayoutRect>,
    /// Placed containers, background first, then in document order.
    pub items: Vec<LayoutItem>,
}

/// Something layout did that the template did not say exactly. Each one is
/// disclosed to the operator by whatever renders or saves the layout.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LayoutWarning {
    /// A `table`/`row` container laid as positioned until Pass 647.7.
    FlowedLayoutPending {
        /// Its template path.
        path: Vec<usize>,
        /// Its strategy.
        strategy: LayoutStrategy,
    },
    /// `pageSet@relation` is a qualified pagination, laid as
    /// ordered-occurrence.
    QualifiedPagination,
    /// A page-background-only `pageArea` with unbounded `occur@max`, treated
    /// as one instance (Ch.8 p.301).
    UnboundedBoilerplate {
        /// Its template path.
        path: Vec<usize>,
    },
    /// A break target names no single `pageArea`/`contentArea`; layout went
    /// to the next one instead (Ch.9 p.361).
    InvalidBreakTarget {
        /// Path of the object carrying the break.
        path: Vec<usize>,
        /// The `target` as written.
        target: String,
    },
    /// A `min*` exceeded its `max*`; the two were swapped (Ch.8 p.276).
    MinGreaterThanMax {
        /// Its template path.
        path: Vec<usize>,
        /// The `min*` attribute.
        attribute: &'static str,
    },
    /// A resolved coordinate was negative (gap G-L1).
    NegativeCoordinate {
        /// Its template path.
        path: Vec<usize>,
    },
    /// A measurement whose unit this layout cannot resolve was ignored.
    UnresolvedMeasurement {
        /// Its template path.
        path: Vec<usize>,
        /// The attribute.
        attribute: &'static str,
    },
    /// The page areas ran out; this top-level object and everything after it
    /// was discarded (Ch.8 p.301, gap G-L6).
    ContentDiscarded {
        /// Its template path.
        path: Vec<usize>,
    },
    /// An object taller than an empty content area of a flowed root, and
    /// not splittable there, was placed past the area's bottom edge (Ch.8
    /// p.295).
    ContentOverflow {
        /// Its template path.
        path: Vec<usize>,
    },
    /// [`MAX_LAYOUT_PAGES`] was reached.
    PageLimit,
    /// [`MAX_LAYOUT_ITEMS`] was reached.
    ItemLimit,
}

/// The result of [`layout_template`].
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct XfaLayout {
    /// The pages, in order.
    pub pages: Vec<LayoutPage>,
    /// What to disclose, in the order it happened, without repeats.
    pub warnings: Vec<LayoutWarning>,
}

/// Why a template could not be laid out at all.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum LayoutError {
    /// The template has no root `subform`.
    #[error("the XFA template has no root subform")]
    NoRootSubform,
    /// The root subform has no `pageSet` with a usable `pageArea` (fatal,
    /// Ch.8 p.296).
    #[error("the XFA template has no page area to lay content into")]
    NoPageArea,
}

/// Lay `template` out (empty merge) per `opts`.
///
/// Start rule (Ch.8 p.296): the root subform's `breakBefore`, then its first
/// child's, then the first `contentArea` of the first `pageArea` of its
/// first `pageSet`. Page areas without a `contentArea` are emitted as
/// background-only pages on the way. When content ends, the current page
/// area and the rest of the current pass are padded to `occur@min`.
///
/// # Errors
///
/// [`LayoutError::NoRootSubform`], [`LayoutError::NoPageArea`].
///
/// # Example
///
/// ```
/// use pdfcer_core::xfa::{LayoutOptions, layout_template, read_template, XdpDocument};
/// let xdp = XdpDocument::parse(
///     r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template>
///     <subform name="f"><pageSet><pageArea>
///       <contentArea x="1in" y="1in" w="6in" h="9in"/></pageArea></pageSet>
///       <field name="a" x="1in" y="2in" w="2in" h="20pt"/>
///     </subform></template></xdp:xdp>"#.to_owned(),
/// )?;
/// let template = read_template(&xdp)?.expect("a template packet");
/// let layout = layout_template(&template, &LayoutOptions::new())?;
/// let field = &layout.pages[0].items[0];
/// assert_eq!((field.rect.x, field.rect.y), (144.0, 216.0));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn layout_template(
    template: &Template,
    opts: &LayoutOptions,
) -> Result<XfaLayout, LayoutError> {
    let root_el = template.element();
    let (ri, root) = root_el
        .children
        .iter()
        .enumerate()
        .find(|(_, c)| c.tag == "subform")
        .ok_or(LayoutError::NoRootSubform)?;
    let (si, set) = root
        .children
        .iter()
        .enumerate()
        .find(|(_, c)| c.tag == "pageSet")
        .ok_or(LayoutError::NoPageArea)?;
    let mut run = Run {
        ctx: Ctx {
            opts,
            warnings: Vec::new(),
            items: 0,
        },
        set,
        set_path: vec![ri, si],
        pager: Pager::new(set, vec![ri, si]),
        pages: Vec::new(),
        filled: Vec::new(),
        area_index: 0,
        exhausted: false,
        root_strategy: root.layout(),
        flow: Flow::new(LayoutStrategy::Position, None),
        flow_key: (0, usize::MAX),
    };
    if !matches!(set.attr("relation"), None | Some("orderedOccurrence")) {
        run.ctx.warn(LayoutWarning::QualifiedPagination);
    }
    if !run.new_page() {
        return Err(LayoutError::NoPageArea);
    }
    let root_path = vec![ri];
    if root.layout() != LayoutStrategy::Position && !is_flow(root.layout()) {
        run.ctx.warn(LayoutWarning::FlowedLayoutPending {
            path: root_path.clone(),
            strategy: root.layout(),
        });
    }
    run.apply_break(root, &root_path, "breakBefore");
    for (i, c) in root.children.iter().enumerate() {
        if is_placeable(c) {
            let mut p = root_path.clone();
            p.push(i);
            run.place_top_level(c, &p);
        }
    }
    run.apply_break(root, &root_path, "breakAfter");
    run.pad_minimums();
    Ok(XfaLayout {
        pages: run.pages,
        warnings: run.ctx.warnings,
    })
}

/// One layout run's page state.
struct Run<'t, 'o> {
    ctx: Ctx<'o>,
    set: &'t XfaNode,
    set_path: Vec<usize>,
    pager: Pager<'t>,
    pages: Vec<LayoutPage>,
    /// Per content area of the current page: has it received content?
    filled: Vec<bool>,
    area_index: usize,
    exhausted: bool,
    root_strategy: LayoutStrategy,
    /// The flow cursor of the content area `flow_key` (page, area) names.
    flow: Flow,
    flow_key: (usize, usize),
}

impl Run<'_, '_> {
    /// Effective `occur@max` of an area (Ch.8 p.301).
    fn max_of(ctx: &mut Ctx<'_>, a: &Area<'_>) -> Option<u32> {
        let max = a.node.occur().and_then(|o| o.max);
        if max.is_none() && a.node.child("contentArea").is_none() {
            ctx.warn(LayoutWarning::UnboundedBoilerplate {
                path: a.path.clone(),
            });
            return Some(1);
        }
        max
    }

    /// Open the next page instance; `false` once page areas are exhausted.
    fn new_page(&mut self) -> bool {
        if self.exhausted {
            return false;
        }
        if self.pages.len() >= MAX_LAYOUT_PAGES {
            self.ctx.warn(LayoutWarning::PageLimit);
            self.exhausted = true;
            return false;
        }
        let ctx = &mut self.ctx;
        let Some(area) = self.pager.advance(&mut |a| Self::max_of(ctx, a)) else {
            self.exhausted = true;
            return false;
        };
        self.open(&area);
        true
    }

    fn open(&mut self, area: &Area<'_>) {
        let page = page_for(&mut self.ctx, area);
        self.filled = vec![false; page.content_areas.len()];
        self.area_index = 0;
        self.pages.push(page);
    }

    /// Make sure the cursor is on a content area, opening pages (background
    /// only ones included) until one has one. `false` once exhausted: a
    /// break that found no page left ends layout (Ch.8 p.301).
    fn ensure_content_area(&mut self) -> bool {
        if self.exhausted {
            return false;
        }
        while self.area_index >= self.filled.len() {
            if !self.new_page() {
                return false;
            }
        }
        true
    }

    fn current_empty(&self) -> bool {
        !self.filled.get(self.area_index).copied().unwrap_or(false)
    }

    fn page_empty(&self) -> bool {
        !self.filled.contains(&true)
    }

    fn place_top_level(&mut self, node: &XfaNode, path: &[usize]) {
        self.apply_break(node, path, "breakBefore");
        let mut placed = Vec::new();
        instances(&mut self.ctx, node, path, self.root_strategy, &mut placed);
        for p in placed {
            if !self.put(p, path) {
                self.ctx.warn(LayoutWarning::ContentDiscarded {
                    path: path.to_vec(),
                });
                return;
            }
        }
        self.apply_break(node, path, "breakAfter");
    }

    /// The current content area's rectangle, opening pages as needed.
    fn content_area(&mut self) -> Option<LayoutRect> {
        if !self.ensure_content_area() {
            return None;
        }
        let ca = self
            .pages
            .last()?
            .content_areas
            .get(self.area_index)
            .copied()?;
        let key = (self.pages.len(), self.area_index);
        if self.flow_key != key {
            self.flow_key = key;
            self.flow = Flow::new(self.root_strategy, Some(ca.w));
        }
        Some(ca)
    }

    /// Put one top-level instance in the current content area. A flowed
    /// root moves it to the next area when it does not fit the rest of a
    /// non-empty one; a `tb` root first splits it there when it can
    /// (splitting needs every ancestor `tb`, Ch.8 p.281). A split part is
    /// one more item per container it cuts, each with the same `path`.
    /// `false` once the page areas are exhausted.
    fn put(&mut self, mut p: Placed, path: &[usize]) -> bool {
        loop {
            let Some(ca) = self.content_area() else {
                return false;
            };
            if !is_flow(self.root_strategy) {
                let at = (p.rect.x, p.rect.y);
                self.emit(p, ca, at);
                return true;
            }
            let at = self.flow.slot(p.rect.w);
            let fits = at.1 + p.rect.h <= ca.h;
            if !fits && self.root_strategy == LayoutStrategy::Tb {
                match split(p, ca.h - at.1) {
                    Ok((head, tail)) => {
                        self.flow.commit(head.rect.w, head.rect.h);
                        self.emit(head, ca, at);
                        self.area_index += 1;
                        p = tail;
                        continue;
                    }
                    Err(whole) => p = whole,
                }
            }
            if !fits && !self.current_empty() {
                self.area_index += 1;
                continue;
            }
            if !fits {
                self.ctx.warn(LayoutWarning::ContentOverflow {
                    path: path.to_vec(),
                });
            }
            self.flow.commit(p.rect.w, p.rect.h);
            self.emit(p, ca, at);
            return true;
        }
    }

    /// Append `p`'s items at `at` in content area `ca` of the current page.
    fn emit(&mut self, p: Placed, ca: LayoutRect, at: (f64, f64)) {
        let index = self.area_index;
        let Some(page) = self.pages.last_mut() else {
            return;
        };
        let before = page.items.len();
        page.items
            .extend(shifted(p.items, ca.x + at.0, ca.y + at.1).map(|mut i| {
                i.content_area = Some(index);
                i
            }));
        if page.items.len() > before
            && let Some(f) = self.filled.get_mut(index)
        {
            *f = true;
        }
    }

    /// `breakBefore`/`breakAfter` (Ch.9 pp.359-362). Without a target, a
    /// `contentArea`/`pageArea` break moves on unless the current one is
    /// still empty and `startNew` is unset (derived reading: an empty region
    /// already is the "new" one).
    fn apply_break(&mut self, node: &XfaNode, path: &[usize], which: &str) {
        let Some(b) = node.child(which) else { return };
        let start_new = b.attr("startNew").map(str::trim) == Some("1");
        let target = b.attr("target").map(str::trim).filter(|t| !t.is_empty());
        match b.attr("targetType").unwrap_or("auto") {
            kind @ ("contentArea" | "pageArea") => {
                if let Some(t) = target {
                    if self.break_to_target(t, kind, start_new) {
                        return;
                    }
                    self.ctx.warn(LayoutWarning::InvalidBreakTarget {
                        path: path.to_vec(),
                        target: t.to_owned(),
                    });
                }
                if kind == "contentArea" {
                    if start_new || !self.current_empty() {
                        self.area_index += 1;
                    }
                } else if start_new || !self.page_empty() {
                    self.new_page();
                }
            }
            kind @ ("pageEven" | "pageOdd") => {
                let want_odd = kind == "pageOdd";
                if !self.page_empty() || start_new {
                    self.new_page();
                }
                // Gap G-L3: the first page is odd.
                while !self.exhausted && (self.pages.len() % 2 == 1) != want_odd {
                    self.new_page();
                }
            }
            _ => {}
        }
    }

    /// A `#id` or plain-name target naming exactly one `pageArea` or
    /// `contentArea` of the root `pageSet`; `false` when it names none or
    /// several. Full SOM resolution is not in this pass.
    fn break_to_target(&mut self, target: &str, kind: &str, start_new: bool) -> bool {
        let base = self.set_path.clone();
        let Some(path) = find_unique(self.set, &base, target, kind) else {
            return false;
        };
        let (area_path, ca_slot) = if kind == "contentArea" {
            let Some((&i, parent)) = path.split_last() else {
                return false;
            };
            let Some(area) = resolve(self.set, &base, parent) else {
                return false;
            };
            let slot = area
                .children
                .iter()
                .take(i)
                .filter(|c| c.tag == "contentArea")
                .count();
            (parent.to_vec(), slot)
        } else {
            (path, 0)
        };
        let on_it = self.pages.last().is_some_and(|p| p.page_area == area_path);
        let stay = on_it
            && !start_new
            && (self.area_index == ca_slot && self.current_empty()
                || kind == "pageArea" && self.page_empty()
                || kind == "contentArea" && ca_slot > self.area_index);
        if !stay {
            let Some(node) = resolve(self.set, &base, &area_path) else {
                return false;
            };
            if self.pages.len() >= MAX_LAYOUT_PAGES {
                self.ctx.warn(LayoutWarning::PageLimit);
                self.exhausted = true;
                return true;
            }
            self.pager.jump(self.set, &area_path);
            self.open(&Area {
                node,
                path: area_path,
            });
        }
        self.area_index = ca_slot;
        true
    }

    /// Pad the current area, then the rest of the current pass, to
    /// `occur@min` instances.
    fn pad_minimums(&mut self) {
        let Some(last) = self.pages.last().map(|p| p.page_area.clone()) else {
            return;
        };
        let base = self.set_path.clone();
        if let Some(area) = resolve(self.set, &base, &last) {
            let min = area.occur().map_or(0, |o| o.min);
            for _ in self.pager.used()..min {
                let a = Area {
                    node: area,
                    path: last.clone(),
                };
                if !self.push_padding(&a) {
                    return;
                }
            }
        }
        for a in self.pager.rest_of_pass() {
            for _ in 0..a.node.occur().map_or(0, |o| o.min) {
                if !self.push_padding(&a) {
                    return;
                }
            }
        }
    }

    fn push_padding(&mut self, a: &Area<'_>) -> bool {
        if self.pages.len() >= MAX_LAYOUT_PAGES {
            self.ctx.warn(LayoutWarning::PageLimit);
            return false;
        }
        self.open(a);
        true
    }
}

/// A page instance of `area`: size from `medium`, content areas, and the
/// page background laid positioned from the page's top-left.
fn page_for(ctx: &mut Ctx<'_>, area: &Area<'_>) -> LayoutPage {
    let (width, height) = medium_size(ctx, area);
    let mut content_areas = Vec::new();
    let mut items = Vec::new();
    for (i, c) in area.node.children.iter().enumerate() {
        let mut p = area.path.clone();
        p.push(i);
        if c.tag == "contentArea" {
            let mut len = |a| ctx.length(c, &p, a).unwrap_or(0.0);
            content_areas.push(LayoutRect {
                x: len("x"),
                y: len("y"),
                w: len("w"),
                h: len("h"),
            });
        } else if is_placeable(c) {
            lay(ctx, c, &p, (0.0, 0.0), &mut items);
        }
    }
    for i in &mut items {
        i.background = true;
    }
    LayoutPage {
        page_area: area.path.clone(),
        name: area.node.name().map(str::to_owned),
        width,
        height,
        content_areas,
        items,
    }
}

/// `medium` short/long edges (default letter), long edge vertical unless
/// `orientation="landscape"` (Ch.17 p.780).
fn medium_size(ctx: &mut Ctx<'_>, area: &Area<'_>) -> (f64, f64) {
    let Some(m) = area.node.child("medium") else {
        return LETTER;
    };
    let stock = stock_size(m.attr("stock").map(str::trim).unwrap_or("letter"));
    let short = ctx.length(m, &area.path, "short").unwrap_or(stock.0);
    let long = ctx.length(m, &area.path, "long").unwrap_or(stock.1);
    if m.attr("orientation").map(str::trim) == Some("landscape") {
        (long, short)
    } else {
        (short, long)
    }
}

/// The few `medium@stock` names in common use; anything else is letter.
fn stock_size(stock: &str) -> (f64, f64) {
    match stock {
        "legal" => (612.0, 1008.0),
        "a4" => (595.276, 841.89),
        "a3" => (841.89, 1190.551),
        "tabloid" | "ledger" => (792.0, 1224.0),
        _ => LETTER,
    }
}

/// The node at `path` (from the template root) inside `set`, whose own path
/// is `base`.
fn resolve<'t>(set: &'t XfaNode, base: &[usize], path: &[usize]) -> Option<&'t XfaNode> {
    let rel = path.strip_prefix(base)?;
    rel.iter().try_fold(set, |n, &i| n.children.get(i))
}

/// The one `kind` element under `set` whose `id` (for `#id`) or `name`
/// equals `target`.
fn find_unique(set: &XfaNode, base: &[usize], target: &str, kind: &str) -> Option<Vec<usize>> {
    let (attr, want) = target
        .strip_prefix('#')
        .map_or(("name", target), |id| ("id", id));
    let mut found = Vec::new();
    let mut stack = vec![(set, base.to_vec())];
    while let Some((n, p)) = stack.pop() {
        for (i, c) in n.children.iter().enumerate() {
            let mut cp = p.clone();
            cp.push(i);
            if c.tag == kind && c.attr(attr) == Some(want) {
                found.push(cp.clone());
            }
            if matches!(c.tag.as_str(), "pageSet" | "pageArea") {
                stack.push((c, cp));
            }
        }
    }
    if found.len() == 1 { found.pop() } else { None }
}

#[cfg(test)]
mod tests;
