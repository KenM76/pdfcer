//! Probe: read a stamp collection file and print what pdfcer sees in it.
//!
//! Written to check `stamp_file::read` against **Adobe's own shipped stamp
//! files** rather than against a fixture pdfcer authored — a reader that only
//! ever sees its own writer's output proves nothing about compatibility.
//!
//! ```text
//! cargo run -p pdfcer-core --example stamp_file_probe -- <stamp.pdf> [more...]
//! ```

use pdfcer_core::document::Document;
use pdfcer_core::stamp_file;
use std::path::Path;

fn main() {
    for arg in std::env::args().skip(1) {
        let path = Path::new(&arg);
        let Ok(doc) = Document::load(path) else {
            println!("SKIP (does not open)  {arg}");
            continue;
        };
        let c = stamp_file::read(&doc);
        println!(
            "{}\n  category: {:?}   stamps: {}   is_stamp_file: {}",
            path.file_name().unwrap_or_default().to_string_lossy(),
            c.category,
            c.stamps.len(),
            c.is_stamp_file()
        );
        for s in &c.stamps {
            println!(
                "    {:<28} display={:<24} page={:?}{}",
                s.internal,
                s.display,
                s.page_index,
                if s.dynamic { "  DYNAMIC" } else { "" }
            );
        }
    }
}
