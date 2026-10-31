use super::*;
use crate::object::ObjId;

struct NoGraph;
impl ObjectGraph for NoGraph {
    fn value(&self, _id: ObjId) -> Option<&Object> {
        None
    }
    fn trailer_entry(&self, _key: &[u8]) -> Option<&Object> {
        None
    }
}

fn round_trip(m: &ThreeDMeasurement) -> ThreeDMeasurement {
    read_one(&NoGraph, &measurement_dict(m)).expect("reads back")
}

fn full(kind: ThreeDMeasureKind) -> ThreeDMeasurement {
    let mut m = ThreeDMeasurement::new(kind);
    m.name = Some("Measurement 1".to_owned());
    m.plane = Some([0.0, 0.0, 1.0]);
    m.a1 = Some([1.0, 2.0, 3.0]);
    m.a2 = Some([4.0, 6.0, 3.0]);
    m.a3 = Some([0.5, 0.0, 0.0]);
    m.a4 = Some([0.0, 0.5, 0.0]);
    m.node1 = Some("Bolt".to_owned());
    m.node2 = Some("Plate".to_owned());
    m.d1 = Some([0.0, 1.0, 0.0]);
    m.d2 = Some([1.0, 0.0, 0.0]);
    m.text_anchor = Some([2.5, 4.0, 3.0]);
    m.text_x = Some([1.0, 0.0, 0.0]);
    m.text_up = Some([0.0, 1.0, 0.0]);
    m.text_size = Some(9.0);
    m.color = Some([1.0, 0.0, 0.0]);
    m.value = Some(5.0);
    m.units = Some("mm".to_owned());
    m.places = Some(2);
    m.user_text = Some(" typ.".to_owned());
    m.degrees = Some(false);
    m.extension = Some(40.0);
    m.show_circle = Some(true);
    m.radius = Some(false);
    m.text_box = Some([20, 3]);
    m
}

#[test]
fn every_kind_round_trips_every_key() {
    for kind in [
        ThreeDMeasureKind::Linear,
        ThreeDMeasureKind::Perpendicular,
        ThreeDMeasureKind::Angular,
        ThreeDMeasureKind::Radial,
        ThreeDMeasureKind::Comment,
    ] {
        let m = full(kind);
        assert_eq!(check_measurement(&m), Ok(()), "{kind:?}");
        assert_eq!(round_trip(&m), m, "{kind:?}");
    }
}

#[test]
fn written_dict_is_typed_and_has_no_comment_link() {
    let mut m = full(ThreeDMeasureKind::Linear);
    m.has_comment = true;
    let d = measurement_dict(&m);
    assert_eq!(
        d.get(b"Type"),
        Some(&Object::Name(Name::from(b"3DMeasure")))
    );
    assert_eq!(d.get(b"Subtype"), Some(&Object::Name(Name::from(b"LD3"))));
    assert!(!d.contains_key(b"S"));
    assert!(!round_trip(&m).has_comment);
}

#[test]
fn reading_notes_a_comment_link_and_skips_unknown_entries() {
    let mut d = measurement_dict(&full(ThreeDMeasureKind::Linear));
    d.insert(Name::from(b"S"), Object::Reference(ObjId::new(9, 0)));
    let mut unknown = Dict::new();
    unknown.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"XD3")));
    let mut view = Dict::new();
    view.insert(
        Name::from(b"MA"),
        Object::Array(vec![
            Object::Dict(unknown),
            Object::Integer(7),
            Object::Dict(d),
        ]),
    );
    let read = read_measurements(&NoGraph, &view);
    assert_eq!(read.len(), 1);
    assert!(read[0].has_comment);
}

#[test]
fn malformed_values_read_as_absent() {
    let mut d = measurement_dict(&ThreeDMeasurement::new(ThreeDMeasureKind::Linear));
    d.insert(
        Name::from(b"A1"),
        Object::Array(vec![Object::Integer(1), Object::Integer(2)]),
    );
    d.insert(Name::from(b"P"), Object::Integer(-1));
    d.insert(Name::from(b"DR"), Object::Integer(1));
    d.insert(
        Name::from(b"TB"),
        Object::Array(vec![Object::Integer(-1), Object::Integer(2)]),
    );
    let m = read_one(&NoGraph, &d).expect("a linear measurement");
    assert_eq!(
        (m.a1, m.places, m.degrees, m.text_box),
        (None, None, None, None)
    );
}

#[test]
fn check_names_the_missing_key_and_a_non_finite_number() {
    let mut m = full(ThreeDMeasureKind::Radial);
    m.a3 = None;
    assert_eq!(
        check_measurement(&m),
        Err("it has no /A3, which its kind requires".to_owned())
    );
    let mut m = full(ThreeDMeasureKind::Linear);
    m.a1 = Some([f64::NAN, 0.0, 0.0]);
    assert!(
        check_measurement(&m)
            .expect_err("refused")
            .contains("not finite")
    );
    let mut m = full(ThreeDMeasureKind::Linear);
    m.value = Some(f64::INFINITY);
    assert!(
        check_measurement(&m)
            .expect_err("refused")
            .contains("not finite")
    );
    let mut m = ThreeDMeasurement::new(ThreeDMeasureKind::Comment);
    m.a1 = Some([0.0; 3]);
    m.text_anchor = Some([0.0; 3]);
    assert_eq!(check_measurement(&m), Ok(()));
}

#[test]
fn linear_builds_a_writable_measurement() {
    let m = ThreeDMeasurement::linear([1.0, 1.0, 0.0], [1.0, 3.0, 0.0], [0.0, 0.0, 1.0], "in")
        .expect("a measurement");
    assert_eq!(m.value, Some(2.0));
    assert_eq!(m.text_anchor, Some([1.0, 2.0, 0.0]));
    assert_eq!(m.text_up, Some([-1.0, 0.0, 0.0]));
    assert_eq!(check_measurement(&m), Ok(()));
}

#[test]
fn linear_refuses_degenerate_input() {
    let z = [0.0, 0.0, 1.0];
    assert!(ThreeDMeasurement::linear([1.0; 3], [1.0; 3], z, "mm").is_none());
    assert!(ThreeDMeasurement::linear([0.0; 3], [0.0, 0.0, 4.0], z, "mm").is_none());
    assert!(ThreeDMeasurement::linear([0.0; 3], [f64::NAN, 1.0, 0.0], z, "mm").is_none());
}

#[test]
fn value_text_formats_places_units_and_suffix() {
    let mut m = full(ThreeDMeasureKind::Linear);
    assert_eq!(m.value_text().as_deref(), Some("5.00 mm typ."));
    m.places = None;
    m.units = Some(String::new());
    m.user_text = None;
    assert_eq!(m.value_text().as_deref(), Some("5.000"));
    m.value = None;
    assert_eq!(m.value_text(), None);
    let note = full(ThreeDMeasureKind::Comment);
    assert_eq!(note.value_text().as_deref(), Some(" typ."));
}
