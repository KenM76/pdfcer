//! The structure-tree reader and the checks over it: role mapping,
//! parents, alternate descriptions, headings, notes, language and the
//! table/list/TOC containment rules (ISO 14289-1 §7.1, §7.2, §7.3, §7.4,
//! §7.7, §7.9; ISO 14289-2 §8.2; ISO 32000-1 §14.7, §14.8.4).

use std::collections::{HashMap, HashSet};

use super::{FindingTarget, Out, PdfuaPart, name_of, string_of, text};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::view::DocumentView;

/// Structure elements nest at most this deep.
const MAX_DEPTH: usize = 256;
/// The reader stops after this many elements and reports the tree as
/// incompletely read.
const MAX_ELEMS: usize = 200_000;
/// A role-map chain longer than this is treated as circular.
const MAX_ROLE_STEPS: usize = 64;

/// How an element's `/S` reached a standard type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mapping {
    /// `/S` is itself standard.
    Standard,
    /// `/S` is standard but the role map remaps it (ISO 14289-1 §7.1 ¶5).
    RemappedStandard,
    /// A non-standard `/S` maps, through the role map, to a standard type.
    Mapped,
    /// The role-map chain loops.
    Circular,
    /// The chain ends at a non-standard type.
    Unmapped,
}

/// One structure element.
#[derive(Debug, Clone)]
pub(super) struct Elem {
    pub(super) id: Option<ObjId>,
    /// `/S` as written.
    pub(super) raw: Vec<u8>,
    /// The standard type after role mapping; empty when there is none.
    pub(super) std: Vec<u8>,
    pub(super) mapping: Mapping,
    pub(super) parent: Option<usize>,
    /// Child structure elements, in order.
    pub(super) kids: Vec<usize>,
    /// Marked-content and object-reference kids.
    pub(super) content_kids: usize,
    pub(super) has_p: bool,
    /// A non-empty `/Alt`.
    pub(super) alt: Option<Vec<u8>>,
    pub(super) actual_text: bool,
    pub(super) expansion: bool,
    /// `/ActualText`, then `/Alt`, holds a Private Use Area code point.
    pub(super) private_use: [bool; 2],
    /// The element's own `/Lang`.
    pub(super) lang: Option<Vec<u8>>,
    pub(super) note_id: Option<Vec<u8>>,
    /// An attribute object carries `/Role` (ISO 14289-1 §7.18.4).
    pub(super) role_attr: bool,
    /// Objects referenced through `/OBJR` kids.
    pub(super) objr: Vec<ObjId>,
}

/// The document's structure tree, flattened in pre-order.
#[derive(Debug, Default)]
pub(super) struct Tree {
    /// The catalog has a `/StructTreeRoot` dictionary.
    pub(super) present: bool,
    pub(super) root_kids: Vec<usize>,
    pub(super) elems: Vec<Elem>,
    /// `(page, MCID)` to the element that owns the marked content.
    pub(super) mcids: HashMap<(ObjId, i64), usize>,
    /// An annotation or form XObject to its `/OBJR` parent element.
    pub(super) objr: HashMap<ObjId, usize>,
    /// The depth or element cap stopped the reader.
    pub(super) truncated: bool,
    /// The catalog's `/Lang`, when present and non-empty.
    pub(super) catalog_lang: Option<Vec<u8>>,
}

impl Tree {
    /// The element's own or an ancestor's `/Lang`, or the catalog's.
    pub(super) fn has_lang(&self, mut i: usize) -> bool {
        if self.catalog_lang.is_some() {
            return true;
        }
        for _ in 0..MAX_DEPTH {
            let Some(e) = self.elems.get(i) else {
                return false;
            };
            if e.lang.is_some() {
                return true;
            }
            match e.parent {
                Some(p) => i = p,
                None => return false,
            }
        }
        false
    }

    /// The standard type of element `i`, or `b""`.
    pub(super) fn std(&self, i: usize) -> &[u8] {
        self.elems.get(i).map_or(&[], |e| e.std.as_slice())
    }

    fn parent_std(&self, i: usize) -> &[u8] {
        self.elems
            .get(i)
            .and_then(|e| e.parent)
            .map_or(&[], |p| self.std(p))
    }

    fn kid_types(&self, i: usize) -> Vec<&[u8]> {
        self.elems
            .get(i)
            .map(|e| e.kids.iter().map(|&k| self.std(k)).collect())
            .unwrap_or_default()
    }
}

/// Read the structure tree. Never fails: an absent tree reads as
/// `present == false`, a capped one as `truncated`.
pub(super) fn read(view: &DocumentView<'_>, part: PdfuaPart) -> Tree {
    let mut tree = Tree::default();
    let Some(catalog) = view.graph().catalog_dict() else {
        return tree;
    };
    tree.catalog_lang = string_of(view, catalog.get(b"Lang"))
        .filter(|s| !text(s).is_empty())
        .map(<[u8]>::to_vec);
    let Some(root) = catalog
        .get(b"StructTreeRoot")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
    else {
        return tree;
    };
    tree.present = true;
    let role_map = root
        .get(b"RoleMap")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict);
    let mut reader = Reader {
        view,
        part,
        role_map,
        tree,
        visited: HashSet::new(),
    };
    if let Some(k) = root.get(b"K") {
        reader.walk(k);
    }
    reader.tree
}

struct Reader<'v, 'd> {
    view: &'v DocumentView<'d>,
    part: PdfuaPart,
    role_map: Option<&'v Dict>,
    tree: Tree,
    visited: HashSet<ObjId>,
}

/// One pending kid: the object, its parent element, the inherited `/Pg`
/// and the depth.
type Pending<'v> = (&'v Object, Option<usize>, Option<ObjId>, usize);

impl<'v> Reader<'v, '_> {
    fn walk(&mut self, root_k: &'v Object) {
        let mut stack: Vec<Pending<'v>> = vec![(root_k, None, None, 0)];
        while let Some((obj, parent, page, depth)) = stack.pop() {
            if depth > MAX_DEPTH || self.tree.elems.len() >= MAX_ELEMS {
                self.tree.truncated = true;
                continue;
            }
            let id = obj.as_reference();
            match self.view.resolve(obj) {
                Object::Array(items) => {
                    for item in items.iter().rev() {
                        stack.push((item, parent, page, depth));
                    }
                }
                Object::Integer(mcid) => self.mcid(parent, page, *mcid),
                Object::Dict(d) => self.dict(d, id, parent, page, depth, &mut stack),
                _ => {}
            }
        }
    }

    fn dict(
        &mut self,
        d: &'v Dict,
        id: Option<ObjId>,
        parent: Option<usize>,
        page: Option<ObjId>,
        depth: usize,
        stack: &mut Vec<Pending<'v>>,
    ) {
        let page_of = |d: &Dict| d.get(b"Pg").and_then(Object::as_reference).or(page);
        match name_of(self.view, d.get(b"Type")) {
            Some(b"MCR") => {
                let mcid = d.get(b"MCID").map(|o| self.view.resolve(o));
                if let Some(Object::Integer(n)) = mcid {
                    self.mcid(parent, page_of(d), *n);
                }
                return;
            }
            Some(b"OBJR") => {
                if let (Some(p), Some(target)) =
                    (parent, d.get(b"Obj").and_then(Object::as_reference))
                {
                    self.tree.objr.insert(target, p);
                    if let Some(e) = self.tree.elems.get_mut(p) {
                        e.objr.push(target);
                        e.content_kids += 1;
                    }
                }
                return;
            }
            _ => {}
        }
        if !d.contains_key(b"S") || id.is_some_and(|id| !self.visited.insert(id)) {
            return;
        }
        let index = self.tree.elems.len();
        let elem = self.elem(d, id, parent);
        self.tree.elems.push(elem);
        match parent.and_then(|p| self.tree.elems.get_mut(p)) {
            Some(p) => p.kids.push(index),
            None => self.tree.root_kids.push(index),
        }
        if let Some(k) = d.get(b"K") {
            stack.push((k, Some(index), page_of(d), depth + 1));
        }
    }

    fn mcid(&mut self, parent: Option<usize>, page: Option<ObjId>, mcid: i64) {
        let (Some(p), Some(page)) = (parent, page) else {
            return;
        };
        self.tree.mcids.insert((page, mcid), p);
        if let Some(e) = self.tree.elems.get_mut(p) {
            e.content_kids += 1;
        }
    }

    fn elem(&self, d: &Dict, id: Option<ObjId>, parent: Option<usize>) -> Elem {
        let view = self.view;
        let raw = name_of(view, d.get(b"S")).unwrap_or_default().to_vec();
        let (std, mapping) = self.standard(&raw);
        let alt_raw = string_of(view, d.get(b"Alt"));
        let actual = string_of(view, d.get(b"ActualText"));
        let alt = alt_raw.filter(|s| !text(s).is_empty()).map(<[u8]>::to_vec);
        Elem {
            id,
            raw,
            std,
            mapping,
            parent,
            kids: Vec::new(),
            content_kids: 0,
            has_p: d.contains_key(b"P"),
            alt,
            actual_text: actual.is_some(),
            private_use: [actual, alt_raw].map(|s| s.is_some_and(|s| has_private_use(&text(s)))),
            expansion: string_of(view, d.get(b"E")).is_some(),
            lang: string_of(view, d.get(b"Lang")).map(<[u8]>::to_vec),
            note_id: string_of(view, d.get(b"ID")).map(<[u8]>::to_vec),
            role_attr: has_role_attribute(view, d.get(b"A")),
            objr: Vec::new(),
        }
    }

    /// Follow the role map from `raw` (ISO 32000-1 §14.8.3; ISO 14289-1
    /// §7.1 ¶5–7).
    fn standard(&self, raw: &[u8]) -> (Vec<u8>, Mapping) {
        let lookup = |t: &[u8]| self.role_map.and_then(|m| name_of(self.view, m.get(t)));
        if is_standard(raw, self.part) {
            let mapping = if lookup(raw).is_some() {
                Mapping::RemappedStandard
            } else {
                Mapping::Standard
            };
            return (raw.to_vec(), mapping);
        }
        let mut seen: Vec<&[u8]> = vec![raw];
        let mut current = raw;
        for _ in 0..MAX_ROLE_STEPS {
            let Some(next) = lookup(current) else {
                return (Vec::new(), Mapping::Unmapped);
            };
            if is_standard(next, self.part) {
                return (next.to_vec(), Mapping::Mapped);
            }
            if seen.contains(&next) {
                return (Vec::new(), Mapping::Circular);
            }
            seen.push(next);
            current = next;
        }
        (Vec::new(), Mapping::Circular)
    }
}

fn has_role_attribute(view: &DocumentView<'_>, a: Option<&Object>) -> bool {
    let has = |o: &Object| {
        view.resolve(o)
            .as_dict()
            .is_some_and(|d| d.contains_key(b"Role"))
    };
    match a.map(|o| view.resolve(o)) {
        Some(Object::Array(items)) => items.iter().any(has),
        Some(o) => has(o),
        None => false,
    }
}

/// The standard structure types of ISO 32000-1 §14.8.4 (UA-1) and, for
/// UA-2, also ISO 32000-2 §14.8.4's additions.
const STANDARD_17: &[&[u8]] = &[
    b"Document",
    b"Part",
    b"Art",
    b"Sect",
    b"Div",
    b"BlockQuote",
    b"Caption",
    b"TOC",
    b"TOCI",
    b"Index",
    b"NonStruct",
    b"Private",
    b"P",
    b"H",
    b"L",
    b"LI",
    b"Lbl",
    b"LBody",
    b"Table",
    b"TR",
    b"TH",
    b"TD",
    b"THead",
    b"TBody",
    b"TFoot",
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
    b"Figure",
    b"Formula",
    b"Form",
];
const STANDARD_20: &[&[u8]] = &[
    b"DocumentFragment",
    b"Aside",
    b"Title",
    b"FENote",
    b"Sub",
    b"Em",
    b"Strong",
    b"Artifact",
];

fn is_standard(t: &[u8], part: PdfuaPart) -> bool {
    STANDARD_17.contains(&t)
        || heading_level(t).is_some()
        || (part == PdfuaPart::Ua2 && STANDARD_20.contains(&t))
}

/// `Some(n)` for `Hn`, `n >= 1` written without a leading zero.
pub(super) fn heading_level(t: &[u8]) -> Option<u32> {
    let digits = t.strip_prefix(b"H")?;
    if digits.is_empty() || digits.first() == Some(&b'0') || digits.len() > 4 {
        return None;
    }
    std::str::from_utf8(digits).ok()?.parse().ok()
}

/// ISO 14289-1 §7.2 ¶3 / veraPDF 7.2-29, ISO 14289-2 §8.4.4: a language
/// tag of the shape `/^[a-zA-Z]{1,8}(-[a-zA-Z0-9]{1,8})*$/`.
pub(crate) fn lang_well_formed(tag: &str) -> bool {
    let mut parts = tag.split('-');
    let first_ok = parts
        .next()
        .is_some_and(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphabetic()));
    first_ok
        && parts.all(|p| (1..=8).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// ISO 14289-2 §8.4.3: a Unicode Private Use Area code point.
fn has_private_use(s: &str) -> bool {
    s.chars()
        .any(|c| matches!(c as u32, 0xE000..=0xF8FF | 0xF_0000..=0xF_FFFD | 0x10_0000..=0x10_FFFD))
}

fn target(e: &Elem) -> FindingTarget {
    e.id.map_or(FindingTarget::Document, FindingTarget::StructElem)
}

fn label(e: &Elem) -> String {
    let raw = String::from_utf8_lossy(&e.raw);
    match e.id {
        Some(id) => format!("<{raw}> (object {id})"),
        None => format!("<{raw}>"),
    }
}

/// Every check over the tree.
pub(super) fn check(view: &DocumentView<'_>, tree: &Tree, out: &mut Out) {
    if !tree.present {
        let message = "The catalog has no structure tree, so the document is not tagged.".into();
        out.violation(
            "PUI-V-7.1-11",
            "PUI2-8.2.1-1",
            None,
            FindingTarget::Document,
            message,
        );
        return;
    }
    if tree.truncated {
        let message = format!(
            "The structure tree is deeper than {MAX_DEPTH} levels or larger than {MAX_ELEMS} \
             elements; the rest was not read."
        );
        out.incomplete(
            "PUI-V-7.1-12",
            "PUI2-8.2.1-2",
            None,
            FindingTarget::Document,
            message,
        );
    }
    if out.part == PdfuaPart::Ua2 {
        document_root(tree, out);
    }
    if let Some(lang) = &tree.catalog_lang {
        lang_shape(&text(lang), "the catalog", FindingTarget::Document, out);
    }
    let mut note_ids: HashMap<&[u8], usize> = HashMap::new();
    for e in &tree.elems {
        if let Some(id) = &e.note_id {
            *note_ids.entry(id.as_slice()).or_default() += 1;
        }
    }
    for (i, e) in tree.elems.iter().enumerate() {
        element(tree, i, e, out);
        notes(e, &note_ids, out);
        super::annots::form_element(view, e, out);
        if out.part == PdfuaPart::Ua1 {
            for (row, reasons) in containment(tree, i) {
                let message = format!("{} is misplaced: {}.", label(e), reasons.join("; "));
                out.violation(row, "", None, target(e), message);
            }
        }
    }
    if out.part == PdfuaPart::Ua1 {
        headings(tree, out);
    }
}

/// ISO 14289-2 §8.2.5.2 / veraPDF 8.2.5.2-1: one root kid, a `Document`.
fn document_root(tree: &Tree, out: &mut Out) {
    let types: Vec<&[u8]> = tree.root_kids.iter().map(|&k| tree.std(k)).collect();
    if types != [b"Document".as_slice()] {
        let message = format!(
            "The structure tree root has {} kid(s); it must have exactly one, a <Document>.",
            types.len()
        );
        out.violation("", "PUI2-8.2.5.2-1", None, FindingTarget::Document, message);
    }
}

fn lang_shape(tag: &str, owner: &str, target: FindingTarget, out: &mut Out) {
    if !lang_well_formed(tag) {
        let message = format!("The language tag \"{tag}\" on {owner} is not well formed.");
        out.violation("PUI-V-7.2-29", "PUI2-8.4.4-2", None, target, message);
    }
}

/// The per-element rules that need nothing but the element and its
/// ancestors.
fn element(tree: &Tree, i: usize, e: &Elem, out: &mut Out) {
    let t = target(e);
    if !e.has_p {
        let message = format!("{} has no /P entry naming its parent.", label(e));
        out.violation("PUI-V-7.1-12", "PUI2-8.2.1-2", None, t, message);
    }
    if out.part == PdfuaPart::Ua1 {
        role_mapping(e, out);
    }
    let described = e.alt.is_some() || e.actual_text;
    match e.std.as_slice() {
        b"Figure" if !described => {
            let message = format!("The figure {} has neither /Alt nor /ActualText.", label(e));
            out.violation("PUI-13-004", "PUI2-8.2.5.28.2-1", None, t, message);
        }
        b"Formula" if !described => {
            let message = format!("The formula {} has neither /Alt nor /ActualText.", label(e));
            out.violation("PUI-17-002", "", None, t, message);
        }
        b"H" => {
            let message = format!("{} uses the unnumbered heading type <H>.", label(e));
            out.violation("", "PUI2-8.2.5.12-1", None, t, message);
        }
        _ => {}
    }
    if (e.alt.is_some() || e.actual_text || e.expansion) && !tree.has_lang(i) {
        let message = format!(
            "The language of {}'s /Alt, /ActualText or /E text cannot be determined.",
            label(e)
        );
        out.violation("PUI-11-002", "", None, t, message);
    }
    for (row, key, hit) in [
        ("PUI2-8.4.3-2", "ActualText", e.private_use[0]),
        ("PUI2-8.4.3-3", "Alt", e.private_use[1]),
    ] {
        if hit {
            let message = format!("{}'s /{key} uses Private Use Area characters.", label(e));
            out.violation("", row, None, t, message);
        }
    }
    if let Some(lang) = &e.lang {
        lang_shape(&text(lang), &label(e), t, out);
    }
    let h_kids = tree.kid_types(i).iter().filter(|k| **k == b"H").count();
    if h_kids > 1 {
        let message = format!(
            "{} has {h_kids} <H> kids; at most one is allowed.",
            label(e)
        );
        out.violation("PUI-14-006", "", None, t, message);
    }
}

/// ISO 14289-1 §7.1 ¶5–7 (Matterhorn 02-001, 02-003, 02-004).
fn role_mapping(e: &Elem, out: &mut Out) {
    let (row, what) = match e.mapping {
        Mapping::Standard | Mapping::Mapped => return,
        Mapping::RemappedStandard => ("PUI-02-004", "is a standard type the role map remaps"),
        Mapping::Circular => ("PUI-02-003", "has a circular role mapping"),
        Mapping::Unmapped => ("PUI-02-001", "is not mapped to a standard structure type"),
    };
    out.violation(row, "", None, target(e), format!("{} {what}.", label(e)));
}

/// ISO 14289-1 §7.9 ¶2 (Matterhorn 19-003, 19-004).
fn notes(e: &Elem, ids: &HashMap<&[u8], usize>, out: &mut Out) {
    if out.part != PdfuaPart::Ua1 || e.std != b"Note" {
        return;
    }
    match &e.note_id {
        Some(id) if !id.is_empty() => {
            if ids.get(id.as_slice()).copied().unwrap_or(0) > 1 {
                let message = format!("The note {} shares its /ID with another note.", label(e));
                out.violation("PUI-19-004", "", None, target(e), message);
            }
        }
        _ => {
            let message = format!("The note {} has no /ID.", label(e));
            out.violation("PUI-19-003", "", None, target(e), message);
        }
    }
}

/// ISO 14289-1 §7.4.2 and §7.4.4 (Matterhorn 14-002, 14-003, 14-007):
/// numbered headings start at `H1` and never skip a level going deeper,
/// and `H` and `Hn` are not both used.
fn headings(tree: &Tree, out: &mut Out) {
    let mut previous: Option<u32> = None;
    let mut first_hn = None;
    let mut first_h = None;
    for e in &tree.elems {
        if e.std == b"H" {
            first_h.get_or_insert(e);
        }
        let Some(level) = heading_level(&e.std) else {
            continue;
        };
        first_hn.get_or_insert(e);
        match previous {
            None if level != 1 => {
                let message = format!("The first numbered heading is {}, not <H1>.", label(e));
                out.violation("PUI-14-002", "", None, target(e), message);
            }
            Some(p) if level > p + 1 => {
                let message = format!("The heading {} skips a level after <H{p}>.", label(e));
                out.violation("PUI-14-003", "", None, target(e), message);
            }
            _ => {}
        }
        previous = Some(level);
    }
    if let (Some(h), Some(_)) = (first_h, first_hn) {
        let message = format!("The document uses both <H> (first: {}) and <Hn>.", label(h));
        out.violation("PUI-14-007", "", None, target(h), message);
    }
}

/// ISO 32000-1 Tables 333, 336 and 337, as ISO 14289-1 §7.2 ¶1 applies
/// them (veraPDF 7.2-3 … 7.2-40; Matterhorn 09-004 table, 09-005 list,
/// 09-006 TOC). Returns each failing row with its reasons.
fn containment(tree: &Tree, i: usize) -> Vec<(&'static str, Vec<String>)> {
    let ty = tree.std(i);
    let parent = tree.parent_std(i);
    let kids = tree.kid_types(i);
    let mut reasons = Vec::new();
    let mut parent_in = |allowed: &[&[u8]], what: &str| {
        if !allowed.contains(&parent) {
            reasons.push(format!("its parent must be {what}"));
        }
    };
    let row = match ty {
        b"TR" => {
            parent_in(
                &[b"Table", b"THead", b"TBody", b"TFoot"],
                "a table or table section",
            );
            "PUI-09-004"
        }
        b"THead" | b"TBody" | b"TFoot" => {
            parent_in(&[b"Table"], "<Table>");
            "PUI-09-004"
        }
        b"TH" | b"TD" => {
            parent_in(&[b"TR"], "<TR>");
            "PUI-09-004"
        }
        b"LI" => {
            parent_in(&[b"L"], "<L>");
            "PUI-09-005"
        }
        b"LBody" => {
            parent_in(&[b"LI"], "<LI>");
            "PUI-09-005"
        }
        b"TOCI" => {
            parent_in(&[b"TOC"], "<TOC>");
            "PUI-09-006"
        }
        b"Table" => "PUI-09-004",
        b"L" => "PUI-09-005",
        b"TOC" => "PUI-09-006",
        _ => return Vec::new(),
    };
    reasons.extend(kid_rules(ty, &kids));
    if reasons.is_empty() {
        Vec::new()
    } else {
        vec![(row, reasons)]
    }
}

fn kid_rules(ty: &[u8], kids: &[&[u8]]) -> Vec<String> {
    let allowed: &[&[u8]] = match ty {
        b"Table" => &[b"TR", b"THead", b"TBody", b"TFoot", b"Caption"],
        b"TR" => &[b"TH", b"TD"],
        b"THead" | b"TBody" | b"TFoot" => &[b"TR"],
        b"L" => &[b"L", b"LI", b"Caption"],
        b"LI" => &[b"Lbl", b"LBody"],
        b"TOC" => &[b"TOC", b"TOCI", b"Caption"],
        _ => return Vec::new(),
    };
    let mut reasons = Vec::new();
    if let Some(bad) = kids.iter().find(|k| !allowed.contains(k)) {
        let bad = String::from_utf8_lossy(bad);
        reasons.push(format!("it may not contain <{bad}>"));
    }
    let count = |t: &[u8]| kids.iter().filter(|k| **k == t).count();
    let caption_late = kids.iter().skip(1).any(|k| *k == b"Caption");
    match ty {
        b"Table" => {
            if count(b"THead") > 1 || count(b"TFoot") > 1 {
                reasons.push("it has more than one <THead> or <TFoot>".into());
            }
            if (count(b"THead") > 0 || count(b"TFoot") > 0) && count(b"TBody") == 0 {
                reasons.push("it has a <THead> or <TFoot> but no <TBody>".into());
            }
            let middle = kids
                .get(1..kids.len().saturating_sub(1))
                .is_some_and(|m| m.contains(&b"Caption".as_slice()));
            if count(b"Caption") > 1 || middle {
                reasons.push("its <Caption> must be single and first or last".into());
            }
        }
        b"L" | b"TOC" if caption_late => reasons.push("its <Caption> must be the first kid".into()),
        _ => {}
    }
    reasons
}
