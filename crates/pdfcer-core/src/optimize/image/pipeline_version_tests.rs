#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::pipeline_ccitt_tests::{GREY_1, dashes, one_bit, saved};
use super::pipeline_tests::{AT_100, doc, plan, skip};
use super::{ImageEncode, ImageSkip, VersionRaise};
use crate::PdfVersion;
use crate::document::Document;
use crate::object::{Name, ObjId, Object};
use crate::optimize::{OptimizedFile, RebuildOptions, SkipReason, StreamAction};
use crate::writer::SaveOptions;

const PDF_1_4: PdfVersion = PdfVersion { major: 1, minor: 4 };

/// A PDF 1.3 document with one 1-bit image.
fn pdf_1_3() -> Vec<u8> {
    let mut bytes = doc(AT_100, &[one_bit(GREY_1, &dashes(false))]);
    assert!(bytes.starts_with(b"%PDF-1.7"));
    bytes[7] = b'3';
    bytes
}

#[test]
fn version_raise_parses_and_prints() {
    let a = StreamAction::parse("image(encode=jbig2, version-raise=refuse)").unwrap();
    assert_eq!(a.to_string(), "image(encode=jbig2,version-raise=refuse)");
    let StreamAction::Image(p) = a else { panic!() };
    assert_eq!(p.version_raise, Some(VersionRaise::Refuse));
    assert!(StreamAction::parse("image(version-raise=maybe)").is_err());
}

/// By default a named JBIG2 encoder raises a PDF 1.3 document to 1.4
/// through the catalog's `/Version`; the header is left as it was.
#[test]
fn jbig2_in_a_pdf_1_3_document_raises_the_catalog_version() {
    let (mut s, p) = plan(pdf_1_3(), &[("image(encode=jbig2)", None)]);
    let img = p.changes.iter().find(|c| c.id.num == 5).unwrap();
    assert_eq!(img.image.as_ref().unwrap().raises_version, Some(PDF_1_4));
    assert_eq!(p.version_raise(), Some(PDF_1_4));
    s.apply_optimize(&p).unwrap();
    assert_eq!(s.view().version(), PDF_1_4);
    let OptimizedFile::Smaller(bytes) = s
        .save_optimized(&p, &SaveOptions::default(), &RebuildOptions::none())
        .unwrap()
        .0
    else {
        panic!("expected a smaller file")
    };
    assert!(bytes.starts_with(b"%PDF-1.3"));
    let d = Document::from_bytes(bytes).unwrap();
    assert_eq!(d.version(), PDF_1_4);
    assert_eq!(
        d.catalog().unwrap().get(b"Version"),
        Some(&Object::Name(Name::from(b"1.4")))
    );
    let Some(Object::Stream(st)) = d.get(ObjId::new(5, 0)).map(|o| o.value.clone()) else {
        panic!()
    };
    assert_eq!(
        st.dict.get(b"Filter"),
        Some(&Object::Name(Name::from(b"JBIG2Decode")))
    );
}

/// The raised image still decodes to its samples.
#[test]
fn the_raised_image_decodes_to_the_same_samples() {
    let px = dashes(false);
    let mut bytes = doc(AT_100, &[one_bit(GREY_1, &px)]);
    bytes[7] = b'3';
    let (s, p) = plan(bytes, &[("image(encode=jbig2)", None)]);
    let (dict, samples) = saved(s, &p);
    assert_eq!(
        dict.get(b"Filter"),
        Some(&Object::Name(Name::from(b"JBIG2Decode")))
    );
    assert_eq!(samples, px);
}

/// `version-raise=refuse` skips the image by name and never touches the
/// version.
#[test]
fn version_raise_refuse_skips_the_image() {
    let (s, p) = plan(
        pdf_1_3(),
        &[("image(encode=jbig2,version-raise=refuse)", None)],
    );
    let why = skip(&p, 5).unwrap();
    assert!(matches!(
        why,
        SkipReason::ImageUnsupported(ImageSkip::VersionTooLow {
            encode: ImageEncode::Jbig2,
            ..
        })
    ));
    let text = why.to_string();
    assert!(
        text.contains("needs PDF 1.4") && text.contains("version-raise=refuse"),
        "{text}"
    );
    assert_eq!(p.version_raise(), None);
    assert_eq!(s.view().version().minor, 3);
}

/// A document already at 1.4 or above is never raised, and its catalog is
/// not rewritten.
#[test]
fn a_document_at_the_needed_version_is_not_raised() {
    let (mut s, p) = plan(
        doc(AT_100, &[one_bit(GREY_1, &dashes(false))]),
        &[("image(encode=jbig2)", None)],
    );
    assert_eq!(p.version_raise(), None);
    s.apply_optimize(&p).unwrap();
    let root = s.view().version();
    assert_eq!(root, PdfVersion { major: 1, minor: 7 });
    let catalog = s.value(ObjId::new(1, 0)).and_then(Object::as_dict).unwrap();
    assert_eq!(catalog.get(b"Version"), None);
}

/// `auto` in a PDF 1.3 document never raises it.
#[test]
fn auto_never_raises_the_version() {
    let (_, p) = plan(pdf_1_3(), &[("image(encode=auto)", None)]);
    assert_eq!(p.version_raise(), None);
}
