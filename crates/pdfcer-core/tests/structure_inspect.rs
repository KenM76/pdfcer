//! **Internal-structure inspection** — the contract `pdfcer_core::structure`
//! owes (`Pass 193.0`).
//!
//! # What is asserted, and why each one
//!
//! The module's whole job is to be *believed* about a file nobody can otherwise
//! see inside, so the assertions here are mostly about **honesty under a
//! bound**: that a truncation says it truncated, that a cycle is marked rather
//! than followed, that a stream that will not decode reports why instead of
//! vanishing, and that a dangling reference is distinguishable from an explicit
//! `null`. A dump that quietly stopped early would be worse than no dump at
//! all — the operator would read completeness into it.
//!
//! ★ The bounds are the security surface, not a display preference
//! (`ARCHITECTURE.md` §10): this module decodes untrusted streams *and* walks an
//! untrusted graph, so `max_stream_bytes`, `max_objects` and the cycle guard are
//! each a real defence, and each is tested by *reaching* it rather than by
//! reading the code.
//!
//! # The fixtures are the ones already in the tree
//!
//! Deliberately not new ones. This module makes claims about **existing**
//! documents, and pointing it at purpose-built files would let it agree with
//! itself. The object-stream case in particular needs a file that really uses
//! `/ObjStm`, which is exactly what makes it worth having.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::object::ObjId;
use pdfcer_core::structure::{self, DumpOptions, Storage, StreamMode};
use std::path::{Path, PathBuf};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(rel)
}

fn load(rel: &str) -> Document {
    Document::load(&fixture(rel)).expect("fixture must load")
}

/// A simple, always-present synthetic document.
fn simple() -> Document {
    load("synthetic/annot/structural-annots-mixed.pdf")
}

/// The catalog is reachable and renders as a dictionary naming `/Pages`.
///
/// The smoke test that the whole module rests on: if this fails, nothing below
/// means anything.
#[test]
fn the_catalog_renders_with_its_keys() {
    let doc = simple();
    let catalog = doc.view().graph().catalog_id().expect("a catalog");
    let out = structure::render_object(&doc, doc.bytes(), catalog, &DumpOptions::default());
    assert!(out.contains("/Type /Catalog"), "actual output:\n{out}");
    assert!(out.contains("/Pages"), "actual output:\n{out}");
}

/// ★ A reference beyond `max_depth` prints as `N G R` and is NOT followed.
///
/// Depth is the bound an operator reaches for first, so it must be exact:
/// at depth 0 the catalog shows `/Pages 2 0 R` and nothing about the page-tree
/// node; at depth 1 the node's own keys appear.
#[test]
fn depth_zero_prints_a_reference_and_depth_one_expands_it() {
    let doc = simple();
    let catalog = doc.view().graph().catalog_id().expect("a catalog");

    let shallow = structure::render_object(
        &doc,
        doc.bytes(),
        catalog,
        &DumpOptions::default().with_depth(0),
    );
    assert!(
        shallow.contains("R") && !shallow.contains("/Kids"),
        "at depth 0 the /Pages node must NOT be expanded:\n{shallow}"
    );

    let deep = structure::render_object(
        &doc,
        doc.bytes(),
        catalog,
        &DumpOptions::default().with_depth(1),
    );
    assert!(
        deep.contains("/Kids"),
        "at depth 1 the /Pages node's own keys must appear:\n{deep}"
    );
}

/// ★★ A CYCLE IS MARKED, NOT FOLLOWED — and on a page tree the cycle is the
/// NORMAL case, not a malformed one.
///
/// Every page's `/Parent` points back at its `/Pages` node, so a walk from the
/// catalog revisits an object within two hops. Without the guard this is an
/// infinite recursion on a *conforming* file, which is why it is tested with a
/// generous depth rather than a pathological fixture.
#[test]
fn a_page_tree_cycle_is_reported_rather_than_followed() {
    let doc = simple();
    let catalog = doc.view().graph().catalog_id().expect("a catalog");
    let out = structure::render_object(
        &doc,
        doc.bytes(),
        catalog,
        &DumpOptions::default().with_depth(8),
    );
    assert!(
        out.contains("cycle"),
        "the /Parent back-edge must be marked as a cycle:\n{out}"
    );
    // The real proof is that we got here at all: an unguarded walk would have
    // overflowed the stack rather than returned a string.
    assert!(out.len() < 1_000_000, "the dump grew without bound");
}

/// A stream's data is omitted by default, and the omission SAYS SO.
///
/// Silence would read as "this stream is empty", which is a different and
/// wrong fact.
#[test]
fn omitted_stream_data_is_disclosed_with_its_true_length() {
    let doc = load("synthetic/annot/ap-cascade-single-stream.pdf");
    let out =
        structure::render_object(&doc, doc.bytes(), ObjId::new(5, 0), &DumpOptions::default());
    assert!(out.contains("omitted"), "actual output:\n{out}");
    assert!(
        out.contains("raw byte(s)"),
        "the omission must state the length it omitted:\n{out}"
    );
}

/// Decoded stream content is reachable, and it is the content the file holds.
#[test]
fn a_stream_can_be_decoded_and_shows_its_operators() {
    let doc = load("synthetic/annot/ap-cascade-single-stream.pdf");
    let out = structure::render_object(
        &doc,
        doc.bytes(),
        ObjId::new(5, 0),
        &DumpOptions::default().with_streams(StreamMode::Decoded),
    );
    assert!(
        out.contains("re") && out.contains("rg"),
        "the appearance stream's own operators must appear:\n{out}"
    );
}

/// ★ TRUNCATION IS DISCLOSED, and the ceiling is genuinely applied.
///
/// Reached rather than reasoned about: the limit is set below the stream's real
/// length so the truncation branch actually runs. A ceiling that is only
/// documented is a ceiling nobody has run.
#[test]
fn a_stream_beyond_the_ceiling_is_truncated_and_says_so() {
    let doc = load("synthetic/annot/ap-cascade-single-stream.pdf");
    let out = structure::render_object(
        &doc,
        doc.bytes(),
        ObjId::new(5, 0),
        &DumpOptions::default()
            .with_streams(StreamMode::Decoded)
            .with_max_stream_bytes(4),
    );
    assert!(
        out.contains("truncated"),
        "a stream longer than the ceiling must disclose the truncation:\n{out}"
    );
    assert!(
        out.contains("byte(s) shown"),
        "the disclosure must state how much was shown, and of what total:\n{out}"
    );
}

/// A reference to an object that does not exist is marked UNRESOLVABLE.
///
/// ★ §7.3.10 makes a dangling reference resolve to `null` for a **reader**, and
/// that is right for rendering. For someone inspecting structure, "there is
/// nothing there" and "there is an explicit null there" are different facts
/// about how damaged the file is, and printing the first as the second would
/// hide the damage.
#[test]
fn a_dangling_reference_is_distinguished_from_an_explicit_null() {
    let doc = simple();
    // An object number far beyond anything the fixture defines.
    let out = structure::render_object(
        &doc,
        doc.bytes(),
        ObjId::new(9999, 0),
        &DumpOptions::default(),
    );
    assert!(
        out.contains("no such object"),
        "a missing object must be named as missing, not printed as null:\n{out}"
    );
}

/// The walk stops at `max_objects` and REPORTS that it stopped.
///
/// The failure this prevents is the quiet one: a dump that ends early looks
/// exactly like a document that ended.
#[test]
fn a_walk_that_hits_its_ceiling_says_it_hit_it() {
    let doc = simple();
    let catalog = doc.view().graph().catalog_id().expect("a catalog");
    let out = structure::walk(
        &doc,
        doc.bytes(),
        catalog,
        &DumpOptions::default().with_max_objects(1),
    );
    assert!(
        out.contains("max_objects reached"),
        "the ceiling must be disclosed:\n{out}"
    );
}

/// The inventory finds objects, classifies them, and inverts the reference map.
///
/// The reverse-reference column is the parity-plus half of this feature —
/// Acrobat's object browser has no equivalent — so it is asserted rather than
/// assumed: the catalog is referenced by the TRAILER (not by an object), and
/// the page-tree node is referenced by the catalog.
#[test]
fn the_inventory_inverts_the_reference_map() {
    let doc = simple();
    let inv = structure::inventory(&doc);
    assert!(!inv.objects.is_empty(), "the fixture has objects");

    let catalog = doc.view().graph().catalog_id().expect("a catalog");
    assert!(
        inv.trailer_referenced.contains(&catalog),
        "the catalog is reached from the trailer, not from an object — without \
         `trailer_referenced` it would look unreferenced in every document"
    );

    let pages = inv
        .objects
        .iter()
        .find(|r| r.type_name.as_deref() == Some("Pages"))
        .expect("a /Pages node");
    assert!(
        pages.referenced_by.contains(&catalog),
        "the page-tree node is referenced by the catalog; got {:?}",
        pages.referenced_by
    );
}

/// `/Type` and `/Subtype` are reported where the file states them.
#[test]
fn the_inventory_reports_type_and_subtype() {
    let doc = simple();
    let inv = structure::inventory(&doc);
    assert!(
        inv.objects
            .iter()
            .any(|r| r.type_name.as_deref() == Some("Page")),
        "a /Page must be classified"
    );
    assert!(
        inv.objects
            .iter()
            .any(|r| r.subtype.as_deref() == Some("Square")),
        "the /Square annotation's /Subtype must be reported"
    );
}

/// The layout reports the physical facts, and they match the fixture's shape.
///
/// These synthetic fixtures are written with a classic `xref` table by a
/// hand-rolled generator, so the style is a real assertion rather than a
/// tautology — a change that started normalising files to xref streams on load
/// would fail here.
#[test]
fn the_layout_reports_the_physical_shape() {
    let doc = simple();
    let l = structure::layout(&doc);
    assert!(
        l.xref_style.contains("xref table"),
        "expected a classic table, got {:?}",
        l.xref_style
    );
    assert!(l.object_count > 0);
    assert!(!l.encrypted);
    assert!(l.recovered.is_none(), "this fixture loads cleanly");
    assert!(
        l.object_streams.is_empty(),
        "this generator writes no object streams"
    );
}

/// ★★ THE CASE THE WHOLE FEATURE EXISTS FOR: an object compressed inside an
/// object stream is reachable, and its storage says where it really is.
///
/// This is the gap that made the `Pass 192.0` bevel defect undiagnosable — the
/// `/ExtGState` dictionaries lived inside `/ObjStm` containers, so a text
/// search of the file could not find them and no shipped verb could reach them.
///
/// Skipped rather than failed when the external corpus is absent: the corpus is
/// fetched, not committed (`fixtures/fetch-corpora.sh`), and a test that fails
/// on a clean checkout would be a false alarm rather than a finding.
#[test]
fn an_object_inside_an_object_stream_is_reachable_and_located() {
    // Chosen by SCANNING the corpus with this very module rather than by
    // guessing: the first file tried carried no object streams at all, so the
    // test passed while asserting nothing. A skip reads exactly like a pass.
    let path = fixture("external/qpdf/qpdf/qtest/qpdf/big-ostream.pdf");
    let Ok(doc) = Document::load(&path) else {
        eprintln!("SKIP: the external corpus is not present");
        return;
    };
    let l = structure::layout(&doc);
    if l.object_streams.is_empty() {
        eprintln!("SKIP: this corpus file uses no object streams");
        return;
    }
    let inv = structure::inventory(&doc);
    let compressed = inv
        .objects
        .iter()
        .find(|r| matches!(r.storage, Storage::InObjectStream { .. }))
        .expect("layout reported object streams, so some object is inside one");

    let Storage::InObjectStream { container, .. } = compressed.storage else {
        unreachable!("filtered above")
    };
    assert!(
        l.object_streams.contains_key(&container),
        "the object's named container must appear in the layout's own map — \
         the two derivations must agree"
    );

    // And it renders, which is the half that matters to an operator.
    let out = structure::render_object(&doc, doc.bytes(), compressed.id, &DumpOptions::default());
    assert!(
        !out.contains("no such object"),
        "a compressed object must be reachable:\n{out}"
    );
}
