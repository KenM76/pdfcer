#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)] // tests

use super::*;
use crate::document::Document;
use crate::pageops::tests_support::{build_pdf, stream};

/// One page with `resources`; extra objects from 5.
pub(crate) fn one_page(page_extra: &str, resources: &str, extra: &[(u32, String)]) -> Document {
    let page = format!(
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Resources {resources} /Contents 4 0 R {page_extra} >>"
    );
    let content = stream("", "0 0 1 rg 10 10 50 50 re f");
    let mut objects: Vec<(u32, &str)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, &page),
        (4, &content),
    ];
    objects.extend(extra.iter().map(|(n, b)| (*n, b.as_str())));
    build_pdf(&objects)
}

fn kinds(u: &TransparencyUse) -> Vec<TransparencyKind> {
    u.sites.iter().map(|s| s.kind.clone()).collect()
}

#[test]
fn an_opaque_page_is_clean() {
    let doc = one_page(
        "",
        "<< /ExtGState << /G 5 0 R >> >>",
        &[(5, "<< /CA 1 /ca 1.0 /BM /Normal /SMask /None >>".into())],
    );
    let u = has_transparency(&doc.view(), 0).unwrap();
    assert!(u.is_empty(), "{u:?}");
    assert!(!u.in_page_content());
}

#[test]
fn a_graphics_state_reports_each_key_and_its_object() {
    let doc = one_page(
        "",
        "<< /ExtGState << /G 5 0 R /D << /BM [/Compatible /Multiply] >> >> >>",
        &[(
            5,
            "<< /CA 0.5 /ca 0.25 /BM /Screen /SMask << /S /Luminosity /G 6 0 R >> >>".into(),
        )],
    );
    let u = has_transparency(&doc.view(), 0).unwrap();
    let g5 = ObjId {
        num: 5,
        generation: 0,
    };
    let from_g5: Vec<_> = u
        .sites
        .iter()
        .filter(|s| s.holder == TransparencyHolder::GraphicsState(Some(g5)))
        .map(|s| s.kind.clone())
        .collect();
    assert_eq!(
        from_g5,
        vec![
            TransparencyKind::StrokeAlpha(0.5),
            TransparencyKind::FillAlpha(0.25),
            TransparencyKind::BlendMode(b"Screen".to_vec()),
            TransparencyKind::SoftMask,
        ]
    );
    let direct = u
        .sites
        .iter()
        .find(|s| s.holder == TransparencyHolder::GraphicsState(None))
        .unwrap();
    assert_eq!(
        direct.kind,
        TransparencyKind::BlendMode(b"Multiply".to_vec())
    );
    assert_eq!(direct.resource_name.as_deref(), Some(&b"D"[..]));
    assert!(u.in_page_content());
}

#[test]
fn images_groups_and_nested_forms_are_found() {
    let doc = one_page(
        "/Group << /S /Transparency /CS /DeviceRGB >>",
        "<< /XObject << /Im 5 0 R /Fm 6 0 R /Jp 8 0 R >> >>",
        &[
            (
                5,
                stream(
                    "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8 /SMask 7 0 R",
                    "x",
                ),
            ),
            (
                6,
                stream(
                    "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Group << /S /Transparency >> /Resources << /ExtGState << /H << /ca 0.3 >> >> /XObject << /Self 6 0 R >> >>",
                    "",
                ),
            ),
            (
                7,
                stream(
                    "/Type /XObject /Subtype /Image /Width 1 /Height 1 /ColorSpace /DeviceGray /BitsPerComponent 8",
                    "x",
                ),
            ),
            (
                8,
                stream(
                    "/Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter /JPXDecode /SMaskInData 1",
                    "x",
                ),
            ),
        ],
    );
    let u = has_transparency(&doc.view(), 0).unwrap();
    let k = kinds(&u);
    assert_eq!(u.sites[0].holder, TransparencyHolder::Page);
    assert_eq!(k[0], TransparencyKind::Group);
    assert!(k.contains(&TransparencyKind::ImageSoftMask));
    assert!(k.contains(&TransparencyKind::ImageSoftMaskInData));
    assert!(k.contains(&TransparencyKind::FillAlpha(0.3)));
    let form = ObjId {
        num: 6,
        generation: 0,
    };
    assert!(
        u.sites
            .iter()
            .any(|s| s.holder == TransparencyHolder::Form(form))
    );
    // The self-referencing form is visited once.
    assert_eq!(
        k.iter().filter(|k| **k == TransparencyKind::Group).count(),
        2
    );
}

#[test]
fn an_annotation_and_its_appearance_are_attributed_to_it() {
    let doc = one_page(
        "/Annots [5 0 R]",
        "<< >>",
        &[
            (
                5,
                "<< /Type /Annot /Subtype /Square /Rect [0 0 20 20] /CA 0.4 /AP << /N 6 0 R >> >>"
                    .into(),
            ),
            (
                6,
                stream(
                    "/BBox [0 0 20 20] /Resources << /ExtGState << /A << /BM /Multiply >> >> >>",
                    "",
                ),
            ),
        ],
    );
    let u = has_transparency(&doc.view(), 0).unwrap();
    let annot = ObjId {
        num: 5,
        generation: 0,
    };
    assert_eq!(u.sites.len(), 2, "{u:?}");
    assert!(
        u.sites
            .iter()
            .all(|s| s.holder == TransparencyHolder::Annotation(annot))
    );
    assert!(!u.in_page_content());
    assert_eq!(u.annotations(), vec![annot]);
}

#[test]
fn a_page_past_the_end_is_an_error() {
    let doc = one_page("", "<< >>", &[]);
    assert_eq!(
        has_transparency(&doc.view(), 3),
        Err(TransparencyScanError::PageOutOfRange { index: 3, count: 1 })
    );
}

#[test]
fn preflight_points_each_finding_at_its_carrier() {
    use crate::preflight::{self, Check, FindingTarget, Profile, ProfileRule, Severity};
    let doc = one_page(
        "/Annots [6 0 R]",
        "<< /ExtGState << /G 5 0 R >> >>",
        &[
            (5, "<< /ca 0.5 >>".into()),
            (
                6,
                "<< /Type /Annot /Subtype /Square /Rect [0 0 20 20] /BM /Multiply >>".into(),
            ),
        ],
    );
    let json = br#"{"name": "t", "rules": [{"id": "tr", "check": "transparency-used", "severity": "warning"}]}"#;
    let profile = Profile::from_json(json).unwrap();
    assert_eq!(
        profile.rules,
        vec![ProfileRule::new(
            "tr",
            Check::TransparencyUsed,
            Severity::Warning
        )]
    );
    let report = preflight::run(&doc.view(), &profile);
    let targets: Vec<_> = report.findings.iter().map(|f| f.target).collect();
    let id = |num| ObjId { num, generation: 0 };
    assert_eq!(
        targets,
        vec![
            FindingTarget::GraphicsState(id(5)),
            FindingTarget::Annotation(id(6))
        ]
    );
    assert_eq!(
        report.findings[0].message,
        "A graphics state (/G) uses fill alpha 0.5."
    );
    assert!(Profile::from_json(profile.to_json().as_bytes()).is_ok());
}
