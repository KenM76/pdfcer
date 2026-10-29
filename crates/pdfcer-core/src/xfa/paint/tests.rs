#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)] // a panic here is the test failing

use super::*;
use crate::xfa::{LayoutOptions, XdpDocument, layout_template, read_template};

const PAGE: &str = r#"<pageSet><pageArea name="P"><contentArea x="36pt" y="36pt" w="540pt" h="720pt"/></pageArea></pageSet>"#;

/// Lay `body` under a positioned root and paint page 0; also return the
/// rect of the item named `f1` in PDF space (x, bottom, w, h).
fn paint(body: &str, opts: &PaintOptions) -> (String, Vec<PaintNote>, [f64; 4]) {
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="f">{PAGE}{body}</subform></template></xdp:xdp>"#
    );
    let t = read_template(&XdpDocument::parse(xdp).unwrap())
        .unwrap()
        .unwrap();
    let l = layout_template(&t, &LayoutOptions::new()).unwrap();
    let page = &l.pages[0];
    let r = page
        .items
        .iter()
        .find(|i| i.name.as_deref() == Some("f1"))
        .map_or(LayoutRect::default(), |i| i.rect);
    let out = paint_page(&t, page, opts);
    let text = String::from_utf8(out.content).unwrap();
    let ops = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (ops, out.notes, [r.x, page.height - r.y - r.h, r.w, r.h])
}

fn field(border: &str) -> String {
    format!(
        r#"<field name="f1" x="10pt" y="20pt" w="100pt" h="20pt"><border>{border}</border></field>"#
    )
}

fn num(v: f64) -> String {
    let mut out = Vec::new();
    crate::writer::content::emit_number(&mut out, v);
    String::from_utf8(out).unwrap()
}

fn re(x: f64, y: f64, w: f64, h: f64) -> String {
    format!("{} {} {} {} re", num(x), num(y), num(w), num(h))
}

#[test]
fn one_edge_strokes_all_four_sides_centred_on_the_extent() {
    let (ops, notes, [x, y, w, h]) = paint(&field("<edge/>"), &PaintOptions::new());
    assert_eq!((x, y, w, h), (46.0, 716.0, 100.0, 20.0));
    assert!(
        ops.contains(&format!("0.5 w 2 J 0 0 0 RG {} S", re(x, y, w, h))),
        "{ops}"
    );
    assert!(!ops.contains(" f"), "no fill element, no fill: {ops}");
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn edge_placement_inward_keeps_the_stroke_inside_the_extent() {
    let border = r#"<edge thickness="2pt"/>"#;
    let inward = PaintOptions::new().with_edge_placement(EdgePlacement::Inward);
    let (ops, _, [x, y, w, h]) = paint(&field(border), &inward);
    assert!(
        ops.contains(&re(x + 1.0, y + 1.0, w - 2.0, h - 2.0)),
        "{ops}"
    );
    let (ops, _, _) = paint(&field(border), &PaintOptions::new());
    assert!(ops.contains(&re(x, y, w, h)), "grammar centres it: {ops}");
    let left = field(border).replacen("<border>", r#"<border hand="left">"#, 1);
    let (ops, _, _) = paint(&left, &inward);
    assert!(
        ops.contains(&re(x - 1.0, y - 1.0, w + 2.0, h + 2.0)),
        "explicit hand wins: {ops}"
    );
}

#[test]
fn a_missing_fill_paints_white_only_when_asked() {
    let white = PaintOptions::new().with_missing_fill(MissingFill::White);
    let (ops, _, [x, y, w, h]) = paint(&field("<edge/>"), &white);
    assert!(
        ops.starts_with(&format!("1 1 1 rg {} f", re(x, y, w, h))),
        "fill under edges: {ops}"
    );
    let (ops, _, _) = paint(&field(""), &PaintOptions::new());
    assert!(ops.is_empty(), "no edge, no fill: nothing: {ops}");
}

#[test]
fn a_solid_fill_uses_its_colour_inside_the_border_margin() {
    let border = r#"<fill><color value="255,0,0"/></fill><margin leftInset="2pt" topInset="4pt"/>"#;
    let (ops, notes, [x, y, w, h]) = paint(&field(border), &PaintOptions::new());
    assert_eq!(
        ops,
        format!("1 0 0 rg {} f", re(x + 2.0, y, w - 2.0, h - 4.0))
    );
    assert!(notes.is_empty());
    let (ops, _, _) = paint(&field("<fill/>"), &PaintOptions::new());
    assert!(
        ops.starts_with("1 1 1 rg"),
        "a fill with no colour is white: {ops}"
    );
}

#[test]
fn distinct_edges_stroke_each_side_with_its_own_style() {
    let border = r#"<edge thickness="1pt"><color value="0,0,255"/></edge><edge thickness="3pt"/>"#;
    let (ops, _, [x, y, w, h]) = paint(&field(border), &PaintOptions::new());
    let (x1, top) = (x + w, y + h);
    let line = |t: f64, rgb: &str, a: (f64, f64), b: (f64, f64)| {
        format!(
            "{} w 2 J {rgb} RG {} {} m {} {} l S",
            num(t),
            num(a.0),
            num(a.1),
            num(b.0),
            num(b.1)
        )
    };
    let expect = [
        line(1.0, "0 0 1", (x, top), (x1, top)),
        line(3.0, "0 0 0", (x1, top), (x1, y)),
        line(3.0, "0 0 0", (x1, y), (x, y)),
        line(3.0, "0 0 0", (x, y), (x, top)),
    ]
    .join(" ");
    assert_eq!(ops, expect);
}

#[test]
fn an_invisible_ancestor_or_border_draws_nothing() {
    let inner = field("<edge/>");
    for body in [
        format!(r#"<subform name="s" x="0pt" y="0pt" presence="invisible">{inner}</subform>"#),
        field(r#"<edge/>"#).replace("<border>", r#"<border presence="hidden">"#),
        field(r#"<edge presence="invisible"/>"#),
    ] {
        let (ops, _, _) = paint(&body, &PaintOptions::new());
        assert!(ops.is_empty(), "{body}: {ops}");
    }
}

#[test]
fn what_is_not_drawn_yet_is_disclosed() {
    let border = r#"<edge stroke="dashed"/><corner radius="2pt"/><fill><pattern/></fill>"#;
    let body = field(border)
        .replace("<border>", r#"<border break="open">"#)
        .replace("</field>", "<caption/><color/></field>");
    let (ops, notes, _) = paint(&body, &PaintOptions::new());
    assert!(ops.is_empty(), "{ops}");
    let kinds: Vec<_> = notes.iter().map(|n| n.kind).collect();
    use PaintNoteKind as K;
    assert_eq!(
        kinds,
        [K::OpenBreak, K::FillType, K::Corner, K::Stroke, K::Content]
    );
}

#[test]
fn an_unreadable_colour_falls_back_and_is_disclosed() {
    let border = r#"<edge><color value="300,0,0"/></edge>"#;
    let (ops, notes, _) = paint(&field(border), &PaintOptions::new());
    assert!(ops.contains("0 0 0 RG"), "{ops}");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].kind, PaintNoteKind::Color);
    assert_eq!(parse_rgb(" 0, 128 ,255"), Some([0.0, 128.0 / 255.0, 1.0]));
    assert_eq!(parse_rgb("1,2"), None);
    assert_eq!(parse_rgb("1,2,3,4"), None);
}
