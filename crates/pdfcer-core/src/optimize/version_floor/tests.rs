#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::*;
use crate::object::ObjId;
use crate::optimize::tests::{build, v};
use crate::space_audit::tests::stream;

/// 1 catalog, 2 pages, 3 page, 4 content `ops`; `extra` from 5.
fn doc(catalog: &str, page: &str, ops: &[u8], extra: &[Vec<u8>]) -> VersionFloor {
    let mut bodies = vec![
        format!("<< /Type /Catalog /Pages 2 0 R {catalog} >>").into_bytes(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] /Contents 4 0 R {page} >>")
            .into_bytes(),
        stream("", ops),
    ];
    bodies.extend_from_slice(extra);
    version_floor(&Document::from_bytes(build(&bodies)).unwrap())
}

fn constructs(f: &VersionFloor) -> Vec<&str> {
    f.requirements
        .iter()
        .map(|r| r.construct.as_str())
        .collect()
}

#[test]
fn a_plain_document_needs_only_1_3() {
    let f = doc("", "", b"0 0 m 10 10 l S", &[]);
    assert_eq!(f.floor, v(1, 3));
    assert!(f.requirements.is_empty() && f.proven(), "{f:?}");
}

#[test]
fn a_page_key_sets_the_floor_and_names_its_object() {
    let f = doc("", "/UserUnit 2", b"", &[]);
    assert_eq!(f.floor, v(1, 6));
    assert_eq!(f.requirements[0].construct, "/UserUnit");
    assert_eq!(f.requirements[0].example, Some(ObjId::new(3, 0)));
}

#[test]
fn constant_alpha_depends_on_the_annotation_kind() {
    let annot = |sub: &str| {
        format!("<< /Type /Annot /Subtype /{sub} /Rect [0 0 9 9] /CA 0.5 >>").into_bytes()
    };
    let link = doc("", "/Annots [5 0 R]", b"", &[annot("Link")]);
    assert_eq!(link.floor, v(2, 0), "{link:?}");
    let square = doc("", "/Annots [5 0 R]", b"", &[annot("Square")]);
    assert_eq!(square.floor, v(1, 4), "{square:?}");
}

#[test]
fn an_untyped_graphics_state_is_known_by_the_edge_that_reaches_it() {
    let f = doc(
        "",
        "/Resources << /ExtGState << /G 5 0 R >> >>",
        b"",
        &[b"<< /BM /Multiply >>".to_vec()],
    );
    assert_eq!(f.floor, v(1, 4));
    assert_eq!(constructs(&f), ["/BM"]);
}

#[test]
fn an_outline_colour_is_1_4_but_an_annotation_colour_is_not() {
    let outline = doc(
        "/Outlines 5 0 R",
        "",
        b"",
        &[
            b"<< /First 6 0 R /Last 6 0 R /Count 1 >>".to_vec(),
            b"<< /Title (a) /Parent 5 0 R /C [1 0 0] >>".to_vec(),
        ],
    );
    assert_eq!(outline.floor, v(1, 4), "{outline:?}");
    let annot = doc(
        "",
        "/Annots [5 0 R]",
        b"",
        &[b"<< /Type /Annot /Subtype /Text /Rect [0 0 9 9] /C [1 0 0] >>".to_vec()],
    );
    assert_eq!(annot.floor, v(1, 3), "{annot:?}");
}

#[test]
fn a_kid_inherits_its_field_type_through_kids() {
    let f = doc(
        "/AcroForm << /Fields [5 0 R] >>",
        "",
        b"",
        &[
            b"<< /FT /Ch /T (c) /Kids [6 0 R] >>".to_vec(),
            b"<< /Parent 5 0 R /T (k) /I [0] >>".to_vec(),
        ],
    );
    assert_eq!(constructs(&f), ["/I on a choice field"]);
    assert_eq!(f.requirements[0].example, Some(ObjId::new(6, 0)));
}

#[test]
fn filters_count_from_a_name_or_an_array() {
    let img = |filter: &str| {
        stream(
            &format!("/Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter {filter}"),
            b"x",
        )
    };
    let jpx = doc("", "", b"", &[img("/JPXDecode")]);
    assert_eq!(jpx.floor, v(1, 5));
    let brotli = doc("", "", b"", &[img("[/BrotliDecode]")]);
    assert_eq!(constructs(&brotli), ["/Filter /BrotliDecode"]);
    assert_eq!(brotli.floor, v(2, 0));
}

#[test]
fn content_streams_are_read_for_inline_images_and_marked_content() {
    let f = doc(
        "",
        "",
        b"/OC /L1 BDC EMC BI /W 1 /H 1 /BPC 8 /CS /G /L 1 ID x EI",
        &[],
    );
    assert_eq!(
        constructs(&f),
        ["inline image /L", "marked-content tag /OC"]
    );
    assert_eq!(f.floor, v(2, 0));
}

#[test]
fn actual_text_on_a_span_is_1_4_and_elsewhere_1_5() {
    let span = doc("", "", b"/Span << /ActualText (a) >> BDC EMC", &[]);
    assert_eq!(span.floor, v(1, 4));
    let p = doc("", "", b"/P << /ActualText (a) >> BDC EMC", &[]);
    assert_eq!(p.floor, v(1, 5));
}

#[test]
fn an_unreadable_content_stream_leaves_the_floor_unproven() {
    let bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Contents 4 0 R >>".to_vec(),
        stream("/Filter /FlateDecode", b"not zlib"),
    ];
    let f = version_floor(&Document::from_bytes(build(&bodies)).unwrap());
    assert_eq!(
        f.unprovable,
        [Unprovable::UnreadableContent(ObjId::new(4, 0))]
    );
    assert!(!f.proven());
}

#[test]
fn the_catalog_version_is_structural_and_does_not_raise_the_floor() {
    let f = doc("/Version /2.0", "", b"", &[]);
    assert_eq!(f.floor, v(1, 3));
    assert_eq!(f.structural.len(), 1);
    assert_eq!(f.structural[0].version, v(2, 0));
}

#[test]
fn field_flags_and_encryption_versions_need_their_bits() {
    let f = doc(
        "/AcroForm << /Fields [5 0 R] >>",
        "",
        b"",
        &[format!("<< /FT /Tx /T (t) /Ff {} >>", 1 << 22).into_bytes()],
    );
    assert_eq!(constructs(&f), ["field flag"]);
    assert_eq!(f.floor, v(1, 4));
}

#[test]
fn a_utf8_text_string_is_2_0() {
    let f = doc("", "", b"", &[b"<< /Title <EFBBBF61> >>".to_vec()]);
    assert_eq!(constructs(&f), ["UTF-8 text string"]);
}

#[test]
fn render_mode_is_1_7_on_a_3d_view_and_2_0_on_a_3d_node() {
    let annot = b"<< /Type /Annot /Subtype /3D /Rect [0 0 9 9] /3DV 6 0 R >>".to_vec();
    let view = |na: &str| {
        format!("<< /Type /3DView /XN (v) /RM << /Subtype /Solid >> {na} >>").into_bytes()
    };
    let only_view = doc("", "/Annots [5 0 R]", b"", &[annot.clone(), view("")]);
    assert!(
        only_view.requirements.iter().all(|r| r.version <= v(1, 7)),
        "{only_view:?}"
    );
    let node = b"<< /Type /3DNode /N (n) /RM << /Subtype /Wireframe >> >>".to_vec();
    let with_node = doc(
        "",
        "/Annots [5 0 R]",
        b"",
        &[annot, view("/NA [7 0 R]"), node],
    );
    assert_eq!(with_node.floor, v(2, 0), "{with_node:?}");
    assert_eq!(with_node.requirements[0].construct, "/RM");
}

#[test]
fn a_structure_type_newer_than_the_document_raises_nothing() {
    let f = doc(
        "/StructTreeRoot 5 0 R",
        "",
        b"",
        &[
            b"<< /Type /StructTreeRoot /K 6 0 R >>".to_vec(),
            b"<< /Type /StructElem /S /Title /P 5 0 R >>".to_vec(),
        ],
    );
    assert!(f.requirements.iter().all(|r| r.version < v(2, 0)), "{f:?}");
}

#[test]
fn a_predefined_cmap_name_needs_the_version_that_lists_it() {
    let f = doc(
        "",
        "/Resources << /Font << /F 5 0 R >> >>",
        b"",
        &[b"<< /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /UniJIS-UTF16-H >>".to_vec()],
    );
    assert_eq!(constructs(&f), ["/Encoding /UniJIS-UTF16-H"]);
    assert_eq!(f.floor, v(1, 5));
}

#[test]
fn an_icc_profile_version_is_reported_and_raises_only_when_strict() {
    let mut header = vec![0u8; 128];
    header[8] = 4;
    header[9] = 0x30;
    let bodies = |profile: &[u8]| {
        vec![
            b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] /Resources << /ColorSpace << /C [/ICCBased 4 0 R] >> >> >>"
                .to_vec(),
            stream("/N 3", profile),
        ]
    };
    let load = || Document::from_bytes(build(&bodies(&header))).unwrap();
    let permissive = version_floor(&load());
    assert_eq!(permissive.floor, v(1, 3));
    assert_eq!(permissive.icc_profiles.len(), 1);
    assert_eq!(
        (
            permissive.icc_profiles[0].major,
            permissive.icc_profiles[0].minor
        ),
        (4, 3)
    );
    assert_eq!(permissive.icc_profiles[0].strict, Some(v(2, 0)));
    let strict = version_floor_with(
        &load(),
        FloorOptions::default().with_icc(IccVersionGate::Strict),
    );
    assert_eq!(strict.floor, v(2, 0));
    assert_eq!(constructs(&strict), ["ICC profile version 4.3"]);
    header[8] = 2;
    header[9] = 0x10;
    let old = version_floor_with(
        &Document::from_bytes(build(&bodies(&header))).unwrap(),
        FloorOptions::default().with_icc(IccVersionGate::Strict),
    );
    assert_eq!(old.floor, v(1, 3), "{old:?}");
}

#[test]
fn single_dictionary_keys_count_only_in_their_dictionary() {
    let page = doc(
        "",
        "/Path [[1 2]] /DL 5 /SS 1.0 /CI 6 0 R /EP 6 0 R",
        b"",
        &[],
    );
    assert!(page.requirements.is_empty(), "{page:?}");
    assert_eq!(page.floor, v(1, 3));

    let ink = doc(
        "",
        "/Annots [5 0 R]",
        b"",
        &[b"<< /Type /Annot /Subtype /Ink /Rect [0 0 9 9] /Path [[1 2 3 4]] >>".to_vec()],
    );
    assert_eq!(ink.floor, v(2, 0), "{ink:?}");

    let dl = doc(
        "",
        "/Resources << /XObject << /X 5 0 R >> >>",
        b"",
        &[stream("/Subtype /Form /BBox [0 0 1 1] /DL 0", b"")],
    );
    assert_eq!(dl.floor, v(1, 5), "{dl:?}");
    assert_eq!(constructs(&dl), ["/DL"]);

    let ci = doc("", "", b"", &[b"<< /CI << >> >>".to_vec()]);
    assert_eq!(ci.floor, v(1, 7), "{ci:?}");
}

/// 1 catalog, 2 pages, 3 page with `page` entries; `extra` from 4.
fn page_doc(page: &str, extra: &[Vec<u8>]) -> VersionFloor {
    let mut bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 9 9] {page} >>").into_bytes(),
    ];
    bodies.extend_from_slice(extra);
    version_floor(&Document::from_bytes(build(&bodies)).unwrap())
}

#[test]
fn a_contents_array_is_read_as_one_stream() {
    let f = page_doc(
        "/Contents [4 0 R 5 0 R]",
        &[stream("", b"/OC /MC0"), stream("", b"BDC EMC")],
    );
    assert_eq!(constructs(&f), ["marked-content tag /OC"]);
    assert_eq!(f.floor, v(1, 5));
    assert_eq!(f.requirements[0].example, Some(ObjId::new(3, 0)));
    assert!(f.proven(), "{f:?}");
}

#[test]
fn an_indirect_contents_array_carries_content_context() {
    let f = page_doc(
        "/Contents 4 0 R",
        &[b"[5 0 R]".to_vec(), stream("", b"/OC /MC0 BDC EMC")],
    );
    assert_eq!(f.floor, v(1, 5), "{f:?}");
    let bad = page_doc(
        "/Contents 4 0 R",
        &[
            b"[5 0 R]".to_vec(),
            stream("/Filter /FlateDecode", b"not zlib"),
        ],
    );
    assert_eq!(
        bad.unprovable,
        [Unprovable::UnreadableContent(ObjId::new(5, 0))]
    );
}

#[test]
fn an_indirect_fields_array_carries_field_context() {
    let f = doc(
        "/AcroForm << /Fields 5 0 R >>",
        "",
        b"",
        &[
            b"[6 0 R]".to_vec(),
            b"<< /T (untyped) /Lock << /Action /All >> >>".to_vec(),
        ],
    );
    assert_eq!(constructs(&f), ["/Lock"]);
}

#[test]
fn a_filter_given_by_reference_counts_for_each_stream_using_it() {
    let img = || {
        stream(
            "/Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter 7 0 R",
            b"x",
        )
    };
    let f = doc("", "", b"", &[img(), img(), b"/JPXDecode".to_vec()]);
    assert_eq!(constructs(&f), ["/Filter /JPXDecode"]);
    assert_eq!(f.requirements[0].objects, 2);
    assert_eq!(f.floor, v(1, 5));
    let array = doc(
        "",
        "",
        b"",
        &[
            img(),
            b"null".to_vec(),
            b"[/FlateDecode 8 0 R]".to_vec(),
            b"/BrotliDecode".to_vec(),
        ],
    );
    assert_eq!(constructs(&array), ["/Filter /BrotliDecode"]);
}

#[test]
fn only_contents_and_cert_of_a_signature_are_binary() {
    let sig = |reason: &str| {
        format!(
            "<< /Type /Sig /ByteRange [0 1 2 3] /Contents <EFBBBF00> /Cert [<EFBBBF01>] /Reason {reason} >>"
        )
        .into_bytes()
    };
    let utf8 = doc("", "", b"", &[sig("<EFBBBF61>")]);
    assert_eq!(constructs(&utf8), ["UTF-8 text string"]);
    assert_eq!(utf8.floor, v(2, 0));
    let plain = doc("", "", b"", &[sig("(a)")]);
    assert!(plain.requirements.is_empty(), "{plain:?}");
}

#[test]
fn an_unreferenced_name_object_reports_nothing() {
    let f = doc("", "", b"", &[b"/JPXDecode".to_vec()]);
    assert!(f.requirements.is_empty(), "{f:?}");
}

#[test]
fn a_self_referencing_filter_array_ends() {
    let f = doc(
        "",
        "",
        b"",
        &[
            stream(
                "/Type /XObject /Subtype /Image /Width 1 /Height 1 /Filter 6 0 R",
                b"x",
            ),
            b"[6 0 R 6 0 R /JPXDecode]".to_vec(),
        ],
    );
    assert_eq!(f.floor, v(1, 5), "{f:?}");
}
