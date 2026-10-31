#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use super::check_tests::painted;
use super::*;

/// A draw at 10pt,20pt in the 36pt content area: its top-left is 46,736.
fn draw(w: &str, h: &str, value: &str) -> String {
    format!(r#"<draw name="d" x="10pt" y="20pt" w="{w}" h="{h}"><value>{value}</value></draw>"#)
}

#[test]
fn a_rounded_rectangle_value_paints_like_a_border() {
    let v =
        r#"<rectangle><edge thickness="3pt"/><corner radius="0.2in" join="round"/></rectangle>"#;
    let (ops, notes) = painted(&draw("1.4in", "0.9in", v), &PaintOptions::new());
    assert!(notes.is_empty(), "{notes:?}");
    assert!(ops.contains(" c ") && ops.contains("3 w"), "{ops}");
    // `join` defaults to square (p.663): a cut corner, no curve.
    let cut = v.replace(r#" join="round""#, "");
    let (ops, _) = painted(&draw("1.4in", "0.9in", &cut), &PaintOptions::new());
    assert!(
        !ops.contains(" c ") && ops.contains("46 721.6 m 46 736 l 60.4 736 l S"),
        "{ops}"
    );
}

#[test]
fn lines_run_corner_to_corner_or_straight() {
    let opts = PaintOptions::new();
    let (ops, notes) = painted(&draw("1.4in", "0.9in", r#"<line slope="/"/>"#), &opts);
    assert!(notes.is_empty(), "{notes:?}");
    assert!(ops.contains("46 671.2 m 146.8 736 l S"), "{ops}");
    let (ops, _) = painted(&draw("1.4in", "0.9in", "<line/>"), &opts);
    assert!(ops.contains("46 736 m 146.8 671.2 l S"), "{ops}");
    let (ops, _) = painted(&draw("0in", "1.2in", "<line/>"), &opts);
    assert!(ops.contains("46 736 m 46 649.6 l S"), "vertical: {ops}");
    let (ops, _) = painted(&draw("1.6in", "0in", "<line/>"), &opts);
    assert!(ops.contains("46 736 m 161.2 736 l S"), "horizontal: {ops}");
}

#[test]
fn dashed_and_dotted_lines_differ() {
    let line = |s: &str| {
        draw(
            "2in",
            "0in",
            &format!(r#"<line><edge stroke="{s}" thickness="3pt"/></line>"#),
        )
    };
    let (dashed, _) = painted(&line("dashed"), &PaintOptions::new());
    let (dotted, _) = painted(&line("dotted"), &PaintOptions::new());
    assert!(dashed.contains("[12 6] 0 d 3 w 2 J"), "{dashed}");
    assert!(dotted.contains("[3 6] 0 d 3 w 2 J"), "{dotted}");
}

#[test]
fn a_line_cap_and_hand_are_read() {
    let v = r#"<line hand="left"><edge cap="round" thickness="2pt"/></line>"#;
    let (ops, _) = painted(&draw("1in", "0in", v), &PaintOptions::new());
    // Left of a rightward line is up: one half-thickness.
    assert!(
        ops.contains("1 J") && ops.contains("46 737 m 118 737 l S"),
        "{ops}"
    );
}

#[test]
fn a_whole_arc_is_four_curves_closed() {
    let (ops, notes) = painted(&draw("2in", "1in", "<arc/>"), &PaintOptions::new());
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(ops.matches(" c ").count(), 4, "{ops}");
    // Starts at 3 o'clock: the right edge, half way down.
    assert!(ops.contains("190 700 m") && ops.contains(" h S"), "{ops}");
}

#[test]
fn a_filled_quarter_arc_is_a_pie_or_a_chord() {
    let v = r#"<arc startAngle="0" sweepAngle="90"><fill/></arc>"#;
    let (pie, _) = painted(&draw("1in", "1in", v), &PaintOptions::new());
    assert!(
        pie.contains("82 700 m 118 700 l"),
        "pie from the centre: {pie}"
    );
    let chord = PaintOptions::new().with_arc_fill(ArcFill::Chord);
    let (ops, _) = painted(&draw("1in", "1in", v), &chord);
    assert!(!ops.contains("82 700 m"), "{ops}");
    assert_eq!(ops.matches(" c ").count(), 2, "one fill, one stroke: {ops}");
}

#[test]
fn a_circular_arc_takes_the_smaller_side() {
    let v = r#"<arc circular="1"/>"#;
    let (ops, _) = painted(&draw("2in", "1in", v), &PaintOptions::new());
    // Centre 118,700; radius 36 (half of 1in).
    assert!(ops.contains("154 700 m"), "{ops}");
    let radius = PaintOptions::new().with_arc_circle(ArcCircle::Radius);
    let (ops, _) = painted(&draw("2in", "1in", v), &radius);
    assert!(ops.contains("190 700 m"), "{ops}");
}

#[test]
fn unknown_values_fall_back_and_huge_ones_stay_bounded() {
    let v = r#"<line slope="?"><edge cap="sharp"/></line>"#;
    let (ops, notes) = painted(&draw("1in", "1in", v), &PaintOptions::new());
    assert!(ops.contains("46 736 m 118 664 l S"), "{ops}");
    assert!(
        notes.iter().all(|n| n.kind == PaintNoteKind::Shape),
        "{notes:?}"
    );
    let v = r#"<arc startAngle="1e300" sweepAngle="-9e99"/>"#;
    let (ops, notes) = painted(&draw("1in", "1in", v), &PaintOptions::new());
    assert!(notes.is_empty(), "{notes:?}");
    assert_eq!(ops.matches(" c ").count(), 4, "{ops}");
    let v = r#"<arc sweepAngle="NaN"/>"#;
    let (_, notes) = painted(&draw("1in", "1in", v), &PaintOptions::new());
    assert_eq!(notes[0].kind, PaintNoteKind::Shape);
}

#[test]
fn a_rotated_draw_paints_upright_then_turns_about_its_centre() {
    let v = r#"<draw name="d" x="10pt" y="60pt" w="40pt" h="10pt" rotate="90"><value><line/></value></draw>"#;
    let (ops, notes) = painted(v, &PaintOptions::new());
    assert!(notes.is_empty(), "{notes:?}");
    // Extent 46,56 10x40 (layout); centre 51,716 in PDF space.
    assert!(ops.contains("q 0 1 -1 0 767 665 cm"), "{ops}");
    // The upright line, top-left to bottom-right of the 40x10 box.
    assert!(ops.contains("31 721 m 71 711 l S Q"), "{ops}");
    let odd = v.replace(r#""90""#, r#""45""#);
    let (ops, notes) = painted(&odd, &PaintOptions::new());
    assert!(!ops.contains(" cm"), "{ops}");
    assert_eq!(notes[0].kind, PaintNoteKind::Rotate);
    let sub = r#"<subform name="s" w="40pt" h="10pt" rotate="90"/>"#;
    assert_eq!(
        painted(sub, &PaintOptions::new()).1[0].kind,
        PaintNoteKind::Rotate
    );
}
