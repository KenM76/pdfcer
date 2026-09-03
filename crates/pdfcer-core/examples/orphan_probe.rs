//! Throwaway measurement: **what does an orphaned widget actually look like**
//! after `insert_pages` brings a page across from an AcroForm document?
//!
//! Written for `Pass 103.1` (adopt an existing widget into a field), because
//! the verb's shape depends entirely on the answer and guessing would pick
//! the wrong one. Two possibilities, with very different costs:
//!
//! 1. **The widget is self-describing** — a merged field-widget carrying its
//!    own `/FT`, `/T`, `/V`, `/DA`. Adoption is then "append it to
//!    `/AcroForm/Fields`", and nothing needs re-authoring.
//! 2. **The widget is a bare kid** — no field keys, only `/Parent` pointing
//!    at a field dictionary. Adoption then depends on whether that parent
//!    came across too. If it did, adoption is "re-register the parent". If
//!    it did not, the widget has no name, no type and no value anywhere, and
//!    pdfcer would have to invent them.
//!
//! `fixtures/external/pdfbox/.../compression/acroform.pdf` carries **both**
//! shapes in one file — 12 fields over 13 widgets, of which `GroupOption` is
//! a two-kid radio group — which is why it is the probe subject.
//!
//! Run: `cargo run -p pdfcer-core --example orphan_probe`

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::pageops::InsertPosition;

fn main() {
    let src_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/external/pdfbox/pdfbox/src/test/resources/input/compression/acroform.pdf"
    );
    let Ok(src_bytes) = std::fs::read(src_path) else {
        eprintln!("SKIP: the external pdfbox corpus is not present");
        return;
    };
    let src = Document::from_bytes(src_bytes).expect("source must parse");

    // A blank one-page target, so every widget in the result arrived from
    // the source and nothing is ambiguous about provenance.
    let target_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/outline/no-outline.pdf"
    );
    let target = Document::from_bytes(std::fs::read(target_path).expect("target"))
        .expect("target must parse");
    let mut session = EditSession::new(target);

    let outcome = session
        .insert_pages(&src.view(), &[0], InsertPosition::End)
        .expect("insert must succeed");
    println!(
        "inserted={} orphaned_widgets={} unrecoverable={}",
        outcome.pages_inserted, outcome.orphaned_widgets, outcome.orphaned_widgets_unrecoverable
    );

    let view = session.view();
    let has_acroform = view
        .catalog_id()
        .and_then(|id| match view.resolved(id) {
            Object::Dict(d) => d.get(b"AcroForm").cloned(),
            _ => None,
        })
        .is_some();
    println!("target has /AcroForm afterwards: {has_acroform}");

    let pages = session.page_slots().expect("pages");
    let inserted = pages.last().expect("at least one page");
    let Object::Dict(page) = view.resolved(inserted.id) else {
        panic!("page is not a dict")
    };
    let Some(annots) = page.get(b"Annots") else {
        println!("the inserted page has NO /Annots");
        return;
    };
    let Object::Array(annots) = view.resolve(annots).clone() else {
        panic!("/Annots is not an array")
    };

    // The SOURCE side of the widgets with no `/T`, so a key the copy DROPPED
    // is distinguishable from one that was never there. That distinction is
    // the finding: the source kids carry `/Parent` and the copies do not.
    println!("--- SOURCE page 0, the widgets with no /T ---");
    let sv = src.view();
    let src_slots = pdfcer_core::page_tree::page_slots(sv.graph()).expect("src pages");
    let src_annots = match sv.resolved(src_slots[0].id) {
        Object::Dict(sp) => match sp.get(b"Annots").map(|a| sv.resolve(a).clone()) {
            Some(Object::Array(arr)) => arr,
            _ => Vec::new(),
        },
        _ => Vec::new(),
    };
    for r in &src_annots {
        let Object::Reference(id) = r else { continue };
        let Object::Dict(d) = sv.resolved(*id) else {
            continue;
        };
        if d.get(b"T").is_some() {
            continue;
        }
        let keys: Vec<String> = d
            .iter()
            .map(|(k, _)| String::from_utf8_lossy(&k.0).into_owned())
            .collect();
        println!("  src obj {:>4}: {}", id.num, keys.join(" "));
        if let Some(Object::Reference(p)) = d.get(b"Parent") {
            report_parent(&sv, *p);
        }
    }

    println!("\n{} annotation(s) on the inserted page:", annots.len());
    for (i, a) in annots.iter().enumerate() {
        let Object::Reference(id) = a else {
            println!("  [{i}] not a reference");
            continue;
        };
        let Object::Dict(d) = view.resolved(*id) else {
            println!("  [{i}] {id:?} does not resolve to a dict");
            continue;
        };
        let key = |k: &[u8]| -> String {
            match d.get(k) {
                Some(Object::Name(n)) => format!("/{}", String::from_utf8_lossy(&n.0)),
                Some(Object::String(s)) => format!("{:?}", String::from_utf8_lossy(s)),
                Some(Object::Reference(r)) => format!("{} 0 R", r.num),
                Some(_) => "<other>".to_owned(),
                None => "-".to_owned(),
            }
        };
        println!(
            "  [{i}] obj {:>4}  FT={:<6} T={:<24} V={:<10} Parent={}",
            id.num,
            key(b"FT"),
            key(b"T"),
            key(b"V"),
            key(b"Parent"),
        );

        // If it is a bare kid, does its parent field dictionary exist in the
        // TARGET? That is the whole question for shape 2.
        if d.get(b"T").is_none() {
            let keys: Vec<String> = d
                .iter()
                .map(|(k, _)| String::from_utf8_lossy(&k.0).into_owned())
                .collect();
            println!("        every key it has: {}", keys.join(" "));
            match d.get(b"Parent") {
                Some(Object::Reference(p)) => report_parent(&view, *p),
                Some(other) => println!("        /Parent is not a reference: {other:?}"),
                None => println!("        NO /Parent KEY AT ALL"),
            }
        }
    }
}

fn report_parent(view: &impl ObjectGraph, p: ObjId) {
    match view.resolved(p) {
        Object::Dict(pd) => {
            let t = match pd.get(b"T") {
                Some(Object::String(s)) => String::from_utf8_lossy(s).into_owned(),
                _ => "<no /T>".to_owned(),
            };
            let ft = match pd.get(b"FT") {
                Some(Object::Name(n)) => format!("/{}", String::from_utf8_lossy(&n.0)),
                _ => "-".to_owned(),
            };
            let kids = match pd.get(b"Kids") {
                Some(Object::Array(k)) => k.len(),
                _ => 0,
            };
            println!(
                "        -> parent obj {} EXISTS: T={t:?} FT={ft} kids={kids}",
                p.num
            );
        }
        other => println!("        -> parent obj {} is MISSING ({other:?})", p.num),
    }
}
