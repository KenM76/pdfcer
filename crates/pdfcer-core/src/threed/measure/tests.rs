use super::*;
use crate::object::ObjId;
use pdfcer_vec3::length;

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

fn close(a: [f64; 3], b: [f64; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-12)
}

#[test]
fn a_perpendicular_measures_along_the_normal_in_a_plane_holding_it() {
    let normal = [0.0, 0.0, 2.0];
    let camera = [0.0, -1.0, 1.0];
    let m = ThreeDMeasurement::perpendicular([0.0; 3], [5.0, 1.0, 3.0], normal, camera, "mm")
        .expect("a measurement");
    assert!(check_measurement(&m).is_ok());
    assert_eq!(m.kind, ThreeDMeasureKind::Perpendicular);
    assert_eq!(m.value, Some(3.0));
    assert_eq!(m.value_text().as_deref(), Some("3.000 mm"));
    let (facing, leader) = (m.plane.unwrap(), m.d1.unwrap());
    assert!(
        dot(facing, normal).abs() < 1e-12,
        "the plane holds the normal"
    );
    assert!(
        dot(leader, normal).abs() < 1e-12,
        "the leaders cross the line"
    );
    assert!(
        dot(leader, facing).abs() < 1e-12,
        "the leaders are in the plane"
    );
    assert!(
        dot(facing, camera) > 0.0,
        "the plane is the one nearest the ask"
    );
    assert_eq!(m.text_anchor, Some([2.5, 0.5, 1.5]));
    // Going the other way reads the same length.
    let back = ThreeDMeasurement::perpendicular([5.0, 1.0, 3.0], [0.0; 3], normal, camera, "mm");
    assert_eq!(back.unwrap().value, Some(3.0));
}

#[test]
fn a_perpendicular_with_nothing_to_draw_is_refused() {
    let (z, y) = ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]);
    let build = ThreeDMeasurement::perpendicular;
    assert!(
        build([0.0; 3], [1.0, 0.0, 0.0], z, y, "mm").is_none(),
        "no separation"
    );
    assert!(build([0.0; 3], [0.0; 3], z, y, "mm").is_none());
    assert!(
        build([0.0; 3], z, z, z, "mm").is_none(),
        "the plane is the normal"
    );
    assert!(build([0.0; 3], z, [0.0; 3], y, "mm").is_none());
    assert!(build([0.0; 3], z, z, [0.0; 3], "mm").is_none());
    assert!(build([f64::NAN; 3], z, z, y, "mm").is_none());
    assert!(build([f64::MAX; 3], [-f64::MAX; 3], z, y, "mm").is_none());
}

#[test]
fn an_angle_is_at_its_vertex_in_the_plane_of_its_points() {
    let vertex = [1.0, 1.0, 1.0];
    let up = [0.0, 0.0, 1.0];
    let m = ThreeDMeasurement::angular(vertex, [3.0, 1.0, 1.0], [1.0, 4.0, 1.0], up)
        .expect("a measurement");
    assert!(check_measurement(&m).is_ok());
    assert_eq!(m.kind, ThreeDMeasureKind::Angular);
    assert_eq!(m.plane, Some(up));
    assert_eq!((m.d1, m.d2), (Some([1.0, 0.0, 0.0]), Some([0.0, 1.0, 0.0])));
    assert!((m.value.unwrap() - 90.0).abs() < 1e-12);
    assert_eq!(m.value_text().as_deref(), Some("90.000°"));
    // Inside the angle, at the nearer anchor's distance.
    let tp = sub(m.text_anchor.unwrap(), vertex);
    assert!((length(tp) - 2.0).abs() < 1e-12, "{tp:?}");
    assert!(tp[0] > 0.0 && tp[1] > 0.0, "{tp:?}");

    // `plane` chooses the side, not the plane.
    let tilted = [0.3, -0.4, -2.0];
    let flipped = ThreeDMeasurement::angular(vertex, [3.0, 1.0, 1.0], [1.0, 4.0, 1.0], tilted)
        .expect("a measurement");
    assert_eq!(flipped.plane, Some([0.0, 0.0, -1.0]));

    let wide = ThreeDMeasurement::angular([0.0; 3], [1.0, 0.0, 0.0], [-1.0, 1.0, 0.0], up);
    assert!((wide.unwrap().value.unwrap() - 135.0).abs() < 1e-12);
}

#[test]
fn an_angle_with_no_two_lines_is_refused() {
    let (x, y, z) = ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
    let at = [0.0; 3];
    let build = ThreeDMeasurement::angular;
    assert!(build(at, at, y, z).is_none(), "an anchor on the vertex");
    assert!(build(at, x, at, z).is_none());
    assert!(build(at, x, [2.0, 0.0, 0.0], z).is_none(), "the same line");
    assert!(build(at, x, [-2.0, 0.0, 0.0], z).is_none(), "opposed lines");
    assert!(build(at, x, y, at).is_none(), "a zero plane");
    assert!(build(at, x, y, [f64::NAN; 3]).is_none());
    assert!(build([f64::INFINITY; 3], x, y, z).is_none());
}

#[test]
fn a_radius_is_the_circle_through_its_three_points() {
    let m = ThreeDMeasurement::radial([4.0, 1.0, 5.0], [1.0, 4.0, 5.0], [-2.0, 1.0, 5.0], "mm")
        .expect("a measurement");
    assert!(check_measurement(&m).is_ok());
    assert_eq!(m.kind, ThreeDMeasureKind::Radial);
    assert!(close(m.a1.unwrap(), [1.0, 1.0, 5.0]), "{:?}", m.a1);
    assert_eq!(m.a2, Some([1.0, 4.0, 5.0]));
    assert_eq!(
        (m.a3, m.a4),
        (Some([4.0, 1.0, 5.0]), Some([-2.0, 1.0, 5.0]))
    );
    assert!((m.value.unwrap() - 3.0).abs() < 1e-12);
    assert!(close(m.plane.unwrap(), [0.0, 0.0, 1.0]), "{:?}", m.plane);
    assert!(
        close(m.text_anchor.unwrap(), [1.0, 5.5, 5.0]),
        "{:?}",
        m.text_anchor
    );
    assert_eq!(m.value_text().as_deref(), Some("R3.000 mm"));
}

#[test]
fn a_radius_is_right_off_the_axes() {
    let centre = [1.0, -2.0, 0.5];
    let (u, v) = ([3.0 / 5.0, 4.0 / 5.0, 0.0], [0.0, 0.0, 1.0]);
    let on = |t: f64| -> [f64; 3] {
        std::array::from_fn(|i| centre[i] + 7.0 * (t.cos() * u[i] + t.sin() * v[i]))
    };
    let m = ThreeDMeasurement::radial(on(0.1), on(0.2), on(1.9), "in").expect("a measurement");
    assert!((m.value.unwrap() - 7.0).abs() < 1e-9, "{:?}", m.value);
    assert!(length(sub(m.a1.unwrap(), centre)) < 1e-9, "{:?}", m.a1);
}

#[test]
fn three_points_with_no_circle_are_refused() {
    let build = ThreeDMeasurement::radial;
    let (o, x) = ([0.0; 3], [1.0, 0.0, 0.0]);
    assert!(build(o, x, [2.0, 0.0, 0.0], "mm").is_none(), "collinear");
    assert!(build(o, o, x, "mm").is_none());
    assert!(build(x, o, x, "mm").is_none());
    assert!(build([f64::NAN; 3], x, [0.0, 1.0, 0.0], "mm").is_none());
    assert!(build(o, [f64::MAX, 0.0, 0.0], [0.0, f64::MAX, 0.0], "mm").is_none());
}

#[test]
fn a_note_is_its_text_at_its_point() {
    let at = [1.0, 2.0, 3.0];
    let m = ThreeDMeasurement::comment(at, "a: b").expect("a note");
    assert!(check_measurement(&m).is_ok());
    assert_eq!(m.kind, ThreeDMeasureKind::Comment);
    assert_eq!((m.a1, m.text_anchor), (Some(at), Some(at)));
    assert_eq!(m.value_text().as_deref(), Some("a: b"));
    assert!(ThreeDMeasurement::comment(at, "").is_none());
    assert!(ThreeDMeasurement::comment([f64::NAN; 3], "a").is_none());
}

#[test]
fn value_text_marks_a_radius_and_an_angle() {
    let mut radius = full(ThreeDMeasureKind::Radial);
    radius.user_text = None;
    radius.radius = None;
    assert_eq!(radius.value_text().as_deref(), Some("R5.00 mm"));
    radius.radius = Some(false);
    assert_eq!(radius.value_text().as_deref(), Some("⌀5.00 mm"));
    let mut angle = full(ThreeDMeasureKind::Angular);
    angle.user_text = None;
    assert_eq!(
        angle.value_text().as_deref(),
        Some("5.00 mm"),
        "its own units win"
    );
    angle.units = None;
    angle.degrees = None;
    assert_eq!(angle.value_text().as_deref(), Some("5.00°"));
    angle.degrees = Some(false);
    assert_eq!(angle.value_text().as_deref(), Some("5.00 rad"));
}
