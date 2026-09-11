//! Dump one page's DECODED content stream, with a window around a byte offset.
//!
//! Written 2026-09-11 to find why `redact-offpage`'s output had three pages
//! that neither pdfcer nor its renderer could read (*"malformed operand at
//! byte 6701"*) while the input parsed cleanly. Ad-hoc zlib scanning from a
//! script kept landing on font programs; this asks the same parser that
//! reports the error.
//!
//! ```text
//! cargo run -p pdfcer-core --example offpage_probe -- <file.pdf> <page-1based> <offset>
//! ```

use pdfcer_core::content::ContentStream;
use pdfcer_core::document::Document;
use pdfcer_core::page_tree;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let Some(path) = args.get(1) else {
        eprintln!("usage: offpage_probe <file.pdf> <page-1based> [offset]");
        std::process::exit(2);
    };
    let page_no: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let offset: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(0);

    let bytes = std::fs::read(path).expect("read");
    let doc = Document::from_bytes(bytes).expect("open");
    let pages = page_tree::pages(&doc).expect("pages");
    let page = pages.get(page_no - 1).expect("page in range");
    let view = doc.view();

    // Concatenate exactly as the reader does, WITHOUT parsing -- the parse is
    // what fails, so this has to see the bytes it fails on.
    let mut buf: Vec<u8> = Vec::new();
    for id in &page.contents {
        let Some(pdfcer_core::object::Object::Stream(s)) = view.graph().value(*id) else {
            continue;
        };
        let raw = view.slice(s.data_span).unwrap_or_default();
        let decoded = pdfcer_core::filters::decode_stream(&s.dict, raw).unwrap_or_default();
        buf.extend_from_slice(&decoded);
    }
    println!("decoded content: {} bytes", buf.len());

    let lo = offset.saturating_sub(120).min(buf.len());
    let hi = (offset + 120).min(buf.len());
    println!("--- bytes {lo}..{hi} ---");
    println!("{}", String::from_utf8_lossy(&buf[lo..hi]));
    println!("--- end ---");

    match ContentStream::parse(buf.clone()) {
        Ok(_) => println!("PARSES CLEAN"),
        Err(e) => println!("PARSE ERROR: {e}"),
    }
}
