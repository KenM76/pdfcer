//! Decision 172 route B under decision 187: a character the run's embedded
//! TrueType program outlines but its simple font cannot encode is set
//! through a new `/Type0` + `/CIDFontType2` resource over the same program.
//!
//! The fixture is `word-shaped-subset-shared-tounicode.pdf`: its `/ToUnicode`
//! is shared with a second font, so routes A and the allocated code refuse
//! `Δ`, which the program (object 7, carrying a `cmap`) outlines.

use pdfcer_core::document::Document;
use pdfcer_core::edit::EditSession;
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::page_tree;
use pdfcer_core::text_edit::{
    self, CidFontProgram, CidProgramUse, EditOptions, EditOutcome, EditRequest, FallbackSource,
};
use pdfcer_core::view::DocumentView;
use pdfcer_render::font::embedded_glyphs::EmbeddedProgramGlyphs;
use pdfcer_render::{RenderOptions, render_page_with};

const PROGRAM: ObjId = ObjId::new(7, 0);

fn base_bytes() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/text/word-shaped-subset-shared-tounicode.pdf"
    ))
    .unwrap()
}

/// The fixture with an appended revision whose catalog claims PDF/A-2b.
fn pdfa_bytes() -> Vec<u8> {
    let mut b = base_bytes();
    let xmp = b"<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
        xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description \
        xmlns:pdfaid=\"http://www.aiim.org/pdfa/ns/id/\"><pdfaid:part>2</pdfaid:part>\
        <pdfaid:conformance>B</pdfaid:conformance></rdf:Description></rdf:RDF></x:xmpmeta>";
    let catalog = b.len();
    b.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R /Metadata 12 0 R >>\nendobj\n");
    let meta = b.len();
    b.extend_from_slice(
        format!(
            "12 0 obj\n<< /Type /Metadata /Subtype /XML /Length {} >>\nstream\n",
            xmp.len()
        )
        .as_bytes(),
    );
    b.extend_from_slice(xmp);
    b.extend_from_slice(b"\nendstream\nendobj\n");
    let xref = b.len();
    b.extend_from_slice(
        format!(
            "xref\n0 2\n0000000000 65535 f \n{catalog:010} 00000 n \n12 1\n{meta:010} 00000 n \n\
             trailer\n<< /Size 13 /Root 1 0 R /Prev 2370 >>\nstartxref\n{xref}\n%%EOF\n"
        )
        .as_bytes(),
    );
    b
}

fn edit(bytes: Vec<u8>, mode: CidFontProgram) -> Result<EditOutcome, String> {
    let opts = EditOptions::default()
        .with_embedded_glyphs(&EmbeddedProgramGlyphs)
        .with_cid_font_program(mode);
    let doc = Document::from_bytes(bytes).unwrap();
    text_edit::edit_text(
        &doc,
        &EditRequest::find_replace(0, "ABC", "AB\u{394}"),
        &opts,
    )
    .map_err(|e| e.to_string())
}

/// The `/FontFile2` reference of the newest `/CIDFontType2` descriptor.
fn cid_program(doc: &Document) -> ObjId {
    let view = DocumentView::new(doc, doc.bytes(), doc.version());
    let cid = (1..40)
        .rev()
        .filter_map(|n| view.value(ObjId::new(n, 0)))
        .filter_map(Object::as_dict)
        .find(|d| {
            d.get(b"Subtype")
                .and_then(Object::as_name)
                .is_some_and(|n| n.0 == b"CIDFontType2")
        })
        .expect("a CIDFontType2 was added");
    let desc = view
        .resolve(cid.get(b"FontDescriptor").unwrap())
        .as_dict()
        .unwrap();
    desc.get(b"FontFile2")
        .and_then(Object::as_reference)
        .unwrap()
}

fn stream_bytes(doc: &Document, id: ObjId) -> Vec<u8> {
    let view = DocumentView::new(doc, doc.bytes(), doc.version());
    let Some(Object::Stream(s)) = view.value(id) else {
        panic!("{id:?} is not a stream")
    };
    pdfcer_core::filters::decode_stream(&s.dict, view.slice(s.data_span).unwrap()).unwrap()
}

fn has_cmap(program: &[u8]) -> bool {
    let n = usize::from(u16::from_be_bytes([program[4], program[5]]));
    (0..n).any(|i| &program[12 + 16 * i..16 + 16 * i] == b"cmap")
}

fn draws_cleanly(bytes: &[u8]) {
    let doc = Document::from_bytes(bytes.to_vec()).unwrap();
    let page = page_tree::pages(&doc).unwrap().remove(0);
    let r = render_page_with(&doc, &page, 1.0, &RenderOptions::default()).unwrap();
    assert_eq!(r.diagnostics.glyphs_notdef, 0);
    assert_eq!(r.diagnostics.glyphs_substituted, 0);
    let text = pdfcer_core::text_extract::extract_page(&doc, &page, 0, &Default::default())
        .unwrap()
        .sourced_text();
    assert!(text.contains("AB\u{394}"), "{text}");
}

#[test]
fn strip_embeds_a_copy_without_cmap_and_leaves_the_original_untouched() {
    let base = Document::from_bytes(base_bytes()).unwrap();
    let original = stream_bytes(&base, PROGRAM);
    let out = edit(base_bytes(), CidFontProgram::StripCmap).unwrap();
    let used = out.report.fallback.as_ref().unwrap();
    assert_eq!(used.source, FallbackSource::SameProgram);
    assert_eq!(used.cid_program, Some(CidProgramUse::StrippedCopy));
    let saved = Document::from_bytes(out.bytes.clone()).unwrap();
    let copy = cid_program(&saved);
    assert_ne!(copy, PROGRAM);
    assert!(has_cmap(&original) && !has_cmap(&stream_bytes(&saved, copy)));
    assert_eq!(stream_bytes(&saved, PROGRAM), original);
    assert!(
        !out.bytes[base_bytes().len()..]
            .windows(8)
            .any(|w| w == b"\n7 0 obj"),
        "the original program is not re-emitted"
    );
    draws_cleanly(&out.bytes);
}

#[test]
fn share_points_at_the_original_program_and_states_the_nonconformance() {
    let out = edit(base_bytes(), CidFontProgram::ShareStream).unwrap();
    let used = out.report.fallback.as_ref().unwrap();
    assert_eq!(used.cid_program, Some(CidProgramUse::SharedWithCmap));
    assert_eq!(
        cid_program(&Document::from_bytes(out.bytes.clone()).unwrap()),
        PROGRAM
    );
    assert!(
        out.report
            .disclosures
            .iter()
            .any(|d| d.contains("§9.9") && d.contains("shall not")),
        "{:?}",
        out.report.disclosures
    );
    draws_cleanly(&out.bytes);
}

#[test]
fn a_pdfa_claim_turns_share_into_strip_and_says_why() {
    let out = edit(pdfa_bytes(), CidFontProgram::ShareStream).unwrap();
    let used = out.report.fallback.as_ref().unwrap();
    assert_eq!(used.cid_program, Some(CidProgramUse::StrippedCopy));
    assert!(out.report.disclosures.iter().any(|d| d.contains("PDF/A")));
    assert_ne!(
        cid_program(&Document::from_bytes(out.bytes).unwrap()),
        PROGRAM
    );
}

#[test]
fn off_keeps_the_refusal() {
    let err = edit(base_bytes(), CidFontProgram::Off).unwrap_err();
    assert!(err.contains("U+0394"), "{err}");
}

#[test]
fn the_repertoire_agrees_with_the_edit() {
    let session = EditSession::new(Document::from_bytes(base_bytes()).unwrap());
    let with = |mode| {
        let opts = EditOptions::default()
            .with_embedded_glyphs(&EmbeddedProgramGlyphs)
            .with_cid_font_program(mode);
        session.run_repertoire_with(0, "ABC", None, &opts).unwrap()
    };
    assert!(with(CidFontProgram::StripCmap).accepts('\u{394}'));
    assert!(
        !with(CidFontProgram::StripCmap).accepts('E'),
        "E has no outline"
    );
    assert!(!with(CidFontProgram::Off).accepts('\u{394}'));
}
