//! `EditSession::insert_pages` — the session-mutating twin of
//! [`pdfcer_core::pageops::insert`].
//!
//! # Why this file exists
//!
//! The `pdfcer-gui` session declined to wire "insert from file" and said why:
//!
//! > *"`pageops::insert` returns new document bytes rather than mutating the
//! > session, so wiring it as-is would discard your undo history. The session
//! > already has `delete_pages`, `reorder_pages`, `rotate_pages` — insert is
//! > the missing member of that family."*
//!
//! That is a correctness claim about **undo**, not about page counts, so the
//! tests here assert the undo property directly. A test that only checked
//! "the page arrived" would pass against the very implementation the report
//! was refusing to ship.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::object::Object;
use pdfcer_core::pageops::InsertPosition;

/// Build a minimal multi-page PDF whose pages carry distinguishable
/// content, so an inserted page can be told apart from a native one.
/// A one-page document whose page carries two widgets, registered in an
/// `/AcroForm` -- the minimum that makes `insert_pages` report orphaned
/// widgets when the target has no field tree of its own.
fn doc_with_one_form_page() -> Document {
    let objects: Vec<(u32, &str)> = vec![
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [4 0 R 5 0 R] >> >>",
        ),
        (
            2,
            "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 200 200] /Resources << >> >>",
        ),
        (3, "<< /Type /Page /Parent 2 0 R /Annots [4 0 R 5 0 R] >>"),
        (
            4,
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Name) /Rect [10 150 190 180] >>",
        ),
        (
            5,
            "<< /Type /Annot /Subtype /Widget /FT /Tx /T (Address) /Rect [10 100 190 130] >>",
        ),
    ];
    let mut out = String::from(
        "%PDF-1.7
",
    );
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &objects {
        offsets.push((*num, out.len()));
        out.push_str(&format!(
            "{num} 0 obj
{body}
endobj
"
        ));
    }
    let xref_at = out.len();
    out.push_str(&format!(
        "xref
0 {}
0000000000 65535 f 
",
        objects.len() + 1
    ));
    for (_, off) in &offsets {
        out.push_str(&format!(
            "{off:010} 00000 n 
"
        ));
    }
    out.push_str(&format!(
        "trailer
<< /Size {} /Root 1 0 R >>
startxref
{xref_at}
%%EOF
",
        objects.len() + 1
    ));
    Document::from_bytes(out.into_bytes()).expect("synthetic form page parses")
}

fn doc_with_pages(marker: &str, count: usize) -> Document {
    let mut objects: Vec<(u32, String)> = Vec::new();
    let kids: Vec<String> = (0..count).map(|i| format!("{} 0 R", 3 + i * 2)).collect();
    objects.push((1, "<< /Type /Catalog /Pages 2 0 R >>".to_owned()));
    objects.push((
        2,
        format!(
            "<< /Type /Pages /Kids [{}] /Count {count} /MediaBox [0 0 200 200] \
             /Resources << >> >>",
            kids.join(" ")
        ),
    ));
    for i in 0..count {
        let page_num = 3 + i * 2;
        let content_num = page_num + 1;
        objects.push((
            page_num as u32,
            format!("<< /Type /Page /Parent 2 0 R /Contents {content_num} 0 R >>"),
        ));
        let payload = format!("% {marker}-{i}\n");
        objects.push((
            content_num as u32,
            format!(
                "<< /Length {} >>\nstream\n{payload}endstream",
                payload.len()
            ),
        ));
    }

    let mut out = String::from("%PDF-1.7\n");
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in &objects {
        offsets.push((*num, out.len()));
        out.push_str(&format!("{num} 0 obj\n{body}\nendobj\n"));
    }
    let xref_at = out.len();
    let max = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
    out.push_str(&format!("xref\n0 {}\n0000000000 65535 f \n", max + 1));
    for n in 1..=max {
        let off = offsets
            .iter()
            .find(|(num, _)| *num == n)
            .map_or(0, |(_, o)| *o);
        out.push_str(&format!("{off:010} 00000 n \n"));
    }
    out.push_str(&format!(
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
        max + 1
    ));
    Document::from_bytes(out.into_bytes()).expect("fixture must load")
}

fn page_count(session: &EditSession) -> usize {
    session.page_slots().expect("page tree walks").len()
}

/// THE PROPERTY THE REPORT WAS ABOUT.
///
/// Insert, then undo, and the document is back where it started — and,
/// crucially, an edit made *before* the insert is still undoable after it.
/// That second half is what "discard your undo history" actually costs, and
/// a page-count check alone would not see it.
#[test]
fn inserting_pages_leaves_earlier_history_undoable() {
    let target = doc_with_pages("target", 2);
    let src_doc = doc_with_pages("source", 1);
    let mut session = EditSession::new(target);

    // An edit BEFORE the insert, so there is history to lose.
    session
        .rotate_pages(&[0], 90)
        .expect("rotate is a page-attribute change");
    assert_eq!(page_count(&session), 2);

    let src_view = src_doc.view();
    let added = session
        .insert_pages(&src_view, &[0], InsertPosition::End)
        .expect("insert succeeds");
    assert_eq!(added.pages_inserted, 1);
    assert_eq!(
        added.orphaned_widgets, 0,
        "this fixture's page carries no widgets, so nothing is orphaned"
    );
    assert_eq!(page_count(&session), 3);

    // Undo the insert.
    assert!(session.undo().is_some(), "the insert must be undoable");
    assert_eq!(page_count(&session), 2, "the inserted page is gone");

    // And the PRIOR command is still on the stack. If `insert_pages` had
    // replaced the session the way `pageops::insert` forces a caller to,
    // this is the assertion that would fail.
    assert!(
        session.undo().is_some(),
        "the rotate before the insert is still undoable"
    );
    assert!(
        session.undo().is_none(),
        "the stack is now empty; nothing else was recorded"
    );
}

/// Redo restores the insert, so it is a real command rather than a one-way
/// mutation that merely happens to be reversible.
#[test]
fn an_undone_insert_can_be_redone() {
    let target = doc_with_pages("target", 1);
    let src_doc = doc_with_pages("source", 2);
    let mut session = EditSession::new(target);
    let src_view = src_doc.view();

    session
        .insert_pages(&src_view, &[0, 1], InsertPosition::Start)
        .expect("insert succeeds");
    assert_eq!(page_count(&session), 3);
    assert!(session.undo().is_some());
    assert_eq!(page_count(&session), 1);
    assert!(session.redo().is_some(), "an insert must redo");
    assert_eq!(page_count(&session), 3);
}

/// The inserted page's CONTENT comes across, not just its dictionary.
///
/// This is the half that a reference-remapping bug breaks silently: the page
/// arrives, the count is right, and the page is blank because `/Contents`
/// still points at an object number that means something else in this
/// document — or nothing at all.
#[test]
fn the_inserted_pages_content_stream_is_copied_and_repointed() {
    let target = doc_with_pages("target", 1);
    let src_doc = doc_with_pages("source", 1);
    let mut session = EditSession::new(target);
    let src_view = src_doc.view();

    session
        .insert_pages(&src_view, &[0], InsertPosition::End)
        .expect("insert succeeds");

    let slots = session.page_slots().expect("page tree walks");
    let inserted = slots.last().expect("a last page");
    let page = session
        .value(inserted.id)
        .and_then(Object::as_dict)
        .cloned()
        .expect("the inserted page is a dictionary");

    // /Parent points into THIS document's tree, not the source's.
    let parent = page.get(b"Parent").and_then(Object::as_reference);
    assert_eq!(
        parent, inserted.parent,
        "the inserted page must belong to the target's page tree"
    );

    // /Contents resolves, and carries the SOURCE's marker.
    let contents_id = page
        .get(b"Contents")
        .and_then(Object::as_reference)
        .expect("/Contents is an indirect reference");
    let stream = session
        .value(contents_id)
        .cloned()
        .expect("the copied content stream exists in this session");
    let Object::Stream(stream) = stream else {
        panic!("/Contents must resolve to a stream");
    };
    let bytes = session
        .view()
        .source()
        .slice(stream.data_span)
        .expect("the staged payload is addressable in the session's coordinates");
    assert!(
        bytes.starts_with(b"% source-0"),
        "the copied stream must carry the SOURCE's bytes, got {:?}",
        String::from_utf8_lossy(bytes)
    );
}

/// An out-of-range source index is refused by name, against the SOURCE's
/// page count — the document the index actually addresses.
#[test]
fn an_out_of_range_source_page_is_refused_against_the_source() {
    let target = doc_with_pages("target", 5);
    let src_doc = doc_with_pages("source", 2);
    let mut session = EditSession::new(target);
    let src_view = src_doc.view();

    let err = session
        .insert_pages(&src_view, &[7], InsertPosition::End)
        .expect_err("page 7 of a 2-page source does not exist");
    let text = err.to_string();
    assert!(
        text.contains('2'),
        "the refusal must name the SOURCE's count (2), not the target's (5): {text}"
    );
    assert_eq!(
        page_count(&session),
        5,
        "a refused insert must not have changed the document"
    );
}

/// Inserting nothing is a successful no-op that records no command, so an
/// empty selection cannot pollute the undo stack.
#[test]
fn inserting_no_pages_records_no_command() {
    let target = doc_with_pages("target", 1);
    let src_doc = doc_with_pages("source", 1);
    let mut session = EditSession::new(target);
    let src_view = src_doc.view();

    assert_eq!(
        session
            .insert_pages(&src_view, &[], InsertPosition::End)
            .expect("an empty insert is not an error"),
        pdfcer_core::edit::InsertOutcome::default()
    );
    assert!(
        session.undo().is_none(),
        "nothing should be on the undo stack"
    );
}

/// END TO END: the saved FILE has the inserted page, and it reloads.
///
/// Every assertion above is about session state. This one is about bytes,
/// because "the in-memory tree says three pages" and "the file a viewer
/// opens has three pages" are different claims, and only the second is what
/// an operator gets. It is also the check that would catch a copied stream
/// staged into the session buffer but never written into the appended
/// revision.
#[test]
fn the_saved_file_carries_the_inserted_page_and_reloads() {
    let target = doc_with_pages("target", 2);
    let src_doc = doc_with_pages("source", 1);
    let mut session = EditSession::new(target);
    let src_view = src_doc.view();

    session
        .insert_pages(&src_view, &[0], InsertPosition::End)
        .expect("insert succeeds");

    let (bytes, _report) = session
        .to_incremental_bytes(&pdfcer_core::writer::SaveOptions::default())
        .expect("an incremental save of an insert must succeed");

    let reloaded = Document::from_bytes(bytes).expect("the saved file must reload");
    let slots =
        pdfcer_core::page_tree::page_slots(&reloaded).expect("the reloaded page tree walks");
    assert_eq!(
        slots.len(),
        3,
        "the saved file must carry all three pages, not just the session"
    );
}

/// Inserting a page of form fields carries the fields (`Pass 102.1`), so the
/// widgets arrive fillable and none is orphaned.
///
/// Both fields' widgets are on the inserted page, so both are wholly carried;
/// the target had no `/AcroForm`, so one is created. Undo removes the fields
/// with the page, because it is one command.
#[test]
fn inserting_a_form_page_carries_its_fields() {
    let src = doc_with_one_form_page();
    let src_view = src.view();
    let target = doc_with_pages("target", 2);
    let mut session = EditSession::new(target);
    let out = session
        .insert_pages(&src_view, &[0], InsertPosition::End)
        .expect("insert succeeds");

    assert_eq!(out.pages_inserted, 1);
    assert_eq!(out.fields_carried, 2);
    assert_eq!(out.orphaned_widgets, 0, "every widget's field came across");
    assert_eq!(out.orphaned_widgets_unrecoverable, 0);
    assert_eq!(out.fields_split, 0);
    assert!(out.acroform_created);

    let mut names: Vec<String> = pdfcer_core::forms::parse_acroform(&session.view())
        .expect("the target gained an /AcroForm")
        .fields
        .iter()
        .map(|f| f.fully_qualified_name.clone())
        .collect();
    names.sort();
    assert_eq!(names, ["Address", "Name"]);

    assert!(session.undo().is_some());
    assert!(
        pdfcer_core::forms::parse_acroform(&session.view()).is_none(),
        "one undo removes the carried fields with the page"
    );
}
