//! Annotation and form-field checks (ISO 14289-1 §7.18; ISO 14289-2
//! §8.2.5.20, §8.9, §8.10.1; veraPDF PDFUA-1 7.18.x, PDFUA-2 8.9.x).
//!
//! UA-1 exempts an annotation that is hidden (`/F` bit 2) or lies wholly
//! outside the crop box; UA-2's rules bind only *real-content*
//! annotations, those enclosed by a structure element other than an
//! `Artifact`. `Popup` is exempt from UA-1 §7.18.1 (see the module
//! documentation of [`super`]).

use super::structure::{Elem, Tree};
use super::{FindingTarget, Out, PdfuaPart, name_of, string_of, text};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::{Page, Rect};
use crate::view::DocumentView;

/// Annotation flags, ISO 32000-1 Table 165.
const INVISIBLE: i64 = 1;
const HIDDEN: i64 = 2;
const NO_VIEW: i64 = 32;
const TOGGLE_NO_VIEW: i64 = 256;
/// Form-field `/Parent` chains are followed at most this far.
const MAX_FIELD_DEPTH: usize = 32;

/// The markup annotation subtypes, ISO 32000-2 Table 171.
const MARKUP: &[&[u8]] = &[
    b"Text",
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
    b"Stamp",
    b"Ink",
    b"FileAttachment",
    b"Sound",
    b"Redact",
    b"Projection",
];

struct Annot<'a> {
    id: ObjId,
    dict: &'a Dict,
    subtype: &'a [u8],
    flags: i64,
    contents: bool,
    /// The structure element whose `/OBJR` names this annotation.
    elem: Option<usize>,
    /// UA-1: hidden or wholly outside the crop box.
    exempt: bool,
}

impl Annot<'_> {
    fn label(&self) -> String {
        format!(
            "The {} annotation {}",
            String::from_utf8_lossy(self.subtype),
            self.id
        )
    }
}

/// Check one page's annotations.
pub(super) fn check(
    view: &DocumentView<'_>,
    tree: &Tree,
    page: &Page,
    number: usize,
    out: &mut Out,
) {
    let ids = crate::preflight::walk::annotations(view, page.id);
    if ids.is_empty() {
        return;
    }
    tabs(view, page, number, out);
    for id in ids {
        let Some(dict) = view.graph().value(id).and_then(Object::as_dict) else {
            continue;
        };
        let flags = match dict.get(b"F").map(|o| view.resolve(o)) {
            Some(Object::Integer(f)) => *f,
            _ => 0,
        };
        let rect = rect_of(view, dict.get(b"Rect"));
        let a = Annot {
            id,
            dict,
            subtype: name_of(view, dict.get(b"Subtype")).unwrap_or_default(),
            flags,
            contents: string_of(view, dict.get(b"Contents")).is_some_and(|s| !text(s).is_empty()),
            elem: tree.objr.get(&id).copied(),
            exempt: flags & HIDDEN != 0 || rect.is_some_and(|r| outside(r, page.crop_box)),
        };
        match out.part {
            PdfuaPart::Ua1 => {
                ua1(view, tree, &a, number, out);
                ua1_language(view, tree, &a, number, out);
            }
            PdfuaPart::Ua2 => {
                ua2(view, tree, &a, rect, number, out);
                ua2_subtype(view, &a, number, out);
            }
        }
    }
}

/// ISO 14289-1 §7.18.3 (Matterhorn 28-008, 28-009) / ISO 14289-2 §8.9.3.3.
fn tabs(view: &DocumentView<'_>, page: &Page, number: usize, out: &mut Out) {
    let dict = view.graph().value(page.id).and_then(Object::as_dict);
    let tabs = dict.and_then(|d| name_of(view, d.get(b"Tabs")));
    let at = Some(number);
    let t = FindingTarget::Page;
    match (out.part, tabs) {
        (PdfuaPart::Ua1, Some(b"S")) | (PdfuaPart::Ua2, Some(b"S" | b"A" | b"W")) => {}
        (_, None) => {
            let message = "The page has annotations but no /Tabs entry.".into();
            out.violation("PUI-28-008", "PUI2-8.9.3.3-1", at, t, message);
        }
        (_, Some(other)) => {
            let shown = String::from_utf8_lossy(other);
            let message = format!("The page's /Tabs is /{shown}; structure order (/S) is needed.");
            out.violation("PUI-28-009", "PUI2-8.9.3.3-1", at, t, message);
        }
    }
}

fn rect_of(view: &DocumentView<'_>, o: Option<&Object>) -> Option<Rect> {
    let items = o.map(|o| view.resolve(o)).and_then(Object::as_array)?;
    let n: Vec<f64> = items
        .iter()
        .filter_map(|i| view.resolve(i).as_number())
        .collect();
    let [a, b, c, d] = n.as_slice() else {
        return None;
    };
    Some(Rect {
        llx: a.min(*c),
        lly: b.min(*d),
        urx: a.max(*c),
        ury: b.max(*d),
    })
}

fn outside(r: Rect, crop: Rect) -> bool {
    r.urx < crop.llx || r.llx > crop.urx || r.ury < crop.lly || r.lly > crop.ury
}

/// ISO 14289-1 §7.18.1, §7.18.2, §7.18.4, §7.18.5, §7.18.8.
fn ua1(view: &DocumentView<'_>, tree: &Tree, a: &Annot<'_>, page: usize, out: &mut Out) {
    if a.exempt || a.subtype == b"Popup" {
        return;
    }
    let at = Some(page);
    let t = FindingTarget::Annotation(a.id);
    let parent = a.elem.map_or(&b""[..], |e| tree.std(e));
    let alt = a
        .elem
        .and_then(|e| tree.elems.get(e))
        .is_some_and(|e| e.alt.is_some());
    let mut fail = |row: &'static str, what: &str| {
        out.violation(row, "", at, t, format!("{} {what}.", a.label()));
    };
    let s = a.subtype;
    if !matches!(s, b"Widget" | b"PrinterMark" | b"Link") && parent != b"Annot" {
        fail(
            "PUI-28-002",
            "is not enclosed in an <Annot> structure element",
        );
    }
    if s != b"Widget" && !a.contents && !alt {
        fail(
            "PUI-28-004",
            "has neither /Contents nor an alternate description",
        );
    }
    match s {
        b"Widget" => {
            if !alt && field_tooltip(view, a.dict).is_none() {
                fail(
                    "PUI-28-005",
                    "has no /TU tooltip and no alternate description",
                );
            }
            if parent != b"Form" {
                fail(
                    "PUI-28-010",
                    "is not enclosed in a <Form> structure element",
                );
            }
        }
        b"TrapNet" => fail("PUI-28-007", "is a trap network annotation"),
        b"Link" => {
            if parent != b"Link" {
                fail(
                    "PUI-28-011",
                    "is not enclosed in a <Link> structure element",
                );
            }
            if !a.contents {
                fail("PUI-28-012", "has no /Contents describing it");
            }
        }
        b"PrinterMark" if a.elem.is_some() => fail("PUI-28-017", "is in the structure tree"),
        _ => {}
    }
}

/// ISO 14289-1 §7.2 ¶3 (Matterhorn 11-004, 11-005; veraPDF 7.2-24, 7.2-25).
fn ua1_language(view: &DocumentView<'_>, tree: &Tree, a: &Annot<'_>, page: usize, out: &mut Out) {
    let known = tree.catalog_lang.is_some() || a.elem.is_some_and(|e| tree.has_lang(e));
    if known {
        return;
    }
    let at = Some(page);
    let t = FindingTarget::Annotation(a.id);
    if a.contents {
        let message = format!(
            "The language of {}'s /Contents cannot be determined.",
            a.label()
        );
        out.violation("PUI-11-004", "", at, t, message);
    }
    if a.subtype == b"Widget" && field_tooltip(view, a.dict).is_some() {
        let message = format!("The language of {}'s /TU cannot be determined.", a.label());
        out.violation("PUI-11-005", "", at, t, message);
    }
}

/// The field's `/TU`, inherited through `/Parent` (ISO 32000-1 §12.7.3.1).
fn field_tooltip(view: &DocumentView<'_>, widget: &Dict) -> Option<String> {
    let mut d = widget;
    for _ in 0..MAX_FIELD_DEPTH {
        if let Some(tu) = string_of(view, d.get(b"TU"))
            .map(text)
            .filter(|s| !s.is_empty())
        {
            return Some(tu);
        }
        d = d
            .get(b"Parent")
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict)?;
    }
    None
}

/// ISO 14289-2 §8.2.5.20, §8.9.2.2, §8.9.2.3, §8.9.2.4.9, .13, .14, .16,
/// §8.9.4.2 and §8.10.1: the rules over real-content annotations.
fn ua2(
    view: &DocumentView<'_>,
    tree: &Tree,
    a: &Annot<'_>,
    rect: Option<Rect>,
    page: usize,
    out: &mut Out,
) {
    let at = Some(page);
    let t = FindingTarget::Annotation(a.id);
    let elem: Option<&Elem> = a.elem.and_then(|e| tree.elems.get(e));
    let parent = a.elem.map_or(&b""[..], |e| tree.std(e));
    let mut fail = |row: &'static str, what: &str| {
        out.violation("", row, at, t, format!("{} {what}.", a.label()));
    };
    if a.subtype == b"Popup" && elem.is_some() {
        fail("PUI2-8.9.2.4.9-1", "is in the structure tree");
    }
    if let (Some(e), true) = (elem, a.contents) {
        let contents = string_of(view, a.dict.get(b"Contents")).map(text);
        if e.alt
            .as_deref()
            .is_some_and(|alt| Some(text(alt)) != contents)
        {
            fail(
                "PUI2-8.9.4.2-1",
                "has /Contents that differs from its element's /Alt",
            );
        }
    }
    let real = elem.is_some() && parent != b"Artifact";
    if !real {
        return;
    }
    if a.flags & INVISIBLE != 0 {
        fail("PUI2-8.9.2.2-1", "is real content but flagged Invisible");
    }
    if a.flags & NO_VIEW != 0 && a.flags & TOGGLE_NO_VIEW == 0 {
        fail("PUI2-8.9.2.2-2", "is real content but flagged NoView");
    }
    let s = a.subtype;
    if MARKUP.contains(&s) && parent != b"Annot" {
        fail(
            "PUI2-8.9.2.3-1",
            "is not enclosed in an <Annot> structure element",
        );
    }
    match s {
        b"Widget" => {
            if rect.is_some_and(|r| r.urx - r.llx == 0.0 && r.ury - r.lly == 0.0) {
                fail("PUI2-8.9.2.4.13-1", "has zero width and height");
            }
            if parent != b"Form" {
                fail(
                    "PUI2-8.10.1-1",
                    "is not enclosed in a <Form> structure element",
                );
            }
        }
        b"Link" if !matches!(parent, b"Link" | b"Reference") => {
            fail(
                "PUI2-8.2.5.20-1",
                "is not enclosed in <Link> or <Reference>",
            );
        }
        b"PrinterMark" => fail(
            "PUI2-8.9.2.4.14-1",
            "is real content; it must be an artifact",
        ),
        b"Watermark" if parent != b"Annot" => {
            fail(
                "PUI2-8.9.2.4.16-1",
                "is not enclosed in an <Annot> structure element",
            );
        }
        _ => {}
    }
}

/// ISO 14289-2 §8.9.2.4.7 … §8.9.2.4.19: per-subtype rules that bind every
/// annotation of the subtype.
fn ua2_subtype(view: &DocumentView<'_>, a: &Annot<'_>, page: usize, out: &mut Out) {
    let at = Some(page);
    let t = FindingTarget::Annotation(a.id);
    let has_contents = a.dict.contains_key(b"Contents");
    let mut fail = |row: &'static str, what: &str| {
        out.violation("", row, at, t, format!("{} {what}.", a.label()));
    };
    match a.subtype {
        b"Stamp" if !a.dict.contains_key(b"Name") && !has_contents => {
            fail("PUI2-8.9.2.4.7-1", "has neither /Name nor /Contents");
        }
        b"Ink" if !has_contents => fail("PUI2-8.9.2.4.8-1", "has no /Contents"),
        b"Screen" if !has_contents => fail("PUI2-8.9.2.4.12-1", "has no /Contents"),
        b"3D" if !has_contents => fail("PUI2-8.9.2.4.19-1", "has no /Contents"),
        b"RichMedia" if !has_contents => fail("PUI2-8.9.2.4.19-2", "has no /Contents"),
        b"Sound" => fail("PUI2-8.9.2.4.11-1", "uses the deprecated Sound type"),
        b"Movie" => fail("PUI2-8.9.2.4.11-2", "uses the deprecated Movie type"),
        b"TrapNet" => fail("PUI2-8.9.2.4.15-1", "uses the deprecated TrapNet type"),
        b"FileAttachment" => {
            let spec = a
                .dict
                .get(b"FS")
                .map(|o| view.resolve(o))
                .and_then(Object::as_dict);
            if spec.is_some_and(|fs| !fs.contains_key(b"AFRelationship")) {
                fail(
                    "PUI2-8.9.2.4.10-1",
                    "has a file specification with no /AFRelationship",
                );
            }
        }
        _ => {}
    }
}

/// The `<Form>` element rules: ISO 14289-1 §7.18.4 (veraPDF 7.18.4-2: a
/// `/Role` attribute or exactly one interactive kid) and ISO 14289-2
/// §8.10.1 (veraPDF 8.10.1-2: at most one widget).
pub(super) fn form_element(view: &DocumentView<'_>, e: &Elem, out: &mut Out) {
    if e.std != b"Form" {
        return;
    }
    let target =
        e.id.map_or(FindingTarget::Document, FindingTarget::StructElem);
    let shown = e.id.map_or_else(
        || "A <Form> element".to_owned(),
        |id| format!("The <Form> {id}"),
    );
    match out.part {
        PdfuaPart::Ua1 if !e.role_attr && e.objr.len() != 1 => {
            let n = e.objr.len();
            let message =
                format!("{shown} has no /Role attribute and {n} interactive kids, not one.");
            out.violation("PUI-V-7.18.4-2", "", None, target, message);
        }
        PdfuaPart::Ua2 => {
            let widgets = e
                .objr
                .iter()
                .filter(|id| {
                    let d = view.graph().value(**id).and_then(Object::as_dict);
                    d.and_then(|d| name_of(view, d.get(b"Subtype"))) == Some(b"Widget")
                })
                .count();
            if widgets > 1 {
                let message = format!("{shown} encloses {widgets} widget annotations.");
                out.violation("", "PUI2-8.10.1-2", None, target, message);
            }
        }
        PdfuaPart::Ua1 => {}
    }
}
