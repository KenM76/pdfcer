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
        .replace(
            "</field>",
            r#"<caption placement="inline"/><color/></field>"#,
        );
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

/// Paint `body` and return the whole page plus the normalized operators.
fn painted(body: &str, opts: &PaintOptions) -> (PaintedPage, String) {
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="f">{PAGE}{body}</subform></template></xdp:xdp>"#
    );
    let t = read_template(&XdpDocument::parse(xdp).unwrap())
        .unwrap()
        .unwrap();
    let l = layout_template(&t, &LayoutOptions::new()).unwrap();
    let out = paint_page(&t, &l.pages[0], opts);
    let ops = String::from_utf8(out.content.clone()).unwrap();
    let ops = ops.split_whitespace().collect::<Vec<_>>().join(" ");
    (out, ops)
}

fn draw(font: &str, para: &str, text: &str) -> String {
    format!(
        r#"<draw name="f1" x="10pt" y="20pt" w="200pt" h="40pt">{font}{para}<value><text>{text}</text></value></draw>"#
    )
}

fn base_font(p: &PaintedPage, name: &[u8]) -> String {
    let fonts = p.resources.get(b"Font").unwrap().as_dict().unwrap();
    let f = fonts.get(name).unwrap().as_dict().unwrap();
    String::from_utf8(
        f.get(b"BaseFont")
            .unwrap()
            .as_name()
            .unwrap()
            .as_bytes()
            .to_vec(),
    )
    .unwrap()
}

#[test]
fn draw_text_is_set_at_its_first_baseline_in_a_named_standard_face() {
    let body = draw(r#"<font typeface="Helvetica" size="12pt"/>"#, "", "Hello");
    let (p, ops) = painted(&body, &PaintOptions::new());
    assert!(p.notes.is_empty(), "{:?}", p.notes);
    assert_eq!(base_font(&p, b"XF0"), "Helvetica");
    // Top of region = 792 - 36 - 20 = 736; baseline = top - ascender (718/1000 * 12).
    let baseline = 736.0 - 0.718 * 12.0;
    assert!(
        ops.contains(&format!(
            "BT /XF0 12 Tf 1 0 0 1 46 {} Tm (Hello) Tj ET",
            num(baseline)
        )),
        "{ops}"
    );
    assert!(
        ops.contains(&format!("{} W n", re(46.0, 696.0, 200.0, 40.0))),
        "{ops}"
    );
}

#[test]
fn the_default_face_is_courier_10_and_its_weight_is_a_setting() {
    let body = draw("", "", "x");
    let (p, ops) = painted(&body, &PaintOptions::new());
    assert_eq!(base_font(&p, b"XF0"), "Courier");
    assert!(ops.contains("/XF0 10 Tf"), "{ops}");
    let (bold, _) = painted(
        &body,
        &PaintOptions::new().with_font_weight(FontWeight::Bold),
    );
    assert_eq!(base_font(&bold, b"XF0"), "Courier-Bold");
    let explicit = draw(r#"<font weight="normal"/>"#, "", "x");
    let (normal, _) = painted(
        &explicit,
        &PaintOptions::new().with_font_weight(FontWeight::Bold),
    );
    assert_eq!(
        base_font(&normal, b"XF0"),
        "Courier",
        "an explicit weight wins"
    );
}

#[test]
fn a_substituted_typeface_is_disclosed() {
    let body = draw(r#"<font typeface="Myriad Pro" posture="italic"/>"#, "", "x");
    let (p, _) = painted(&body, &PaintOptions::new());
    assert_eq!(base_font(&p, b"XF0"), "Helvetica-Oblique");
    assert_eq!(p.notes.len(), 1);
    assert_eq!(p.notes[0].kind, PaintNoteKind::Font);
    let serif = draw(r#"<font typeface="Times New Roman"/>"#, "", "x");
    assert_eq!(
        base_font(&painted(&serif, &PaintOptions::new()).0, b"XF0"),
        "Times-Roman"
    );
}

#[test]
fn h_align_and_v_align_place_the_line_in_the_region() {
    let font = r#"<font typeface="Courier" size="10pt"/>"#;
    let para = r#"<para hAlign="right" vAlign="bottom"/>"#;
    let (_, ops) = painted(&draw(font, para, "ab"), &PaintOptions::new());
    // Courier is 600/em: "ab" is 12pt wide; region x 46..246, bottom 696.
    let line_h = (629.0 + 157.0) / 1000.0 * 10.0;
    let baseline = 696.0 + line_h - 6.29;
    assert!(
        ops.contains(&format!("1 0 0 1 234 {} Tm (ab) Tj", num(baseline))),
        "{ops}"
    );
}

#[test]
fn long_text_wraps_at_spaces_and_overflow_is_disclosed() {
    let font = r#"<font typeface="Courier" size="10pt"/>"#;
    // 200pt holds 33 Courier characters at 10pt.
    let words = "aaaaaaaaaa ".repeat(6);
    let (p, ops) = painted(&draw(font, "", words.trim_end()), &PaintOptions::new());
    assert_eq!(ops.matches(" Tj").count(), 2, "{ops}");
    assert!(p.notes.is_empty(), "{:?}", p.notes);
    let tall = "a\nb\nc\nd\ne\nf";
    let (p, ops) = painted(&draw(font, "", tall), &PaintOptions::new());
    assert_eq!(ops.matches(" Tj").count(), 6, "{ops}");
    assert_eq!(p.notes[0].kind, PaintNoteKind::Text, "clipped lines");
}

#[test]
fn an_unencodable_character_is_disclosed_and_an_inline_caption_is_not_drawn() {
    let (p, ops) = painted(&draw("", "", "a\u{4e00}"), &PaintOptions::new());
    assert!(ops.contains("(a?) Tj"), "{ops}");
    assert_eq!(p.notes[0].kind, PaintNoteKind::Text);
    let field = r#"<field name="f1" w="100pt" h="20pt"><ui><textEdit/></ui><value><text>v</text></value></field>"#;
    let (p, ops) = painted(field, &PaintOptions::new());
    assert!(
        ops.contains("(v) Tj") && p.notes.is_empty(),
        "{ops} {:?}",
        p.notes
    );
    let captioned = field.replace("<ui>", r#"<caption placement="inline"/><ui>"#);
    let (p, _) = painted(&captioned, &PaintOptions::new());
    assert_eq!(p.notes[0].kind, PaintNoteKind::Content);
}

/// A 200 x 40pt field at (46, 56) top-left in page space, value `v`.
fn captioned(caption: &str, font: &str) -> String {
    format!(
        r#"<field name="f1" x="10pt" y="20pt" w="200pt" h="40pt">{caption}{font}<ui><textEdit/></ui><value><text>v</text></value></field>"#
    )
}

const COURIER: &str = r#"<font typeface="Courier"/>"#;

/// Courier 10pt: ascent 6.29, line height 7.86; the item top is 736.
const ASCENT: f64 = 6.29;

#[test]
fn a_left_caption_takes_its_reserve_and_the_value_takes_the_rest() {
    let cap = r#"<caption reserve="50pt"><font typeface="Courier"/><value><text>Name</text></value></caption>"#;
    let (p, ops) = painted(&captioned(cap, COURIER), &PaintOptions::new());
    let y = num(736.0 - ASCENT);
    assert!(
        ops.contains(&format!("1 0 0 1 46 {y} Tm (Name) Tj")),
        "{ops}"
    );
    assert!(ops.contains(&format!("1 0 0 1 96 {y} Tm (v) Tj")), "{ops}");
    assert!(
        ops.contains(&re(46.0, 696.0, 50.0, 40.0)),
        "caption clip: {ops}"
    );
    assert!(p.notes.is_empty(), "{:?}", p.notes);
}

#[test]
fn an_unreserved_caption_is_sized_to_its_text_and_margins() {
    let top = r#"<caption placement="top"><margin topInset="2pt" bottomInset="3pt"/><font typeface="Courier"/><value><text>T</text></value></caption>"#;
    let (_, ops) = painted(&captioned(top, COURIER), &PaintOptions::new());
    let line_h = (629.0 + 157.0) / 1000.0 * 10.0;
    assert!(
        ops.contains(&format!(
            "1 0 0 1 46 {} Tm (T) Tj",
            num(736.0 - 2.0 - ASCENT)
        )),
        "{ops}"
    );
    let value_y = num(736.0 - (line_h + 5.0) - ASCENT);
    assert!(
        ops.contains(&format!("1 0 0 1 46 {value_y} Tm (v) Tj")),
        "{ops}"
    );
    // "ab" is 12pt of Courier: a right caption starts 12pt from the right.
    let right = r#"<caption placement="right" reserve="-1"><font typeface="Courier"/><value><text>ab</text></value></caption>"#;
    let (_, ops) = painted(&captioned(right, COURIER), &PaintOptions::new());
    let y = num(736.0 - ASCENT);
    assert!(
        ops.contains(&format!("1 0 0 1 234 {y} Tm (ab) Tj")),
        "{ops}"
    );
    assert!(ops.contains(&format!("1 0 0 1 46 {y} Tm (v) Tj")), "{ops}");
}

#[test]
fn an_invisible_caption_keeps_its_space_and_a_hidden_one_does_not() {
    let cap = |presence: &str| {
        format!(
            r#"<caption reserve="30pt" presence="{presence}"><value><text>N</text></value></caption>"#
        )
    };
    let y = num(736.0 - ASCENT);
    let (_, ops) = painted(&captioned(&cap("invisible"), COURIER), &PaintOptions::new());
    assert!(!ops.contains("(N)"), "{ops}");
    assert!(ops.contains(&format!("1 0 0 1 76 {y} Tm (v) Tj")), "{ops}");
    let (_, ops) = painted(&captioned(&cap("hidden"), COURIER), &PaintOptions::new());
    assert!(ops.contains(&format!("1 0 0 1 46 {y} Tm (v) Tj")), "{ops}");
}

#[test]
fn a_caption_without_a_font_uses_the_defaults_or_the_fields_font_by_setting() {
    let cap = r#"<caption reserve="50pt"><value><text>N</text></value></caption>"#;
    let body = captioned(cap, r#"<font typeface="Helvetica"/>"#);
    let (own, _) = painted(&body, &PaintOptions::new());
    assert_eq!(
        base_font(&own, b"XF0"),
        "Courier",
        "the caption, in the defaults"
    );
    assert_eq!(base_font(&own, b"XF1"), "Helvetica", "the value");
    let (field, _) = painted(
        &body,
        &PaintOptions::new().with_caption_font(CaptionFont::Field),
    );
    let fonts = field.resources.get(b"Font").unwrap().as_dict().unwrap();
    assert_eq!(fonts.len(), 1, "caption and value share Helvetica");
    assert_eq!(base_font(&field, b"XF0"), "Helvetica");
}

#[test]
fn a_rich_text_caption_is_disclosed_and_its_space_still_carved() {
    let cap =
        r#"<caption reserve="40pt"><value><exData contentType="text/html"/></value></caption>"#;
    let (p, ops) = painted(&captioned(cap, COURIER), &PaintOptions::new());
    assert_eq!(p.notes[0].kind, PaintNoteKind::Content);
    assert!(
        ops.contains(&format!("1 0 0 1 86 {} Tm (v) Tj", num(736.0 - ASCENT))),
        "{ops}"
    );
}
