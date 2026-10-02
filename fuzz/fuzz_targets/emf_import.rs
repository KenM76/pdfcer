//! Fuzz target: EMF import and placement (`emf_import::import`,
//! `EditSession::add_emf`).
//!
//! Every byte is pdfcer's own parsing: record framing, the header, object
//! tables, the DC stack, poly and path records, clip regions, DIB headers
//! handed to the image importer, and EMR_EXTTEXTOUTW offsets.
//!
//! Invariant: any input yields `Ok` or a structured `EmfImportError`; a
//! successful import places on a page, saves, and the saved bytes reopen.
//! Seeds are built record by record from [MS-EMF] (`docs/LEGAL.md` §5).

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::emf_import;
use pdfcer_core::page_tree::Rect;
use pdfcer_core::writer::SaveOptions;

const BLANK: &[u8] = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n\
3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>\nendobj\n\
trailer\n<< /Size 4 /Root 1 0 R >>\n%%EOF\n";

fuzz_target!(|data: &[u8]| {
    let Ok(emf) = emf_import::import(data) else {
        return;
    };
    let (w, h) = emf.natural_size_pt();
    assert!(w > 0.0 && h > 0.0, "an imported EMF has a size");
    assert_eq!(emf.notes().is_empty(), emf.notes().summary().is_empty());

    let doc = Document::from_bytes(BLANK.to_vec()).expect("the blank page opens");
    let mut session = EditSession::new(doc);
    let rect = Rect {
        llx: 10.0,
        lly: 10.0,
        urx: 190.0,
        ury: 90.0,
    };
    let placed = session
        .add_emf(0, rect, &emf)
        .expect("a valid import places");
    assert_eq!(placed.notes, *emf.notes());
    let (bytes, _) = session
        .to_full_bytes(&SaveOptions::default())
        .expect("the placement saves");
    Document::from_bytes(bytes).expect("the saved file reopens");
});
