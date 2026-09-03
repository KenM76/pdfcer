//! Integration tests for the Pass 12.M2 dimensioning subsystem's in-document
//! wiring (decision 011 §2.4): additive authoring (existing content
//! byte-verbatim), the hybrid storage (`/Line` + `/IT /LineDimension` + baked
//! `/AP` + portable `/Measure` mirror + authoritative `/PieceInfo` sidecar),
//! the per-group `/OCG` layer registered in `/OCProperties`, and the
//! scale-change → regenerate-all-members story.
//!
//! Public-API only (the same surface the CLI/GUI use). A minimal synthetic PDF
//! is built inline (catalog = obj 1, pages = obj 2, page = obj 3).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_core::dimension::{
    DEFAULT_GROUP_ID, DimensionId, DimensionKind, FitCircle, GroupId, NumberFormat, ScaleState,
    Unit, deserialize_model,
};
use pdfcer_core::document::Document;
use pdfcer_core::edit::{EditError, EditSession, GroupDeletion};
use pdfcer_core::graph::ObjectGraph;
use pdfcer_core::object::{ObjId, Object};
use pdfcer_core::vector::{AxisConstraint, Point};
use pdfcer_core::writer::SaveOptions;

/// Build a minimal one-page PDF: catalog(1) → pages(2) → page(3).
fn minimal_pdf() -> Vec<u8> {
    minimal_pdf_with_catalog("<< /Type /Catalog /Pages 2 0 R >>")
}

/// The same one-page PDF with an arbitrary catalog body — so a test can plant
/// a sidecar the writer would never produce (a newer schema version) without
/// duplicating the byte-offset bookkeeping, which is the part that is easy to
/// get subtly wrong and hard to notice.
fn minimal_pdf_with_catalog(catalog: &str) -> Vec<u8> {
    let bodies = [
        catalog,
        "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 400] /Resources << >> >>",
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

fn linear() -> DimensionKind {
    DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
    }
}

/// An angular ce dimension: two arms meeting at a right angle at the origin
/// of the test page (`Pass 68.0`).
fn angular() -> DimensionKind {
    DimensionKind::Angular {
        apex: Point::new(100.0, 100.0),
        dir_a: Point::new(1.0, 0.0),
        dir_b: Point::new(0.0, 1.0),
        radius: 40.0,
        text_along: 0.0,
    }
}

fn session() -> (Vec<u8>, EditSession) {
    let bytes = minimal_pdf();
    let doc = Document::from_bytes(bytes.clone()).unwrap();
    (bytes, EditSession::new(doc))
}

fn save(session: &EditSession) -> Vec<u8> {
    session
        .to_incremental_bytes(&SaveOptions::identity())
        .unwrap()
        .0
}

#[test]
fn dimension_is_additive_existing_content_byte_verbatim() {
    // The R46 zero-exception acceptance: an additive dimension leaves every
    // original byte in place (incremental append), so the saved file starts
    // with the original file verbatim.
    let (original, mut s) = session();
    s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    let out = save(&s);
    assert!(
        out.starts_with(&original),
        "an additive dimension must not modify any original byte"
    );
    assert!(
        out.len() > original.len(),
        "the dimension objects were appended"
    );
}

#[test]
fn dimension_authors_line_it_oc_measure_and_registers_the_ocg() {
    let (_orig, mut s) = session();
    // Calibrate the default group first so the dimension carries a /Measure.
    s.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: 0.01 },
        NumberFormat::decimal(Unit::Meter, 2),
    )
    .unwrap();
    let (annot_id, _dim) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();

    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let Object::Dict(annot) = &reloaded.get(annot_id).unwrap().value else {
        panic!("annotation is not a dict");
    };
    assert_eq!(
        annot.get(b"Subtype").unwrap().as_name().unwrap().as_bytes(),
        b"Line"
    );
    assert_eq!(
        annot.get(b"IT").unwrap().as_name().unwrap().as_bytes(),
        b"LineDimension"
    );
    assert!(annot.get(b"AP").is_some(), "baked /AP present");
    assert!(
        annot.get(b"Measure").is_some(),
        "portable /Measure scale mirror present"
    );
    // The /OC points at the group OCG.
    let oc = annot
        .get(b"OC")
        .and_then(Object::as_reference)
        .expect("/OC ref");

    // The OCG is registered in the catalog /OCProperties (MANDATORY §8.11.4.2).
    let catalog = reloaded.catalog().unwrap();
    let ocp = catalog.get(b"OCProperties").unwrap().as_dict().unwrap();
    let ocgs = ocp.get(b"OCGs").unwrap().as_array().unwrap();
    assert!(
        ocgs.contains(&Object::Reference(oc)),
        "the annotation's OCG must be registered in /OCProperties /OCGs"
    );
    assert!(
        ocp.get(b"D")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Order")
            .is_some()
    );
}

#[test]
fn sidecar_survives_the_save_round_trip_and_matches_the_session_model() {
    let (_orig, mut s) = session();
    s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    let g = s
        .add_dimension_group("Floor Plan", Unit::FeetInches)
        .unwrap();
    s.set_group_scale(
        g,
        ScaleState::Calibrated { scale: 0.002 },
        NumberFormat::feet_inches(8, false),
    )
    .unwrap();
    let expected = s.dimension_model();

    // Reload from disk and read the /PieceInfo /pdfcer /Private sidecar back.
    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let catalog = reloaded.catalog().unwrap();
    let piece = catalog.get(b"PieceInfo").unwrap().as_dict().unwrap();
    let pdfcer = piece.get(b"pdfcer").unwrap().as_dict().unwrap();
    // §14.5 Table 319: /LastModified is Required.
    assert!(pdfcer.get(b"LastModified").is_some());
    let private = pdfcer.get(b"Private").unwrap();
    let recovered = deserialize_model(private).expect("sidecar deserializes");

    assert_eq!(
        recovered.groups().len(),
        expected.groups().len(),
        "every group survived"
    );
    assert_eq!(recovered.dimensions().len(), 1, "the dimension survived");
    // The Floor Plan group's feet-inches scale survived exactly.
    let fp = recovered
        .groups()
        .iter()
        .find(|gr| gr.name == "Floor Plan")
        .unwrap();
    assert_eq!(fp.unit(), Unit::FeetInches);
    assert!(matches!(fp.scale, ScaleState::Calibrated { .. }));
}

#[test]
fn changing_group_scale_regenerates_all_member_labels() {
    // The decision-011 headline: change the group scale → all members update.
    let (_orig, mut s) = session();
    let (a_id, _) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    let (b_id, _) = s
        .add_dimension(
            0,
            DEFAULT_GROUP_ID,
            DimensionKind::Linear {
                a: Point::new(0.0, 0.0),
                b: Point::new(100.0, 0.0),
                constraint: AxisConstraint::Horizontal,
                offset: 0.0,
                text_along: 0.0,
            },
        )
        .unwrap();

    // Calibrate: 1 pt = 0.01 m. The 200-pt and 100-pt lines become 2 m and 1 m.
    let regenerated = s
        .set_group_scale(
            DEFAULT_GROUP_ID,
            ScaleState::Calibrated { scale: 0.01 },
            NumberFormat::decimal(Unit::Meter, 2),
        )
        .unwrap();
    assert_eq!(regenerated, 2, "both members regenerated in one command");

    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let contents = |id: ObjId| -> String {
        let Object::Dict(d) = &reloaded.get(id).unwrap().value else {
            panic!()
        };
        match d.get(b"Contents").unwrap() {
            Object::String(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            _ => panic!("contents not a string"),
        }
    };
    assert_eq!(contents(a_id), "2.00 m");
    assert_eq!(contents(b_id), "1.00 m");
}

/// **Moving a ce dimension relocates it without re-measuring it.**
///
/// A translation is a rigid motion, so the measured value must be identical
/// before and after — moving a dimension repositions the annotation, it does
/// not change what was measured. The `/Rect` and `/L` must shift by exactly
/// the requested delta.
///
/// `/L` is the assertion that matters. The regeneration this shares with
/// `set_group_scale` used to rewrite only `/Rect`, `/Contents` and `/Measure`,
/// which is indistinguishable from correct for a scale change and leaves a
/// moved dimension's measured line (§12.5.6.7) pointing at where it used to
/// be — a file that renders right and reports the wrong endpoints to every
/// other reader.
#[test]
fn moving_a_ce_dimension_shifts_its_geometry_and_keeps_its_value() {
    let (_orig, mut s) = session();
    let (annot_id, dim_id) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    s.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: 0.01 },
        NumberFormat::decimal(Unit::Meter, 2),
    )
    .unwrap();

    let read = |bytes: Vec<u8>| -> (Vec<f64>, Vec<f64>, String) {
        let doc = Document::from_bytes(bytes).unwrap();
        let Object::Dict(d) = &doc.get(annot_id).unwrap().value else {
            panic!("annotation is not a dictionary")
        };
        let nums = |key: &[u8]| -> Vec<f64> {
            d.get(key)
                .and_then(Object::as_array)
                .map(|a| a.iter().filter_map(Object::as_number).collect())
                .unwrap_or_default()
        };
        let label = match d.get(b"Contents").unwrap() {
            Object::String(b) => String::from_utf8_lossy(b).into_owned(),
            _ => panic!("contents not a string"),
        };
        (nums(b"Rect"), nums(b"L"), label)
    };

    let (rect0, l0, label0) = read(save(&s));
    assert_eq!(
        l0.len(),
        4,
        "a /Line annotation carries /L as [x1 y1 x2 y2]"
    );

    s.move_dimension(dim_id, 25.0, -10.0).unwrap();
    let (rect1, l1, label1) = read(save(&s));

    assert_eq!(
        label1, label0,
        "a translation preserves every distance, so the measured value must not change"
    );
    for (i, (after, before)) in l1.iter().zip(&l0).enumerate() {
        let expected = before + if i % 2 == 0 { 25.0 } else { -10.0 };
        assert!(
            (after - expected).abs() < 0.001,
            "/L component {i} must shift by the requested delta: {after} vs {expected}"
        );
    }
    for (i, (after, before)) in rect1.iter().zip(&rect0).enumerate() {
        let expected = before + if i % 2 == 0 { 25.0 } else { -10.0 };
        assert!(
            (after - expected).abs() < 0.001,
            "/Rect component {i} must shift by the requested delta: {after} vs {expected}"
        );
    }
}

/// `dimension_rects` reports what is CURRENTLY on the page, overlay and all.
///
/// The overlay half is the point: a shell hit-tests this to let an operator
/// click a ce dimension, so it has to describe where the dimension is now, not
/// where the file said it was on open. Reporting the stale rect would make a
/// moved dimension clickable at its old position and dead at its new one.
#[test]
fn dimension_rects_reports_the_current_position_not_the_opened_one() {
    let (_orig, mut s) = session();
    let (_annot, dim_id) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();

    let before = s.dimension_rects(0);
    assert_eq!(before.len(), 1, "the authored dimension is on page 0");
    assert_eq!(before[0].0, dim_id);

    s.move_dimension(dim_id, 30.0, 15.0).unwrap();
    let after = s.dimension_rects(0);
    assert_eq!(after.len(), 1);
    for (i, (a, b)) in after[0].1.iter().zip(&before[0].1).enumerate() {
        let expected = b + if i % 2 == 0 { 30.0 } else { 15.0 };
        assert!(
            (a - expected).abs() < 0.001,
            "rect component {i} must reflect the move already made this session"
        );
    }

    // A page with no ce dimensions reports none rather than every page's.
    assert!(
        s.dimension_rects(7).is_empty(),
        "an out-of-range page must be empty, not a fallback to page 0"
    );
}

/// **Deleting a ce dimension removes all four of its traces.**
///
/// The interesting assertion is not "it's gone from the page" — it is that the
/// SIDECAR record went too. Leaving it would make pdfcer keep believing in a
/// dimension the file no longer contains, and the next group-wide re-format
/// would try to regenerate an annotation that is not there.
#[test]
fn deleting_a_ce_dimension_removes_the_annotation_and_the_sidecar_record() {
    let (_orig, mut s) = session();
    let (annot_id, dim_id) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    assert_eq!(s.dimension_rects(0).len(), 1);

    s.delete_dimension(dim_id).unwrap();

    assert!(
        s.dimension_rects(0).is_empty(),
        "nothing is left on the page to click"
    );
    assert!(
        s.dimension_model().dimension(dim_id).is_none(),
        "the sidecar record must go too, or pdfcer keeps believing in it"
    );
    // The group survives on purpose — it carries a calibrated scale that is
    // not cheap to redo, and losing it as a side effect would be silent.
    assert!(
        s.dimension_model().group(DEFAULT_GROUP_ID).is_some(),
        "removing the last member must not take the group with it"
    );

    // On reload the page must not still point at the annotation, and the
    // annotation object must be gone. Checked from the saved FILE rather than
    // the session, because a removal that only exists in the overlay would
    // pass every in-memory assertion and still ship a dangling reference.
    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let pages = pdfcer_core::page_tree::pages(&reloaded).unwrap();
    let page = reloaded.get(pages[0].id).unwrap().value.clone();
    let refs: Vec<ObjId> = page
        .as_dict()
        .and_then(|d| d.get(b"Annots").cloned())
        .map(|a| reloaded.view().resolve(&a).clone())
        .and_then(|a| {
            a.as_array()
                .map(|arr| arr.iter().filter_map(Object::as_reference).collect())
        })
        .unwrap_or_default();
    assert!(
        !refs.contains(&annot_id),
        "the /Annots reference must be dropped, or the page points at nothing: {refs:?}"
    );
}

/// Undo restores a deleted ce dimension completely.
#[test]
fn undoing_a_ce_dimension_delete_restores_it() {
    let (_orig, mut s) = session();
    let (_annot, dim_id) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    let before = save(&s);
    s.delete_dimension(dim_id).unwrap();
    assert_ne!(save(&s), before, "the delete must actually change the file");
    s.undo().expect("undo the delete");
    assert_eq!(
        save(&s),
        before,
        "undoing a delete must restore the byte-identical prior save"
    );
    assert_eq!(
        s.dimension_rects(0).len(),
        1,
        "and it must be clickable again"
    );
}

/// A stale id is refused by name rather than silently doing nothing.
#[test]
fn deleting_an_unknown_ce_dimension_is_refused_by_name() {
    let (_orig, mut s) = session();
    let err = s
        .delete_dimension(pdfcer_core::dimension::DimensionId(999))
        .expect_err("an unknown id must refuse");
    assert!(
        matches!(
            err,
            pdfcer_core::edit::EditError::DimensionNotFound { id: 999 }
        ),
        "got {err:?}"
    );
}

/// **The defect the operator reported, pinned as an invariant.**
///
/// *"It looks like it give me the correct horizontal or vertical dimension but
/// it shows at an angle."* — 2026-08-04.
///
/// `leader_endpoints` returned the two PICKED points verbatim, so a dimension
/// constrained to Horizontal was drawn along whatever angle the two clicks
/// happened to make, while `measured_length` correctly reported only the
/// horizontal component. The line disagreed with its own caption.
///
/// The invariant is the fix stated as a property: the dimension line's length
/// equals the measured value, for every constraint and every pick pair. That
/// is checked here over a spread of inputs rather than one, because the old
/// behaviour was CORRECT whenever the picks happened to be axis-aligned — a
/// single well-chosen example would have passed before the fix.
#[test]
fn the_drawn_line_is_exactly_as_long_as_the_number_printed_on_it() {
    use pdfcer_core::vector::AxisConstraint;

    let picks = [
        ((100.0, 200.0), (300.0, 200.0)), // already horizontal
        ((100.0, 200.0), (300.0, 260.0)), // the reported case: skewed
        ((300.0, 260.0), (100.0, 200.0)), // reversed pick order
        ((100.0, 200.0), (100.0, 400.0)), // already vertical
        ((120.0, 180.0), (260.0, 90.0)),  // skewed the other way
    ];
    for constraint in [
        AxisConstraint::Horizontal,
        AxisConstraint::Vertical,
        AxisConstraint::Aligned,
    ] {
        for (offset, ((ax, ay), (bx, by))) in
            [0.0_f64, 25.0, -40.0].into_iter().zip(picks.iter().cycle())
        {
            let kind = DimensionKind::Linear {
                a: Point::new(*ax, *ay),
                b: Point::new(*bx, *by),
                constraint,
                offset,
                text_along: 0.0,
            };
            let Some((dim_a, dim_b, ext_a, ext_b)) = kind.linear_geometry() else {
                continue; // degenerate aligned pick: no axis, refused by design
            };
            let drawn = (dim_b.x - dim_a.x).hypot(dim_b.y - dim_a.y);
            let measured = kind.measured_points();
            assert!(
                (drawn - measured).abs() < 0.001,
                "{constraint:?} offset={offset}: the drawn line is {drawn} long but the \
                 label says {measured}"
            );
            // And the extension lines really do reach the measured points —
            // that is the other half of what was asked for.
            assert_eq!(
                (ext_a.x, ext_a.y),
                (*ax, *ay),
                "the first extension line must anchor on the first picked point"
            );
            assert_eq!((ext_b.x, ext_b.y), (*bx, *by));
        }
    }
}

/// A horizontal ce dimension is drawn HORIZONTALLY even when the picks are not.
///
/// The invariant above would also be satisfied by a line of the right length
/// pointing the wrong way; this pins the direction.
#[test]
fn a_constrained_dimension_line_runs_along_its_constraint() {
    use pdfcer_core::vector::AxisConstraint;

    let h = DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 260.0),
        constraint: AxisConstraint::Horizontal,
        offset: 0.0,
        text_along: 0.0,
    };
    let (a, b, _, _) = h.linear_geometry().unwrap();
    assert!(
        (a.y - b.y).abs() < 0.001,
        "a horizontal dimension's line must have equal y at both ends, got {a:?} {b:?}"
    );

    let v = DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(160.0, 400.0),
        constraint: AxisConstraint::Vertical,
        offset: 0.0,
        text_along: 0.0,
    };
    let (a, b, _, _) = v.linear_geometry().unwrap();
    assert!(
        (a.x - b.x).abs() < 0.001,
        "a vertical dimension's line must have equal x at both ends, got {a:?} {b:?}"
    );
}

/// The standoff's SIGN must not depend on which point was clicked first.
///
/// Without canonicalising the normal, clicking right-to-left negates it, and
/// the same positive offset puts the dimension line on the opposite side of
/// the drawing — which an operator experiences as the control working
/// backwards at random.
#[test]
fn the_standoff_direction_does_not_depend_on_pick_order() {
    use pdfcer_core::vector::AxisConstraint;

    let forward = DimensionKind::Linear {
        a: Point::new(100.0, 200.0),
        b: Point::new(300.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 30.0,
        text_along: 0.0,
    };
    let backward = DimensionKind::Linear {
        a: Point::new(300.0, 200.0),
        b: Point::new(100.0, 200.0),
        constraint: AxisConstraint::Horizontal,
        offset: 30.0,
        text_along: 0.0,
    };
    let (fa, _, _, _) = forward.linear_geometry().unwrap();
    let (ba, _, _, _) = backward.linear_geometry().unwrap();
    assert!(
        fa.y > 200.0 && ba.y > 200.0,
        "a positive standoff must put the line ABOVE the feature either way: \
         {fa:?} vs {ba:?}"
    );
}

/// A sidecar written before the offset field existed still loads completely.
///
/// The hazard this guards is specific and severe: `deserialize_model` gates on
/// `Version` with exact equality and answers `None` on a mismatch, which the
/// caller turns into a FRESH model — so a schema-version bump would have
/// silently discarded every group, every calibrated scale and every membership
/// of every existing dimensioned file, while the `/Line` annotations kept
/// rendering perfectly. `/Offset` is therefore an OPTIONAL key at the existing
/// version, and this proves the old shape still round-trips.
#[test]
fn a_sidecar_without_the_offset_key_still_loads_every_group_and_dimension() {
    use pdfcer_core::object::{Dict, Name};

    let (_orig, mut s) = session();
    s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    s.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: 0.01 },
        NumberFormat::decimal(Unit::Meter, 2),
    )
    .unwrap();

    // Serialise, then strip /Offset from every dimension — exactly the shape a
    // pre-27.0 build wrote.
    let serialized = pdfcer_core::dimension::serialize_model(&s.dimension_model());
    let mut d: Dict = serialized.as_dict().unwrap().clone();
    let stripped: Vec<Object> = d
        .get(b"Dimensions")
        .and_then(Object::as_array)
        .unwrap()
        .iter()
        .map(|dim| {
            let mut c = dim.as_dict().unwrap().clone();
            c.remove(b"Offset");
            Object::Dict(c)
        })
        .collect();
    d.insert(Name::from(b"Dimensions"), Object::Array(stripped));

    let recovered = deserialize_model(&Object::Dict(d)).expect("an older sidecar must still load");
    assert_eq!(
        recovered.dimensions().len(),
        1,
        "the dimension must survive a sidecar with no /Offset"
    );
    assert!(
        matches!(
            recovered.group(DEFAULT_GROUP_ID).map(|g| g.scale),
            Some(ScaleState::Calibrated { .. })
        ),
        "and so must the calibrated scale — losing it is the failure mode this pins"
    );
}

/// **A sidecar from a newer pdfcer is refused for writing, never overwritten.**
///
/// This is the other half of the version-gate hazard, and the dangerous half.
/// The gate used to demand exact equality and answer `None` on a mismatch,
/// which the session turns into a FRESH model — so an older build opening a
/// newer file would start empty and the next save would write that emptiness
/// over the operator's groups, calibrated scales and memberships. Nothing
/// would look wrong in between: the `/Line` annotations keep rendering
/// perfectly, so the loss is invisible until it is permanent.
///
/// The assertion that matters is the second one: after the refusal, nothing
/// has been staged, so there is no emptiness waiting to be written.
#[test]
fn a_sidecar_from_a_newer_build_refuses_writes_instead_of_discarding_it() {
    let doc = Document::from_bytes(minimal_pdf_with_catalog(
        "<< /Type /Catalog /Pages 2 0 R /PieceInfo << /pdfcer << /LastModified (D:20260804000000Z) /Private << /Version 999 /Groups [] /Dimensions [] >> >> >> >>",
    ))
    .unwrap();
    let mut s = EditSession::new(doc);

    for (what, result) in [
        ("add", s.add_dimension(0, DEFAULT_GROUP_ID, linear()).err()),
        (
            "scale",
            s.set_group_scale(
                DEFAULT_GROUP_ID,
                ScaleState::Calibrated { scale: 0.01 },
                NumberFormat::decimal(Unit::Meter, 2),
            )
            .err(),
        ),
        (
            "delete",
            s.delete_dimension(pdfcer_core::dimension::DimensionId(0))
                .err(),
        ),
    ] {
        assert!(
            matches!(
                result,
                Some(pdfcer_core::edit::EditError::SidecarWrittenByNewerBuild { found: 999, .. })
            ),
            "the {what} path must refuse a newer sidecar by name, got {result:?}"
        );
    }
    assert!(
        !s.is_modified(),
        "every refusal happens BEFORE any mutation — nothing is staged to overwrite the sidecar"
    );
}

/// **The two standards actually draw differently** (Pass 27.2).
///
/// Asserted on the appearance BYTES rather than on a flag, because a
/// `DimStandard` field that nothing reads would satisfy every other test in
/// this file. The specific difference checked is the one ISO 129-1:2018
/// cl. 4.1.1 mandates and that is visible at a glance: ANSI breaks the
/// dimension line and centres the value in the gap, ISO runs the line unbroken
/// with the value above it — so the ANSI appearance strokes the dimension line
/// in TWO pieces and the ISO one in a single piece.
#[test]
fn ansi_breaks_the_dimension_line_for_the_value_and_iso_does_not() {
    use pdfcer_core::dimension::{DimStandard, DimensionStyle, author_dimension};

    let kind = linear();
    let style = |standard| {
        DimensionStyle::new(
            ScaleState::Calibrated { scale: 0.01 },
            NumberFormat::decimal(Unit::Meter, 2),
            standard,
        )
    };
    let ansi = author_dimension(&kind, style(DimStandard::Ansi));
    let iso = author_dimension(&kind, style(DimStandard::Iso));

    assert_ne!(
        ansi.ap_content, iso.ap_content,
        "the two standards must produce different appearances, or the setting is inert"
    );
    // `m` begins a subpath. The dimension line plus two extension lines is
    // three under ISO; ANSI's broken line makes it four.
    let subpaths = |c: &[u8]| c.windows(3).filter(|w| w == b" m\n" || w == b" m ").count();
    assert!(
        subpaths(&ansi.ap_content) > subpaths(&iso.ap_content),
        "ANSI must stroke the dimension line in two pieces around the value, ISO in one: \
         ansi={} iso={}",
        subpaths(&ansi.ap_content),
        subpaths(&iso.ap_content)
    );
}

/// **ISO's mandated comma reaches both the label and the portable dict.**
///
/// The label and the `/Measure` dict are computed by different code, and a
/// reader that trusts `/Measure` computes its own string — so a comma in one
/// and a point in the other is a document that contradicts itself depending on
/// who reads it.
///
/// The `/RT` assertion is the subtle half: §12.9 Table 263 gives the thousands
/// separator a spec default of COMMA, so setting `/RD` to a comma without
/// pinning `/RT` yields `1,234,56` in a conforming reader — wrong in a way
/// pdfcer's own baked label would never reveal.
#[test]
fn switching_a_group_to_iso_sets_the_comma_in_the_label_and_the_measure_dict() {
    let (_orig, mut s) = session();
    s.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: 0.01 },
        NumberFormat::decimal(Unit::Meter, 2),
    )
    .unwrap();
    let (annot_id, _dim) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();

    let members = s
        .set_group_standard(DEFAULT_GROUP_ID, pdfcer_core::dimension::DimStandard::Iso)
        .expect("the group switches to ISO");
    assert_eq!(members, 1, "the member regenerates in the same command");

    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let Object::Dict(annot) = &reloaded.get(annot_id).unwrap().value else {
        panic!("annotation is not a dict")
    };
    let label = match annot.get(b"Contents").unwrap() {
        Object::String(b) => String::from_utf8_lossy(b).into_owned(),
        _ => panic!("contents not a string"),
    };
    assert_eq!(label, "2,00 m", "ISO mandates a comma decimal marker");

    // And the portable dict agrees.
    let view = reloaded.view();
    let measure = view
        .resolve(annot.get(b"Measure").expect("/Measure"))
        .as_dict()
        .expect("measure dict")
        .clone();
    let x = view.resolve(measure.get(b"X").unwrap()).clone();
    let first = x.as_array().unwrap().first().unwrap().clone();
    let nf = view.resolve(&first).as_dict().unwrap().clone();
    assert_eq!(
        nf.get(b"RD").and_then(|o| match o {
            Object::String(b) => Some(b.clone()),
            _ => None,
        }),
        Some(b",".to_vec()),
        "/RD must carry the comma"
    );
    assert!(
        nf.get(b"RT").is_some(),
        "/RT must be pinned too — its spec default is ALSO a comma, so /RD alone yields 1,234,56"
    );

    // Switching back restores the point, so the marker is not welded to ISO.
    s.set_group_standard(DEFAULT_GROUP_ID, pdfcer_core::dimension::DimStandard::Ansi)
        .unwrap();
    let back = Document::from_bytes(save(&s)).unwrap();
    let Object::Dict(annot) = &back.get(annot_id).unwrap().value else {
        panic!()
    };
    match annot.get(b"Contents").unwrap() {
        Object::String(b) => assert_eq!(String::from_utf8_lossy(b), "2.00 m"),
        _ => panic!(),
    }
}

/// A group's standard survives the sidecar round trip, and its absence means
/// ANSI — the additive-key discipline again.
#[test]
fn the_drafting_standard_round_trips_and_defaults_to_ansi() {
    let (_orig, mut s) = session();
    s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    assert_eq!(
        s.dimension_model()
            .group(DEFAULT_GROUP_ID)
            .unwrap()
            .standard,
        pdfcer_core::dimension::DimStandard::Ansi,
        "ANSI is the factory default (operator, 2026-08-04)"
    );
    s.set_group_standard(DEFAULT_GROUP_ID, pdfcer_core::dimension::DimStandard::Iso)
        .unwrap();

    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let catalog = reloaded.catalog().unwrap();
    let view = reloaded.view();
    let piece = view
        .resolve(catalog.get(b"PieceInfo").unwrap())
        .as_dict()
        .unwrap()
        .clone();
    let pdfcer = view
        .resolve(piece.get(b"pdfcer").unwrap())
        .as_dict()
        .unwrap()
        .clone();
    let private = view.resolve(pdfcer.get(b"Private").unwrap()).clone();
    let model = deserialize_model(&private).expect("sidecar");
    assert_eq!(
        model.group(DEFAULT_GROUP_ID).unwrap().standard,
        pdfcer_core::dimension::DimStandard::Iso,
        "the standard must survive the round trip"
    );
}

/// **A corrupt sidecar cannot make a ce dimension vanish, or write nonsense
/// into `/Rect`.**
///
/// `/Offset` and `/TextAlong` come out of the FILE and feed geometry that ends
/// up in the annotation. Measured on 2026-08-05 before the guard:
///
/// - `/Offset 1e308` wrote a **300-digit decimal** into `/Rect`, far past
///   PDF's ~3.4e38 architectural limit for a real (Annex C.1);
/// - `/Offset inf` produced `/Rect [-2 -2 3 3]` and `/L [0 0 0 0]` — the
///   measurement gone from the page while `/Contents` still read "200.00 pt".
///
/// The second is the worse one: the bounds accumulator drops non-finite points
/// by design, which is exactly what made the failure quiet. A dimension that
/// disappears while still claiming a value tells the operator nothing.
#[test]
fn hostile_placement_values_in_a_sidecar_are_refused_not_drawn() {
    use pdfcer_core::dimension::{DimensionModel, serialize_model};
    use pdfcer_core::object::{Dict, Name};

    for bad in [f64::INFINITY, f64::NAN, 1e308, -1e308] {
        let mut model = DimensionModel::new();
        let id = model.add_dimension(DEFAULT_GROUP_ID, linear());
        let _ = id;
        let obj = serialize_model(&model);
        let mut d: Dict = obj.as_dict().unwrap().clone();
        let dims: Vec<Object> = d
            .get(b"Dimensions")
            .and_then(Object::as_array)
            .unwrap()
            .iter()
            .map(|dim| {
                let mut c = dim.as_dict().unwrap().clone();
                c.insert(Name::from(b"Offset"), Object::Real(bad));
                c.insert(Name::from(b"TextAlong"), Object::Real(bad));
                Object::Dict(c)
            })
            .collect();
        d.insert(Name::from(b"Dimensions"), Object::Array(dims));

        let recovered = deserialize_model(&Object::Dict(d)).expect("the sidecar still loads");
        let rec = recovered
            .dimensions()
            .first()
            .expect("the dimension survives");
        let DimensionKind::Linear {
            offset, text_along, ..
        } = rec.kind
        else {
            panic!("expected linear")
        };
        assert_eq!(
            (offset, text_along),
            (0.0, 0.0),
            "a placement of {bad} must fall back to the default, not reach the geometry"
        );
    }
}

/// A corrupt MEASURED POINT drops the whole record, rather than drawing a
/// dimension between coordinates nobody chose.
///
/// Stricter than the placement guard on purpose: a standoff has a meaningful
/// zero, so a bad one costs the dimension's position. A measured point does
/// not — a dimension whose geometry is corrupt has no meaning to preserve.
#[test]
fn a_hostile_measured_point_drops_the_record() {
    use pdfcer_core::dimension::{DimensionModel, serialize_model};
    use pdfcer_core::object::{Dict, Name};

    let mut model = DimensionModel::new();
    model.add_dimension(DEFAULT_GROUP_ID, linear());
    let obj = serialize_model(&model);
    let mut d: Dict = obj.as_dict().unwrap().clone();
    let dims: Vec<Object> = d
        .get(b"Dimensions")
        .and_then(Object::as_array)
        .unwrap()
        .iter()
        .map(|dim| {
            let mut c = dim.as_dict().unwrap().clone();
            c.insert(
                Name::from(b"A"),
                Object::Array(vec![Object::Real(f64::INFINITY), Object::Real(0.0)]),
            );
            Object::Dict(c)
        })
        .collect();
    d.insert(Name::from(b"Dimensions"), Object::Array(dims));

    let recovered = deserialize_model(&Object::Dict(d)).expect("the sidecar still loads");
    assert!(
        recovered.dimensions().is_empty(),
        "a dimension with a non-finite measured point must not survive"
    );
}

/// **ISO text never reads upside down, including straight down.**
///
/// ISO 129-1:2018 cl. 4.1.1 requires aligned text to read from the bottom
/// (and vertical text from the right). The appearance flips the text direction
/// when it would otherwise be inverted — but the original condition tested
/// only `u.x < 0`, which misses an ALIGNED dimension pointing straight down,
/// where `u = (0, -1)`: `u.x` is exactly zero, no flip fires, and the value
/// reads top-to-bottom.
///
/// Asserted on the text matrix in the appearance stream, because that is the
/// only place the orientation exists — there is no flag to check.
#[test]
fn iso_text_never_reads_upside_down_in_any_direction() {
    use pdfcer_core::dimension::{DimStandard, DimensionStyle, author_dimension};

    // The text matrix is emitted as `a b c d e f Tm`. For our rotation the
    // first two are the text direction; `b < 0` means it runs downward.
    let direction = |content: &[u8]| -> (f64, f64) {
        let text = String::from_utf8_lossy(content).into_owned();
        let tm = text
            .lines()
            .find(|l| l.trim_end().ends_with(" Tm"))
            .expect("the appearance sets a text matrix");
        let n: Vec<f64> = tm
            .split_whitespace()
            .filter_map(|t| t.parse().ok())
            .collect();
        (n[0], n[1])
    };

    for (name, bx, by) in [
        ("down", 100.0, 0.0), // straight down: u = (0, -1)
        ("up", 100.0, 400.0), // straight up
        ("left", 0.0, 200.0), // right-to-left
        ("right", 300.0, 200.0),
    ] {
        let kind = DimensionKind::Linear {
            a: Point::new(100.0, 200.0),
            b: Point::new(bx, by),
            constraint: AxisConstraint::Aligned,
            offset: 20.0,
            text_along: 0.0,
        };
        let authored = author_dimension(
            &kind,
            DimensionStyle::new(
                ScaleState::Calibrated { scale: 0.01 },
                NumberFormat::decimal(Unit::Meter, 2),
                DimStandard::Iso,
            ),
        );
        let (dx, dy) = direction(&authored.ap_content);
        assert!(
            dy >= -0.0001 && (dy > 0.0001 || dx > 0.0),
            "{name}: ISO text must never run downward or right-to-left —              direction was ({dx}, {dy})"
        );
    }
}

/// Undo of a move restores the dimension exactly.
#[test]
fn undoing_a_ce_dimension_move_restores_it() {
    let (_orig, mut s) = session();
    let (_annot, dim_id) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    let before = save(&s);
    s.move_dimension(dim_id, 40.0, 40.0).unwrap();
    assert_ne!(save(&s), before, "the move must actually change the file");
    s.undo().expect("undo the move");
    assert_eq!(
        save(&s),
        before,
        "undoing a move must restore the byte-identical prior save"
    );
}

/// A stale dimension id is refused by name rather than silently doing nothing.
#[test]
fn moving_an_unknown_ce_dimension_is_refused_by_name() {
    let (_orig, mut s) = session();
    let err = s
        .move_dimension(pdfcer_core::dimension::DimensionId(999), 1.0, 1.0)
        .expect_err("an unknown id must refuse");
    assert!(
        matches!(
            err,
            pdfcer_core::edit::EditError::DimensionNotFound { id: 999 }
        ),
        "got {err:?}"
    );
}

#[test]
fn toggling_a_layer_moves_the_group_ocg_into_d_off() {
    let (_orig, mut s) = session();
    let g = s.add_dimension_group("Hidden", Unit::Millimeter).unwrap();
    // Author a dimension so the group gets an OCG.
    s.add_dimension(0, g, linear()).unwrap();
    // Hide the layer.
    let visible = s.toggle_dimension_layer(g, false).unwrap();
    assert!(!visible);

    let reloaded = Document::from_bytes(save(&s)).unwrap();
    // The group's OCG must now be in /OCProperties /D /OFF.
    let model = s.dimension_model();
    let ocg = model
        .groups()
        .iter()
        .find(|gr| gr.name == "Hidden")
        .and_then(|gr| gr.ocg)
        .expect("group has an OCG");
    let catalog = reloaded.catalog().unwrap();
    let d = catalog
        .get(b"OCProperties")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"D")
        .unwrap()
        .as_dict()
        .unwrap();
    let off = d.get(b"OFF").unwrap().as_array().unwrap();
    assert!(
        off.contains(&Object::Reference(ocg)),
        "a hidden group's OCG must be in /D /OFF"
    );
}

#[test]
fn undo_of_a_dimension_removes_everything() {
    let (original, mut s) = session();
    s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    assert!(s.is_modified());
    s.undo().expect("undo the dimension");
    assert!(!s.is_modified(), "undo restores the pristine session");
    // A save now is byte-identical to the original (nothing was written).
    let out = save(&s);
    assert_eq!(out, original, "after undo, the file is byte-identical");
}

// ---------------------------------------------------------------------------
// Pass 34.2 — `set_dimension_display`: radius↔diameter AFTER placement.
// ---------------------------------------------------------------------------

/// A circular ce dimension over a unit-friendly circle: centre (200, 400),
/// radius 50 pt, exact fit. Radius display, so a flipped one is visibly `2×`.
fn circular(show_diameter: bool) -> DimensionKind {
    DimensionKind::Circular {
        fit: FitCircle {
            center: Point::new(200.0, 400.0),
            radius: 50.0,
            residual: 0.0,
        },
        show_diameter,
    }
}

/// The whole point of the Pass: the choice made at draw time is not final.
///
/// Asserts the MODEL flips and — separately — that the baked `/AP` label
/// changes with it, because a model that flips while the drawn appearance does
/// not is exactly the "half-applied" failure R92's single-regeneration-path
/// rule exists to prevent.
#[test]
fn a_placed_circular_ce_dimension_can_be_switched_to_diameter_after_the_fact() {
    let (_orig, mut s) = session();
    let (annot_id, dim_id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, circular(false))
        .unwrap();

    // Both readings are checked through a SAVED, REOPENED document, so the
    // assertion covers what a third-party reader would see rather than a
    // session-internal value.
    let label_and_ap = |s: &EditSession| -> (String, String) {
        let reloaded = Document::from_bytes(save(s)).unwrap();
        let Object::Dict(annot) = &reloaded.get(annot_id).unwrap().value else {
            panic!("annotation is not a dict");
        };
        let contents = match annot.get(b"Contents").unwrap() {
            Object::String(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            other => panic!("/Contents is not a string: {other:?}"),
        };
        let ap_id = annot
            .get(b"AP")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"N")
            .and_then(Object::as_reference)
            .expect("/AP /N reference");
        let Some(Object::Stream(ap)) = reloaded.get(ap_id).map(|io| &io.value) else {
            panic!("/AP /N is not a stream");
        };
        let ap =
            String::from_utf8_lossy(ap.data_span.slice(reloaded.bytes()).unwrap()).into_owned();
        (contents, ap)
    };

    let (before_label, before_ap) = label_and_ap(&s);
    s.set_dimension_display(dim_id, true).unwrap();
    let (after_label, after_ap) = label_and_ap(&s);

    // 50 pt radius under the default group's never-set scale prints as the
    // radius; the diameter reading is exactly twice it. Asserted as a real
    // doubling rather than "the strings differ", because a regeneration that
    // changed the label for the wrong reason would still differ.
    assert_ne!(
        before_label, after_label,
        "the /Contents label must report the new reading"
    );
    assert_ne!(
        before_ap, after_ap,
        "the baked /AP must be regenerated, not left drawing the old reading"
    );
    let number = |s: &str| -> f64 {
        s.chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect::<String>()
            .parse()
            .unwrap_or_else(|_| panic!("no number in label {s:?}"))
    };
    assert!(
        (number(&after_label) - 2.0 * number(&before_label)).abs() < 1e-6,
        "the diameter reading must be exactly twice the radius one          (radius {before_label:?} -> diameter {after_label:?})"
    );

    let model = s.dimension_model();
    let DimensionKind::Circular { fit, show_diameter } = model.dimension(dim_id).unwrap().kind
    else {
        panic!("the ce dimension stopped being circular");
    };
    assert!(show_diameter, "the display flag must have flipped");
    // The measured geometry is untouched — the guarantee that makes this a
    // display change rather than a silent re-measure.
    assert!((fit.radius - 50.0).abs() < 1e-9, "the fitted radius moved");
    assert!(
        (fit.center.x - 200.0).abs() < 1e-9 && (fit.center.y - 400.0).abs() < 1e-9,
        "the fitted centre moved"
    );
}

/// The refusal is by NAME, not a silent no-op — the mirror of
/// `NotALinearDimension`, and the reason a GUI can report why a control it
/// offered did nothing.
#[test]
fn setting_the_display_of_a_linear_ce_dimension_is_refused_by_name() {
    let (_orig, mut s) = session();
    let (_annot, dim_id) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    let err = s
        .set_dimension_display(dim_id, true)
        .expect_err("a linear ce dimension has no radius/diameter reading");
    assert!(
        matches!(err, pdfcer_core::edit::EditError::NotACircularDimension { id } if id == dim_id.0),
        "expected NotACircularDimension, got {err:?}"
    );
}

/// ★★ The label BAKED INTO THE PAGE for an angular ce dimension reads in
/// DEGREES, not points (`Pass 68.0`).
///
/// `DimensionModel::display` had the angular branch; `author_dimension` did
/// not, and computed the same value a second way. So the Tool Options pane
/// read `77.5°` while the `/AP` stamped onto the page read **`77.47 pt`** — an
/// angle through the length formatter, wearing a unit it does not have. This
/// asserts against the SAVED document, because the baked copy is the one that
/// outlives the session and the one another reader will show.
#[test]
fn an_angular_ce_dimensions_baked_label_is_in_degrees_not_points() {
    let (_orig, mut s) = session();
    let (annot_id, _dim) = s.add_dimension(0, DEFAULT_GROUP_ID, angular()).unwrap();

    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let Object::Dict(annot) = &reloaded.get(annot_id).unwrap().value else {
        panic!("annotation is not a dict");
    };
    let Some(Object::String(bytes)) = annot.get(b"Contents") else {
        panic!("the authored label is stored as /Contents");
    };
    let contents = String::from_utf8_lossy(bytes).into_owned();

    assert!(
        contents.contains('\u{b0}'),
        "an angular ce dimension's label must carry the degree sign, got {contents:?}"
    );
    assert!(
        !contents.contains("pt"),
        "an angle is not a length and must not be stamped with a unit, got {contents:?}"
    );
    // The right-angle fixture: 90 degrees.
    assert!(
        contents.starts_with("90"),
        "expected 90 degrees, got {contents:?}"
    );
}

/// ★★ The degree sign is written as ONE `WinAnsi` byte in the appearance
/// stream, not as two UTF-8 bytes (`Pass 68.0`).
///
/// The label font is declared `/WinAnsiEncoding`, and the baker wrote
/// `label.as_bytes()` — raw UTF-8. That was correct for as long as every ce
/// dimension label was ASCII, and it was, until an angle put U+00B0 in one:
/// `C2 B0` rendered as `Â°` on the page.
///
/// This asserts on the CONTENT STREAM rather than on `/Contents`, because
/// `/Contents` is a text string the viewer never draws — the bytes inside
/// `BT … Tj … ET` are what an operator actually sees, and they are what was
/// wrong.
#[test]
fn the_degree_sign_is_one_winansi_byte_in_the_appearance_stream() {
    let (_orig, mut s) = session();
    let (annot_id, _dim) = s.add_dimension(0, DEFAULT_GROUP_ID, angular()).unwrap();
    let reloaded = Document::from_bytes(save(&s)).unwrap();

    let Object::Dict(annot) = &reloaded.get(annot_id).unwrap().value else {
        panic!("annotation is not a dict");
    };
    let ap_id = annot
        .get(b"AP")
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N"))
        .and_then(Object::as_reference)
        .expect("a baked /AP /N reference");
    let Some(Object::Stream(ap)) = reloaded.get(ap_id).map(|io| &io.value) else {
        panic!("/AP /N is not a stream");
    };
    let stream = String::from_utf8_lossy(ap.data_span.slice(reloaded.bytes()).unwrap());

    // The content-stream writer escapes high bytes as octal, so the question
    // is which octal escapes are present — NOT whether the raw bytes are.
    // Asserting on raw bytes passes vacuously here, because BOTH the correct
    // and the broken encoding get escaped and neither appears literally.
    assert!(
        stream.contains("\\260"),
        "the degree sign must be the single WinAnsi byte 0xB0 (octal 260): {stream}"
    );
    assert!(
        !stream.contains("\\302\\260"),
        "the degree sign must not be raw UTF-8 (C2 B0 -> octal 302 260): {stream}"
    );
}

/// ★ The pane and the page agree, by construction rather than by care.
///
/// The two used to be computed separately. This pins that the model's own
/// displayed value is exactly the string baked into the annotation — if a
/// future change reintroduces a second formatting path, this fails before an
/// operator sees two different numbers for one ce dimension.
#[test]
fn the_displayed_value_and_the_baked_label_are_the_same_string() {
    let (_orig, mut s) = session();
    let (annot_id, dim_id) = s.add_dimension(0, DEFAULT_GROUP_ID, angular()).unwrap();
    let shown = s
        .dimension_model()
        .display(dim_id)
        .expect("the model displays it")
        .text;

    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let Object::Dict(annot) = &reloaded.get(annot_id).unwrap().value else {
        panic!("annotation is not a dict");
    };
    let Some(Object::String(bytes)) = annot.get(b"Contents") else {
        panic!("/Contents");
    };
    let baked = String::from_utf8_lossy(bytes).into_owned();
    assert_eq!(
        shown, baked,
        "what the operator reads and what the file carries must be one string"
    );
}

/// ★ An ANGULAR ce dimension is placeable (`Pass 68.0`).
///
/// It was refused until this Pass — not by decision, but because
/// `place_dimension`'s guard asked "is this `Linear`" back when the only other
/// kind was circular. An angular ce dimension has both placement components
/// the verb sets: an arc radius to stand off by, and a text position along
/// that arc. The operator-visible symptom was a drag that did nothing, which
/// is precisely what `NotALinearDimension`'s own docs exist to prevent.
#[test]
fn placing_an_angular_ce_dimension_moves_its_arc_not_its_value() {
    let (_orig, mut s) = session();
    let (_annot, dim_id) = s.add_dimension(0, DEFAULT_GROUP_ID, angular()).unwrap();
    let before = s
        .dimension_model()
        .dimension(dim_id)
        .expect("just added")
        .kind
        .measured_points();

    s.place_dimension(dim_id, 75.0, 12.0)
        .expect("an angular ce dimension has a standoff and a text position");

    let after = s
        .dimension_model()
        .dimension(dim_id)
        .expect("still there")
        // `.clone()`: `DimensionKind` stopped being `Copy` at `Pass 107.0`.
        .kind
        .clone();
    match after {
        DimensionKind::Angular {
            radius, text_along, ..
        } => {
            assert!((radius - 75.0).abs() < 1e-9, "arc radius, got {radius}");
            assert!((text_along - 12.0).abs() < 1e-9, "got {text_along}");
        }
        other => panic!("expected Angular, got {other:?}"),
    }
    // Placement is value-preserving by construction: the angle is unchanged.
    assert!(
        (after.measured_points() - before).abs() < 1e-9,
        "placing a ce dimension must never change what it reports"
    );
}

/// ★ Dragging an arc inward past its own vertex clamps rather than collapsing
/// it. A zero-radius arc sits on the apex, unreadable and with no handle left
/// to drag it back out — the mark would be present and unrecoverable.
#[test]
fn placing_an_angular_ce_dimension_clamps_a_negative_arc_radius() {
    let (_orig, mut s) = session();
    let (_annot, dim_id) = s.add_dimension(0, DEFAULT_GROUP_ID, angular()).unwrap();
    s.place_dimension(dim_id, -30.0, 0.0)
        .expect("an overshot drag is a slip, not an error");
    match &s.dimension_model().dimension(dim_id).unwrap().kind {
        DimensionKind::Angular { radius, .. } => {
            let radius = *radius;
            assert!(radius > 0.0, "the arc must stay visible, got {radius}");
            assert!(
                (radius - 30.0).abs() < 1e-9,
                "a negative standoff reads as its magnitude, got {radius}"
            );
        }
        other => panic!("expected Angular, got {other:?}"),
    }

    s.place_dimension(dim_id, 0.0, 0.0)
        .expect("still placeable");
    match &s.dimension_model().dimension(dim_id).unwrap().kind {
        DimensionKind::Angular { radius, .. } => {
            let radius = *radius;
            assert!(
                (radius - pdfcer_core::edit::MIN_DIMENSION_ARC_RADIUS).abs() < 1e-9,
                "a zero radius must clamp to the floor, got {radius}"
            );
        }
        other => panic!("expected Angular, got {other:?}"),
    }
}

/// A CIRCULAR ce dimension is still refused by name — the narrowing did not
/// open the door to the kind that genuinely has nowhere to stand off to.
#[test]
fn placing_a_circular_ce_dimension_is_still_refused_by_name() {
    let (_orig, mut s) = session();
    let fit = pdfcer_core::dimension::fit_circle_taubin(&[
        Point::new(100.0, 0.0),
        Point::new(0.0, 100.0),
        Point::new(-100.0, 0.0),
    ])
    .expect("three non-collinear points fit a circle");
    let (_annot, dim_id) = s
        .add_dimension(
            0,
            DEFAULT_GROUP_ID,
            DimensionKind::Circular {
                fit,
                show_diameter: false,
            },
        )
        .unwrap();
    let err = s
        .place_dimension(dim_id, 10.0, 0.0)
        .expect_err("a circle has no axis to stand off from");
    assert!(
        matches!(err, pdfcer_core::edit::EditError::NotALinearDimension { id } if id == dim_id.0),
        "expected NotALinearDimension, got {err:?}"
    );
}

/// An unknown id is refused before anything is written.
#[test]
fn setting_the_display_of_an_unknown_ce_dimension_is_refused_by_name() {
    let (_orig, mut s) = session();
    let err = s
        .set_dimension_display(pdfcer_core::dimension::DimensionId(4242), true)
        .expect_err("no such ce dimension");
    assert!(
        matches!(err, pdfcer_core::edit::EditError::DimensionNotFound { id } if id == 4242),
        "expected DimensionNotFound, got {err:?}"
    );
}

/// One undo step, and it restores the previous reading — not a partially
/// reverted state where the model says radius and the drawing says diameter.
#[test]
fn undoing_a_display_change_restores_the_previous_reading() {
    let (_orig, mut s) = session();
    let (_annot, dim_id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, circular(false))
        .unwrap();
    s.set_dimension_display(dim_id, true).unwrap();
    s.undo().expect("undo the display change");
    let model = s.dimension_model();
    let DimensionKind::Circular { show_diameter, .. } = model.dimension(dim_id).unwrap().kind
    else {
        panic!("the ce dimension stopped being circular");
    };
    assert!(
        !show_diameter,
        "one undo must put the reading back to radius"
    );
}

/// The display choice survives save-and-reopen through the `/PieceInfo`
/// sidecar, which is what makes it an EDITABLE property rather than a
/// session-only one.
#[test]
fn a_display_change_round_trips_through_the_sidecar() {
    let (_orig, mut s) = session();
    let (_annot, dim_id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, circular(false))
        .unwrap();
    s.set_dimension_display(dim_id, true).unwrap();
    let bytes = save(&s);

    let doc = Document::from_bytes(bytes).unwrap();
    let reopened = EditSession::new(doc);
    let model = reopened.dimension_model();
    let DimensionKind::Circular { show_diameter, .. } = model.dimension(dim_id).unwrap().kind
    else {
        panic!("the reopened ce dimension is not circular");
    };
    assert!(
        show_diameter,
        "the diameter choice must survive save-and-reopen"
    );
}

/// Renaming a group changes the label and nothing else.
#[test]
fn a_dimension_group_can_be_renamed() {
    let (_orig, mut s) = session();
    let g = s
        .add_dimension_group("Floor Plan", Unit::Millimeter)
        .unwrap();
    s.rename_dimension_group(g, "Ground Floor").unwrap();
    let model = s.dimension_model();
    assert_eq!(model.group(g).unwrap().name, "Ground Floor");
    // Undo restores the old label -- one entry, like every other verb.
    s.undo().unwrap();
    assert_eq!(s.dimension_model().group(g).unwrap().name, "Floor Plan");
}

/// Names are NOT unique, deliberately -- see the verb's docs.
#[test]
fn two_groups_may_share_a_name() {
    let (_orig, mut s) = session();
    let a = s.add_dimension_group("Plan", Unit::Millimeter).unwrap();
    let b = s.add_dimension_group("Other", Unit::Millimeter).unwrap();
    s.rename_dimension_group(b, "Plan")
        .expect("a duplicate name is a label collision, not an error");
    let m = s.dimension_model();
    assert_eq!(m.group(a).unwrap().name, m.group(b).unwrap().name);
}

/// An empty group deletes; a populated one REFUSES and says how many.
///
/// The refusal is the interesting half: it is the orphan question, and
/// the count is in the error because "not empty" and "holds forty"
/// prompt different decisions.
#[test]
fn deleting_a_populated_group_refuses_by_name() {
    let (_orig, mut s) = session();
    let empty = s.add_dimension_group("Empty", Unit::Millimeter).unwrap();
    s.delete_dimension_group(empty)
        .expect("an empty group deletes");
    assert!(s.dimension_model().group(empty).is_none());

    let g = s.add_dimension_group("Held", Unit::Millimeter).unwrap();
    s.add_dimension(0, g, linear()).unwrap();
    match s.delete_dimension_group(g) {
        Err(EditError::DimensionGroupNotEmpty { members, .. }) => {
            assert_eq!(members, 1);
        }
        other => panic!("a populated group must refuse by name; got {other:?}"),
    }
    // And the refusal left the model ALONE -- validated before mutating.
    assert!(s.dimension_model().group(g).is_some());
}

/// Reassigning moves the members and then deletes the group.
#[test]
fn deleting_with_reassign_moves_the_members() {
    let (_orig, mut s) = session();
    let from = s.add_dimension_group("From", Unit::Millimeter).unwrap();
    let to = s.add_dimension_group("To", Unit::Millimeter).unwrap();
    let (_a, d) = s.add_dimension(0, from, linear()).unwrap();

    let moved = s
        .delete_dimension_group_with(from, GroupDeletion::Reassign(to))
        .unwrap();
    assert_eq!(moved, 1);
    let m = s.dimension_model();
    assert!(m.group(from).is_none(), "the group is gone");
    assert_eq!(m.dimension(d).unwrap().group, to, "the member moved");
}

/// Reassigning to the group being deleted is refused by name.
#[test]
fn reassigning_a_group_to_itself_is_refused() {
    let (_orig, mut s) = session();
    let g = s.add_dimension_group("G", Unit::Millimeter).unwrap();
    s.add_dimension(0, g, linear()).unwrap();
    assert!(matches!(
        s.delete_dimension_group_with(g, GroupDeletion::Reassign(g)),
        Err(EditError::DimensionGroupSelfReassign { .. })
    ));
}

/// An unknown destination is refused before anything is touched.
#[test]
fn reassigning_to_an_unknown_group_is_refused_and_changes_nothing() {
    let (_orig, mut s) = session();
    let g = s.add_dimension_group("G", Unit::Millimeter).unwrap();
    s.add_dimension(0, g, linear()).unwrap();
    assert!(matches!(
        s.delete_dimension_group_with(g, GroupDeletion::Reassign(GroupId(9999))),
        Err(EditError::DimensionGroupNotFound { .. })
    ));
    assert!(
        s.dimension_model().group(g).is_some(),
        "a refused deletion must leave the group in place"
    );
}

/// ★ Moving a dimension between groups RE-MEASURES it.
///
/// # The assertion that makes this verb more than a field write
///
/// A ce dimension's label is derived from its GROUP's scale and number
/// format, not from the dimension. So the two groups here are given
/// deliberately different ones — 1:1 in millimetres, and 1 cm per point in
/// metres — and the same geometry must therefore READ differently in each.
///
/// ★★ The first version of this test asserted only that `d.group` changed
/// and that undo put it back. **That passes against an implementation that
/// does nothing but write the field**, which is precisely the wrong version
/// of this verb — a dimension displaying a measurement its own group
/// disagrees with. It is the same mistake made an hour earlier in
/// `a_foreign_revision_cloud_survives_a_restyle`, where a dictionary
/// assertion could not see an appearance defect.
///
/// So this compares the annotation's `/Contents` — the label an operator
/// actually reads — across the move.
#[test]
fn moving_a_dimension_between_groups_re_measures_it() {
    let (_orig, mut s) = session();
    let a = s.add_dimension_group("A", Unit::Millimeter).unwrap();
    let b = s.add_dimension_group("B", Unit::Millimeter).unwrap();
    s.set_group_scale(
        a,
        ScaleState::OneToOne,
        NumberFormat::decimal(Unit::Millimeter, 1),
    )
    .unwrap();
    s.set_group_scale(
        b,
        ScaleState::Calibrated { scale: 0.01 },
        NumberFormat::decimal(Unit::Meter, 2),
    )
    .unwrap();

    let (annot, d) = s.add_dimension(0, a, linear()).unwrap();

    let label = |s: &EditSession| -> String {
        let view = s.view();
        let Object::Dict(dict) = view.resolved(annot) else {
            panic!("the dimension annotation must resolve");
        };
        match dict.get(b"Contents").map(|o| view.resolve(o)) {
            Some(Object::String(t)) => String::from_utf8_lossy(t).into_owned(),
            other => panic!("/Contents must be a string; got {other:?}"),
        }
    };

    let before = label(&s);
    s.set_dimension_group(d, b).unwrap();
    let after = label(&s);

    assert_eq!(s.dimension_model().dimension(d).unwrap().group, b);
    assert_ne!(
        before, after,
        "the label must be RE-MEASURED against the destination group: {before:?} \
         and {after:?} are the same, which means the verb wrote a field and \
         left the dimension reading its old group's scale"
    );

    // Undo restores both the group AND the label -- a half-undone move would
    // leave the right group with the wrong number on it.
    s.undo().unwrap();
    assert_eq!(s.dimension_model().dimension(d).unwrap().group, a);
    assert_eq!(
        label(&s),
        before,
        "undo must restore the label, not only the group id"
    );
}

/// A stale handle is refused rather than ignored, on both arguments.
#[test]
fn set_dimension_group_refuses_unknown_ids() {
    let (_orig, mut s) = session();
    let g = s.add_dimension_group("G", Unit::Millimeter).unwrap();
    let (_a, d) = s.add_dimension(0, g, linear()).unwrap();
    assert!(matches!(
        s.set_dimension_group(d, GroupId(9999)),
        Err(EditError::DimensionGroupNotFound { .. })
    ));
    assert!(matches!(
        s.set_dimension_group(DimensionId(9999), g),
        Err(EditError::DimensionNotFound { .. })
    ));
}

// ---------------------------------------------------------------------------
// Pass 107.0 / 107.1 — the PERIMETER ce dimension and its vertex editing.
//
// The operator's ask (Ken, 2026-08-20): "give me perimeter measuring tool as
// well where I click around to make a shape and it adds the distance of all
// the segments together for the dimension display. let me right click and add
// segments to the dimension. also I want to be able to edit the endpoints of
// the lines to adjust the shape. this should come with all the scaling options
// of the other dimension tools."
//
// Each test below pins one clause of that sentence, plus the structural
// consequences the request itself flagged (the `Copy` break, the sidecar
// migration) and the refusals the consuming shell asked to be able to
// predict.
// ---------------------------------------------------------------------------

/// A 100 x 60 rectangle traced clockwise from the bottom-left. Open, it is
/// three sides (100 + 60 + 100 = 260); closed, four (320).
fn perimeter(closed: bool) -> DimensionKind {
    DimensionKind::Perimeter {
        points: vec![
            Point::new(50.0, 50.0),
            Point::new(150.0, 50.0),
            Point::new(150.0, 110.0),
            Point::new(50.0, 110.0),
        ],
        closed,
        offset: 0.0,
        text_along: 0.0,
    }
}

/// The `/PieceInfo /pdfcer /Private` sidecar object of a reloaded document,
/// resolved through the view so an indirect reference is followed.
fn read_sidecar(doc: &Document) -> Object {
    let catalog = doc.catalog().unwrap();
    let view = doc.view();
    let piece = view
        .resolve(catalog.get(b"PieceInfo").unwrap())
        .as_dict()
        .unwrap()
        .clone();
    let pdfcer = view
        .resolve(piece.get(b"pdfcer").unwrap())
        .as_dict()
        .unwrap()
        .clone();
    view.resolve(pdfcer.get(b"Private").unwrap()).clone()
}

/// ★ The headline clause: ONE number, the sum of every segment — and the
/// closing segment is the entire difference between the two readings.
#[test]
fn a_perimeter_sums_its_segments_and_closure_adds_exactly_one() {
    let open = perimeter(false).measured_points();
    let closed = perimeter(true).measured_points();
    assert!(
        (open - 260.0).abs() < 1e-9,
        "an open path is its three sides, got {open}"
    );
    assert!(
        (closed - 320.0).abs() < 1e-9,
        "a closed perimeter adds the fourth side, got {closed}"
    );
    assert!(
        (closed - open - 60.0).abs() < 1e-9,
        "the difference must be exactly the closing segment"
    );
}

/// "all the scaling options of the other dimension tools" — the clause that
/// decided this had to be a `DimensionKind` rather than a markup annotation
/// with a number in its `/Contents`. A perimeter goes through the group's
/// scale and number format, so calibrating the group re-values it.
#[test]
fn a_perimeter_scales_through_its_group_like_every_other_kind() {
    let (_orig, mut s) = session();
    let (_annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    // 1 pt = 0.01 m, so 320 pt = 3.20 m.
    s.set_group_scale(
        DEFAULT_GROUP_ID,
        ScaleState::Calibrated { scale: 0.01 },
        NumberFormat::decimal(Unit::Meter, 2),
    )
    .unwrap();
    let shown = s.dimension_model().display(id).unwrap().text;
    assert!(
        shown.starts_with("3.2"),
        "a perimeter must be scaled by its group, got {shown:?}"
    );
}

/// The sidecar migration the request asked for a word on: a perimeter-bearing
/// file declares schema **3**, which is what makes an older build refuse to
/// WRITE over it rather than silently drop the ce dimension and save.
#[test]
fn a_perimeter_declares_a_sidecar_version_older_builds_refuse_to_overwrite() {
    let (_orig, mut s) = session();
    s.add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let sidecar = read_sidecar(&reloaded);
    let version = pdfcer_core::dimension::sidecar_version(&sidecar).expect("a version");
    assert_eq!(
        version, 3,
        "a new KIND is not a defaultable key — it must bump the schema"
    );
}

/// Every field survives the round trip, including the two that a defaulting
/// reader would get wrong: the vertex ORDER and the `closed` flag.
#[test]
fn a_perimeter_round_trips_through_the_sidecar() {
    let (_orig, mut s) = session();
    let (_annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(false))
        .unwrap();
    s.place_dimension(id, 17.0, -4.0).unwrap();
    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let sidecar = read_sidecar(&reloaded);
    let model = deserialize_model(&sidecar).expect("a readable model");
    let record = model.dimension(id).expect("the ce dimension");
    match &record.kind {
        DimensionKind::Perimeter {
            points,
            closed,
            offset,
            text_along,
        } => {
            assert_eq!(points.len(), 4, "every vertex must survive");
            assert!(!closed, "the open/closed flag must survive");
            assert!(
                (points[1].x - 150.0).abs() < 1e-9,
                "vertex order must survive"
            );
            assert!((points[1].y - 50.0).abs() < 1e-9);
            assert!(
                (offset - 17.0).abs() < 1e-9,
                "the placement pair must survive"
            );
            assert!((text_along + 4.0).abs() < 1e-9);
        }
        other => panic!("expected Perimeter, got {other:?}"),
    }
}

/// ★ "edit the endpoints of the lines to adjust the shape" — and this verb is
/// the first ce-dimension operation that deliberately RE-MEASURES. The
/// before/after labels are the rule-4 disclosure, carried on the outcome
/// because the shell cannot reconstruct the old value afterwards.
#[test]
fn moving_a_vertex_re_measures_and_reports_both_labels() {
    let (_orig, mut s) = session();
    let (_annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    let before = s.dimension_model().display(id).unwrap().text;
    // Drag the bottom-right corner 40 pt further right. TWO segments change,
    // which is the honest shape of a corner drag on a polygon: the bottom
    // lengthens and the right-hand side becomes a slant.
    let out = s.move_dimension_vertex(id, 1, 40.0, 0.0).unwrap();
    assert_eq!(out.vertices, 4, "a move changes no count");
    assert!(out.closed);
    assert_eq!(
        out.previous_label, before,
        "the disclosure's BEFORE must be the label the operator was looking at"
    );
    assert_ne!(
        out.label, out.previous_label,
        "a vertex move re-measures — that is the whole point of the verb"
    );
    let after = s
        .dimension_model()
        .dimension(id)
        .unwrap()
        .kind
        .measured_points();
    // Bottom (50,50)->(190,50) = 140; right (190,50)->(150,110) = hypot(40,60);
    // top = 100; closing = 60. Written out rather than as one number so the
    // test states its own arithmetic and a future edit to the fixture cannot
    // quietly make it pass against the wrong shape.
    let expected = 140.0 + 40.0_f64.hypot(60.0) + 100.0 + 60.0;
    assert!(
        (after - expected).abs() < 1e-9,
        "the new total must be the new geometry's, got {after} want {expected}"
    );
    assert_eq!(
        s.dimension_model().display(id).unwrap().text,
        out.label,
        "the reported label must be the label the model now holds"
    );
}

/// One drag, one Ctrl+Z. A corner drag that took two undo entries would leave
/// an operator who pressed once holding a shape he never chose.
#[test]
fn a_vertex_move_is_exactly_one_undo_entry() {
    let (_orig, mut s) = session();
    let (_annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    let before = s
        .dimension_model()
        .dimension(id)
        .unwrap()
        .kind
        .measured_points();
    s.move_dimension_vertex(id, 1, 40.0, 25.0).unwrap();
    s.undo().expect("the move must be undoable");
    let after = s
        .dimension_model()
        .dimension(id)
        .unwrap()
        .kind
        .measured_points();
    assert!(
        (after - before).abs() < 1e-9,
        "ONE undo must restore the whole edit, got {after} from {before}"
    );
}

/// "let me right click and add segments" — inserting after the LAST index is
/// the right-click on the closing segment, and it must land there rather than
/// being refused as out of range.
#[test]
fn inserting_after_the_last_vertex_adds_a_point_on_the_closing_segment() {
    let (_orig, mut s) = session();
    let (_annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    // The midpoint of the closing segment (50,110) -> (50,50).
    let out = s
        .insert_dimension_vertex(id, 3, Point::new(50.0, 80.0))
        .unwrap();
    assert_eq!(out.vertices, 5);
    // A point ON the segment adds no length: the sum is unchanged, which is
    // the strongest available check that it landed on the closing segment and
    // not somewhere else.
    let total = s
        .dimension_model()
        .dimension(id)
        .unwrap()
        .kind
        .measured_points();
    assert!(
        (total - 320.0).abs() < 1e-9,
        "a vertex inserted ON a segment must not change the total, got {total}"
    );
}

/// The degenerate-shape refusals, and their asymmetry: an open path keeps two
/// vertices, a closed one three. Two "closed" vertices would draw as a single
/// stroke and print twice the distance between two points.
#[test]
fn removing_a_vertex_refuses_below_the_minimum_for_the_shape() {
    let (_orig, mut s) = session();
    let (_annot, closed_id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    // 4 -> 3 is fine; 3 -> 2 is not, for a closed shape.
    s.remove_dimension_vertex(closed_id, 0).unwrap();
    let err = s.remove_dimension_vertex(closed_id, 0).unwrap_err();
    assert!(
        matches!(
            err,
            EditError::PerimeterWouldBeDegenerate {
                remaining: 2,
                minimum: 3,
                ..
            }
        ),
        "a closed perimeter needs three vertices, got {err:?}"
    );

    let (_annot, open_id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(false))
        .unwrap();
    s.remove_dimension_vertex(open_id, 0).unwrap();
    s.remove_dimension_vertex(open_id, 0).unwrap();
    let err = s.remove_dimension_vertex(open_id, 0).unwrap_err();
    assert!(
        matches!(
            err,
            EditError::PerimeterWouldBeDegenerate {
                remaining: 1,
                minimum: 2,
                ..
            }
        ),
        "an open path needs two vertices, got {err:?}"
    );
}

/// ★ The preflight the shell asked for, and the property that makes it worth
/// having: it answers the same question the verb would, WITHOUT mutating.
/// A greyed menu item derived from a preview that could disagree with the
/// verb is worse than no preview.
#[test]
fn the_vertex_preview_predicts_the_verb_and_changes_nothing() {
    use pdfcer_core::edit::VertexEdit;
    let (_orig, mut s) = session();
    let (_annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    s.remove_dimension_vertex(id, 0).unwrap(); // now 3, at the minimum

    let predicted = s.vertex_edit_preview(id, VertexEdit::Remove { index: 0 });
    assert!(
        predicted.is_err(),
        "the preview must refuse what the verb would refuse"
    );
    // Nothing moved.
    let unchanged = s.dimension_model().dimension(id).unwrap().kind.clone();
    assert_eq!(
        unchanged.polyline().map(|(p, _)| p.len()),
        Some(3),
        "a preview must not mutate"
    );

    // And the successful case predicts the successful outcome exactly.
    let forecast = s
        .vertex_edit_preview(
            id,
            VertexEdit::Move {
                index: 0,
                dx: 5.0,
                dy: 5.0,
            },
        )
        .unwrap();
    let actual = s.move_dimension_vertex(id, 0, 5.0, 5.0).unwrap();
    assert_eq!(
        forecast, actual,
        "preview and verb share one body; they cannot disagree"
    );
}

/// A vertex edit aimed at a kind that has no vertices is refused BY THE
/// PROPERTY, not by the kind — the R186 lesson `NotALinearDimension` already
/// learned once.
#[test]
fn a_fit_derived_ce_dimension_has_no_vertices_to_edit() {
    let (_orig, mut s) = session();
    let (_annot, circ) = s
        .add_dimension(0, DEFAULT_GROUP_ID, circular(false))
        .unwrap();
    let err = s.move_dimension_vertex(circ, 0, 1.0, 1.0).unwrap_err();
    assert!(
        matches!(err, EditError::DimensionHasNoVertices { .. }),
        "a fitted circle has no picked points left to address, got {err:?}"
    );

    let (_annot, ang) = s.add_dimension(0, DEFAULT_GROUP_ID, angular()).unwrap();
    let err = s.move_dimension_vertex(ang, 0, 1.0, 1.0).unwrap_err();
    assert!(matches!(err, EditError::DimensionHasNoVertices { .. }));
}

/// ★ The addition that was NOT requested: a linear ce dimension's two picked
/// points have been un-editable since `Pass 12.M2`, so a mis-picked end meant
/// deleting and redrawing. Moving one re-measures; the axis constraint is a
/// decision the operator already made and a drag does not revoke it.
#[test]
fn a_linear_ce_dimension_can_have_an_endpoint_moved_and_keeps_its_constraint() {
    let (_orig, mut s) = session();
    let (_annot, id) = s.add_dimension(0, DEFAULT_GROUP_ID, linear()).unwrap();
    // 100,200 -> 300,200, Horizontal: 200 pt.
    let out = s.move_dimension_vertex(id, 1, 50.0, 90.0).unwrap();
    assert_eq!(out.vertices, 2, "a linear ce dimension is structurally two");
    assert!(!out.closed);
    match &s.dimension_model().dimension(id).unwrap().kind {
        DimensionKind::Linear { constraint, .. } => {
            assert_eq!(
                *constraint,
                AxisConstraint::Horizontal,
                "a drag must not revoke the constraint"
            );
        }
        other => panic!("expected Linear, got {other:?}"),
    }
    let measured = s
        .dimension_model()
        .dimension(id)
        .unwrap()
        .kind
        .measured_points();
    assert!(
        (measured - 250.0).abs() < 1e-9,
        "Horizontal measures the horizontal span only, got {measured}"
    );

    // The count is structural: gaining or losing one is refused by name.
    let err = s
        .insert_dimension_vertex(id, 0, Point::new(10.0, 10.0))
        .unwrap_err();
    assert!(
        matches!(err, EditError::DimensionVertexCountFixed { count: 2, .. }),
        "got {err:?}"
    );
    let err = s.remove_dimension_vertex(id, 0).unwrap_err();
    assert!(matches!(err, EditError::DimensionVertexCountFixed { .. }));
}

/// An index that names nothing reports the COUNT as well, because the caller
/// that got it wrong is usually a shell one edit out of date.
#[test]
fn a_vertex_index_out_of_range_reports_the_count() {
    let (_orig, mut s) = session();
    let (_annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    let err = s.move_dimension_vertex(id, 9, 1.0, 1.0).unwrap_err();
    assert!(
        matches!(
            err,
            EditError::VertexIndexOutOfRange {
                index: 9,
                count: 4,
                ..
            }
        ),
        "got {err:?}"
    );
}

/// A NaN reaching the writer produces an appearance stream no reader can draw,
/// and it is invisible until the file is opened somewhere else. Refused before
/// any mutation, by the same predicate the sidecar reader applies.
#[test]
fn a_non_finite_vertex_is_refused_before_anything_moves() {
    let (_orig, mut s) = session();
    let (_annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    let err = s.move_dimension_vertex(id, 0, f64::NAN, 0.0).unwrap_err();
    assert!(
        matches!(err, EditError::VertexNotPlaceable { .. }),
        "got {err:?}"
    );
    let total = s
        .dimension_model()
        .dimension(id)
        .unwrap()
        .kind
        .measured_points();
    assert!(
        (total - 320.0).abs() < 1e-9,
        "a refusal must not have moved anything, got {total}"
    );
}

/// Placement is value-preserving on a perimeter for the same structural reason
/// it is on a linear one: it writes fields `measured_points` does not read.
#[test]
fn placing_a_perimeter_moves_its_label_and_not_its_value() {
    let (_orig, mut s) = session();
    let (_annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    let before = s.dimension_model().display(id).unwrap().text;
    s.place_dimension(id, 30.0, -12.0)
        .expect("a perimeter is placeable");
    assert_eq!(
        s.dimension_model().display(id).unwrap().text,
        before,
        "placing must never change what a ce dimension reports"
    );
    // The anchor moved by exactly the pair, from the vertex centroid.
    let kind = s.dimension_model().dimension(id).unwrap().kind.clone();
    let c = kind.polyline_centroid().unwrap();
    let anchor = kind.label_anchor().unwrap();
    assert!(
        (anchor.x - (c.x - 12.0)).abs() < 1e-9,
        "text_along is page +x"
    );
    assert!((anchor.y - (c.y + 30.0)).abs() < 1e-9, "offset is page +y");
}

/// ★ The label follows the shape rather than teleporting. This is the property
/// that decided the centroid convention over the CAD-conventional
/// longest-segment one: under longest-segment, dragging a corner can change
/// WHICH segment is longest and the label jumps across the shape for a reason
/// no operator can see.
#[test]
fn the_label_anchor_drifts_with_an_edited_shape_rather_than_jumping() {
    let (_orig, mut s) = session();
    let (_annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    let before = s
        .dimension_model()
        .dimension(id)
        .unwrap()
        .kind
        .label_anchor()
        .unwrap();
    // Make the SHORT side the long one: 60 -> 400. Under a longest-segment
    // convention the label's host axis would change identity here.
    s.move_dimension_vertex(id, 2, 0.0, 340.0).unwrap();
    let after = s
        .dimension_model()
        .dimension(id)
        .unwrap()
        .kind
        .label_anchor()
        .unwrap();
    // One of four vertices moved 340 pt, so the centroid moved 85 pt: a
    // drift proportional to the edit, not a jump to a different segment.
    assert!(
        (after.y - before.y - 85.0).abs() < 1e-9,
        "the anchor must track the centroid, got {before:?} -> {after:?}"
    );
}

/// Translating the whole ce dimension preserves every distance, exactly as it
/// does for the fixed-arity kinds — a rigid motion is a rigid motion.
#[test]
fn moving_a_whole_perimeter_preserves_its_value() {
    let (_orig, mut s) = session();
    let (_annot, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    let before = s.dimension_model().display(id).unwrap().text;
    s.move_dimension(id, 25.0, -10.0).unwrap();
    assert_eq!(
        s.dimension_model().display(id).unwrap().text,
        before,
        "a translation preserves every distance"
    );
}

/// ★ The annotation half, against the standard rather than against a habit.
///
/// ISO 32000-1 §12.5.6.9 Table 178: a closed shape is a `/Polygon`, an open one
/// a `/PolyLine`, the geometry key is a FLAT `/Vertices` array of alternating
/// x and y in default user space, and `/L` — the `/Line` annotation's geometry
/// key — must not be there at all. A dictionary carrying both would give a
/// reader that honours `/L` and one that honours `/Vertices` two different
/// pictures of the same annotation.
#[test]
fn a_perimeter_authors_a_polygon_and_an_open_path_a_polyline() {
    for (closed, subtype, intent) in [
        (true, &b"Polygon"[..], &b"PolygonDimension"[..]),
        (false, &b"PolyLine"[..], &b"PolyLineDimension"[..]),
    ] {
        let (_orig, mut s) = session();
        let (annot_id, _id) = s
            .add_dimension(0, DEFAULT_GROUP_ID, perimeter(closed))
            .unwrap();
        let reloaded = Document::from_bytes(save(&s)).unwrap();
        let Object::Dict(annot) = &reloaded.get(annot_id).unwrap().value else {
            panic!("annotation is not a dict");
        };
        assert_eq!(
            annot.get(b"Subtype").unwrap().as_name().unwrap().as_bytes(),
            subtype,
            "closed={closed}"
        );
        assert_eq!(
            annot.get(b"IT").unwrap().as_name().unwrap().as_bytes(),
            intent,
            "closed={closed}"
        );
        assert!(
            annot.get(b"L").is_none(),
            "a polygon/polyline must not carry the /Line geometry key"
        );
        let verts = annot.get(b"Vertices").unwrap().as_array().unwrap();
        // ★ FOUR vertices, not five. The closing segment of a `/Polygon` is
        // supplied by the READER; repeating the first vertex is undefined, and
        // the spec corpus names failing-to-close and over-closing as the two
        // real hazards. pdfcer closes the ring in its own measurement
        // (`polyline_length`) and leaves this array as the picked vertices.
        assert_eq!(
            verts.len(),
            8,
            "flat [x y x y ...] over 4 vertices, closed={closed}"
        );
        assert!((verts[0].as_number().unwrap() - 50.0).abs() < 1e-9);
        assert!((verts[1].as_number().unwrap() - 50.0).abs() < 1e-9);
        assert!((verts[2].as_number().unwrap() - 150.0).abs() < 1e-9);
    }
}

/// ★ `/Rect` must equal the `/AP` `/BBox`, and this is the single
/// highest-risk authoring bug for a ce dimension — it is invisible in an
/// object dump.
///
/// §12.5.5's placement algorithm SCALES the transformed appearance box to fill
/// `/Rect` exactly. A `/Rect` that does not match the drawn extent therefore
/// stretches the picture silently, and a perimeter would render at a length
/// that disagrees with the number printed inside it. Nothing clips; everything
/// scales.
#[test]
fn a_perimeter_rect_equals_its_appearance_bbox() {
    let (_orig, mut s) = session();
    let (annot_id, _id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let Object::Dict(annot) = &reloaded.get(annot_id).unwrap().value else {
        panic!("annotation is not a dict");
    };
    let rect: Vec<f64> = annot
        .get(b"Rect")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_number().unwrap())
        .collect();
    let ap_ref = annot
        .get(b"AP")
        .unwrap()
        .as_dict()
        .unwrap()
        .get(b"N")
        .unwrap()
        .as_reference()
        .unwrap();
    let Object::Stream(ap) = &reloaded.get(ap_ref).unwrap().value else {
        panic!("/AP /N is not a stream");
    };
    let bbox: Vec<f64> = ap
        .dict
        .get(b"BBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o.as_number().unwrap())
        .collect();
    assert_eq!(rect, bbox, "/Rect must equal the /AP /BBox exactly");
    // And it must actually contain the shape (50..150 x 50..110), which is
    // what makes the equality meaningful rather than two matching mistakes.
    assert!(rect[0] <= 50.0 && rect[1] <= 50.0);
    assert!(rect[2] >= 150.0 && rect[3] >= 110.0);
}

/// A vertex edit regenerates the annotation from the new geometry — and leaves
/// every key authoring does not own alone. `/P`, `/OC` and `/F` are the ones
/// that would break the page wiring, the layer and printability if a
/// regeneration rebuilt the dictionary from scratch instead of overwriting.
#[test]
fn a_vertex_edit_rewrites_the_vertices_and_preserves_foreign_keys() {
    let (_orig, mut s) = session();
    let (annot_id, id) = s
        .add_dimension(0, DEFAULT_GROUP_ID, perimeter(true))
        .unwrap();
    s.move_dimension_vertex(id, 1, 40.0, 0.0).unwrap();
    let reloaded = Document::from_bytes(save(&s)).unwrap();
    let Object::Dict(annot) = &reloaded.get(annot_id).unwrap().value else {
        panic!("annotation is not a dict");
    };
    let verts = annot.get(b"Vertices").unwrap().as_array().unwrap();
    assert!(
        (verts[2].as_number().unwrap() - 190.0).abs() < 1e-9,
        "the moved vertex must be in the file, got {:?}",
        verts[2]
    );
    for key in [&b"P"[..], &b"OC"[..], &b"F"[..], &b"AP"[..]] {
        assert!(
            annot.get(key).is_some(),
            "regeneration must not drop /{}",
            String::from_utf8_lossy(key)
        );
    }
    // `/Contents` is the label mirror and must have kept up with the geometry.
    let Some(Object::String(bytes)) = annot.get(b"Contents") else {
        panic!("/Contents");
    };
    assert_eq!(
        String::from_utf8_lossy(bytes),
        s.dimension_model().display(id).unwrap().text,
        "the baked caption and the model's value are one string"
    );
}

/// ★ WIDENING THE WORLD PAST A REFUSAL IS THE SAME ACT AS REMOVING IT.
///
/// `set_markup_style` refuses a ce dimension by name, because regenerating one
/// as plain markup drops its measured label and its witness lines silently.
/// That guard was written when every ce dimension was a `/Line` with
/// `/IT /LineDimension`, and it tested exactly that string.
///
/// `Pass 107.0` authors a perimeter as a `/Polygon` — stroked, coloured,
/// byte-shaped exactly like a markup polygon pdfcer can author — so the
/// un-widened guard would have let it through and reduced a measurement to a
/// bare outline. Nothing would have reported that; the file would simply have
/// stopped saying how long the fence was.
#[test]
fn restyling_a_perimeter_as_plain_markup_is_refused_by_name() {
    for closed in [true, false] {
        let (_orig, mut s) = session();
        let (annot_id, _id) = s
            .add_dimension(0, DEFAULT_GROUP_ID, perimeter(closed))
            .unwrap();
        let err = s
            .set_markup_style(
                annot_id,
                &pdfcer_core::edit::MarkupStyle {
                    stroke: Some(pdfcer_core::edit::StyleEdit::Set(
                        pdfcer_core::annot_author::Color::Rgb(1.0, 0.0, 0.0),
                    )),
                    ..pdfcer_core::edit::MarkupStyle::default()
                },
            )
            .unwrap_err();
        assert!(
            matches!(err, EditError::AnnotationIsCeDimension { .. }),
            "closed={closed}: a perimeter ce dimension must be signposted to set_dimension_style, got {err:?}"
        );
    }
}
