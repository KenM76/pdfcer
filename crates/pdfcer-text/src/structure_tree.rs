//! Read a tagged PDF's logical structure tree (ISO 32000-1 §14.7,
//! ISO 32000-2 §14.7–§14.8) back as elements in logical order, each joined
//! to the text extraction's runs through its marked-content identifiers.
//!
//! # Contract
//!
//! - **Order.** [`StructureTree::elements`] is the depth-first pre-order of
//!   `/StructTreeRoot /K`, which is the document's logical order
//!   (§14.7.2: "the order of the children … is the logical order").
//! - **Types.** Every element carries its raw `/S` **and** the type reached by
//!   role mapping (§14.8.4.1 NOTE 2: raw for tagged export, mapped for
//!   presentation). Mapping looks `/S` up even when it is already standard
//!   (§14.7.3 NOTE 3, PDF ≥ 1.5 — applied to every file; the pre-1.5
//!   exception is not version-gated here), then follows the chain until a
//!   standard name or a revisit (NOTE 2). An element with an `/NS`
//!   (ISO 32000-2 Table 356) maps through that namespace's `/RoleMapNS`,
//!   whose values are a name in the default namespace or `[name, ns]`.
//!   A type that never reaches a standard name keeps its last name and is
//!   counted in [`StructureDiagnostics::non_standard_types`] — the standard
//!   states no fallback.
//! - **Content.** Integer `/K` items are MCIDs on the element's page; `/MCR`
//!   dictionaries name a page (`/Pg`) and optionally a form XObject (`/Stm`)
//!   that scopes the MCID (§14.7.4.2); `/OBJR` dictionaries name an
//!   annotation or XObject (§14.7.4.3). An MCID is joined to runs whose
//!   [`TextRun::mcid`](crate::text_extract::TextRun::mcid) and
//!   [`mcid_stream`](crate::text_extract::TextRun::mcid_stream) both match.
//! - **Disclosure.** MCIDs the tree names that no `BDC` declares, and
//!   declared MCIDs no element claims, are counted — a broken tree is
//!   reported, not half-trusted. So are a `/Pg` inherited from an ancestor
//!   and an element reached twice (a cycle or a shared kid; walked once).
//! - **Attributes.** Table (`RowSpan`, `ColSpan`, `Scope`, `Headers`) and
//!   List (`ListNumbering`) attributes resolve `/A` (a later object wins),
//!   then the `/C` classes through `/ClassMap`, then inheritance where the
//!   attribute is inheritable, then the default (§14.7.5, §14.8.5.7).
//! - No structure tree is not an error: the result has no elements and
//!   [`StructureDiagnostics::struct_tree_present`] is `false`.

use std::collections::{HashMap, HashSet};

use pdfcer_fonts::textstring::decode_text_string;
use pdfcer_model::graph::ObjectGraph;
use pdfcer_model::object::{Dict, ObjId, Object};
use pdfcer_model::page_tree::{self, Rect};
use pdfcer_model::view::DocumentView;

use crate::text_extract::{
    ContentStreamRef, ExtractError, ExtractOptions, ExtractedText, extract_document_view,
};

/// The default standard structure namespace (ISO 32000-2 §14.8.6.1).
pub const PDF_1_7_NAMESPACE: &str = "http://iso.org/pdf/ssn";
/// The PDF 2.0 standard structure namespace (ISO 32000-2 §14.8.6.1).
pub const PDF_2_0_NAMESPACE: &str = "http://iso.org/pdf2/ssn";
/// The one domain namespace ISO 32000-2 §14.8.6.3 names; needs no role map.
pub const MATHML_NAMESPACE: &str = "http://www.w3.org/1998/Math/MathML";

/// ISO 32000-1 §14.8.4, Tables 333–340 (the `pdf/ssn` namespace).
const STANDARD_1_7: &[&str] = &[
    "Document",
    "Part",
    "Art",
    "Sect",
    "Div",
    "BlockQuote",
    "Caption",
    "TOC",
    "TOCI",
    "Index",
    "NonStruct",
    "Private",
    "P",
    "H",
    "H1",
    "H2",
    "H3",
    "H4",
    "H5",
    "H6",
    "L",
    "LI",
    "Lbl",
    "LBody",
    "Table",
    "TR",
    "TH",
    "TD",
    "THead",
    "TBody",
    "TFoot",
    "Span",
    "Quote",
    "Note",
    "Reference",
    "BibEntry",
    "Code",
    "Link",
    "Annot",
    "Ruby",
    "RB",
    "RT",
    "RP",
    "Warichu",
    "WT",
    "WP",
    "Figure",
    "Formula",
    "Form",
];

/// ISO 32000-2 §14.8.4 (the `pdf2/ssn` namespace), less `Hn`, which is
/// matched by pattern.
const STANDARD_2_0: &[&str] = &[
    "Document",
    "DocumentFragment",
    "Part",
    "Sect",
    "Div",
    "Aside",
    "NonStruct",
    "P",
    "H",
    "Title",
    "FENote",
    "Sub",
    "Lbl",
    "Span",
    "Em",
    "Strong",
    "Link",
    "Annot",
    "Form",
    "Ruby",
    "RB",
    "RT",
    "RP",
    "Warichu",
    "WT",
    "WP",
    "L",
    "LI",
    "LBody",
    "Table",
    "TR",
    "TH",
    "TD",
    "THead",
    "TBody",
    "TFoot",
    "Caption",
    "Figure",
    "Formula",
    "Artifact",
];

/// How a consumer should treat an element because of its resolved type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum StructTreatment {
    /// An ordinary element.
    #[default]
    Normal,
    /// `NonStruct` (§14.8.4.2): no structural meaning of its own — treat its
    /// kids as its parent's.
    NonStruct,
    /// `Private` (ISO 32000-1 §14.8.4.2): producer-private content that
    /// "shall not be interpreted or exported". Its subtree is still walked
    /// so the MCID counts stay honest; [`StructureTree::element_text`]
    /// skips it.
    Private,
    /// `Artifact` (ISO 32000-2 §14.8.4.x): the subtree is not real content.
    /// Walked, but skipped by [`StructureTree::element_text`].
    Artifact,
}

/// One item of an element's `/K`, in order.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum StructKid {
    /// A child element: an index into [`StructureTree::elements`].
    Element(usize),
    /// A marked-content sequence (an integer `/K` or an `/MCR`).
    MarkedContent {
        /// Zero-based page, or `None` when no `/Pg` could be resolved.
        page_index: Option<usize>,
        /// The content stream the MCID is scoped to (`/Stm`, else the page).
        stream: ContentStreamRef,
        /// The marked-content identifier.
        mcid: u32,
        /// Indices into that page's
        /// [`PageText::runs`](crate::text_extract::PageText::runs) carrying
        /// this MCID, in content order. Empty for a sequence that paints no
        /// text (an image) or that is not declared.
        runs: Vec<usize>,
        /// Whether a `BDC` on that page declares this MCID. `false` is a
        /// broken reference, counted in
        /// [`StructureDiagnostics::named_not_declared`].
        declared: bool,
    },
    /// An object reference (`/OBJR`, §14.7.4.3): an annotation or XObject.
    Object {
        /// Zero-based page, or `None` when no `/Pg` could be resolved.
        page_index: Option<usize>,
        /// The referenced object.
        object: ObjId,
        /// Its `/Subtype` (e.g. `Link`, `Widget`, `Image`), if any.
        subtype: Option<String>,
        /// Its `/Rect` in default user space, when it has one.
        rect: Option<Rect>,
    },
}

/// One structure element.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct StructElement {
    /// `/S` as written by the producer (lossy UTF-8).
    pub raw_type: String,
    /// The type role mapping reached (equal to `raw_type` when unmapped).
    pub resolved_type: String,
    /// The namespace URI `resolved_type` belongs to; `None` is the default
    /// namespace ([`PDF_1_7_NAMESPACE`]).
    pub namespace: Option<String>,
    /// Whether `resolved_type` is a standard type of its namespace (or in
    /// the MathML namespace).
    pub standard: bool,
    /// Consumer treatment implied by `resolved_type`.
    pub treatment: StructTreatment,
    /// The parent element, or `None` for a root.
    pub parent: Option<usize>,
    /// Depth below the structure tree root (roots are 0).
    pub depth: usize,
    /// The element's object, when it is indirect.
    pub object: Option<ObjId>,
    /// `/ID` (§14.7.2 Table 323).
    pub id: Option<String>,
    /// `/T`, the title.
    pub title: Option<String>,
    /// `/Alt`, alternate description (§14.9.3).
    pub alt: Option<String>,
    /// `/ActualText` (§14.9.4): replaces this element and its descendants.
    pub actual_text: Option<String>,
    /// `/E`, expansion of an abbreviation (§14.9.5).
    pub expansion: Option<String>,
    /// `/Lang` on this element (§14.9.2).
    pub lang: Option<String>,
    /// The language in force: this element's, else the nearest ancestor's,
    /// else the catalog's `/Lang`.
    pub effective_lang: Option<String>,
    /// The element's own `/Pg`, resolved to a zero-based page index.
    pub page_index: Option<usize>,
    /// `/K`, in order.
    pub kids: Vec<StructKid>,
    /// Table `RowSpan` (Table 384; default 1) — `Some` only on `TH`/`TD`.
    pub row_span: Option<u32>,
    /// Table `ColSpan` (default 1) — `Some` only on `TH`/`TD`.
    pub col_span: Option<u32>,
    /// Table `Scope` (`Row`, `Column`, `Both`) as written; no default.
    pub scope: Option<String>,
    /// Table `Headers`: the `/ID`s of the header cells for this cell.
    pub headers: Vec<String>,
    /// List `ListNumbering` (inheritable; `None` when absent throughout).
    pub list_numbering: Option<String>,
}

/// What reading the structure tree found wrong or had to derive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct StructureDiagnostics {
    /// The catalog has a `/StructTreeRoot`.
    pub struct_tree_present: bool,
    /// Elements read.
    pub elements: usize,
    /// Elements whose type never reached a standard name.
    pub non_standard_types: usize,
    /// Role-map chains that returned to a visited type (legal, §14.7.3
    /// NOTE 2) before reaching a standard one.
    pub role_map_cycles: usize,
    /// Element references reached a second time (a cycle or a shared kid)
    /// and not walked again.
    pub elements_revisited: usize,
    /// MCID references in the tree.
    pub mcids_named: usize,
    /// MCIDs the tree names that no `BDC` on the page declares.
    pub named_not_declared: usize,
    /// Declared MCIDs that no element claims.
    pub declared_unclaimed: usize,
    /// MCIDs claimed by more than one reference.
    pub claimed_twice: usize,
    /// `/OBJR` references.
    pub object_refs: usize,
    /// `/K` items that are neither an integer, an `/MCR`, an `/OBJR` nor an
    /// element, or content items directly under the root.
    pub malformed_kids: usize,
    /// Content items whose page came from an ancestor's `/Pg` rather than
    /// their own element's — derived, the standard does not say so.
    pub page_inherited: usize,
    /// Content items with no resolvable page at all.
    pub page_unresolved: usize,
    /// Human-readable notes, one per distinct finding.
    pub notes: Vec<String>,
}

/// A document's structure tree, joined to its text extraction.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct StructureTree {
    /// Every element, depth-first pre-order (logical order).
    pub elements: Vec<StructElement>,
    /// The top-level elements (`/StructTreeRoot /K`), in order.
    pub roots: Vec<usize>,
    /// The text extraction the MCIDs index into.
    pub text: ExtractedText,
    /// What the read found.
    pub diagnostics: StructureDiagnostics,
}

/// Read the structure tree of the document as `view` presents it, and
/// extract its text with `options` to join the content to.
///
/// # Errors
///
/// [`ExtractError::PageTree`] when the page tree cannot be walked. A missing
/// or malformed structure tree is not an error; it is reported in
/// [`StructureTree::diagnostics`].
///
/// # Examples
///
/// ```no_run
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use pdfcer_model::document::Document;
/// use pdfcer_text::structure_tree::read_structure_tree;
/// use pdfcer_text::text_extract::ExtractOptions;
///
/// let doc = Document::from_bytes(std::fs::read("tagged.pdf")?)?;
/// let tree = read_structure_tree(&doc.view(), &ExtractOptions::default())?;
/// for (i, e) in tree.elements.iter().enumerate() {
///     println!("{}{} {:?}", "  ".repeat(e.depth), e.resolved_type, tree.element_text(i));
/// }
/// # Ok(())
/// # }
/// ```
pub fn read_structure_tree(
    view: &DocumentView<'_>,
    options: &ExtractOptions,
) -> Result<StructureTree, ExtractError> {
    let text = extract_document_view(view, options)?;
    let pages = page_tree::pages_in(view)?;
    let page_of: HashMap<ObjId, usize> = pages.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let mut reader = Reader {
        doc: view,
        page_of,
        role_map: None,
        class_map: None,
        elements: Vec::new(),
        roots: Vec::new(),
        diag: StructureDiagnostics::default(),
        visited: HashSet::new(),
    };
    let catalog = view.catalog_dict();
    let root = catalog
        .and_then(|c| c.get(b"StructTreeRoot"))
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict);
    if let Some(root) = root {
        reader.diag.struct_tree_present = true;
        reader.role_map = root
            .get(b"RoleMap")
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict);
        reader.class_map = root
            .get(b"ClassMap")
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict);
        let catalog_lang = catalog
            .and_then(|c| c.get(b"Lang"))
            .and_then(|o| text_of(view, o));
        reader.walk(root, catalog_lang);
    }
    let mut tree = StructureTree {
        elements: reader.elements,
        roots: reader.roots,
        text,
        diagnostics: reader.diag,
    };
    tree.join_content();
    Ok(tree)
}

/// One `/K` being walked: its owner (`None` = the root) and its items.
struct Frame<'a> {
    owner: Option<usize>,
    items: Vec<&'a Object>,
    next: usize,
}

struct Reader<'a, 'v> {
    doc: &'a DocumentView<'v>,
    page_of: HashMap<ObjId, usize>,
    role_map: Option<&'a Dict>,
    class_map: Option<&'a Dict>,
    elements: Vec<StructElement>,
    roots: Vec<usize>,
    diag: StructureDiagnostics,
    visited: HashSet<ObjId>,
}

/// The kind of one `/K` item.
enum KidKind<'a> {
    Mcid(u32),
    Mcr(&'a Dict),
    Objr(&'a Dict),
    Element(&'a Dict, Option<ObjId>),
    Malformed,
}

impl<'a> Reader<'a, '_> {
    /// Iterative depth-first walk from the root dictionary; the visited set
    /// is the cycle guard (the standard sets no depth limit).
    fn walk(&mut self, root: &'a Dict, catalog_lang: Option<String>) {
        let mut stack = vec![Frame {
            owner: None,
            items: k_items(self.doc, root),
            next: 0,
        }];
        let mut langs: Vec<Option<String>> = Vec::new();
        while let Some(frame) = stack.last_mut() {
            let Some(&item) = frame.items.get(frame.next) else {
                stack.pop();
                continue;
            };
            frame.next += 1;
            let owner = frame.owner;
            match self.classify(item) {
                KidKind::Element(dict, object) => {
                    if let Some(id) = object
                        && !self.visited.insert(id)
                    {
                        self.diag.elements_revisited += 1;
                        continue;
                    }
                    let depth = owner.map_or(0, |p| {
                        self.elements
                            .get(p)
                            .map_or(0, |e| e.depth.saturating_add(1))
                    });
                    langs.truncate(depth);
                    let inherited_lang = langs
                        .iter()
                        .rev()
                        .flatten()
                        .next()
                        .cloned()
                        .or_else(|| catalog_lang.clone());
                    let index = self.elements.len();
                    let element = self.element(dict, object, owner, depth, inherited_lang);
                    langs.push(element.lang.clone());
                    self.elements.push(element);
                    match owner {
                        Some(p) => self.push_kid(p, StructKid::Element(index)),
                        None => self.roots.push(index),
                    }
                    stack.push(Frame {
                        owner: Some(index),
                        items: k_items(self.doc, dict),
                        next: 0,
                    });
                }
                KidKind::Malformed => self.diag.malformed_kids += 1,
                content => {
                    let Some(owner) = owner else {
                        self.diag.malformed_kids += 1;
                        continue;
                    };
                    if let Some(kid) = self.content_kid(owner, content) {
                        self.push_kid(owner, kid);
                    }
                }
            }
        }
        self.diag.elements = self.elements.len();
    }

    fn classify(&self, item: &'a Object) -> KidKind<'a> {
        let reference = item.as_reference();
        let resolved = self.doc.resolve(item);
        if let Some(n) = resolved.as_int() {
            return u32::try_from(n).map_or(KidKind::Malformed, KidKind::Mcid);
        }
        let Some(dict) = resolved.as_dict() else {
            return KidKind::Malformed;
        };
        match name_of(self.doc, dict.get(b"Type")).as_deref() {
            Some(b"MCR") => KidKind::Mcr(dict),
            Some(b"OBJR") => KidKind::Objr(dict),
            // Table 323: /Type is optional on an element, and `StructElem`
            // is its only value.
            Some(b"StructElem") | None if dict.contains_key(b"S") => {
                KidKind::Element(dict, reference)
            }
            _ => KidKind::Malformed,
        }
    }

    fn push_kid(&mut self, owner: usize, kid: StructKid) {
        if let Some(e) = self.elements.get_mut(owner) {
            e.kids.push(kid);
        }
    }

    /// The page for a content item: its own `/Pg`, else its element's, else
    /// the nearest ancestor's (counted as derived).
    fn content_page(&mut self, owner: usize, own_pg: Option<&Object>) -> Option<usize> {
        if let Some(p) = own_pg.and_then(|o| self.page_index(o)) {
            return Some(p);
        }
        let mut at = Some(owner);
        let mut first = true;
        while let Some(i) = at {
            let e = self.elements.get(i)?;
            if let Some(p) = e.page_index {
                if !first {
                    self.diag.page_inherited += 1;
                }
                return Some(p);
            }
            first = false;
            at = e.parent;
        }
        self.diag.page_unresolved += 1;
        None
    }

    fn content_kid(&mut self, owner: usize, kind: KidKind<'a>) -> Option<StructKid> {
        match kind {
            KidKind::Mcid(mcid) => {
                self.diag.mcids_named += 1;
                Some(StructKid::MarkedContent {
                    page_index: self.content_page(owner, None),
                    stream: ContentStreamRef::Page,
                    mcid,
                    runs: Vec::new(),
                    declared: false,
                })
            }
            KidKind::Mcr(dict) => {
                let mcid = dict
                    .get(b"MCID")
                    .map(|o| self.doc.resolve(o))
                    .and_then(Object::as_int)
                    .and_then(|n| u32::try_from(n).ok());
                let Some(mcid) = mcid else {
                    self.diag.malformed_kids += 1;
                    return None;
                };
                self.diag.mcids_named += 1;
                let stream = dict
                    .get(b"Stm")
                    .and_then(Object::as_reference)
                    .map_or(ContentStreamRef::Page, |id| ContentStreamRef::Form {
                        object: id.num,
                    });
                Some(StructKid::MarkedContent {
                    page_index: self.content_page(owner, dict.get(b"Pg")),
                    stream,
                    mcid,
                    runs: Vec::new(),
                    declared: false,
                })
            }
            KidKind::Objr(dict) => {
                let Some(object) = dict.get(b"Obj").and_then(Object::as_reference) else {
                    self.diag.malformed_kids += 1;
                    return None;
                };
                self.diag.object_refs += 1;
                let target = self.doc.resolved(object).as_dict();
                let subtype = target
                    .and_then(|d| name_of(self.doc, d.get(b"Subtype")))
                    .map(|n| String::from_utf8_lossy(&n).into_owned());
                let rect = target
                    .and_then(|d| d.get(b"Rect"))
                    .and_then(|o| page_tree::parse_rect(self.doc, o, "Rect").ok());
                Some(StructKid::Object {
                    page_index: self.content_page(owner, dict.get(b"Pg")),
                    object,
                    subtype,
                    rect,
                })
            }
            KidKind::Element(..) | KidKind::Malformed => None,
        }
    }

    fn page_index(&self, pg: &Object) -> Option<usize> {
        pg.as_reference()
            .and_then(|id| self.page_of.get(&id).copied())
    }

    fn element(
        &mut self,
        dict: &'a Dict,
        object: Option<ObjId>,
        parent: Option<usize>,
        depth: usize,
        inherited_lang: Option<String>,
    ) -> StructElement {
        let raw = name_of(self.doc, dict.get(b"S")).unwrap_or_default();
        let (resolved, namespace, standard, cycled) = self.resolve_type(&raw, dict.get(b"NS"));
        if cycled {
            self.diag.role_map_cycles += 1;
        }
        let resolved_type = String::from_utf8_lossy(&resolved).into_owned();
        if !standard {
            self.diag.non_standard_types += 1;
            self.diag.notes.push(format!(
                "structure: type {:?} is not standard and no role map reaches a standard type \
                 (ISO 32000-1 \u{a7}14.8.4.1) \u{2014} kept as written",
                String::from_utf8_lossy(&raw)
            ));
        }
        let is_1_7 = namespace.is_none() || namespace.as_deref() == Some(PDF_1_7_NAMESPACE);
        let treatment = match (standard, resolved_type.as_str()) {
            (true, "NonStruct") => StructTreatment::NonStruct,
            (true, "Private") if is_1_7 => StructTreatment::Private,
            (true, "Artifact") if !is_1_7 => StructTreatment::Artifact,
            _ => StructTreatment::Normal,
        };
        let lang = dict.get(b"Lang").and_then(|o| text_of(self.doc, o));
        let effective_lang = lang.clone().or(inherited_lang);
        let is_cell = standard && matches!(resolved_type.as_str(), "TH" | "TD");
        let mut e = StructElement {
            raw_type: String::from_utf8_lossy(&raw).into_owned(),
            resolved_type,
            namespace,
            standard,
            treatment,
            parent,
            depth,
            object,
            id: dict.get(b"ID").and_then(|o| text_of(self.doc, o)),
            title: dict.get(b"T").and_then(|o| text_of(self.doc, o)),
            alt: dict.get(b"Alt").and_then(|o| text_of(self.doc, o)),
            actual_text: dict.get(b"ActualText").and_then(|o| text_of(self.doc, o)),
            expansion: dict.get(b"E").and_then(|o| text_of(self.doc, o)),
            lang,
            effective_lang,
            page_index: dict.get(b"Pg").and_then(|o| self.page_index(o)),
            ..StructElement::default()
        };
        if is_cell {
            let span = |v: Option<&Object>| {
                v.and_then(|o| self.doc.resolve(o).as_int())
                    .and_then(|n| u32::try_from(n).ok())
                    .filter(|&n| n >= 1)
                    .unwrap_or(1)
            };
            e.row_span = Some(span(self.attribute(dict, b"Table", b"RowSpan")));
            e.col_span = Some(span(self.attribute(dict, b"Table", b"ColSpan")));
            e.scope = self
                .attribute(dict, b"Table", b"Scope")
                .and_then(|o| name_of(self.doc, Some(o)))
                .map(|n| String::from_utf8_lossy(&n).into_owned());
            e.headers = self
                .attribute(dict, b"Table", b"Headers")
                .map(|o| self.doc.resolve(o))
                .and_then(Object::as_array)
                .map(|a| a.iter().filter_map(|o| text_of(self.doc, o)).collect())
                .unwrap_or_default();
        }
        e.list_numbering = self
            .attribute(dict, b"List", b"ListNumbering")
            .and_then(|o| name_of(self.doc, Some(o)))
            .map(|n| String::from_utf8_lossy(&n).into_owned())
            .or_else(|| {
                parent
                    .and_then(|p| self.elements.get(p))
                    .and_then(|p| p.list_numbering.clone())
            });
        e
    }

    /// Resolve `/S` through `/RoleMap` or the element namespace's
    /// `/RoleMapNS`. Returns (type, namespace URI, standard, cycled).
    fn resolve_type(
        &self,
        raw: &[u8],
        ns: Option<&Object>,
    ) -> (Vec<u8>, Option<String>, bool, bool) {
        let mut name = raw.to_vec();
        let mut ns_id = ns.and_then(Object::as_reference);
        let mut seen: HashSet<(Vec<u8>, Option<ObjId>)> = HashSet::new();
        let mut first = true;
        loop {
            let uri = ns_id.and_then(|id| self.namespace_uri(id));
            let recognised = recognised(&name, uri.as_deref());
            let target_uri = uri.clone().filter(|u| u != PDF_1_7_NAMESPACE);
            if (recognised && !first) || uri.as_deref() == Some(MATHML_NAMESPACE) {
                return (name, target_uri, true, false);
            }
            if !seen.insert((name.clone(), ns_id)) {
                return (name, target_uri, recognised, !recognised);
            }
            first = false;
            let map = match ns_id {
                None => self.role_map,
                Some(id) => self
                    .doc
                    .resolved(id)
                    .as_dict()
                    .and_then(|d| d.get(b"RoleMapNS"))
                    .map(|o| self.doc.resolve(o))
                    .and_then(Object::as_dict),
            };
            let Some(value) = map.and_then(|m| m.get(&name)).map(|o| self.doc.resolve(o)) else {
                return (name, target_uri, recognised, false);
            };
            if let Some(n) = value.as_name() {
                name = n.as_bytes().to_vec();
                ns_id = None;
            } else if let Some(arr) = value.as_array()
                && let (Some(n), Some(target)) = (arr.first(), arr.get(1))
            {
                let Some(n) = self.doc.resolve(n).as_name() else {
                    return (name, target_uri, recognised, false);
                };
                name = n.as_bytes().to_vec();
                ns_id = target.as_reference();
            } else {
                return (name, target_uri, recognised, false);
            }
        }
    }

    fn namespace_uri(&self, id: ObjId) -> Option<String> {
        self.doc
            .resolved(id)
            .as_dict()
            .and_then(|d| d.get(b"NS"))
            .and_then(|o| text_of(self.doc, o))
    }

    /// An attribute value for `owner`/`key`: `/A` (later wins), then `/C`
    /// classes through `/ClassMap` (later wins). Inheritance and defaults
    /// are the caller's.
    fn attribute(&self, dict: &'a Dict, owner: &[u8], key: &[u8]) -> Option<&'a Object> {
        let doc = self.doc;
        let find = |objs: Vec<&'a Dict>| {
            objs.into_iter().rev().find_map(|a| {
                (name_of(doc, a.get(b"O")).as_deref() == Some(owner))
                    .then(|| a.get(key))
                    .flatten()
            })
        };
        if let Some(v) = dict.get(b"A").and_then(|a| find(attribute_dicts(doc, a))) {
            return Some(v);
        }
        let classes = dict.get(b"C").map(|c| doc.resolve(c))?;
        let names: Vec<&[u8]> = match classes {
            Object::Name(n) => vec![n.as_bytes()],
            Object::Array(a) => a
                .iter()
                .filter_map(|o| doc.resolve(o).as_name().map(|n| n.as_bytes()))
                .collect(),
            _ => Vec::new(),
        };
        names.into_iter().rev().find_map(|class| {
            self.class_map
                .and_then(|m| m.get(class))
                .and_then(|a| find(attribute_dicts(doc, a)))
        })
    }
}

/// `/A` or a `/ClassMap` value: one attribute dictionary or stream, or an
/// array of them where integers are revision numbers (§14.7.5.2).
fn attribute_dicts<'a>(doc: &'a DocumentView<'_>, obj: &'a Object) -> Vec<&'a Dict> {
    let one = |o: &'a Object| doc.resolve(o).as_dict();
    match doc.resolve(obj) {
        Object::Array(a) => a.iter().filter_map(one).collect(),
        _ => one(obj).into_iter().collect(),
    }
}

/// Whether `name` is standard in the namespace `uri` (`None` = default).
fn recognised(name: &[u8], uri: Option<&str>) -> bool {
    let Ok(name) = std::str::from_utf8(name) else {
        return false;
    };
    match uri {
        None | Some(PDF_1_7_NAMESPACE) => STANDARD_1_7.contains(&name),
        Some(PDF_2_0_NAMESPACE) => {
            STANDARD_2_0.contains(&name)
                || name.strip_prefix('H').is_some_and(|n| {
                    !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) && !n.starts_with('0')
                })
        }
        Some(MATHML_NAMESPACE) => true,
        Some(_) => false,
    }
}

/// `/K` as a list: absent, one item, or an array.
fn k_items<'a>(doc: &'a DocumentView<'_>, dict: &'a Dict) -> Vec<&'a Object> {
    match dict.get(b"K") {
        None => Vec::new(),
        Some(k) => match doc.resolve(k) {
            Object::Array(a) => a.iter().collect(),
            Object::Null => Vec::new(),
            _ => vec![k],
        },
    }
}

fn name_of(doc: &DocumentView<'_>, obj: Option<&Object>) -> Option<Vec<u8>> {
    obj.map(|o| doc.resolve(o))
        .and_then(Object::as_name)
        .map(|n| n.as_bytes().to_vec())
}

/// A text string (§7.9.2.2), decoded.
fn text_of(doc: &DocumentView<'_>, obj: &Object) -> Option<String> {
    match doc.resolve(obj) {
        Object::String(bytes) => Some(decode_text_string(bytes).text),
        _ => None,
    }
}

impl StructureTree {
    /// Fill each marked-content kid's `runs` and `declared`, and count the
    /// MCIDs the tree and the content disagree on.
    fn join_content(&mut self) {
        type Key = (usize, ContentStreamRef, u32);
        let mut runs_by_key: HashMap<Key, Vec<usize>> = HashMap::new();
        let mut declared: HashSet<Key> = HashSet::new();
        for page in &self.text.pages {
            let p = page.page_index;
            for (i, run) in page.runs.iter().enumerate() {
                if let (Some(m), Some(s)) = (run.mcid, run.mcid_stream) {
                    runs_by_key.entry((p, s, m)).or_default().push(i);
                }
            }
            for &(s, m) in &page.marked_content_ids {
                declared.insert((p, s, m));
            }
        }
        let mut claimed: HashSet<Key> = HashSet::new();
        let mut named_not_declared = 0;
        let mut claimed_twice = 0;
        for e in &mut self.elements {
            for kid in &mut e.kids {
                if let StructKid::MarkedContent {
                    page_index: Some(p),
                    stream,
                    mcid,
                    runs,
                    declared: d,
                } = kid
                {
                    let key = (*p, *stream, *mcid);
                    *d = declared.contains(&key);
                    if !*d {
                        named_not_declared += 1;
                    }
                    if !claimed.insert(key) {
                        claimed_twice += 1;
                    }
                    *runs = runs_by_key.get(&key).cloned().unwrap_or_default();
                }
            }
        }
        let d = &mut self.diagnostics;
        d.named_not_declared = named_not_declared;
        d.claimed_twice = claimed_twice;
        if d.struct_tree_present {
            d.declared_unclaimed = declared.difference(&claimed).count();
        }
        if named_not_declared > 0 {
            d.notes.push(format!(
                "structure: {named_not_declared} MCID reference(s) name marked content no BDC \
                 declares (ISO 32000-1 \u{a7}14.7.4.2)"
            ));
        }
        if d.declared_unclaimed > 0 {
            d.notes.push(format!(
                "structure: {} declared marked-content sequence(s) belong to no structure element",
                d.declared_unclaimed
            ));
        }
        if d.page_inherited > 0 {
            d.notes.push(format!(
                "structure: {} content item(s) took their page from an ancestor's /Pg \
                 (derived \u{2014} Table 323 puts /Pg on the element itself)",
                d.page_inherited
            ));
        }
        if !d.struct_tree_present {
            d.notes
                .push("structure: the document has no /StructTreeRoot (untagged)".to_owned());
        }
        d.notes.sort();
        d.notes.dedup();
    }

    /// The element's text: its `/ActualText` if it has one, else its content
    /// in order, skipping [`StructTreatment::Private`] and
    /// [`StructTreatment::Artifact`] subtrees. Pieces from different content
    /// items are joined with one space when neither side has whitespace at
    /// the joint — a derived separator.
    #[must_use]
    pub fn element_text(&self, index: usize) -> String {
        let mut out = String::new();
        // Kids are visited only when their index is greater than their
        // parent's (pre-order), so this terminates on any input.
        let mut stack = vec![(index, 0usize)];
        while let Some((i, k)) = stack.pop() {
            let Some(e) = self.elements.get(i) else {
                continue;
            };
            if k == 0 {
                if matches!(
                    e.treatment,
                    StructTreatment::Private | StructTreatment::Artifact
                ) {
                    continue;
                }
                if let Some(t) = &e.actual_text {
                    join(&mut out, t);
                    continue;
                }
            }
            let Some(kid) = e.kids.get(k) else {
                continue;
            };
            stack.push((i, k + 1));
            match kid {
                StructKid::Element(c) if *c > i => stack.push((*c, 0)),
                StructKid::MarkedContent {
                    page_index: Some(p),
                    runs,
                    ..
                } => {
                    let Some(page) = self.text.pages.iter().find(|pg| pg.page_index == *p) else {
                        continue;
                    };
                    let piece: String = runs
                        .iter()
                        .filter_map(|&r| page.runs.get(r))
                        .map(|r| r.text.as_str())
                        .collect();
                    join(&mut out, &piece);
                }
                _ => {}
            }
        }
        out
    }

    /// The element's extent per page: the union of the boxes of the runs it
    /// (and its descendants) own and of any `/OBJR` rectangles, in page
    /// order.
    #[must_use]
    pub fn element_bbox(&self, index: usize) -> Vec<(usize, Rect)> {
        let mut by_page: Vec<(usize, Rect)> = Vec::new();
        let mut add = |p: usize, r: Rect| match by_page.iter_mut().find(|(q, _)| *q == p) {
            Some((_, u)) => {
                *u = Rect::from_corners(
                    u.llx.min(r.llx),
                    u.lly.min(r.lly),
                    u.urx.max(r.urx),
                    u.ury.max(r.ury),
                );
            }
            None => by_page.push((p, r)),
        };
        let mut stack = vec![index];
        while let Some(i) = stack.pop() {
            let Some(e) = self.elements.get(i) else {
                continue;
            };
            for kid in &e.kids {
                match kid {
                    StructKid::Element(c) if *c > i => stack.push(*c),
                    StructKid::MarkedContent {
                        page_index: Some(p),
                        runs,
                        ..
                    } => {
                        if let Some(page) = self.text.pages.iter().find(|pg| pg.page_index == *p) {
                            for r in runs.iter().filter_map(|&r| page.runs.get(r)) {
                                if let Some(b) = r.bbox {
                                    add(*p, b);
                                }
                            }
                        }
                    }
                    StructKid::Object {
                        page_index: Some(p),
                        rect: Some(r),
                        ..
                    } => add(*p, *r),
                    _ => {}
                }
            }
        }
        by_page.sort_by_key(|(p, _)| *p);
        by_page
    }
}

fn join(out: &mut String, piece: &str) {
    if piece.is_empty() {
        return;
    }
    let needs_space = out.chars().last().is_some_and(|c| !c.is_whitespace())
        && piece.chars().next().is_some_and(|c| !c.is_whitespace());
    if needs_space {
        out.push(' ');
    }
    out.push_str(piece);
}
