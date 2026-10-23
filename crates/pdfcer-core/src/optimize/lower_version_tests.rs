#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::*;
use crate::optimize::tests::{build, v};
use crate::space_audit::tests::stream;

/// A one-page PDF 1.7 file: catalog extras `catalog`, page extras `page`.
fn file(catalog: &str, page: &str) -> Vec<u8> {
    build(&[
        format!("<< /Type /Catalog /Pages 2 0 R {catalog} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R {page} >>")
            .into_bytes(),
        stream("", b"0 0 m 10 10 l S"),
    ])
}

fn lower(bytes: Vec<u8>, to: PdfVersion) -> Result<LoweredVersion, LowerError> {
    lower_version(bytes, to, &LowerOptions::default(), &SaveOptions::default())
}

#[test]
fn a_plain_file_changes_only_its_header_digits() {
    let input = file("", "");
    let done = lower(input.clone(), v(1, 3)).unwrap();
    assert_eq!((done.from, done.to), (v(1, 7), v(1, 3)));
    assert_eq!(done.route, LowerRoute::InPlace);
    assert!(!done.catalog_version_removed);
    assert_eq!(done.bytes.len(), input.len());
    let changed: Vec<usize> = (0..input.len())
        .filter(|&i| input[i] != done.bytes[i])
        .collect();
    assert_eq!(changed, vec![7], "only the minor digit of %PDF-1.7 differs");
    assert!(done.bytes.starts_with(b"%PDF-1.3\n"));
}

#[test]
fn a_catalog_version_is_removed_by_an_update_after_the_original_bytes() {
    let input = file("/Version /1.7", "");
    let done = lower(input.clone(), v(1, 4)).unwrap();
    assert!(done.catalog_version_removed);
    assert_eq!(done.route, LowerRoute::InPlace);
    assert_eq!(&done.bytes[8..input.len()], &input[8..], "the base is kept");
    let doc = Document::from_bytes(done.bytes).unwrap();
    assert_eq!(doc.version(), v(1, 4));
    assert!(doc.catalog().unwrap().get(b"Version").is_none());
}

#[test]
fn object_streams_below_1_5_are_rebuilt_into_a_classic_table() {
    let doc = Document::from_bytes(file("", "")).unwrap();
    let (packed, _) =
        writer::rebuild(&doc, &RebuildOptions::all(), &SaveOptions::default()).unwrap();
    assert!(uses_streams(&Document::from_bytes(packed.clone()).unwrap()));
    let done = lower(packed, v(1, 4)).unwrap();
    assert!(
        matches!(done.route, LowerRoute::Rebuilt(_)),
        "{:?}",
        done.route
    );
    let doc = Document::from_bytes(done.bytes).unwrap();
    assert_eq!(doc.version(), v(1, 4));
    assert!(!uses_streams(&doc));
    assert!(doc.catalog().unwrap().get(b"Pages").is_some());
}

#[test]
fn object_streams_at_1_5_stay_in_place() {
    let doc = Document::from_bytes(file("", "")).unwrap();
    let (packed, _) =
        writer::rebuild(&doc, &RebuildOptions::all(), &SaveOptions::default()).unwrap();
    let done = lower(packed, v(1, 5)).unwrap();
    assert_eq!(done.route, LowerRoute::InPlace);
}

#[test]
fn content_above_the_target_refuses_and_names_it() {
    let err = lower(file("", "/UserUnit 2"), v(1, 5)).unwrap_err();
    match err {
        LowerError::BelowFloor { floor, needs, .. } => {
            assert_eq!(floor, v(1, 6));
            assert_eq!(needs, "/UserUnit");
        }
        e => panic!("{e:?}"),
    }
}

#[test]
fn a_target_at_or_above_the_declared_version_refuses() {
    assert!(matches!(
        lower(file("", ""), v(1, 7)),
        Err(LowerError::NotLower { .. })
    ));
    assert!(matches!(
        lower(file("", ""), v(2, 0)),
        Err(LowerError::NotLower { .. })
    ));
}

#[test]
fn an_unreadable_content_stream_refuses() {
    let bytes = build(&[
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R >>".to_vec(),
        b"<< /Length 4 /Filter /FlateDecode >>\nstream\njunk\nendstream".to_vec(),
    ]);
    assert!(matches!(
        lower(bytes, v(1, 3)),
        Err(LowerError::Unproven { .. })
    ));
}

#[test]
fn a_signed_file_refuses_unless_allowed() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/synthetic/signature/signed-full-coverage.pdf"
    );
    let bytes = std::fs::read(path).unwrap();
    let declared = Document::from_bytes(bytes.clone()).unwrap().version();
    let to = v(1, 3);
    assert!(declared > to, "the fixture declares {declared}");
    assert!(matches!(
        lower(bytes.clone(), to),
        Err(LowerError::Signed { .. })
    ));
    let allowed = LowerOptions::default().with_allow_signed(true);
    let r = lower_version(bytes, to, &allowed, &SaveOptions::default());
    assert!(!matches!(r, Err(LowerError::Signed { .. })), "{r:?}");
}

#[test]
fn a_four_byte_header_token_takes_the_rebuild_route() {
    let mut input = file("", "");
    input.splice(5..8, b"1.70".iter().copied());
    // Every offset moved by one byte: the loader recovers it.
    let done = lower(input, v(1, 3)).unwrap();
    assert!(matches!(done.route, LowerRoute::Rebuilt(_)));
    assert!(done.bytes.starts_with(b"%PDF-1.3"));
}

#[test]
fn the_header_patch_requires_exactly_three_bytes() {
    let mut a = b"%PDF-1.7\n".to_vec();
    assert!(patch_header(&mut a, v(1, 4)));
    assert_eq!(a, b"%PDF-1.4\n");
    let mut b = b"%PDF-1.70\n".to_vec();
    assert!(!patch_header(&mut b, v(1, 4)));
    let mut c = b"junk%PDF-2.0 \n".to_vec();
    assert!(patch_header(&mut c, v(1, 6)));
    assert_eq!(c, b"junk%PDF-1.6 \n");
    let mut d = b"no header".to_vec();
    assert!(!patch_header(&mut d, v(1, 4)));
}
