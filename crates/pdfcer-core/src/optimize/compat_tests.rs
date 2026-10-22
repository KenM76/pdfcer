#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::tests::{build, content, session};
use super::*;
use crate::PdfVersion;
use crate::document::Document;
use crate::space_audit::tests::stream;
use crate::writer::SaveOptions;

const PDF_1_4: PdfVersion = PdfVersion { major: 1, minor: 4 };
const PDF_1_7: PdfVersion = PdfVersion { major: 1, minor: 7 };

/// 1 catalog, 2 pages, 3 page with `resources`, 4 content; `extra` from 5.
fn doc(catalog_extra: &str, resources: &str, extra: &[Vec<u8>]) -> Vec<u8> {
    let mut bodies = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R \
             /Resources << {resources} >> >>"
        )
        .into_bytes(),
        stream("", &content()),
    ];
    bodies.extend_from_slice(extra);
    build(&bodies)
}

fn used(bytes: Vec<u8>) -> Vec<VersionFeature> {
    features_used(&Document::from_bytes(bytes).unwrap())
        .into_iter()
        .map(|u| u.feature)
        .collect()
}

#[test]
fn a_plain_document_uses_no_versioned_feature() {
    assert_eq!(used(doc("", "", &[])), Vec::<VersionFeature>::new());
}

#[test]
fn transparency_is_found_three_dictionaries_down() {
    let found = used(doc("", "/ExtGState << /GS0 << /ca 0.5 >> >>", &[]));
    assert_eq!(found, vec![VersionFeature::Transparency]);
}

#[test]
fn opaque_and_normal_entries_are_not_transparency() {
    let gs = "/ExtGState << /GS0 << /CA 1 /ca 1.0 /BM [/Normal /Compatible] /SMask /None >> >>";
    assert_eq!(used(doc("", gs, &[])), Vec::<VersionFeature>::new());
}

#[test]
fn blend_modes_groups_and_soft_masks_are_transparency() {
    for entry in [
        "/BM /Multiply",
        "/BM [/Normal /Screen]",
        "/SMask 5 0 R",
        "/S /Transparency",
    ] {
        let gs = format!("/ExtGState << /GS0 << {entry} >> >>");
        assert_eq!(
            used(doc("", &gs, &[])),
            vec![VersionFeature::Transparency],
            "{entry}"
        );
    }
}

#[test]
fn filters_and_layers_are_found_with_their_versions() {
    let jpx = stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode",
        b"x",
    );
    let jbig2 = stream(
        "/Type /XObject /Subtype /Image /Width 1 /Height 1 /BitsPerComponent 1 \
         /Filter [/JBIG2Decode]",
        b"x",
    );
    let bytes = doc("/OCProperties << /OCGs [] /D << >> >>", "", &[jpx, jbig2]);
    let all = features_used(&Document::from_bytes(bytes).unwrap());
    let names: Vec<_> = all.iter().map(|u| u.feature).collect();
    assert_eq!(
        names,
        vec![
            VersionFeature::Jbig2,
            VersionFeature::Jpx,
            VersionFeature::OptionalContent
        ]
    );
    assert_eq!(all[0].example, Some(crate::object::ObjId::new(6, 0)));
    assert!(!all[0].above(PDF_1_4));
    assert!(all[1].above(PDF_1_4));
    assert_eq!(
        VersionFeature::Jpx.to_string(),
        "JPEG 2000 images (PDF 1.5)"
    );
}

fn brotli() -> OptimizeSettings {
    let mut s = OptimizeSettings::new();
    s.streams = StreamAction::Brotli { quality: 11 };
    s
}

#[test]
fn brotli_above_the_target_is_skipped_and_nothing_raises() {
    let mut settings = brotli();
    settings.compatibility = Some(PDF_1_7);
    let plan = session(doc("", "", &[])).plan_optimize(&settings).unwrap();
    assert!(!plan.uses_brotli());
    assert_eq!(plan.version_raise(), None);
    assert!(plan.skipped.iter().any(|s| matches!(
        s.reason,
        SkipReason::AboveCompatibility { what: "Brotli", .. }
    )));
    let shown = plan.skipped[0].reason.to_string();
    assert!(
        shown.contains("needs PDF 2.0, above the compatibility target 1.7"),
        "{shown}"
    );
}

#[test]
fn brotli_at_a_2_0_target_is_written() {
    let mut settings = brotli();
    settings.compatibility = Some(PdfVersion { major: 2, minor: 0 });
    let plan = session(doc("", "", &[])).plan_optimize(&settings).unwrap();
    assert!(plan.uses_brotli());
}

fn packed(compatibility: Option<PdfVersion>) -> (OptimizePlan, Document) {
    let mut settings = OptimizeSettings::new();
    settings.streams = StreamAction::Recompress { level: 9 };
    settings.structure = RebuildOptions::all();
    settings.compatibility = compatibility;
    let mut s = session(doc("", "", &[]));
    let plan = s.plan_optimize(&settings).unwrap();
    s.apply_optimize(&plan).unwrap();
    let (file, _) = s
        .save_optimized(&plan, &SaveOptions::default(), &settings.structure)
        .unwrap();
    let OptimizedFile::Smaller(bytes) = file else {
        panic!("expected a smaller file, got {file:?}")
    };
    (plan, Document::from_bytes(bytes).unwrap())
}

#[test]
fn object_streams_stay_unpacked_under_a_1_4_target() {
    let (plan, after) = packed(Some(PDF_1_4));
    assert!(
        plan.notes
            .iter()
            .any(|n| n.contains("above the compatibility target"))
    );
    let features: Vec<_> = features_used(&after).iter().map(|u| u.feature).collect();
    assert!(
        !features.contains(&VersionFeature::ObjectStreams),
        "{features:?}"
    );
    assert!(!features.contains(&VersionFeature::CrossReferenceStreams));
}

#[test]
fn object_streams_are_packed_without_a_target() {
    let (plan, after) = packed(None);
    assert!(
        !plan
            .notes
            .iter()
            .any(|n| n.contains("compatibility target"))
    );
    let features: Vec<_> = features_used(&after).iter().map(|u| u.feature).collect();
    assert!(
        features.contains(&VersionFeature::ObjectStreams),
        "{features:?}"
    );
    assert!(features.contains(&VersionFeature::CrossReferenceStreams));
}
