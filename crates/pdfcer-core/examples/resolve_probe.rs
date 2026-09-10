//! Probe: which objects did the loader actually recover, and does the page's
//! `/Contents` resolve?
//!
//! Written for `Annotations_output.pdf` (PDFsharp), whose `startxref` points
//! 134 bytes short of its own `xref` keyword — a producer bug. pdfcer's
//! recovery opens it, and the render reported `contents_unresolved=1`, so the
//! question is whether an object that IS in the file was nevertheless lost.

use pdfcer_core::document::Document;
use pdfcer_core::object::Object;
use std::path::Path;

fn main() {
    let Some(arg) = std::env::args().nth(1) else {
        eprintln!("usage: resolve_probe <file.pdf>");
        return;
    };
    let doc = Document::load(Path::new(&arg)).expect("the file opens");

    println!("recovered objects: {}", doc.objects().count());
    for io in doc.objects() {
        let kind = match &io.value {
            Object::Stream(s) => format!("stream len={}", s.data_span.len),
            Object::Dict(d) => format!(
                "dict {}",
                d.get(b"Type")
                    .map_or_else(|| "(untyped)".to_owned(), |o| format!("{o:?}"))
            ),
            other => format!("{other:?}"),
        };
        println!("  {} 0 -> {kind}", io.id.num);
    }

    for a in doc.load_anomalies() {
        println!("anomaly: {} {:?}", a.kind(), a.object());
    }

    match pdfcer_core::page_tree::pages(&doc) {
        Ok(pages) => {
            for (i, p) in pages.iter().enumerate() {
                let contents = doc
                    .get(p.id)
                    .and_then(|io| io.value.as_dict())
                    .and_then(|d| d.get(b"Contents").cloned());
                println!("page {i}: /Contents = {contents:?}");
                if let Some(Object::Reference(id)) = contents {
                    println!("   resolves to: {:?}", doc.get(id).map(|io| &io.value));
                }
            }
        }
        Err(e) => println!("pages(): {e}"),
    }
}
