//! Dash and opacity in the ce dimension style cascade (pdfcer-gui request
//! G159): the dash is baked into the `/AP` as a `d` operator, the opacity is
//! the annotation's `/CA`, and both inherit factory → group → ce dimension.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    ArrowForm, DEFAULT_GROUP_ID, DimDash, DimensionId, DimensionKind, GroupStyle, StyleOverrides,
    StyleSource,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession};
use pdfcer_core::object::{Dict, ObjId, Object};
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;

fn minimal_pdf() -> Vec<u8> {
    let bodies = [
        "<< /Type /Catalog /Pages 2 0 R >>",
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
    ];
    let mut buf = b"%PDF-1.7\n".to_vec();
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

fn linear() -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
        extension_gap: [None; 2],
    }
}

fn session_with_one() -> (EditSession, ObjId, DimensionId) {
    let mut s = EditSession::new(Document::from_bytes(minimal_pdf()).unwrap());
    let (annot, dim) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    (s, annot, dim)
}

/// The saved annotation dictionary and its `/AP /N` content.
fn saved(s: &EditSession, id: ObjId) -> (Dict, String) {
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let doc = Document::from_bytes(bytes).unwrap();
    let Object::Dict(annot) = doc.get(id).unwrap().value.clone() else {
        panic!("not a dict");
    };
    let Object::Dict(ap) = doc.resolve(annot.get(b"AP").unwrap()) else {
        panic!("no /AP");
    };
    let Object::Stream(st) = doc.resolve(ap.get(b"N").unwrap()) else {
        panic!("no /N");
    };
    let raw = st.data_span.slice(doc.bytes()).unwrap();
    let text =
        String::from_utf8_lossy(&pdfcer_core::filters::decode_stream(&st.dict, raw).unwrap())
            .into_owned();
    (annot, text)
}

fn ca(annot: &Dict) -> Option<f64> {
    annot.get(b"CA").and_then(Object::as_number)
}

fn dashed() -> DimDash {
    DimDash::new(&[3.0, 1.5]).unwrap()
}

#[test]
fn a_solid_opaque_ce_dimension_emits_neither_d_nor_ca() {
    let (s, annot, _) = session_with_one();
    let (dict, ap) = saved(&s, annot);
    assert!(!ap.contains(" d\n") && !ap.contains(" d "), "{ap}");
    assert_eq!(ca(&dict), None);
}

#[test]
fn a_group_dash_and_opacity_reach_every_member() {
    let (mut s, annot, _) = session_with_one();
    let (_, before) = saved(&s, annot);
    s.set_group_style(
        DEFAULT_GROUP_ID,
        GroupStyle {
            dash: Some(dashed()),
            opacity: Some(0.4),
            ..GroupStyle::default()
        },
    )
    .unwrap();
    let (dict, ap) = saved(&s, annot);
    assert!(ap.contains("[3 1.5] 0 d"), "{ap}");
    assert_eq!(ca(&dict), Some(0.4));
    assert!(
        !ap.contains(" gs"),
        "opacity is /CA, not an ExtGState: {ap}"
    );
    // The rest of the drawing is the solid one with a `d` inserted.
    assert_eq!(ap.replace("[3 1.5] 0 d\n", ""), before, "{ap}");
}

#[test]
fn an_explicit_solid_override_beats_a_dashed_group() {
    let (mut s, annot, dim) = session_with_one();
    s.set_group_style(
        DEFAULT_GROUP_ID,
        GroupStyle {
            dash: Some(dashed()),
            opacity: Some(0.4),
            ..GroupStyle::default()
        },
    )
    .unwrap();
    s.set_dimension_style(
        dim,
        StyleOverrides {
            dash: Some(DimDash::SOLID),
            opacity: Some(1.0),
            ..StyleOverrides::default()
        },
    )
    .unwrap();
    let (dict, ap) = saved(&s, annot);
    assert!(!ap.contains("0 d"), "{ap}");
    assert_eq!(ca(&dict), None, "1.0 is the default and is not written");
    let model = s.dimension_model();
    let d = model.dimension(dim).unwrap();
    let prov = pdfcer_core::dimension::style_provenance(model.group(d.group).unwrap(), &d.style);
    let each = prov.each();
    assert_eq!(each[11], ("dash", StyleSource::Dimension));
    assert_eq!(each[12], ("opacity", StyleSource::Dimension));
}

#[test]
fn stroked_terminators_stay_solid_outside_the_path_object() {
    for form in [ArrowForm::Open, ArrowForm::Slash] {
        let (mut s, annot, dim) = session_with_one();
        s.set_dimension_style(
            dim,
            StyleOverrides {
                dash: Some(dashed()),
                arrow_form: Some(form),
                ..StyleOverrides::default()
            },
        )
        .unwrap();
        let (_, ap) = saved(&s, annot);
        // §8.2 Figure 9: the `q [] 0 d` precedes the terminator's `m`.
        let solid = ap.matches("q\n[] 0 d\n").count();
        assert_eq!(solid, 2, "{form:?}: {ap}");
        for chunk in ap.split("q\n[] 0 d\n").skip(1) {
            let first = chunk.lines().next().unwrap();
            assert!(first.ends_with(" m"), "{form:?}: path starts after d: {ap}");
        }
    }
}

#[test]
fn the_style_survives_save_and_reopen() {
    let (mut s, _, dim) = session_with_one();
    s.set_dimension_style(
        dim,
        StyleOverrides {
            dash: Some(dashed()),
            opacity: Some(0.25),
            ..StyleOverrides::default()
        },
    )
    .unwrap();
    let bytes = s.to_incremental_bytes(&SaveOptions::identity()).unwrap().0;
    let back = EditSession::new(Document::from_bytes(bytes).unwrap());
    let style = back.dimension_model().dimension(dim).unwrap().style;
    assert_eq!(style.dash, Some(dashed()));
    assert_eq!(style.opacity, Some(0.25));
}

#[test]
fn an_out_of_range_opacity_is_refused_at_both_tiers() {
    let (mut s, _, dim) = session_with_one();
    let depth = s.undo_depth();
    for bad in [1.5, -0.1, f64::NAN] {
        assert!(matches!(
            s.set_dimension_style(
                dim,
                StyleOverrides {
                    opacity: Some(bad),
                    ..StyleOverrides::default()
                }
            ),
            Err(EditError::MarkupOpacityOutOfRange { .. })
        ));
        assert!(matches!(
            s.set_group_style(
                DEFAULT_GROUP_ID,
                GroupStyle {
                    opacity: Some(bad),
                    ..GroupStyle::default()
                }
            ),
            Err(EditError::MarkupOpacityOutOfRange { .. })
        ));
    }
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn set_annot_opacity_signposts_the_style_verb_for_a_ce_dimension() {
    let (mut s, annot, _) = session_with_one();
    let depth = s.undo_depth();
    assert!(matches!(
        s.set_annot_opacity(annot, pdfcer_core::edit::StyleEdit::Set(0.5)),
        Err(EditError::AnnotationIsCeDimension { .. })
    ));
    assert_eq!(s.undo_depth(), depth);
}

#[test]
fn an_opaque_restyle_removes_the_ca() {
    let (mut s, annot, dim) = session_with_one();
    let half = StyleOverrides {
        opacity: Some(0.5),
        ..StyleOverrides::default()
    };
    s.set_dimension_style(dim, half).unwrap();
    assert_eq!(ca(&saved(&s, annot).0), Some(0.5));
    s.set_dimension_style(dim, StyleOverrides::default())
        .unwrap();
    assert_eq!(ca(&saved(&s, annot).0), None);
}
