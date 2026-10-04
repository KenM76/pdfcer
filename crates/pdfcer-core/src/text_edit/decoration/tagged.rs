//! Records a decoration in the structure tree as the Layout attribute
//! `/TextDecorationType` (ISO 32000-1 §14.8.5.4.4, Table 345) on each
//! structure element whose text it covers completely; every other case is
//! disclosed, never split (decision 188).
//!
//! An element's text is the text-show operators in the marked-content
//! sequences it owns (§14.7.4.2 `/K`, §14.7.4.3 marked-content references),
//! plus those of its nested inline-level elements (§14.8.4.4) at any depth,
//! across pages. A form XObject reference, an unreadable page or an
//! unknown element type counts as not covered. The attribute is derived:
//! each sync rebuilds `/A` from the base revision's `/A`, copy-on-write
//! (an indirect attribute object is never mutated), so a value pdfcer
//! wrote disappears when coverage does.

use std::collections::{BTreeSet, HashMap};

use super::{DECORATION_TAG, DecorationSet, read_set};
use crate::content::{ContentStream, ContentTokenKind};
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::page_tree::Page;
use crate::view::DocumentView;

/// The recursion and chain limit for number trees, `/K`, `/P` and role maps.
const MAX_DEPTH: usize = 64;

/// What one sync of a page produced: replacement structure elements (whole
/// dictionaries, `/A` rebuilt) and operator-facing notes.
#[derive(Debug, Default)]
pub(crate) struct StructureSync {
    pub(crate) writes: Vec<(ObjId, Object)>,
    pub(crate) notes: Vec<String>,
}

/// Text-show counts for one marked-content sequence or for untagged text.
#[derive(Debug, Default, Clone, Copy)]
struct Tally {
    text: usize,
    underline: usize,
    strike: usize,
    unknown: bool,
}

impl Tally {
    fn add(&mut self, other: Tally) {
        self.text += other.text;
        self.underline += other.underline;
        self.strike += other.strike;
        self.unknown |= other.unknown;
    }

    fn full(&self, count: usize) -> bool {
        !self.unknown && self.text > 0 && count == self.text
    }

    fn partial(&self, count: usize) -> bool {
        count > 0 && !self.full(count)
    }
}

/// Per-MCID tallies of one page's content; `None` when it cannot be read.
#[derive(Debug, Default)]
struct PageCoverage {
    mcids: HashMap<i64, Tally>,
    untagged: Tally,
}

enum Frame {
    Mcid(i64),
    Deco(DecorationSet),
    Other,
}

/// How a structure type nests (§14.8.4.4, §14.8.4.3): inline elements are
/// part of their parent's text, block elements are not, anything else is
/// not inspected.
#[derive(Debug, PartialEq, Eq)]
enum Level {
    Inline,
    Block,
    Unknown,
}

const INLINE: &[&[u8]] = &[
    b"Span",
    b"Quote",
    b"Note",
    b"Reference",
    b"BibEntry",
    b"Code",
    b"Link",
    b"Annot",
    b"Ruby",
    b"RB",
    b"RT",
    b"RP",
    b"Warichu",
    b"WT",
    b"WP",
    b"Em",
    b"Strong",
    b"Sub",
];
const BLOCK: &[&[u8]] = &[
    b"P", b"H", b"H1", b"H2", b"H3", b"H4", b"H5", b"H6", b"L", b"LI", b"Lbl", b"LBody", b"Table",
];

/// The structure-tree notes and element rewrites for page `index` of
/// `pages`, read through `graph` (the current state); `base` returns an
/// object's value in the base revision.
pub(crate) fn sync_page<'g>(
    graph: &'g DocumentView<'g>,
    base: &dyn Fn(ObjId) -> Option<Object>,
    pages: &[Page],
    index: usize,
) -> StructureSync {
    let mut out = StructureSync::default();
    let Some(root) = struct_tree_root(graph) else {
        return out;
    };
    let Some(page) = pages.get(index) else {
        return out;
    };
    let number = index + 1;
    let mut tree = Tree {
        graph,
        root,
        pages,
        coverage: HashMap::new(),
    };
    let untagged = tree.coverage_of(page.id).map(|c| c.untagged);
    if let Some(t) = untagged {
        for (count, name) in [(t.underline, "underline"), (t.strike, "line-through")] {
            if count > 0 {
                out.notes.push(format!(
                    "{name} on page {number} is not recorded in the structure tree: \
                     this text is not tagged"
                ));
            }
        }
    }
    for element in tree.owners(page.id) {
        tree.sync_element(element, base, number, &mut out);
    }
    out.notes.dedup();
    out
}

fn struct_tree_root<G: ObjectGraph + ?Sized>(graph: &G) -> Option<&Dict> {
    let root = graph.catalog_dict()?.get(b"StructTreeRoot")?;
    graph.resolve(root).as_dict()
}

struct Tree<'g> {
    graph: &'g DocumentView<'g>,
    root: &'g Dict,
    pages: &'g [Page],
    coverage: HashMap<ObjId, Option<PageCoverage>>,
}

impl Tree<'_> {
    /// The tallies of page `id`'s content, computed once per sync.
    fn coverage_of(&mut self, id: ObjId) -> Option<&PageCoverage> {
        if !self.coverage.contains_key(&id) {
            let computed = self
                .pages
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| page_coverage(self.graph, p));
            self.coverage.insert(id, computed);
        }
        self.coverage.get(&id).and_then(Option::as_ref)
    }

    /// The structure elements page `id`'s parent-tree entry lists
    /// (§14.7.4.4), each once, in order.
    fn owners(&self, id: ObjId) -> Vec<ObjId> {
        let g = self.graph;
        let Some(key) = g
            .value(id)
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"StructParents"))
            .and_then(|o| g.resolve(o).as_int())
        else {
            return Vec::new();
        };
        let Some(tree) = self.root.get(b"ParentTree") else {
            return Vec::new();
        };
        let mut visited = BTreeSet::new();
        let Some(entry) = number_tree_get(g, tree, key, 0, &mut visited) else {
            return Vec::new();
        };
        let mut seen = BTreeSet::new();
        g.resolve(entry)
            .as_array()
            .unwrap_or_default()
            .iter()
            .filter_map(Object::as_reference)
            .filter(|r| seen.insert(*r))
            .collect()
    }

    /// Sum of the tallies of every marked-content sequence element
    /// `element` owns, directly or through inline descendants.
    fn element_tally(&mut self, element: &Dict, element_id: ObjId) -> Tally {
        let mut walk = Walk {
            graph: self.graph,
            role_map: self
                .root
                .get(b"RoleMap")
                .and_then(|m| self.graph.resolve(m).as_dict()),
            visited: BTreeSet::from([element_id]),
            leaves: Vec::new(),
        };
        let page = own_page(self.graph, element);
        if let Some(k) = element.get(b"K") {
            walk.kids(k, page, 0);
        }
        let mut total = Tally::default();
        for leaf in walk.leaves {
            let Some((page, mcid)) = leaf else {
                total.unknown = true;
                continue;
            };
            match self.coverage_of(page) {
                Some(c) => total.add(c.mcids.get(&mcid).copied().unwrap_or_default()),
                None => total.unknown = true,
            }
        }
        total
    }

    /// Rebuild element `id`'s `/A` from its coverage, noting what is not
    /// recorded.
    fn sync_element(
        &mut self,
        id: ObjId,
        base: &dyn Fn(ObjId) -> Option<Object>,
        page: usize,
        out: &mut StructureSync,
    ) {
        let Some(current) = self.graph.value(id).and_then(Object::as_dict).cloned() else {
            return;
        };
        let tally = self.element_tally(&current, id);
        let base_dict = base(id)
            .and_then(|o| o.as_dict().cloned())
            .unwrap_or_else(|| current.clone());
        let base_a = base_dict.get(b"A");
        let authored = base_a.and_then(|a| authored_decoration(self.graph, a));
        let kind = self.type_name(&current);
        let (full_u, full_s) = (tally.full(tally.underline), tally.full(tally.strike));
        let desired: Option<&[u8]> = if full_s {
            Some(b"LineThrough")
        } else if full_u {
            Some(b"Underline")
        } else {
            None
        };
        if full_u && full_s {
            out.notes.push(format!(
                "the structure tree records line-through only on a <{kind}> element on page \
                 {page}; the underline is not recorded (PDF allows one decoration type per element)"
            ));
        }
        for (count, name, value) in [
            (tally.underline, "underline", &b"Underline"[..]),
            (tally.strike, "line-through", &b"LineThrough"[..]),
        ] {
            if !tally.partial(count) {
                continue;
            }
            if authored.as_deref() == Some(value) {
                out.notes.push(format!(
                    "the structure tree still marks a <{kind}> element on page {page} as \
                     {name}, but part of its text no longer is"
                ));
            } else {
                out.notes.push(format!(
                    "the structure tree does not record {name} on page {page}: it covers part \
                     of a <{kind}> element; pdfcer does not split structure elements"
                ));
            }
        }
        let a = if authored.as_deref() == desired {
            base_a.cloned()
        } else {
            let revision = base_dict.get(b"R").and_then(Object::as_int).unwrap_or(0);
            derived_attributes(base_a, desired, revision)
        };
        let mut next = current.clone();
        match a {
            Some(a) => {
                next.insert(Name::from(b"A"), a);
            }
            None => {
                next.remove(b"A");
            }
        }
        if next != current {
            out.writes.push((id, Object::Dict(next)));
        }
    }

    /// The element's `/S`, through the role map to a standard type when
    /// one is reached.
    fn type_name(&self, element: &Dict) -> String {
        let role_map = self
            .root
            .get(b"RoleMap")
            .and_then(|m| self.graph.resolve(m).as_dict());
        let name = standard_type(self.graph, role_map, element).unwrap_or_default();
        String::from_utf8_lossy(&name).into_owned()
    }
}

/// The value at `key` in the number tree `node` (§7.9.7), guarded against
/// depth and reference cycles.
fn number_tree_get<'g, G: ObjectGraph + ?Sized>(
    g: &'g G,
    node: &'g Object,
    key: i64,
    depth: usize,
    visited: &mut BTreeSet<ObjId>,
) -> Option<&'g Object> {
    if depth > MAX_DEPTH {
        return None;
    }
    if let Some(r) = node.as_reference()
        && !visited.insert(r)
    {
        return None;
    }
    let dict = g.resolve(node).as_dict()?;
    if let Some(nums) = dict.get(b"Nums").and_then(|n| g.resolve(n).as_array()) {
        let mut pairs = nums.iter();
        while let (Some(k), Some(v)) = (pairs.next(), pairs.next()) {
            if g.resolve(k).as_int() == Some(key) {
                return Some(v);
            }
        }
    }
    let kids = dict.get(b"Kids").and_then(|k| g.resolve(k).as_array())?;
    kids.iter().find_map(|kid| {
        let limits = g
            .resolve(kid)
            .as_dict()
            .and_then(|d| d.get(b"Limits"))
            .and_then(|l| g.resolve(l).as_array());
        if let Some([lo, hi]) = limits {
            let (lo, hi) = (g.resolve(lo).as_int()?, g.resolve(hi).as_int()?);
            if key < lo || key > hi {
                return None;
            }
        }
        number_tree_get(g, kid, key, depth + 1, visited)
    })
}

/// The tallies of every text-show operator on `page`, by the innermost
/// enclosing MCID; `None` when its content cannot be read.
fn page_coverage(g: &DocumentView<'_>, page: &Page) -> Option<PageCoverage> {
    let cs = ContentStream::from_page(g, page).ok()?;
    let buf = cs.buf.as_slice();
    let properties = page
        .resources
        .get(b"Properties")
        .and_then(|p| g.resolve(p).as_dict());
    let mut out = PageCoverage::default();
    let mut stack: Vec<Frame> = Vec::new();
    for op in cs.operations() {
        let name = op.operator_name(buf);
        match name {
            Some(b"BDC") => stack.push(open_frame(g, properties, op.operands)),
            Some(b"BMC") => stack.push(Frame::Other),
            Some(b"EMC") => {
                stack.pop();
            }
            Some(b"Tj" | b"TJ" | b"'" | b"\"" | b"Do") => {
                let mut set = DecorationSet::NONE;
                for frame in &stack {
                    if let Frame::Deco(s) = frame {
                        set.underline |= s.underline;
                        set.strikethrough |= s.strikethrough;
                    }
                }
                let mcid = stack.iter().rev().find_map(|f| match f {
                    Frame::Mcid(m) => Some(*m),
                    _ => None,
                });
                let tally = match mcid {
                    Some(m) => out.mcids.entry(m).or_default(),
                    None => &mut out.untagged,
                };
                if name == Some(b"Do") {
                    tally.unknown |= mcid.is_some();
                    continue;
                }
                tally.text += 1;
                tally.underline += usize::from(set.underline);
                tally.strike += usize::from(set.strikethrough);
            }
            _ => {}
        }
    }
    Some(out)
}

/// The frame a `BDC` opens: a decoration marker, an MCID owner (inline
/// property list or a named `/Properties` resource), or anything else.
fn open_frame<G: ObjectGraph + ?Sized>(
    g: &G,
    properties: Option<&Dict>,
    operands: &[crate::content::ContentToken],
) -> Frame {
    let operand = |i: usize| match operands.get(i).map(|t| &t.kind) {
        Some(ContentTokenKind::Operand(o)) => Some(o),
        _ => None,
    };
    let props = match operand(1) {
        Some(Object::Dict(d)) => Some(d),
        Some(Object::Name(n)) => properties
            .and_then(|p| p.get(n.as_bytes()))
            .and_then(|o| g.resolve(o).as_dict()),
        _ => None,
    };
    let Some(props) = props else {
        return Frame::Other;
    };
    let tag = operand(0).and_then(Object::as_name);
    if tag.is_some_and(|t| t.as_bytes() == DECORATION_TAG) {
        let set = read_set(props.get(b"Line"));
        if props.get(b"Rule").is_none() && !set.is_empty() {
            return Frame::Deco(set);
        }
        return Frame::Other;
    }
    match props.get(b"MCID").and_then(|m| g.resolve(m).as_int()) {
        Some(m) => Frame::Mcid(m),
        None => Frame::Other,
    }
}

/// A structure element's `/Pg`, or its nearest ancestor's (§14.7.4.2).
fn own_page<G: ObjectGraph + ?Sized>(g: &G, element: &Dict) -> Option<ObjId> {
    let mut at = element;
    let mut visited = BTreeSet::new();
    for _ in 0..MAX_DEPTH {
        if let Some(pg) = at.get(b"Pg").and_then(Object::as_reference) {
            return Some(pg);
        }
        let parent = at.get(b"P")?;
        if let Some(r) = parent.as_reference()
            && !visited.insert(r)
        {
            return None;
        }
        at = g.resolve(parent).as_dict()?;
    }
    None
}

/// The standard structure type `element`'s `/S` maps to through
/// `role_map` (§14.7.3), or the last name reached.
fn standard_type<G: ObjectGraph + ?Sized>(
    g: &G,
    role_map: Option<&Dict>,
    element: &Dict,
) -> Option<Vec<u8>> {
    let mut name = element.get(b"S").and_then(|s| g.resolve(s).as_name())?;
    for _ in 0..MAX_DEPTH {
        let bytes = name.as_bytes();
        if INLINE.contains(&bytes) || BLOCK.contains(&bytes) {
            break;
        }
        match role_map
            .and_then(|m| m.get(bytes))
            .and_then(|o| g.resolve(o).as_name())
        {
            Some(next) if next != name => name = next,
            _ => break,
        }
    }
    Some(name.as_bytes().to_vec())
}

/// Collects the `(page, MCID)` leaves under an element's `/K`; `None` is
/// a leaf that cannot be inspected.
struct Walk<'g, G: ObjectGraph + ?Sized> {
    graph: &'g G,
    role_map: Option<&'g Dict>,
    visited: BTreeSet<ObjId>,
    leaves: Vec<Option<(ObjId, i64)>>,
}

impl<G: ObjectGraph + ?Sized> Walk<'_, G> {
    fn kids(&mut self, k: &Object, page: Option<ObjId>, depth: usize) {
        if depth > MAX_DEPTH {
            self.leaves.push(None);
            return;
        }
        match k {
            Object::Integer(mcid) => self.leaves.push(page.map(|p| (p, *mcid))),
            Object::Array(items) => {
                for item in items {
                    self.kids(item, page, depth + 1);
                }
            }
            Object::Reference(r) => {
                if !self.visited.insert(*r) {
                    return;
                }
                if let Some(d) = self.graph.resolved(*r).as_dict() {
                    self.kid_dict(d, page, depth);
                }
            }
            Object::Dict(d) => self.kid_dict(d, page, depth),
            _ => {}
        }
    }

    fn kid_dict(&mut self, d: &Dict, page: Option<ObjId>, depth: usize) {
        let page = d.get(b"Pg").and_then(Object::as_reference).or(page);
        let kind = d.get(b"Type").and_then(Object::as_name).map(Name::as_bytes);
        if kind == Some(b"OBJR") {
            return;
        }
        if kind == Some(b"MCR") || (d.get(b"S").is_none() && d.get(b"MCID").is_some()) {
            let mcid = d.get(b"MCID").and_then(|m| self.graph.resolve(m).as_int());
            let leaf = match (d.get(b"Stm"), page, mcid) {
                (None, Some(p), Some(m)) => Some((p, m)),
                _ => None,
            };
            self.leaves.push(leaf);
            return;
        }
        let level = match standard_type(self.graph, self.role_map, d) {
            Some(t) if INLINE.contains(&t.as_slice()) => Level::Inline,
            Some(t) if BLOCK.contains(&t.as_slice()) => Level::Block,
            _ => Level::Unknown,
        };
        match level {
            Level::Inline => {
                if let Some(k) = d.get(b"K") {
                    self.kids(k, page, depth + 1);
                }
            }
            Level::Block => {}
            Level::Unknown => self.leaves.push(None),
        }
    }
}

/// The attribute objects in `/A` value `a`, resolved, revision numbers
/// skipped.
fn attribute_objects<'g, G: ObjectGraph + ?Sized>(g: &'g G, a: &'g Object) -> Vec<&'g Dict> {
    let one = |o: &'g Object| match g.resolve(o) {
        Object::Dict(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    };
    match g.resolve(a) {
        Object::Array(items) => items.iter().filter_map(one).collect(),
        _ => one(a).into_iter().collect(),
    }
}

fn is_layout(d: &Dict) -> bool {
    d.get(b"O")
        .and_then(Object::as_name)
        .is_some_and(|o| o.as_bytes() == b"Layout")
}

/// The `/TextDecorationType` a Layout attribute object in `a` declares.
fn authored_decoration<G: ObjectGraph + ?Sized>(g: &G, a: &Object) -> Option<Vec<u8>> {
    attribute_objects(g, a)
        .into_iter()
        .filter(|d| is_layout(d))
        .find_map(|d| d.get(b"TextDecorationType").and_then(Object::as_name))
        .map(|n| n.as_bytes().to_vec())
}

/// `/A` rebuilt from the base value `base` to declare `desired`: merged
/// into a direct Layout dictionary, else appended as a new direct one
/// (followed by `revision` when non-zero, §14.7.5.3); `base` unchanged when
/// nothing is desired.
pub(crate) fn derived_attributes(
    base: Option<&Object>,
    desired: Option<&[u8]>,
    revision: i64,
) -> Option<Object> {
    let Some(value) = desired else {
        return base.cloned();
    };
    let declare = |mut d: Dict| {
        d.insert(
            Name::from(b"TextDecorationType"),
            Object::Name(Name::from(value)),
        );
        Object::Dict(d)
    };
    let mut fresh = Dict::new();
    fresh.insert(Name::from(b"O"), Object::Name(Name::from(b"Layout")));
    let appended = |mut items: Vec<Object>| {
        items.push(declare(fresh.clone()));
        if revision != 0 {
            items.push(Object::Integer(revision));
        }
        Object::Array(items)
    };
    Some(match base {
        None if revision == 0 => declare(fresh.clone()),
        None => appended(Vec::new()),
        Some(Object::Dict(d)) if is_layout(d) => declare(d.clone()),
        Some(Object::Array(items)) => {
            let mut items = items.clone();
            match items
                .iter_mut()
                .find(|o| o.as_dict().is_some_and(is_layout))
            {
                Some(slot) => {
                    if let Object::Dict(d) = slot {
                        *slot = declare(d.clone());
                    }
                    Object::Array(items)
                }
                None => appended(items),
            }
        }
        Some(other) => appended(vec![other.clone()]),
    })
}
