//! **Every verb that appends to a page's `/Contents` must survive every legal
//! shape `/Contents` can take** (ISO 32000-1 §7.7.3.3, Table 30).
//!
//! ## The defect these exist for
//!
//! `/Contents` is *"a content stream … or an array of content streams"*, and
//! either form may be reached through an indirect reference. That is four
//! shapes, not two:
//!
//! | shape | who writes it |
//! |---|---|
//! | absent | a page with no marks |
//! | `/Contents 7 0 R` → a **stream** | most simple producers |
//! | `/Contents [7 0 R 8 0 R]` — a **direct array** | producers that split a page |
//! | `/Contents 38 0 R` → an **array** | **Qt, and every CAD exporter Ken uses** |
//!
//! The fourth shape was handled by wrapping the reference — producing
//! `/Contents [38 0 R, new]` where element 0 dereferences to *another array*.
//! Nothing in PDF permits an array of arrays here, so `page_tree::pages`
//! rejected the page, on the in-memory session **and on the saved file**:
//! `add_image` returned `Ok`, the save returned `Ok`, and the result was a
//! document pdfcer itself refuses to open.
//!
//! ★ **It was reachable from three verbs through TWO independent helpers.**
//! `EditSession::append_page_content` served `add_image` and `flatten_fields`;
//! `text_edit::addtext::append_contents` served `add_text` and the OCR text
//! layer. Both were written from the same wrong assumption, separately. That is
//! R92's failure mode exactly — one question answered in two places, and the
//! two answers drifted into being wrong together.
//!
//! ## Why the assertions run over the SAVED FILE and then RELOAD it
//!
//! The in-memory session and the saved bytes failed *separately*, and either
//! could have been fixed alone while the other stayed broken. So each test
//! walks the page tree on the live session, saves, reloads from bytes, and
//! walks it again. **R159**: a defect that lives in the bytes must be asserted
//! in the bytes.
//!
//! ## Fixture provenance
//!
//! Every PDF here is built inline from bytes this file authors — nothing is
//! read from disk and nothing is downloaded (project rule 7, `LEGAL.md` §5).
//! The reporting shell supplied a real Qt-produced file; it is deliberately
//! **not** checked in, and the shape was reproduced synthetically instead.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, NewImage};
use pdfcer_core::object::Object;
use pdfcer_core::page_tree::{self, Rect};
use pdfcer_core::writer::SaveOptions;

/// Build a one-page PDF whose page dictionary is `page_body`, with two content
/// streams (objects 4 and 5) and an array object (6) holding both.
///
/// The caller picks which shape `/Contents` takes by writing the page body, so
/// one builder covers all four cases and the difference between tests is
/// exactly one token.
fn pdf_with_page(page_body: &str) -> Vec<u8> {
    pdf_with_page_and_array(page_body, "[ 4 0 R 5 0 R ]")
}

/// As [`pdf_with_page`], with object 6's body under the caller's control — so
/// a test can plant a `/Contents` array that references itself.
fn pdf_with_page_and_array(page_body: &str, array_body: &str) -> Vec<u8> {
    let s1 = "q 1 0 0 1 0 0 cm Q";
    let s2 = "q 1 0 0 1 0 0 cm Q";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_owned(),
        page_body.to_owned(),
        format!("<< /Length {} >>\nstream\n{s1}\nendstream", s1.len() + 1),
        format!("<< /Length {} >>\nstream\n{s2}\nendstream", s2.len() + 1),
        array_body.to_owned(),
    ];
    let mut buf = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(buf.len());
        buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref_at = buf.len();
    let size = bodies.len() + 1;
    buf.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for off in &offsets {
        buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    buf.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n")
            .as_bytes(),
    );
    buf
}

/// ★ The shape that corrupted: `/Contents` is an indirect reference to an
/// ARRAY. Qt-based exporters and every CAD sheet the operator works on.
fn indirect_array_page() -> Vec<u8> {
    pdf_with_page(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> /Contents 6 0 R >>",
    )
}

/// `/Contents` is an indirect reference to a single STREAM — the shape that
/// always worked, kept as the control.
fn indirect_stream_page() -> Vec<u8> {
    pdf_with_page(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> /Contents 4 0 R >>",
    )
}

/// `/Contents` is a DIRECT array of stream references.
fn direct_array_page() -> Vec<u8> {
    pdf_with_page(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> \
         /Contents [4 0 R 5 0 R] >>",
    )
}

/// A page with no `/Contents` at all — legal, and the fourth shape.
fn no_contents_page() -> Vec<u8> {
    pdf_with_page("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>")
}

fn session(bytes: Vec<u8>) -> EditSession {
    EditSession::new(Document::from_bytes(bytes).expect("the fixture must load"))
}

/// Walk the page tree on the LIVE session and then on the SAVED-AND-RELOADED
/// bytes, and describe any failure in terms of which half broke.
///
/// Both halves are checked because both failed separately in the original
/// defect: the in-memory graph and the written file each rejected the page for
/// the same reason but by different routes, and fixing one without the other
/// would have looked like a fix.
fn assert_page_tree_walks(s: &mut EditSession, what: &str) {
    let live = s.pages().map(|p| p.len());
    assert!(
        live.is_ok(),
        "{what}: the LIVE session's page tree no longer walks: {live:?}"
    );

    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("incremental save")
        .0;
    let doc = Document::from_bytes(bytes).expect("the saved file must reload");
    let walked = page_tree::pages(&doc).map(|p| p.len());
    assert!(
        walked.is_ok(),
        "{what}: the SAVED file's page tree no longer walks: {walked:?} — \
         this is the shape where add_image returned Ok and produced a file \
         pdfcer cannot open"
    );
}

/// The `/Contents` value on page 0 of a reloaded document, resolved one level.
fn contents_of(doc: &Document) -> Object {
    let slots = page_tree::page_slots(doc).expect("page slots");
    let page = doc
        .get(slots[0].id)
        .map(|io| io.value.clone())
        .and_then(|o| o.as_dict().cloned())
        .expect("page dict");
    let raw = page.get(b"Contents").expect("/Contents").clone();
    match raw {
        Object::Reference(r) => doc
            .get(r)
            .map(|io| io.value.clone())
            .unwrap_or(Object::Null),
        other => other,
    }
}

/// Every element of a `/Contents` array must resolve to a STREAM. An element
/// that resolves to an array is the corruption, and it is invisible to any
/// assertion that only checks the array's length.
fn assert_all_elements_are_streams(doc: &Document, what: &str) {
    let contents = contents_of(doc);
    let Object::Array(items) = contents else {
        return; // a single stream is fine and is checked elsewhere
    };
    for (i, item) in items.iter().enumerate() {
        let resolved = match item {
            Object::Reference(r) => doc
                .get(*r)
                .map(|io| io.value.clone())
                .unwrap_or(Object::Null),
            other => other.clone(),
        };
        assert!(
            matches!(resolved, Object::Stream(_)),
            "{what}: /Contents element {i} resolves to {resolved:?}, not a stream — \
             an array nested inside the /Contents array is exactly the corruption"
        );
    }
}

fn tiny_image() -> pdfcer_core::image_import::ImportedImage {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/images/rgb8.png");
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    pdfcer_core::image_import::import(&bytes).expect("import")
}

// ---------------------------------------------------------------------------
// add_image — the verb the defect was reported through
// ---------------------------------------------------------------------------

/// ★ THE REGRESSION. Placing an image on a page whose `/Contents` is an
/// indirect reference to an array must leave a page the reader still accepts.
#[test]
fn add_image_survives_an_indirect_array_contents() {
    let image = tiny_image();
    let mut s = session(indirect_array_page());
    assert!(s.pages().is_ok(), "the fixture itself must be readable");

    s.add_image(&NewImage::new(
        0,
        Rect {
            llx: 10.0,
            lly: 10.0,
            urx: 110.0,
            ury: 110.0,
        },
        &image,
    ))
    .expect("add_image");

    assert_page_tree_walks(&mut s, "add_image / indirect array");

    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    let doc = Document::from_bytes(bytes).expect("reload");
    assert_all_elements_are_streams(&doc, "add_image / indirect array");

    // And the two original streams must still be there, in order, ahead of the
    // overlay — an append that dropped them would also "walk" and would be a
    // worse bug than the one being fixed.
    let Object::Array(items) = contents_of(&doc) else {
        panic!("expected an array after appending to an array");
    };
    assert_eq!(
        items.len(),
        3,
        "two original streams plus the overlay, spliced — not wrapped"
    );
    assert_eq!(items[0].as_reference().map(|r| r.num), Some(4));
    assert_eq!(items[1].as_reference().map(|r| r.num), Some(5));
}

/// The other three shapes must be unaffected — this is the control that stops
/// a fix for one shape from breaking the others.
#[test]
fn add_image_survives_every_other_contents_shape() {
    let image = tiny_image();
    for (what, bytes) in [
        ("indirect stream", indirect_stream_page()),
        ("direct array", direct_array_page()),
        ("absent", no_contents_page()),
    ] {
        let mut s = session(bytes);
        s.add_image(&NewImage::new(
            0,
            Rect {
                llx: 10.0,
                lly: 10.0,
                urx: 110.0,
                ury: 110.0,
            },
            &image,
        ))
        .unwrap_or_else(|e| panic!("{what}: add_image: {e}"));
        assert_page_tree_walks(&mut s, what);

        let out = s
            .to_incremental_bytes(&SaveOptions::identity())
            .expect("save")
            .0;
        let doc = Document::from_bytes(out).expect("reload");
        assert_all_elements_are_streams(&doc, what);
    }
}

// ---------------------------------------------------------------------------
// add_text — the SECOND helper, which had the identical defect
// ---------------------------------------------------------------------------

/// ★ `append_contents` in `text_edit::addtext` is a completely separate
/// implementation of the same append, and it was wrong the same way. This test
/// exists because the reporting shell asked *"if the wrapping is written per
/// verb, the others have it too"* — and they did.
#[test]
fn add_text_survives_an_indirect_array_contents() {
    use pdfcer_core::text_edit::AddTextRequest;

    let mut s = session(indirect_array_page());
    let req = AddTextRequest::new(0, (50.0, 50.0), "hello");
    s.add_text(&req).expect("add_text");

    assert_page_tree_walks(&mut s, "add_text / indirect array");

    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    let doc = Document::from_bytes(bytes).expect("reload");
    assert_all_elements_are_streams(&doc, "add_text / indirect array");
}

/// The same control for the second helper.
#[test]
fn add_text_survives_every_other_contents_shape() {
    use pdfcer_core::text_edit::AddTextRequest;

    for (what, bytes) in [
        ("indirect stream", indirect_stream_page()),
        ("direct array", direct_array_page()),
        ("absent", no_contents_page()),
    ] {
        let mut s = session(bytes);
        let req = AddTextRequest::new(0, (50.0, 50.0), "hello");
        s.add_text(&req)
            .unwrap_or_else(|e| panic!("{what}: add_text: {e}"));
        assert_page_tree_walks(&mut s, what);
    }
}

// ---------------------------------------------------------------------------
// Two appends in a row — the shape a real editing session produces
// ---------------------------------------------------------------------------

/// An operator does not stop at one edit. The second append reads back what
/// the first wrote, so a first append that produced a subtly wrong shape can
/// be compounded rather than merely repeated.
#[test]
fn two_appends_in_one_session_stay_flat() {
    let image = tiny_image();
    let mut s = session(indirect_array_page());
    for _ in 0..2 {
        s.add_image(&NewImage::new(
            0,
            Rect {
                llx: 10.0,
                lly: 10.0,
                urx: 110.0,
                ury: 110.0,
            },
            &image,
        ))
        .expect("add_image");
    }
    assert_page_tree_walks(&mut s, "two appends");

    let bytes = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save")
        .0;
    let doc = Document::from_bytes(bytes).expect("reload");
    assert_all_elements_are_streams(&doc, "two appends");
    let Object::Array(items) = contents_of(&doc) else {
        panic!("expected an array");
    };
    assert_eq!(items.len(), 4, "two originals plus two overlays");
}

// ---------------------------------------------------------------------------
// Reading back a file a SHIPPED pdfcer already damaged
// ---------------------------------------------------------------------------

/// ★ Documents corrupted by builds older than `Pass 111.0` are already on the
/// operator's disk, and some of them no longer have an undamaged original.
/// Refusing to open them would cost him work pdfcer itself destroyed.
///
/// The repair is allowed because it is EXACT rather than a guess: flattening
/// `[[a, b], c]` to `[a, b, c]` recovers precisely the streams the page had,
/// in precisely the order Table 30 concatenates them. The nesting carries no
/// information. Contrast `contents_unresolved`, where content genuinely IS
/// missing and the page really is incomplete.
#[test]
fn a_page_damaged_by_an_older_build_still_opens_and_is_disclosed() {
    // Exactly the byte shape the old `append_page_content` wrote: the page
    // points at an array whose FIRST element is the original array object.
    let bytes = pdf_with_page(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >>          /Contents [6 0 R 4 0 R] >>",
    );
    let doc = Document::from_bytes(bytes).expect("load");
    let pages = page_tree::pages(&doc).expect("a damaged page must still open");
    let page = &pages[0];

    // Object 6 is `[4 0 R 5 0 R]`, so the flattened order is 4, 5, then the
    // element that followed the nested array.
    assert_eq!(
        page.contents.iter().map(|id| id.num).collect::<Vec<_>>(),
        vec![4, 5, 4],
        "the streams must come back in concatenation order, nesting removed"
    );

    // ★ And it is DISCLOSED, not silently healed. Rule 4: pdfcer repaired
    // something, so pdfcer says so.
    assert_eq!(
        page.contents_flattened, 1,
        "one nested array was flattened and the count must say so"
    );
    assert_eq!(
        page.contents_unresolved, 0,
        "nothing was MISSING — that is a different disclosure and must not fire"
    );
}

/// An undamaged page must report zero, or the disclosure is noise.
#[test]
fn an_undamaged_page_reports_no_flattening() {
    for bytes in [
        indirect_array_page(),
        indirect_stream_page(),
        direct_array_page(),
        no_contents_page(),
    ] {
        let doc = Document::from_bytes(bytes).expect("load");
        let pages = page_tree::pages(&doc).expect("walk");
        assert_eq!(
            pages[0].contents_flattened, 0,
            "a healthy page must not claim to have been repaired"
        );
    }
}

/// The recursion is bounded. A `/Contents` array that references ITSELF is
/// legal syntax and must terminate by depth, not by luck
/// (`ARCHITECTURE.md` §10 — no recursive walker without a guard).
#[test]
fn a_self_referential_contents_array_is_refused_not_hung() {
    // Object 6 becomes `[ 6 0 R ]` — an array whose only element is itself.
    let bytes = pdf_with_page_and_array(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> /Contents 6 0 R >>",
        "[ 6 0 R ]",
    );
    let doc = Document::from_bytes(bytes).expect("load");
    // Refused, not hung, and not a stack overflow.
    assert!(
        page_tree::pages(&doc).is_err(),
        "a self-referential /Contents array must be refused by the depth guard"
    );
}
