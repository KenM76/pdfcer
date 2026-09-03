//! # `--set-font` to a face the page does not carry (`Pass 162.0`)
//!
//! Until this Pass, `format_text`'s family change was strictly **read-only**
//! about resources: a face that was not already a `/Font` entry on the page
//! was `TargetFontMissing`, naming the deferral code `FF-C`. That made
//! *"restyle this text to a face the document lacks"* unreachable through any
//! verb — `embed_font` supplies a missing font **program** for a face the file
//! already **references**, and cannot introduce one.
//!
//! A **standard-14** face (ISO 32000-1 §9.6.2.2) is the half of that gap
//! needing no font program at all, so it is now authored on demand. Anything
//! else still refuses.
//!
//! ## ★★ What these tests are really guarding: THREE save paths, not one
//!
//! A newly created font resource has to reach the file, and there are three
//! independent routes that can produce one:
//!
//! 1. `EditSession::format_text` — the page case, into an undoable command;
//! 2. its form-XObject twin, `format_text_in_form`;
//! 3. `text_edit::set_format` — the **one-shot** `&Document` entry point,
//!    which builds a `DirtySet` and shares no code with the session.
//!
//! The first cut of this Pass wired **1 and 2 only**, and every unit test
//! passed, because unit tests exercise the session. The CLI uses route 3: it
//! printed the disclosure saying a resource had been added and saved a file in
//! which it had not — a content stream naming `/pdfceF1` and no `/pdfceF1`
//! anywhere in the document. So the assertions below read the **saved bytes**
//! through both a session save and the one-shot save.
//!
//! ## ★★★ And the failure that would have been worse: INHERITED resources
//!
//! §7.8.3 — a page's own `/Resources` **replaces** the one it inherits from
//! its `/Pages` ancestors; it does not merge with it. So on a page carrying no
//! `/Resources` of its own, creating a direct one holding just the new font
//! **shadows every inherited font** the page's existing content already names.
//! The file still parses. The page's original text simply stops resolving its
//! fonts, which no round-trip or object-count assertion would catch.
//!
//! `inherited_resources_are_patched_not_shadowed` is the test for that, and it
//! asserts the *pre-existing* font still resolves — not merely that the new one
//! arrived.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph as _;
use pdfcer_core::object::{Dict, Object};
use pdfcer_core::text_edit::{FontSelector, FormatOptions, FormatRequest};
use pdfcer_core::writer::SaveOptions;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/synthetic/textedit/format_family.pdf")
}

/// Every `/BaseFont` name reachable from page 0's resources, in the SAVED
/// bytes — the question "did the resource actually land?" asked of the file
/// rather than of the report.
fn page_font_names(doc: &Document) -> Vec<String> {
    let view = doc.view();
    let pages = pdfcer_core::page_tree::pages(doc).expect("page tree");
    let fonts = pages[0]
        .resources
        .get(b"Font")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .cloned()
        .unwrap_or_default();
    let mut out: Vec<String> = fonts
        .iter()
        .filter_map(|(_, v)| {
            view.resolve(v)
                .as_dict()
                .and_then(|d| d.get(b"BaseFont"))
                .map(|o| view.resolve(o))
                .and_then(Object::as_name)
                .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
        })
        .collect();
    out.sort();
    out
}

/// The resource KEYS on page 0, saved-bytes side.
fn page_font_keys(doc: &Document) -> Vec<String> {
    let view = doc.view();
    let pages = pdfcer_core::page_tree::pages(doc).expect("page tree");
    let fonts = pages[0]
        .resources
        .get(b"Font")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .cloned()
        .unwrap_or_default();
    let mut out: Vec<String> = fonts
        .iter()
        .map(|(k, _)| String::from_utf8_lossy(k.as_bytes()).into_owned())
        .collect();
    out.sort();
    out
}

/// Page 0's text, concatenated — asserted on because a resource that arrived
/// but re-encoded wrongly passes every structural check and loses the words.
fn page_text(doc: &Document) -> String {
    let pages = pdfcer_core::page_tree::pages(doc).expect("page tree");
    let pt = pdfcer_core::text_extract::extract_page(
        doc,
        &pages[0],
        0,
        &pdfcer_core::text_extract::ExtractOptions::default(),
    )
    .expect("extract");
    pt.runs.iter().map(|r| r.text.as_str()).collect::<String>()
}

fn request(face: &str) -> FormatRequest {
    FormatRequest::new(0, "hello").font(FontSelector::new(face))
}

// ---------------------------------------------------------------------------
// Route 3 — the ONE-SHOT path the CLI uses. Listed first because it is the
// one that shipped broken.
// ---------------------------------------------------------------------------

#[test]
fn the_one_shot_path_writes_the_resource_it_says_it_added() {
    let doc = Document::load(&fixture()).expect("load fixture");
    assert!(
        !page_font_names(&doc).iter().any(|n| n == "Helvetica"),
        "fixture precondition: the page does NOT carry Helvetica"
    );

    let outcome =
        pdfcer_core::text_edit::set_format(&doc, &request("Helvetica"), &FormatOptions::default())
            .expect("Helvetica is a standard-14 face and must be authorable");

    let saved = Document::from_bytes(outcome.bytes).expect("re-parse the saved bytes");
    assert!(
        page_font_names(&saved).iter().any(|n| n == "Helvetica"),
        "the disclosure said a resource was added; the FILE must contain it. \
         Got: {:?}",
        page_font_names(&saved)
    );
    // And the run still reads. A resource that arrived but re-encoded wrongly
    // would pass the assertion above and lose the text.
    let text = page_text(&saved);
    assert!(
        text.contains("hello"),
        "the restyled run must still read: {text:?}"
    );
}

/// The report and the file must agree. Asserted as a pair on purpose: the
/// shipped defect was precisely a report that claimed more than the file did.
#[test]
fn the_disclosure_and_the_file_agree_about_the_new_key() {
    let doc = Document::load(&fixture()).expect("load fixture");
    let outcome =
        pdfcer_core::text_edit::set_format(&doc, &request("Courier"), &FormatOptions::default())
            .expect("Courier is standard-14");

    let claimed = outcome
        .report
        .disclosures
        .iter()
        .find(|d| d.contains("was NOT a font resource here"))
        .expect("adding a resource must be disclosed — project rule 4")
        .clone();

    let saved = Document::from_bytes(outcome.bytes).expect("re-parse");
    let keys = page_font_keys(&saved);
    let new_key = keys
        .iter()
        .find(|k| k.starts_with("pdfceF"))
        .expect("the new key is in the saved file");
    assert!(
        claimed.contains(new_key.as_str()),
        "the disclosure names {claimed:?} but the file bound it under {new_key:?}"
    );
}

// ---------------------------------------------------------------------------
// Route 1 — the session path, and its undo contract
// ---------------------------------------------------------------------------

#[test]
fn the_session_path_writes_the_resource_too() {
    let mut s = EditSession::new(Document::load(&fixture()).expect("load"));
    s.format_text(&request("Helvetica"), &FormatOptions::default())
        .expect("standard-14");
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    let saved = Document::from_bytes(bytes).expect("re-parse");
    assert!(page_font_names(&saved).iter().any(|n| n == "Helvetica"));
}

/// ★ Undo must remove the font resource as well as the restyle.
///
/// They are ONE command deliberately. If the resource were committed
/// separately, undoing the restyle would leave an object the operator never
/// asked for, bound under a key nothing references — and the next save would
/// append it for no edit, which is a minimal-diff violation as well as a
/// puzzle for anyone reading the file.
#[test]
fn undoing_the_restyle_also_removes_the_font_it_added() {
    let original = std::fs::read(fixture()).expect("read fixture");
    let mut s = EditSession::new(Document::load(&fixture()).expect("load"));
    s.format_text(&request("Helvetica"), &FormatOptions::default())
        .expect("standard-14");
    assert!(!s.dirty_set().is_empty(), "the restyle did something");

    s.undo().expect("the restyle is on the undo stack");
    assert!(
        s.dirty_set().is_empty(),
        "after undo nothing may differ from the base revision — including the \
         font object the restyle created"
    );
    let (bytes, _) = s
        .to_incremental_bytes(&SaveOptions::identity())
        .expect("save");
    assert_eq!(
        bytes, original,
        "edit -> undo -> save must reproduce the input exactly"
    );
}

// ---------------------------------------------------------------------------
// Refusals — what did NOT become authorable
// ---------------------------------------------------------------------------

/// A face outside the standard 14 needs a font program, which is still `FF-C`.
/// The refusal must still name the face and still say why.
#[test]
fn a_non_standard_14_face_is_still_refused_by_name() {
    let doc = Document::load(&fixture()).expect("load");
    let err =
        pdfcer_core::text_edit::set_format(&doc, &request("Arial"), &FormatOptions::default())
            .expect_err("Arial is not standard-14 and needs embedding");
    let msg = err.to_string();
    assert!(
        msg.contains("Arial"),
        "the refusal must name the face: {msg}"
    );
    assert!(
        msg.contains("FF-C"),
        "and must name the deferral, so it is not read as a permanent no: {msg}"
    );
}

/// ★★ The discriminator: `Symbol` IS standard-14, so the resource is
/// synthesized — and then the run is refused because `Symbol`'s built-in
/// encoding has no `h`.
///
/// This is the test that proves the synthesized dictionary goes through the
/// **same coverage gate** as a page font (`R221`), rather than being trusted
/// because pdfcer built it. A refusal naming `TargetFontMissing` here would mean
/// the synthesis never happened; a SUCCESS here would mean pdfcer wrote
/// `.notdef` or silently substituted, which the family-change contract forbids
/// outright.
#[test]
fn a_standard_14_face_that_cannot_show_the_run_is_refused_on_coverage() {
    let doc = Document::load(&fixture()).expect("load");
    let err =
        pdfcer_core::text_edit::set_format(&doc, &request("Symbol"), &FormatOptions::default())
            .expect_err("Symbol cannot show 'hello'");
    let msg = err.to_string();
    assert!(
        msg.contains("U+0068") || msg.contains("'h'"),
        "the refusal must name the character that could not be encoded: {msg}"
    );
    assert!(
        !msg.contains("not an existing font resource"),
        "this must NOT be the missing-resource refusal — the resource was \
         synthesized and then judged on coverage: {msg}"
    );
}

/// A face the page already carries must still resolve to the EXISTING
/// resource. Authoring a second copy would leave the document with two
/// identical fonts and the run pointing at the newer one.
#[test]
fn a_face_already_on_the_page_adds_nothing() {
    let doc = Document::load(&fixture()).expect("load");
    let before = page_font_keys(&doc);
    let outcome = pdfcer_core::text_edit::set_format(
        &doc,
        &request("Times-Roman"),
        &FormatOptions::default(),
    )
    .expect("Times-Roman is /F1 on this page");
    let saved = Document::from_bytes(outcome.bytes).expect("re-parse");
    assert_eq!(
        page_font_keys(&saved),
        before,
        "no new resource key may appear when the face was already present"
    );
}

// ---------------------------------------------------------------------------
// ★★★ INHERITED RESOURCES — §7.8.3
// ---------------------------------------------------------------------------

/// Build a one-page document whose `/Resources` live on the `/Pages` NODE, not
/// on the page — the shape §7.8.3 exists for, and the one a naive
/// implementation destroys.
fn inherited_resources_pdf() -> Vec<u8> {
    let content = "BT /F1 12 Tf 72 700 Td (hello world) Tj ET";
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_owned(),
        // ★ /Resources is HERE, on the Pages node.
        "<< /Type /Pages /Kids [3 0 R] /Count 1 /Resources << /Font << /F1 5 0 R >> >> >>"
            .to_owned(),
        // ★ …and the page has NONE of its own.
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R >>".to_owned(),
        format!(
            "<< /Length {} >>\nstream\n{content}\nendstream",
            content.len() + 1
        ),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Times-Roman /Encoding /WinAnsiEncoding >>"
            .to_owned(),
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

/// ★★★ The page inherits its `/Resources`. Adding a font must NOT give the
/// page a `/Resources` of its own, because that would REPLACE the inherited
/// one and orphan `/F1` — the font the page's existing text already uses.
///
/// The load-bearing assertion is the SECOND one: that `Times-Roman` is still
/// reachable. A test that only checked the new face had arrived would pass on
/// an implementation that shadowed everything else, and the damage would
/// surface as a page whose original text stops rendering.
#[test]
fn inherited_resources_are_patched_not_shadowed() {
    let doc = Document::from_bytes(inherited_resources_pdf()).expect("build fixture");
    assert_eq!(
        page_font_names(&doc),
        vec!["Times-Roman".to_string()],
        "fixture precondition: one inherited font, reachable from the page"
    );

    let outcome =
        pdfcer_core::text_edit::set_format(&doc, &request("Helvetica"), &FormatOptions::default())
            .expect("standard-14");
    let saved = Document::from_bytes(outcome.bytes).expect("re-parse");

    let names = page_font_names(&saved);
    assert!(
        names.iter().any(|n| n == "Helvetica"),
        "the new face must be reachable from the page: {names:?}"
    );
    assert!(
        names.iter().any(|n| n == "Times-Roman"),
        "★ THE INHERITED FONT MUST SURVIVE. If it is gone, the page was given \
         its own /Resources holding only the new font, which §7.8.3 says \
         REPLACES the inherited dictionary rather than merging with it — every \
         glyph the original content stream draws would stop resolving. \
         Got: {names:?}"
    );
    let text = page_text(&saved);
    assert!(
        text.contains("hello"),
        "and the page's text must still read: {text:?}"
    );
}

/// The new key must not collide with one already in the dictionary it is
/// bound into. `pdfceF1` is picked as the first unused name, and the `pdfcer`
/// prefix keeps it clear of the `/F{n}` producer convention — a collision
/// would SHADOW a font the original content depends on (§7.8.3: names are
/// local to the stream, and the page and any appended stream share one
/// effective dictionary).
#[test]
fn the_new_resource_key_does_not_collide() {
    let doc = Document::load(&fixture()).expect("load");
    let before = page_font_keys(&doc);
    let outcome =
        pdfcer_core::text_edit::set_format(&doc, &request("Helvetica"), &FormatOptions::default())
            .expect("standard-14");
    let saved = Document::from_bytes(outcome.bytes).expect("re-parse");
    let after = page_font_keys(&saved);

    for old in &before {
        assert!(
            after.contains(old),
            "the pre-existing key {old:?} vanished — the new resource overwrote it"
        );
    }
    assert_eq!(
        after.len(),
        before.len() + 1,
        "exactly one key was added: {before:?} -> {after:?}"
    );
}

/// The authored dictionary is the FULL form: `/Widths` present, and
/// `/Encoding /WinAnsiEncoding` for a Latin face.
///
/// `/Widths` is not decoration. §9.6.2.2 permits a bare four-key standard-14
/// dictionary, but PDF 1.5 deprecates that special treatment as a `should`,
/// and emitting the widths makes the run's spacing self-contained — a reader
/// with no built-in standard-14 metrics still lays it out correctly. pdfcer
/// already owns the metrics, so it costs nothing.
#[test]
fn the_authored_font_dictionary_carries_widths_and_an_encoding() {
    let doc = Document::load(&fixture()).expect("load");
    let outcome =
        pdfcer_core::text_edit::set_format(&doc, &request("Helvetica"), &FormatOptions::default())
            .expect("standard-14");
    let saved = Document::from_bytes(outcome.bytes).expect("re-parse");
    let view = saved.view();
    let pages = pdfcer_core::page_tree::pages(&saved).expect("page tree");
    let fonts = pages[0]
        .resources
        .get(b"Font")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .cloned()
        .unwrap_or_default();
    let helv: Dict = fonts
        .iter()
        .filter_map(|(_, v)| view.resolve(v).as_dict().cloned())
        .find(|d| {
            d.get(b"BaseFont")
                .map(|o| view.resolve(o))
                .and_then(Object::as_name)
                .is_some_and(|n| n.as_bytes() == b"Helvetica")
        })
        .expect("the authored Helvetica dictionary");

    assert_eq!(
        helv.get(b"Subtype")
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec()),
        Some(b"Type1".to_vec())
    );
    assert!(
        matches!(helv.get(b"Widths"), Some(Object::Array(a)) if a.len() == 224),
        "codes 32..=255 inclusive is 224 widths"
    );
    assert_eq!(
        helv.get(b"Encoding")
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec()),
        Some(b"WinAnsiEncoding".to_vec()),
        "a Latin standard-14 face gets an explicit encoding; Symbol and \
         ZapfDingbats deliberately do not"
    );
    assert!(
        helv.get(b"FontDescriptor").is_none(),
        "no descriptor and no font program — nothing here needs embedding"
    );
}
