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
    let border = r#"<edge stroke="wavy"/><corner radius="2pt" inverted="1" presence="invisible"/><fill><pattern/></fill>"#;
    let body = field(border)
        .replace("<border>", r#"<border break="open">"#)
        .replace(
            "</field>",
            r#"<caption placement="inline"/><color/></field>"#,
        );
    let (ops, notes, _) = paint(&body, &PaintOptions::new());
    // The unknown stroke falls back to a solid one.
    assert!(ops.contains(" l S") && !ops.contains(" d "), "{ops}");
    let kinds: Vec<_> = notes.iter().map(|n| n.kind).collect();
    use PaintNoteKind as K;
    assert_eq!(
        kinds,
        [K::OpenBreak, K::FillType, K::Stroke, K::Corner, K::Content]
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

fn check(ui: &str, rest: &str) -> String {
    format!(r#"<field name="f1" x="10pt" y="20pt" w="40pt" h="20pt"><ui>{ui}</ui>{rest}</field>"#)
}

const ON_OFF: &str = "<items><integer>1</integer><integer>0</integer><integer>2</integer></items>";

#[test]
fn an_on_check_button_draws_its_box_top_left_and_a_cross() {
    let body = check(
        "<checkButton/>",
        &format!("{ON_OFF}<value><integer>1</integer></value>"),
    );
    let (ops, notes, [x, y, _, h]) = paint(&body, &PaintOptions::new());
    let bx = re(x, y + h - 10.0, 10.0, 10.0);
    assert!(ops.contains(&format!("0.5 w 0 J 0 0 0 RG {bx} S")), "{ops}");
    let (cx, cy, s) = (x + 5.0, y + h - 5.0, 2.5);
    let first = format!(
        "{} {} m {} {} l",
        num(cx - s * 0.8),
        num(cy - s * 0.8),
        num(cx + s * 0.8),
        num(cy + s * 0.8)
    );
    assert!(
        ops.contains(&first),
        "a saltire, the default mark of a square box: {ops}"
    );
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn an_off_or_neutral_check_button_draws_the_box_alone() {
    for (v, note) in [("0", false), ("2", true)] {
        let body = check(
            "<checkButton/>",
            &format!("{ON_OFF}<value><integer>{v}</integer></value>"),
        );
        let (ops, notes, _) = paint(&body, &PaintOptions::new());
        assert!(
            ops.ends_with(" re S"),
            "the box and nothing after it: {ops}"
        );
        assert_eq!(
            notes.iter().any(|n| n.kind == PaintNoteKind::Content),
            note,
            "{v}: {notes:?}"
        );
    }
}

#[test]
fn without_items_a_value_of_one_or_on_is_on() {
    for v in ["<integer>1</integer>", "<text>On</text>"] {
        let body = check("<checkButton/>", &format!("<value>{v}</value>"));
        let (ops, _, _) = paint(&body, &PaintOptions::new());
        assert!(ops.contains(" l "), "{v} draws the mark: {ops}");
    }
    let (ops, _, _) = paint(
        &check("<checkButton/>", "<value><text>yes</text></value>"),
        &PaintOptions::new(),
    );
    assert!(!ops.contains(" l "), "{ops}");
}

#[test]
fn a_round_box_strokes_a_circle_and_fills_a_dot() {
    let body = check(
        r#"<checkButton shape="round" size="12pt"/>"#,
        &format!("{ON_OFF}<value><integer>1</integer></value>"),
    );
    let (ops, _, [x, y, _, h]) = paint(&body, &PaintOptions::new());
    let (cx, cy) = (x + 6.0, y + h - 6.0);
    assert!(
        ops.contains(&format!("{} {} m", num(cx + 6.0), num(cy))),
        "{ops}"
    );
    assert!(ops.contains(" h S"), "the circle is stroked: {ops}");
    assert!(
        ops.contains(" h f"),
        "the default round mark is a filled circle: {ops}"
    );
}

#[test]
fn para_alignment_places_the_box_and_a_caption_takes_its_reserve() {
    let body = check(
        "<checkButton><border><edge thickness=\"1pt\"/></border></checkButton>",
        &format!(
            r#"<para hAlign="right" vAlign="middle"/><caption placement="left" reserve="10pt"><value><text>A</text></value></caption>{ON_OFF}"#
        ),
    );
    let (ops, notes, [x, y, w, h]) = paint(&body, &PaintOptions::new());
    let bx = re(x + w - 10.0, y + (h - 10.0) / 2.0, 10.0, 10.0);
    assert!(
        ops.contains(&format!("1 w 2 J 0 0 0 RG {bx} S")),
        "the widget's own border: {ops}"
    );
    assert!(ops.contains("(A) Tj"), "{ops}");
    assert!(
        !notes.iter().any(|n| n.kind == PaintNoteKind::Content),
        "{notes:?}"
    );
}

#[test]
fn a_named_mark_takes_the_font_fill_colour() {
    let body = check(
        r#"<checkButton mark="star"/>"#,
        &format!(
            r#"<font><fill><color value="255,0,0"/></fill></font>{ON_OFF}<value><integer>1</integer></value>"#
        ),
    );
    let (ops, _, _) = paint(&body, &PaintOptions::new());
    assert!(ops.contains("1 0 0 RG 1 0 0 rg"), "{ops}");
    assert_eq!(ops.matches(" l ").count(), 9, "a ten-vertex star: {ops}");
}

#[test]
fn a_round_corner_trims_its_edges_and_draws_an_arc() {
    let border = r#"<edge/><corner join="round" radius="4pt"/>"#;
    let (ops, notes, [x, y, w, h]) = paint(&field(border), &PaintOptions::new());
    let top = y + h;
    let edge = format!(
        "{} {} m {} {} l S",
        num(x + 4.0),
        num(top),
        num(x + w - 4.0),
        num(top)
    );
    assert!(ops.contains(&format!("0.5 w 0 J 0 0 0 RG {edge}")), "{ops}");
    assert!(
        ops.contains(&format!("0 j {} {} m", num(x), num(top - 4.0))),
        "{ops}"
    );
    assert!(
        ops.contains(&format!("{} {} c S", num(x + 4.0), num(top))),
        "{ops}"
    );
    assert_eq!(ops.matches(" c S").count(), 4, "{ops}");
    assert!(!ops.contains(" re"), "{ops}");
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn a_rounded_fill_follows_the_corners() {
    let border = r#"<edge/><corner join="round" radius="4pt"/><fill/>"#;
    let (ops, _, [x, y, _, h]) = paint(&field(border), &PaintOptions::new());
    let fill = ops.split(" f").next().unwrap();
    assert!(
        fill.contains(&format!("1 1 1 rg {} {} m", num(x), num(y + h - 4.0))),
        "{ops}"
    );
    assert_eq!(fill.matches(" c").count(), 4, "{ops}");
    assert!(fill.ends_with(" h"), "{ops}");
}

#[test]
fn an_inverted_square_corner_is_a_notch_and_is_disclosed() {
    let border = r#"<edge/><corner inverted="1" radius="3pt"/>"#;
    let (ops, notes, [x, y, _, h]) = paint(&field(border), &PaintOptions::new());
    let top = y + h;
    let notch = format!(
        "{} {} m {} {} l {} {} l S",
        num(x),
        num(top - 3.0),
        num(x + 3.0),
        num(top - 3.0),
        num(x + 3.0),
        num(top)
    );
    assert!(ops.contains(&notch), "{ops}");
    assert_eq!(notes.len(), 1);
    assert_eq!(notes[0].kind, PaintNoteKind::Corner);
}

#[test]
fn an_invisible_corner_leaves_the_trimmed_gap() {
    let border = r#"<edge/><corner join="round" radius="4pt" presence="invisible"/>"#;
    let (ops, notes, [x, y, w, h]) = paint(&field(border), &PaintOptions::new());
    let top = y + h;
    let edge = format!(
        "{} {} m {} {} l S",
        num(x + 4.0),
        num(top),
        num(x + w - 4.0),
        num(top)
    );
    assert!(ops.contains(&edge), "{ops}");
    assert!(!ops.contains(" c"), "{ops}");
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn a_radius_is_held_to_half_the_shorter_side() {
    let border = r#"<edge/><corner join="round" radius="50pt"/>"#;
    let (ops, _, [x, y, _, h]) = paint(&field(border), &PaintOptions::new());
    assert!(
        ops.contains(&format!("{} {} m", num(x), num(y + h - 10.0))),
        "{ops}"
    );
    assert!(
        ops.contains(&format!("{} {} c S", num(x + 10.0), num(y + h))),
        "{ops}"
    );
}

#[test]
fn a_split_draw_shows_its_own_lines_in_each_piece() {
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="f" layout="tb">{PAGE}<subform name="a" w="10pt" h="690pt"/><draw name="t" w="20pt" minH="60pt"><keep intact="none"/><font typeface="Courier" size="10pt"/><para lineHeight="12pt"/><value><text>aa bb cc dd ee</text></value></draw></subform></template></xdp:xdp>"#
    );
    let t = read_template(&XdpDocument::parse(xdp).unwrap())
        .unwrap()
        .unwrap();
    let l = layout_template(&t, &LayoutOptions::new()).unwrap();
    let ops = |p: usize| {
        let out = paint_page(&t, &l.pages[p], &PaintOptions::new());
        let s = String::from_utf8(out.content).unwrap();
        s.split_whitespace().collect::<Vec<_>>().join(" ")
    };
    let (head, tail) = (ops(0), ops(1));
    assert!(
        head.contains(&format!("{} W n", re(36.0, 42.0, 20.0, 24.0))),
        "{head}"
    );
    assert!(
        tail.contains(&format!("{} W n", re(36.0, 720.0, 20.0, 36.0))),
        "{tail}"
    );
    // The whole draw's top sits 24pt above the area: line 3 is the tail's first.
    let baseline = 792.0 - (36.0 - 24.0) - 6.29 - 24.0;
    assert!(
        tail.contains(&format!("36 {} Tm (cc) Tj", num(baseline))),
        "{tail}"
    );
}
