//! `pdfcer extract-tags`: a tagged PDF's structure tree as text or JSON,
//! optionally with each element's painted bounds (`--painted-bounds`).

use super::*;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::structure_bounds::{NoBoundsReason, PaintedBounds, UnboundedElement};
use pdfcer_core::structure_tree::{StructKid, StructTreatment, StructureTree};
use pdfcer_core::text_extract::ContentStreamRef;

/// The painted-bounds pass, when `--painted-bounds` asked for it.
struct Painted {
    bounds: PaintedBounds,
    unbounded: Vec<UnboundedElement>,
}

impl Painted {
    fn reason(&self, element: usize) -> Option<NoBoundsReason> {
        self.unbounded
            .iter()
            .find(|u| u.element == element)
            .map(|u| u.reason)
    }
}

/// **Read a tagged PDF's structure tree**: elements in logical
/// order with their role-mapped type, text, page and box.
///
/// Text output is one element per line, indented by depth:
/// `<type>[ (<raw>)] p<page> "<text>"`. `--json` carries every field of
/// `pdfcer_core::structure_tree::StructElement` plus the element's text and
/// per-page boxes. Diagnostic notes go to stderr; the result line is on
/// stdout after the payload (or alone with `-o`).
///
/// `painted_bounds` adds, per element, the box of everything it paints
/// (text, paths, images, shadings, clipped), one per page: a
/// ` painted=p<page>:[llx,lly,urx,ury]` token per page in text, or
/// `"painted_boxes"` in JSON. Elements with none are named on stderr with a
/// reason, listed under JSON's top-level `"unbounded"`, and counted by
/// `painted_*` keys appended to the result line. Without the flag the
/// output is byte-identical to before it existed.
pub(crate) fn cmd_extract_tags(
    input: &Path,
    output: Option<&Path>,
    json: bool,
    painted_bounds: bool,
) -> u8 {
    use pdfcer_core::structure_bounds::read_painted_bounds;
    use pdfcer_core::structure_tree::read_structure_tree;
    use pdfcer_core::text_extract::ExtractOptions;

    let doc = match open_document(input) {
        Ok(doc) => doc,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit_code_for_doc(&err);
        }
    };
    let view = doc.view();
    let tree = match read_structure_tree(&view, &ExtractOptions::default()) {
        Ok(tree) => tree,
        Err(err) => {
            eprintln!("pdfcer: {}: {err}", input.display());
            return exit::RUNTIME_ERROR;
        }
    };
    let painted = if painted_bounds {
        match read_painted_bounds(&view, &tree) {
            Ok(bounds) => Some(Painted {
                unbounded: tree.elements_without_painted_bounds(&bounds),
                bounds,
            }),
            Err(err) => {
                eprintln!("pdfcer: {}: {err}", input.display());
                return exit::RUNTIME_ERROR;
            }
        }
    } else {
        None
    };
    let payload = if json {
        tags_json(&tree, painted.as_ref())
    } else {
        tags_text(&tree, painted.as_ref())
    };
    match output {
        Some(path) => {
            if let Err(err) = write_output(path, payload.as_bytes()) {
                eprintln!("pdfcer: {}: {err}", path.display());
                return exit::IO_ERROR;
            }
        }
        None => print!("{payload}"),
    }
    for note in &tree.diagnostics.notes {
        eprintln!("pdfcer: {note}");
    }
    if let Some(p) = &painted {
        painted_notes(&tree, p);
    }
    println!("{}", result_line(input, &tree, painted.as_ref()));
    exit::SUCCESS
}

fn result_line(input: &Path, tree: &StructureTree, painted: Option<&Painted>) -> String {
    let d = &tree.diagnostics;
    let mut line = format!(
        "tags {} struct_tree={} elements={} non_standard={} role_map_cycles={} \
revisited={} mcids_named={} named_not_declared={} declared_unclaimed={} \
claimed_twice={} object_refs={} malformed={} page_inherited={} page_unresolved={}",
        input.display(),
        d.struct_tree_present,
        d.elements,
        d.non_standard_types,
        d.role_map_cycles,
        d.elements_revisited,
        d.mcids_named,
        d.named_not_declared,
        d.declared_unclaimed,
        d.claimed_twice,
        d.object_refs,
        d.malformed_kids,
        d.page_inherited,
        d.page_unresolved,
    );
    if let Some(p) = painted {
        let pd = &p.bounds.diagnostics;
        line.push_str(&format!(
            " painted_unbounded={} painted_pages_unreadable={} painted_pages_truncated={} \
painted_form_cycles={} painted_form_depth_overflows={}",
            p.unbounded.len(),
            pd.pages_unreadable,
            pd.pages_truncated,
            pd.form_cycles,
            pd.form_depth_overflows,
        ));
    }
    line
}

/// One stderr line per element with no painted bounds, plus the walk's
/// own limits when any was hit.
fn painted_notes(tree: &StructureTree, p: &Painted) {
    for u in &p.unbounded {
        let ty = tree
            .elements
            .get(u.element)
            .map_or("?", |e| e.resolved_type.as_str());
        eprintln!(
            "pdfcer: element {} ({ty}): no painted bounds: {}",
            u.element, u.reason
        );
    }
    let d = &p.bounds.diagnostics;
    if d.pages_unreadable > 0 {
        eprintln!(
            "pdfcer: painted bounds: {} page(s) had unreadable content and paint nothing",
            d.pages_unreadable
        );
    }
    if d.pages_truncated > 0 {
        eprintln!(
            "pdfcer: painted bounds: {} page(s) exceeded the operation budget; their \
bounds cover only what was walked",
            d.pages_truncated
        );
    }
    if d.form_cycles + d.form_depth_overflows > 0 {
        eprintln!(
            "pdfcer: painted bounds: {} form XObject cycle(s) and {} over-deep form \
nesting(s) were skipped",
            d.form_cycles, d.form_depth_overflows
        );
    }
}

/// The stable JSON/text spelling of a reason; `other` for one added later.
fn reason_code(r: NoBoundsReason) -> &'static str {
    match r {
        NoBoundsReason::PageUnresolved => "page_unresolved",
        NoBoundsReason::McidNotFound => "mcid_not_found",
        NoBoundsReason::NothingPainted => "nothing_painted",
        NoBoundsReason::NoContent => "no_content",
        _ => "other",
    }
}

fn tags_text(tree: &StructureTree, painted: Option<&Painted>) -> String {
    let mut out = String::new();
    for (i, e) in tree.elements.iter().enumerate() {
        out.push_str(&"  ".repeat(e.depth));
        out.push_str(&e.resolved_type);
        if e.raw_type != e.resolved_type {
            out.push_str(&format!(" ({})", e.raw_type));
        }
        if let Some(p) = e.page_index {
            out.push_str(&format!(" p{}", p + 1));
        }
        let text = tree.element_text(i);
        if !text.is_empty() {
            out.push_str(&format!(" {text:?}"));
        }
        if let Some(p) = painted {
            for (page, r) in tree.element_painted_bounds(i, &p.bounds) {
                out.push_str(&format!(
                    " painted=p{}:[{:.2},{:.2},{:.2},{:.2}]",
                    page + 1,
                    r.llx,
                    r.lly,
                    r.urx,
                    r.ury
                ));
            }
            if let Some(reason) = p.reason(i) {
                out.push_str(&format!(" painted=none:{}", reason_code(reason)));
            }
        }
        out.push('\n');
    }
    out
}

fn tags_json(tree: &StructureTree, painted: Option<&Painted>) -> String {
    let mut out = String::from("{\n  \"elements\": [");
    for i in 0..tree.elements.len() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&element_json(tree, i, painted));
    }
    let roots: Vec<String> = tree.roots.iter().map(ToString::to_string).collect();
    let notes: Vec<String> = tree
        .diagnostics
        .notes
        .iter()
        .map(|t| format!("\"{}\"", json_escape(t)))
        .collect();
    out.push_str(&format!(
        "\n  ],\n  \"roots\": [{}],\n  \"notes\": [{}]",
        roots.join(", "),
        notes.join(", ")
    ));
    if let Some(p) = painted {
        let unbounded: Vec<String> = p
            .unbounded
            .iter()
            .map(|u| {
                format!(
                    "{{\"element\": {}, \"reason\": \"{}\"}}",
                    u.element,
                    reason_code(u.reason)
                )
            })
            .collect();
        out.push_str(&format!(",\n  \"unbounded\": [{}]", unbounded.join(", ")));
    }
    out.push_str("\n}\n");
    out
}

fn json_str(v: &Option<String>) -> String {
    v.as_deref()
        .map_or_else(|| "null".to_owned(), |t| format!("\"{}\"", json_escape(t)))
}

fn json_num<T: std::fmt::Display>(v: Option<T>) -> String {
    v.map_or_else(|| "null".to_owned(), |x| x.to_string())
}

fn boxes_json(boxes: &[(usize, Rect)]) -> String {
    let boxes: Vec<String> = boxes
        .iter()
        .map(|(p, r)| {
            format!(
                "{{\"page\": {}, \"rect\": [{:.2}, {:.2}, {:.2}, {:.2}]}}",
                p + 1,
                r.llx,
                r.lly,
                r.urx,
                r.ury
            )
        })
        .collect();
    boxes.join(", ")
}

fn kid_json(k: &StructKid) -> String {
    match k {
        StructKid::Element(c) => format!("{{\"element\": {c}}}"),
        StructKid::MarkedContent {
            page_index,
            stream,
            mcid,
            runs,
            declared,
        } => {
            let stream = match stream {
                ContentStreamRef::Form { object } => object.to_string(),
                _ => "null".to_owned(),
            };
            format!(
                "{{\"mcid\": {mcid}, \"page\": {}, \"form\": {stream}, \"declared\": {declared}, \"runs\": {runs:?}}}",
                json_num(page_index.map(|p| p + 1))
            )
        }
        StructKid::Object {
            page_index,
            object,
            subtype,
            ..
        } => format!(
            "{{\"object\": \"{object}\", \"page\": {}, \"subtype\": {}}}",
            json_num(page_index.map(|p| p + 1)),
            json_str(subtype)
        ),
        _ => "null".to_owned(),
    }
}

fn element_json(tree: &StructureTree, i: usize, painted: Option<&Painted>) -> String {
    let Some(e) = tree.elements.get(i) else {
        return String::new();
    };
    let treatment = match e.treatment {
        StructTreatment::NonStruct => "NonStruct",
        StructTreatment::Private => "Private",
        StructTreatment::Artifact => "Artifact",
        _ => "Normal",
    };
    let kids: Vec<String> = e.kids.iter().map(kid_json).collect();
    let headers: Vec<String> = e
        .headers
        .iter()
        .map(|h| format!("\"{}\"", json_escape(h)))
        .collect();
    let painted_boxes = painted.map_or_else(String::new, |p| {
        format!(
            ", \"painted_boxes\": [{}]",
            boxes_json(&tree.element_painted_bounds(i, &p.bounds))
        )
    });
    format!(
        "\n    {{\"index\": {i}, \"type\": \"{}\", \"raw_type\": \"{}\", \"namespace\": {}, \
\"standard\": {}, \"treatment\": \"{treatment}\", \"parent\": {}, \"depth\": {}, \
\"page\": {}, \"id\": {}, \"title\": {}, \"alt\": {}, \"actual_text\": {}, \
\"expansion\": {}, \"lang\": {}, \"effective_lang\": {}, \"row_span\": {}, \
\"col_span\": {}, \"scope\": {}, \"headers\": [{}], \"list_numbering\": {}, \
\"text\": \"{}\", \"boxes\": [{}]{painted_boxes}, \"kids\": [{}]}}",
        json_escape(&e.resolved_type),
        json_escape(&e.raw_type),
        json_str(&e.namespace),
        e.standard,
        json_num(e.parent),
        e.depth,
        json_num(e.page_index.map(|p| p + 1)),
        json_str(&e.id),
        json_str(&e.title),
        json_str(&e.alt),
        json_str(&e.actual_text),
        json_str(&e.expansion),
        json_str(&e.lang),
        json_str(&e.effective_lang),
        json_num(e.row_span),
        json_num(e.col_span),
        json_str(&e.scope),
        headers.join(", "),
        json_str(&e.list_numbering),
        json_escape(&tree.element_text(i)),
        boxes_json(&tree.element_bbox(i)),
        kids.join(", "),
    )
}
