//! The font inventory runs over an editing session's overlay as well as over
//! a loaded file, and resolves stream spans against the session's split byte
//! source. A `&Document`-only signature would have made this impossible
//! without a second walk.

#![allow(clippy::unwrap_used)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::fontinfo::inventory;

#[test]
fn inventory_runs_over_an_edit_session_view() {
    let doc = Document::from_bytes(
        include_bytes!("../../../fixtures/synthetic/text/subset-simple-embedded.pdf").to_vec(),
    )
    .unwrap();
    let from_document = inventory(&doc.view());
    let session = EditSession::new(doc);
    let from_session = inventory(&session.view());
    assert_eq!(from_document, from_session);
}
