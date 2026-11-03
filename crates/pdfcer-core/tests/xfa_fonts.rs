//! XFA text set in shell-supplied faces (`XfaFonts`): measured in their
//! widths, written with their programs embedded or referenced, refused
//! when the licence forbids embedding, and disclosed when absent.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use std::sync::Arc;

use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditSession, XfaMaterializeOutcome, XfaSignatures};
use pdfcer_core::fontinfo::FsTypeBits;
use pdfcer_core::writer::SaveOptions;
use pdfcer_core::xfa::{
    FontEmbedding, FontProgramKind, LayoutOptions, MergeOptions, PaintOptions, XfaFace,
    XfaFaceSource, XfaFontProgram, XfaFonts, XfaImages, XfaLayout, layout_template_with,
    merge_data, read_template, read_xdp,
};

use crate::xfa_materialize::{DYNAMIC, contents, pdf_with};

/// Two pages, both setting text in `Wide`; the first also has a growable
/// `g` in `Wide` and an `m` in `Myriad Pro`.
const XDP: &str = r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="form1" layout="tb"><pageSet><pageArea name="P"><contentArea x="36pt" y="36pt" w="540pt" h="720pt"/><medium short="612pt" long="792pt"/></pageArea></pageSet><draw name="g" w="100pt" minH="1pt"><font typeface="Wide" size="10pt"/><value><text>aaaa aaaa aaaa aaaa aaaa aaaa</text></value></draw><draw name="m" w="200pt" h="20pt"><font typeface="Myriad Pro" size="10pt"/><value><text>Myriad</text></value></draw><draw name="t" w="200pt" h="500pt"><font typeface="Wide" size="10pt"/><value><text>Hello</text></value></draw><field name="b" w="200pt" h="400pt"><font typeface="Wide" size="10pt"/></field></subform></template><xfa:datasets xmlns:xfa="http://www.xfa.org/schema/xfa-data/1.0/"><xfa:data><form1><b>Ken</b></form1></xfa:data></xfa:datasets></xdp:xdp>"#;

const PROGRAM: &[u8] = b"not a real TrueType program, only its bytes";

/// `Wide`: every character a full em, with `fs_type` as its `OS/2.fsType`;
/// `None` for every typeface when `absent`.
struct Wide {
    fs_type: u16,
    absent: bool,
}

impl XfaFaceSource for Wide {
    fn face(&self, typeface: &str, _bold: bool, _italic: bool) -> Option<XfaFace> {
        (typeface == "Wide" && !self.absent).then(|| {
            let program = XfaFontProgram::new(
                Arc::from(PROGRAM),
                FontProgramKind::TrueType,
                Some(FsTypeBits::decode(self.fs_type, 4)),
            );
            XfaFace::new("Wide-Regular", |_| Some(1000)).with_program(program)
        })
    }
}

fn wide(fs_type: u16) -> XfaFonts {
    XfaFonts::with_source(Wide {
        fs_type,
        absent: false,
    })
}

fn doc() -> Document {
    Document::from_bytes(pdf_with(XDP, DYNAMIC, "", &[UR3])).unwrap()
}

const UR3: &str = "<< /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached >>";

fn laid_out(doc: &Document, fonts: &XfaFonts) -> (pdfcer_core::xfa::Template, XfaLayout) {
    let xdp = read_xdp(&doc.view()).unwrap().unwrap();
    let template = read_template(&xdp).unwrap().unwrap();
    let merged = merge_data(&template, &xdp, &MergeOptions::new()).unwrap();
    let opts = LayoutOptions::new();
    let layout = layout_template_with(&merged.template, &opts, &XfaImages::new(), fonts).unwrap();
    (merged.template, layout)
}

fn height_of(layout: &XfaLayout, name: &str) -> f64 {
    let item = layout.pages[0]
        .items
        .iter()
        .find(|i| i.name.as_deref() == Some(name))
        .unwrap();
    item.rect.h
}

/// Materializes in `fonts` and returns the outcome and the fully saved bytes.
fn materialized(fonts: &XfaFonts, embedding: FontEmbedding) -> (XfaMaterializeOutcome, Vec<u8>) {
    let doc = doc();
    let (template, layout) = laid_out(&doc, fonts);
    let mut s = EditSession::new(doc);
    let paint = PaintOptions::new().with_font_embedding(embedding);
    let out = s
        .materialize_xfa(
            &template,
            &layout.pages,
            &paint,
            fonts,
            XfaSignatures::Refuse,
        )
        .unwrap();
    (out, s.to_full_bytes(&SaveOptions::identity()).unwrap().0)
}

fn count(hay: &[u8], needle: &[u8]) -> usize {
    hay.windows(needle.len()).filter(|w| *w == needle).count()
}

#[test]
fn supplied_widths_wrap_text_taller_than_the_standard_face() {
    let d = doc();
    let std = height_of(&laid_out(&d, &XfaFonts::new()).1, "g");
    let wide = height_of(&laid_out(&d, &wide(0)).1, "g");
    assert!(wide > std, "Wide {wide} vs Helvetica {std}");
}

#[test]
fn an_embedded_face_is_written_once_and_shared_across_pages() {
    let (out, bytes) = materialized(&wide(0), FontEmbedding::Embed);
    assert_eq!(out.pages_written, 2);
    let supplied: Vec<_> = out.fonts.iter().filter(|f| f.supplied).collect();
    assert_eq!(supplied.len(), 1, "{:?}", out.fonts);
    assert_eq!(supplied[0].base_font, "Wide-Regular");
    assert!(supplied[0].program.is_some());
    assert_eq!(count(&bytes, b"/FontFile2"), 1);
    let length1 = format!("/Length1 {}", PROGRAM.len());
    assert_eq!(count(&bytes, length1.as_bytes()), 1);
    assert_eq!(count(&bytes, b"/BaseFont /Wide-Regular"), 1);
    let saved = Document::from_bytes(bytes).unwrap();
    let text = contents(&saved);
    assert!(text[1].contains("Ken"), "{}", text[1]);
}

#[test]
fn a_referenced_face_writes_no_program() {
    let (out, bytes) = materialized(&wide(0), FontEmbedding::Reference);
    let f = out.fonts.iter().find(|f| f.supplied).unwrap();
    assert!(f.program.is_none());
    assert_eq!(f.refusal, None);
    assert_eq!(count(&bytes, b"/FontFile"), 0);
    assert_eq!(count(&bytes, b"/BaseFont /Wide-Regular"), 1);
}

#[test]
fn a_restricted_face_is_referenced_and_the_refusal_said() {
    let (out, bytes) = materialized(&wide(0x0002), FontEmbedding::Embed);
    let f = out.fonts.iter().find(|f| f.supplied).unwrap();
    assert!(f.program.is_none());
    assert_eq!(f.refusal, Some("Restricted License embedding"));
    assert_eq!(count(&bytes, b"/FontFile"), 0);
}

#[test]
fn a_typeface_with_no_supplied_face_is_reported_as_a_fallback() {
    let fonts = wide(0);
    let _ = materialized(&fonts, FontEmbedding::Embed);
    let fallbacks = fonts.fallbacks();
    assert!(
        fallbacks.contains(&("Myriad Pro".to_owned(), "Helvetica".to_owned())),
        "{fallbacks:?}"
    );
    assert!(!fallbacks.iter().any(|(t, _)| t == "Wide"), "{fallbacks:?}");
}

#[test]
fn a_source_that_supplies_nothing_writes_the_standard_bytes() {
    let none = XfaFonts::with_source(Wide {
        fs_type: 0,
        absent: true,
    });
    let a = materialized(&none, FontEmbedding::Embed).1;
    let b = materialized(&XfaFonts::new(), FontEmbedding::Embed).1;
    assert!(a == b, "a source with no faces changed the written file");
}
