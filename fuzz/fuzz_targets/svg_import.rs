//! Fuzz target: SVG import and placement (`svg_import::import`,
//! `EditSession::add_svg`).
//!
//! usvg parses the XML and resolves CSS; pdfcer's own code is the
//! pre-scan (gzip ceiling, nesting depth over `use`/clip/mask/pattern
//! references), the tree-to-content translation (gradient unrolling,
//! stop stitching, tiling-pattern recursion, soft-mask groups) and the
//! object-id remap at placement. Those are the targets.
//!
//! Invariant: any input yields `Ok` or a structured `SvgImportError`;
//! a successful import places on a page, saves, and the saved bytes
//! reopen. Seeds are hand-written SVGs only (`docs/LEGAL.md` §5).

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::svg_import;
use pdfcer_core::writer::SaveOptions;

const BLANK: &[u8] = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>\nendobj\n\
trailer\n<< /Size 4 /Root 1 0 R >>\n%%EOF\n";

fuzz_target!(|data: &[u8]| {
    let Ok(svg) = svg_import::import(data) else {
        return;
    };
    let (w, h) = svg.size_px();
    assert!(w > 0.0 && h > 0.0, "an imported SVG has a size");
    assert!(svg.object_count() >= 1, "the root form is always present");
    assert_eq!(svg.notes().is_empty(), svg.notes().summary().is_empty());

    let doc = Document::from_bytes(BLANK.to_vec()).expect("the blank page opens");
    let mut session = EditSession::new(doc);
    let rect = Rect {
        llx: 10.0,
        lly: 10.0,
        urx: 190.0,
        ury: 90.0,
    };
    let placed = session
        .add_svg(0, rect, &svg)
        .expect("a valid import places");
    assert_eq!(placed.notes, *svg.notes());
    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("the placement saves");
    Document::from_bytes(bytes).expect("the saved file reopens");
});
