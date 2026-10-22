//! `DestinationReader::named_destinations` (pdfcer-gui request G179): both
//! §12.3.2.3 namespaces, merged as the resolver merges them, each name with
//! the page it resolves to.

use pdfcer_core::document::Document;
use pdfcer_core::outline::{DestView, Destination, DestinationReader};

/// Two pages. Legacy `/Dests` defines `a` (page 1) and `alias` (a name
/// pointing at `a`); the name tree defines `a` again (page 0, so it wins),
/// `b` (a `/D` wrapper to page 1) and `bad` (a number).
const DOC: &[u8] = b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R /Dests << /a [4 0 R /Fit] /alias /b >> /Names << /Dests 5 0 R >> >> endobj\n\
2 0 obj << /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >> endobj\n\
3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >> endobj\n\
4 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >> endobj\n\
5 0 obj << /Names [(a) [3 0 R /Fit] (b) << /D [4 0 R /FitH 500] >> (bad) 7] >> endobj\n\
trailer << /Size 6 /Root 1 0 R >>\n";

#[test]
fn both_namespaces_are_listed_merged_sorted_and_resolved() {
    let doc = Document::from_bytes(DOC.to_vec()).expect("rebuildable");
    let reader = DestinationReader::new(&doc);
    let listed = reader.named_destinations(&doc);
    assert_eq!(listed.len(), reader.named_destination_count());

    let names: Vec<&[u8]> = listed.iter().map(|(n, _)| n.as_slice()).collect();
    assert_eq!(names, [&b"a"[..], b"alias", b"b", b"bad"]);

    let page = |i: usize| listed.get(i).and_then(|(_, d)| d.clone());
    assert_eq!(
        page(0),
        Some(Destination::Page {
            page_index: 0,
            view: DestView::Fit
        }),
        "the name tree wins a key both namespaces define"
    );
    assert!(matches!(
        page(1),
        Some(Destination::Page { page_index: 1, .. })
    ));
    assert!(matches!(
        page(2),
        Some(Destination::Page { page_index: 1, .. })
    ));
    assert_eq!(
        page(3),
        None,
        "a defined name with no usable value is listed"
    );
}

#[test]
fn a_document_without_names_lists_none() {
    let doc = Document::from_bytes(
        b"%PDF-1.7\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
2 0 obj << /Type /Pages /Kids [] /Count 0 >> endobj\n\
trailer << /Size 3 /Root 1 0 R >>\n"
            .to_vec(),
    )
    .expect("rebuildable");
    assert!(
        DestinationReader::new(&doc)
            .named_destinations(&doc)
            .is_empty()
    );
}

#[test]
fn one_name_resolves_as_its_listed_entry() {
    let doc = Document::from_bytes(DOC.to_vec()).expect("rebuildable");
    let reader = DestinationReader::new(&doc);
    for (name, listed) in reader.named_destinations(&doc) {
        assert_eq!(reader.named(&doc, &name), listed, "{name:?}");
    }
    assert!(matches!(
        reader.named(&doc, b"a"),
        Some(Destination::Page { page_index: 0, .. })
    ));
    assert_eq!(reader.named(&doc, b"undefined"), None);
}
