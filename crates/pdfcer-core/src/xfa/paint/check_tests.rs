#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)] // a panic here is the test failing

use super::*;
use crate::xfa::{LayoutOptions, XdpDocument, layout_template, read_template};

const PAGE: &str = r#"<pageSet><pageArea name="P"><contentArea x="36pt" y="36pt" w="540pt" h="720pt"/></pageArea></pageSet>"#;

/// `body` laid out in a 36pt content area and painted: the page content as
/// one space-separated line, and its notes.
pub(super) fn painted(body: &str, opts: &PaintOptions) -> (String, Vec<PaintNote>) {
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="f">{PAGE}{body}</subform></template></xdp:xdp>"#
    );
    let t = read_template(&XdpDocument::parse(xdp).unwrap())
        .unwrap()
        .unwrap();
    let l = layout_template(&t, &LayoutOptions::new()).unwrap();
    let out = paint_page(&t, &l.pages[0], opts);
    let text = String::from_utf8(out.content).unwrap();
    (
        text.split_whitespace().collect::<Vec<_>>().join(" "),
        out.notes,
    )
}

/// One X020-shaped radio: a round box with a `stroke` edge, `minW` and no
/// `w`, and a centred caption with a negative reserve.
fn radio(name: &str, x: &str, stroke: &str, on: &str) -> String {
    format!(
        r#"<field name="{name}" x="{x}" y="0pt" minW="20mm" h="6mm"><ui><checkButton shape="round"><border><edge stroke="{stroke}"/><fill/></border></checkButton></ui><para hAlign="center"/><caption reserve="-5.08mm"><value><text>{name}</text></value></caption><items><text>{on}</text></items></field>"#
    )
}

fn row(stroke: &str) -> String {
    format!(
        r#"<exclGroup name="g" x="10pt" y="20pt"><bind match="none"/>{}{}</exclGroup>"#,
        radio("Yes", "0pt", stroke, "1"),
        radio("No", "30mm", stroke, "2"),
    )
}

#[test]
fn every_exclgroup_member_paints_its_own_round_box() {
    let (ops, notes) = painted(&row("lowered"), &PaintOptions::new());
    assert_eq!(ops.matches("(Yes) Tj").count(), 1, "{ops}");
    assert_eq!(ops.matches("(No) Tj").count(), 1, "{ops}");
    assert!(
        !notes.iter().any(|n| n.kind == PaintNoteKind::Stroke),
        "{notes:?}"
    );
    assert!(ops.matches(" c ").count() >= 8, "two circles: {ops}");
}

#[test]
fn a_round_three_d_box_draws_two_rings_of_half_arcs_and_no_note() {
    let (ops, notes) = painted(&row("raised"), &PaintOptions::new());
    assert!(notes.is_empty(), "{notes:?}");
    // Two buttons x two rings x two half-arcs, each two curves.
    assert_eq!(ops.matches(" c ").count(), 16, "{ops}");
    assert!(
        ops.contains(&format!("{} RG", grey(128))),
        "grey upper: {ops}"
    );
    assert!(ops.contains("1 1 1 RG"), "white lower half: {ops}");
}

#[test]
fn the_plain_setting_strokes_the_circle_and_says_so() {
    let opts = PaintOptions::new().with_three_d_stroke(ThreeDStroke::Plain);
    let (ops, notes) = painted(&row("lowered"), &opts);
    assert_eq!(ops.matches(" h S").count(), 2, "{ops}");
    assert_eq!(
        notes
            .iter()
            .filter(|n| n.kind == PaintNoteKind::StrokeApproximated)
            .count(),
        2,
        "{notes:?}"
    );
}

#[test]
fn an_exclgroup_member_whose_value_is_its_on_value_is_marked() {
    let on = row("lowered").replacen(
        "<items><text>1</text></items>",
        "<items><text>1</text></items><value><text>1</text></value>",
        1,
    );
    let (ops, _) = painted(&on, &PaintOptions::new());
    assert_eq!(ops.matches(" h f").count(), 1, "one filled dot: {ops}");
}

fn grey(v: u8) -> String {
    let mut n = Vec::new();
    crate::writer::content::emit_number(&mut n, f64::from(v) / 255.0);
    let n = String::from_utf8(n).unwrap();
    format!("{n} {n} {n}")
}

fn bordered(stroke: &str) -> String {
    format!(
        r#"<draw name="d" x="10pt" y="20pt" w="40pt" h="20pt"><border><edge stroke="{stroke}" thickness="2pt"/></border></draw>"#
    )
}

#[test]
fn a_lowered_rectangle_is_a_black_ring_and_a_grey_bevel() {
    let (ops, notes) = painted(&bordered("lowered"), &PaintOptions::new());
    assert!(notes.is_empty(), "{notes:?}");
    // Ring: 1pt wide, centred 0.5pt inside the 46,716 40x20 extent.
    assert!(ops.contains("1 w 0 0 0 RG 46.5 716.5 39 19 re S"), "{ops}");
    assert!(ops.contains(&format!("{} rg 47 735 m", grey(128))), "{ops}");
    assert!(ops.contains(&format!("{} rg 85 735 m", grey(192))), "{ops}");
    assert_eq!(ops.matches(" h f").count(), 2, "{ops}");
}

#[test]
fn an_etched_rectangle_is_two_bevels() {
    let (ops, _) = painted(&bordered("etched"), &PaintOptions::new());
    assert_eq!(ops.matches(" h f").count(), 4, "{ops}");
    assert!(!ops.contains(" re S"), "no ring: {ops}");
}

#[test]
fn a_dashed_border_strokes_all_four_edges_dashed() {
    let (ops, notes) = painted(&bordered("dashed"), &PaintOptions::new());
    assert!(notes.is_empty(), "{notes:?}");
    assert!(ops.contains("[8 4] 0 d 2 w 2 J"), "{ops}");
    assert!(ops.contains(" re S"), "{ops}");
}

#[test]
fn dotted_and_dashed_edges_differ_and_the_dash_is_reset() {
    let dotted = format!(
        "{}{}",
        bordered("dotted"),
        bordered("solid").replace(r#"name="d""#, r#"name="e""#)
    );
    let (ops, _) = painted(&dotted, &PaintOptions::new());
    assert!(ops.contains("[2 4] 0 d"), "{ops}");
    assert!(ops.contains("[] 0 d"), "solid after dotted resets: {ops}");
    let round = PaintOptions::new().with_dash_caps(super::DashCaps::Round);
    let (ops, _) = painted(&bordered("dotted"), &round);
    assert!(ops.contains("[0 6] 0 d 2 w 1 J"), "{ops}");
}

#[test]
fn an_unknown_stroke_is_drawn_solid_and_disclosed() {
    let (ops, notes) = painted(&bordered("wavy"), &PaintOptions::new());
    assert!(ops.contains(" re S") && !ops.contains(" d "), "{ops}");
    assert_eq!(notes[0].kind, PaintNoteKind::Stroke);
}

#[test]
fn a_negative_reserve_fits_the_caption_or_leaves_only_the_box() {
    let one = radio("Yes", "0pt", "solid", "1");
    let (fit, _) = painted(&one, &PaintOptions::new());
    let opts = PaintOptions::new().with_check_caption(CheckCaption::Remainder);
    let (rest, _) = painted(&one, &opts);
    let start = |ops: &str| {
        let i = ops.find(" h S").unwrap();
        let head = &ops[..i];
        let m = head.rfind(" m ").unwrap();
        let x: f64 = head[..m].rsplit(' ').nth(1).unwrap().parse().unwrap();
        x
    };
    // The field is 20mm wide at x=36: the remainder reading puts the box's
    // right edge (where its circle starts) at the field's right edge.
    let right = 36.0 + 20.0 * 72.0 / 25.4;
    assert!((start(&rest) - right).abs() < 0.01, "{rest}");
    assert!(start(&fit) < right - 1.0, "{fit}");
}
