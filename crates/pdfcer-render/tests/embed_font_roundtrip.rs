//! FF-C end to end: a donor face on disk becomes an embedded `/Type0` font.
//!
//! This is the only place the whole Pass 21.0 chain runs as one piece —
//! `pdfcer-render` parses and subsets a real donor, `pdfcer-core` emits the PDF
//! objects, and the writer saves them. The unit tests on either side of that
//! seam are each convincing about their own half and say nothing about
//! whether the halves agree.
//!
//! It lives in `pdfcer-render`'s tests because that is the only crate that can
//! see both: `pdfcer-core` must never depend on `pdfcer-render` (the parser
//! stays out of core — decision 021 §3.2, R21), so core's own tests can only
//! ever construct a plan by hand.

use pdfcer_core::document::Document;
use pdfcer_core::text_edit::addtext::{self, AddTextRequest};
use pdfcer_render::font::subset::plan_subset;

/// The synthetic donor from `tools/gen-subset-font-fixtures.py`, carrying
/// outlines for exactly `A`, `B`, `C`.
fn donor() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/text/subset-donor.ttf"
    );
    std::fs::read(path).unwrap_or_else(|e| {
        panic!(
            "missing donor fixture at {path}: {e}. Run `python tools/gen-subset-font-fixtures.py`."
        )
    })
}

/// A one-page document to add text to.
///
/// `hello.pdf` rather than `minimal.pdf`: the latter has no `/Resources` at
/// all, which the page-tree walk requires. It DOES already carry Standard-14
/// fonts, so the assertions below key on things only the embedded path
/// writes — `/Type0`, `/CIDFontType2`, `/Identity-H`, the subset-tagged name
/// — rather than on "a font appeared", which would have passed before this
/// feature existed.
fn base_page() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/hello.pdf"
    );
    std::fs::read(path).unwrap_or_else(|e| panic!("missing fixture {path}: {e}"))
}

#[test]
fn a_donor_face_becomes_an_embedded_type0_font_in_the_saved_file() {
    let plan = plan_subset(&donor(), 0, &['A', 'B'], "pdfceSubsetDemo", "ABCDEF")
        .expect("the donor covers A and B");
    let program_len = plan.program.len();

    let doc = Document::from_bytes(base_page()).expect("fixture parses");
    let req = AddTextRequest::new(0, (72.0, 700.0), "AB").with_embedded_face(plan);
    let out = addtext::add_text(&doc, &req).expect("embedded add-text succeeds");

    let saved = String::from_utf8_lossy(&out.bytes).into_owned();

    // The five objects the emitter promises, each identified by something
    // only it would write.
    assert!(
        saved.contains("/Type0"),
        "no /Type0 wrapper:\n{saved:.2000}"
    );
    assert!(saved.contains("/CIDFontType2"), "no CIDFont descendant");
    assert!(saved.contains("/Identity-H"), "no Identity-H encoding");
    assert!(
        saved.contains("/CIDToGIDMap /Identity"),
        "no identity CID->GID map"
    );
    assert!(
        saved.contains("/FontFile2"),
        "no embedded program reference"
    );
    assert!(saved.contains("/ToUnicode"), "no ToUnicode CMap");
    assert!(
        saved.contains("ABCDEF+pdfceSubsetDemo"),
        "the subset-tagged name must appear as /BaseFont and /FontName"
    );

    // The show operator must be a HEX string of 2-byte CIDs. A literal
    // `(AB) Tj` here would still render — onto whatever glyphs codes 0x41
    // and 0x42 happen to be — which is the failure mode this assertion
    // exists for: wrong text that looks like right text.
    assert!(
        saved.contains("> Tj"),
        "the composite show operator must use a hex string, not a literal one"
    );
    assert!(
        !saved.contains("(AB) Tj"),
        "found a literal single-byte show operator in an Identity-H run — this would address \
         the wrong glyphs while still producing output"
    );

    // The font program must physically be in the file. Checking the byte
    // sequence rather than /Length1 alone: a correct length beside absent
    // bytes is exactly the shape of a staging bug.
    let sfnt = b"\x00\x01\x00\x00";
    assert!(
        out.bytes.windows(4).any(|w| w == sfnt),
        "no sfnt magic in the saved file — the {program_len}-byte program was not written"
    );
    assert!(
        out.bytes.len() > base_page().len() + program_len / 2,
        "the saved file did not grow by anything like the program size"
    );

    // And it must still be a readable PDF afterwards, which the string
    // assertions above cannot tell you.
    let reparsed = Document::from_bytes(out.bytes.clone()).expect("output re-parses");
    assert_eq!(
        pdfcer_core::page_tree::pages(&reparsed)
            .expect("pages")
            .len(),
        1
    );
}

/// The embedded add allocates its objects instead of reusing existing ones.
///
/// # What this proves, and what it does NOT
///
/// It proves at least six object numbers that did not exist before now do —
/// the content stream plus the five font objects — so the operation is
/// genuinely additive in scale.
///
/// It does **not** prove R107 on its own. After an incremental save the
/// re-parsed document presents the merged view of both revisions, so "which
/// objects did the update section rewrite" is not directly visible from
/// here. The authoritative R107 check is the object-id-disjointness test in
/// `pdfcer_core::font_embed`, which asserts it over the emitter's own output
/// where the question is answerable.
///
/// Named for what it checks rather than for the rule it supports, because a
/// test whose name claims more than its assertions is how a gap acquires
/// documentation saying it is covered (R93).
#[test]
fn embedded_add_allocates_at_least_six_new_objects() {
    let base = base_page();
    let doc = Document::from_bytes(base.clone()).expect("fixture parses");

    let before: Vec<u32> = doc.objects().map(|o| o.id.num).collect();
    let page_id = pdfcer_core::page_tree::pages(&doc).expect("pages")[0]
        .id
        .num;

    let plan = plan_subset(&donor(), 0, &['A'], "pdfceSubsetDemo", "ABCDEF").expect("plan");
    let req = AddTextRequest::new(0, (72.0, 700.0), "A").with_embedded_face(plan);
    let out = addtext::add_text(&doc, &req).expect("add");

    // An incremental save appends only the objects it changed, so the update
    // section IS the modified set. Anything in it that existed before and is
    // not the page dictionary is an R107 violation.
    let after = Document::from_bytes(out.bytes).expect("re-parse");
    let now: Vec<u32> = after.objects().map(|o| o.id.num).collect();
    let added: Vec<u32> = now
        .iter()
        .copied()
        .filter(|n| !before.contains(n))
        .collect();

    assert!(
        added.len() >= 6,
        "expected at least six new objects (content + five font objects), got {added:?}"
    );
    assert!(
        before.contains(&page_id),
        "sanity: the page must have existed before the edit"
    );
}
