//! `EditSession::insert_pages` carries form fields whose widgets all arrive
//! (`Pass 102.1`), and `merge_document` re-parents a nested field tree at
//! every depth and renames on a ROOT-name collision.
//!
//! The source has two pages and three top-level fields (ISO 32000-1
//! §12.7.3.1):
//!
//! - `Person` (`/FT /Tx`, inherited) → `Name` → merged widgets `First`, `Last`
//!   on page 0; and → `Addr` on page 1. Inserting page 0 carries
//!   `Person.Name.*` under a pruned `Person` and leaves `Addr` behind.
//! - `Choice`, a radio group with one bare kid on each page: split, so it is
//!   not carried and its page-0 kid is an unrecoverable orphan.
//! - `Sig1`, a signature field on page 1: never reached, so `/SigFlags` is not
//!   carried.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::forms::{FieldType, parse_acroform};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::pageops::InsertPosition;
use pdfcer_core::writer::SaveOptions;

fn build(objects: &[(u32, &str)]) -> Vec<u8> {
    let mut buf = b"%PDF-1.7\n".to_vec();
    let mut offsets: Vec<(u32, usize)> = Vec::new();
    for (num, body) in objects {
        offsets.push((*num, buf.len()));
        buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
    }
    let max = offsets.iter().map(|(n, _)| *n).max().unwrap_or(0);
    let xref_at = buf.len();
    buf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", max + 1).as_bytes());
    for n in 1..=max {
        match offsets.iter().find(|(k, _)| *k == n) {
            Some((_, off)) => buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            None => buf.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    buf.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
            max + 1
        )
        .as_bytes(),
    );
    buf
}

fn source() -> Document {
    Document::from_bytes(build(&[
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [10 0 R 20 0 R 30 0 R] /SigFlags 3 /DA (/Helv 0 Tf 0 g) >> >>",
        ),
        (2, "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Annots [12 0 R 13 0 R 21 0 R] >>",
        ),
        (
            4,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Annots [14 0 R 22 0 R 30 0 R] >>",
        ),
        (10, "<< /T (Person) /FT /Tx /Kids [11 0 R 14 0 R] >>"),
        (11, "<< /T (Name) /Parent 10 0 R /Kids [12 0 R 13 0 R] >>"),
        (
            12,
            "<< /Type /Annot /Subtype /Widget /T (First) /Parent 11 0 R /P 3 0 R /Rect [10 10 100 30] >>",
        ),
        (
            13,
            "<< /Type /Annot /Subtype /Widget /T (Last) /Parent 11 0 R /P 3 0 R /Rect [10 40 100 60] >>",
        ),
        (
            14,
            "<< /Type /Annot /Subtype /Widget /T (Addr) /Parent 10 0 R /P 4 0 R /Rect [10 10 100 30] >>",
        ),
        (20, "<< /T (Choice) /FT /Btn /Ff 49152 /Kids [21 0 R 22 0 R] >>"),
        (
            21,
            "<< /Type /Annot /Subtype /Widget /Parent 20 0 R /P 3 0 R /Rect [10 70 30 90] >>",
        ),
        (
            22,
            "<< /Type /Annot /Subtype /Widget /Parent 20 0 R /P 4 0 R /Rect [10 70 30 90] >>",
        ),
        (
            30,
            "<< /Type /Annot /Subtype /Widget /T (Sig1) /FT /Sig /P 4 0 R /Rect [10 100 100 130] >>",
        ),
    ]))
    .expect("source parses")
}

fn blank_target() -> Document {
    Document::from_bytes(build(&[
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] >>"),
    ]))
    .expect("target parses")
}

/// A target whose form already holds `Person.Other`: root name `Person`.
fn person_target() -> Document {
    Document::from_bytes(build(&[
        (
            1,
            "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [5 0 R] >> >>",
        ),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (
            3,
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 300 300] /Annots [6 0 R] >>",
        ),
        (5, "<< /T (Person) /FT /Tx /Kids [6 0 R] >>"),
        (
            6,
            "<< /Type /Annot /Subtype /Widget /T (Other) /Parent 5 0 R /P 3 0 R /Rect [0 0 50 20] >>",
        ),
    ]))
    .expect("target parses")
}

fn names(session: &EditSession) -> Vec<String> {
    let mut out: Vec<String> = parse_acroform(&session.view())
        .map(|f| {
            f.fields
                .iter()
                .map(|x| x.fully_qualified_name.clone())
                .collect()
        })
        .unwrap_or_default();
    out.sort();
    out
}

/// Every `/Kids` entry below `/AcroForm /Fields` points back at its parent
/// through `/Parent` — at every depth, which is what §12.7.3.2 inheritance
/// walks. Returns how many kids were checked.
fn assert_parent_links(session: &EditSession) -> usize {
    let view = session.view();
    let catalog = view.catalog_dict().expect("catalog");
    let form = view
        .resolve(catalog.get(b"AcroForm").expect("AcroForm"))
        .as_dict()
        .expect("dict")
        .clone();
    let Some(Object::Array(roots)) = form.get(b"Fields").map(|o| view.resolve(o).clone()) else {
        panic!("no /Fields")
    };
    let mut stack: Vec<ObjId> = roots.iter().filter_map(Object::as_reference).collect();
    let mut checked = 0;
    while let Some(id) = stack.pop() {
        let Object::Dict(node) = view.resolved(id) else {
            continue;
        };
        let Some(Object::Array(kids)) = node.get(b"Kids").map(|o| view.resolve(o).clone()) else {
            continue;
        };
        for kid in kids.iter().filter_map(Object::as_reference) {
            let Object::Dict(k) = view.resolved(kid) else {
                panic!("kid {kid:?} not a dict")
            };
            assert_eq!(
                k.get(b"Parent").and_then(Object::as_reference),
                Some(id),
                "kid {kid:?} must point back at {id:?}"
            );
            checked += 1;
            stack.push(kid);
        }
    }
    checked
}

fn saved(session: &EditSession) -> Vec<u8> {
    session
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0
}

fn contains(hay: &[u8], needle: &[u8]) -> bool {
    hay.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn a_wholly_inserted_field_is_carried_under_its_pruned_ancestors() {
    let src = source();
    let mut session = EditSession::new(blank_target());
    let out = session
        .insert_pages(&src.view(), &[0], InsertPosition::End)
        .expect("insert");

    assert_eq!(out.fields_carried, 2, "First and Last");
    assert_eq!(out.fields_split, 1, "Choice has a kid on each page");
    assert_eq!(out.fields_renamed, 0);
    assert!(out.acroform_created);
    assert_eq!(out.orphaned_widgets, 1, "Choice's page-0 kid");
    assert_eq!(out.orphaned_widgets_unrecoverable, 1, "it has no /T");
    assert_eq!(names(&session), ["Person.Name.First", "Person.Name.Last"]);
    assert_eq!(assert_parent_links(&session), 3, "Name, First, Last");

    // /FT lives only on `Person`; it reaches the grandchild only through the
    // pruned ancestor copy (§12.7.3.2).
    let form = parse_acroform(&session.view()).expect("form");
    assert!(
        form.fields
            .iter()
            .all(|f| f.field_type == Some(FieldType::Text)),
        "inherited /FT must resolve through the carried ancestors"
    );
}

#[test]
fn nothing_from_an_uninserted_page_comes_across() {
    let src = source();
    let mut session = EditSession::new(blank_target());
    session
        .insert_pages(&src.view(), &[0], InsertPosition::End)
        .expect("insert");
    let bytes = saved(&session);
    assert!(!contains(&bytes, b"(Addr)"), "the page-1 sibling widget");
    assert!(!contains(&bytes, b"(Sig1)"), "the page-1 signature field");
    assert!(
        !contains(&bytes, b"/SigFlags"),
        "no signature field was carried, so no /SigFlags"
    );
    assert_eq!(session.page_slots().expect("pages").len(), 2);
}

#[test]
fn inserting_both_pages_carries_every_field_and_the_sig_flags() {
    let src = source();
    let mut session = EditSession::new(blank_target());
    let out = session
        .insert_pages(&src.view(), &[0, 1], InsertPosition::End)
        .expect("insert");
    assert_eq!(out.fields_carried, 5, "First, Last, Addr, Choice, Sig1");
    assert_eq!(out.fields_split, 0);
    assert_eq!(out.orphaned_widgets, 0);
    assert_eq!(
        names(&session),
        [
            "Choice",
            "Person.Addr",
            "Person.Name.First",
            "Person.Name.Last",
            "Sig1"
        ]
    );
    assert_parent_links(&session);
    assert!(contains(&saved(&session), b"/SigFlags 3"));
}

#[test]
fn a_root_name_collision_renames_the_arriving_root() {
    let src = source();
    let mut session = EditSession::new(person_target());
    let out = session
        .insert_pages(&src.view(), &[0], InsertPosition::End)
        .expect("insert");
    assert_eq!(out.fields_renamed, 1);
    assert!(!out.acroform_created);
    assert_eq!(
        names(&session),
        ["Person.Other", "Person_2.Name.First", "Person_2.Name.Last"]
    );
}

#[test]
fn one_undo_reverts_the_pages_and_the_fields() {
    let src = source();
    let mut session = EditSession::new(person_target());
    session
        .insert_pages(&src.view(), &[0], InsertPosition::End)
        .expect("insert");
    assert!(session.undo().is_some());
    assert_eq!(names(&session), ["Person.Other"]);
    assert_eq!(session.page_slots().expect("pages").len(), 1);
}

#[test]
fn merge_document_reparents_grandchildren_and_renames_on_the_root() {
    let src = source();
    let mut session = EditSession::new(person_target());
    let out = session
        .merge_document(&src.view(), InsertPosition::End)
        .expect("merge");
    assert_eq!(out.fields_renamed, 1, "root `Person` is taken");
    assert!(names(&session).contains(&"Person_2.Name.First".to_owned()));
    assert!(names(&session).contains(&"Person.Other".to_owned()));
    // Name, First, Last, Addr, two Choice kids, and the target's Other.
    assert_eq!(assert_parent_links(&session), 7);
}
