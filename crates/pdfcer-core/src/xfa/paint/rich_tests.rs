#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use super::check_tests::{kinds, painted};
use super::*;

const XHTML: &str = r#"xmlns="http://www.w3.org/1999/xhtml""#;

/// A 200x40pt draw holding the rich `body` in a Helvetica 10pt font.
fn draw(w: &str, body: &str) -> String {
    format!(
        r#"<draw name="f1" x="10pt" y="20pt" w="{w}" h="40pt"><font typeface="Helvetica"/><value><exData contentType="text/html"><body {XHTML}>{body}</body></exData></value></draw>"#
    )
}

/// The `x y` of every `Tm` in `ops`, in order.
fn origins(ops: &str) -> Vec<(f64, f64)> {
    let t: Vec<&str> = ops.split(' ').collect();
    t.iter()
        .enumerate()
        .filter(|(_, w)| **w == "Tm")
        .map(|(i, _)| (t[i - 2].parse().unwrap(), t[i - 1].parse().unwrap()))
        .collect()
}

#[test]
fn paragraphs_and_runs_are_drawn_in_order_in_their_own_faces() {
    let body = "<p>Hello <b>bold</b> end</p><p>Second</p>";
    let (ops, notes) = painted(&draw("200pt", body), &PaintOptions::new());
    assert!(notes.is_empty(), "{notes:?}");
    let shown: Vec<_> = ops.match_indices(") Tj").map(|(i, _)| i).collect();
    assert_eq!(shown.len(), 4, "{ops}");
    let order = ["(Hello)", "(bold)", "(end)", "(Second)"];
    let at: Vec<_> = order.iter().map(|s| ops.find(s).unwrap()).collect();
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{ops}");
    assert!(ops.contains("/XF1 10 Tf"), "bold in its own face: {ops}");
    let o = origins(&ops);
    assert_eq!(o[0].1, o[2].1, "one line");
    assert!(o[3].1 < o[0].1, "the second paragraph is lower: {o:?}");
    assert_eq!(o[3].0, 46.0, "it starts at the left edge");
}

#[test]
fn a_break_starts_a_line_and_an_unknown_element_is_dropped_and_disclosed() {
    let (ops, notes) = painted(
        &draw("200pt", "<p>one<br/>two<q>gone</q></p>"),
        &PaintOptions::new(),
    );
    let o = origins(&ops);
    assert_eq!(o.len(), 2, "{ops}");
    assert!(o[1].1 < o[0].1 && o[1].0 == o[0].0, "{o:?}");
    assert!(!ops.contains("gone"), "{ops}");
    assert_eq!(kinds(&notes), [PaintNoteKind::Text]);
}

#[test]
fn a_spacerun_keeps_its_spaces_and_a_narrow_region_wraps() {
    let run = |s: &str| {
        let (ops, _) = painted(&draw("200pt", s), &PaintOptions::new());
        let o = origins(&ops);
        o[1].0 - o[0].0
    };
    let collapsed = run("<p>a    b</p>"); // string-gap-exempt: the spaces under test
    let kept = run(r#"<p><span style="xfa-spacerun:yes">a    b</span></p>"#); // string-gap-exempt: the spaces under test
    let space = crate::vartext::text_width(crate::fontdata::Std14::Helvetica, 10.0, " ");
    assert!(
        (kept - collapsed - 3.0 * space).abs() < 1e-9,
        "{kept} {collapsed}"
    );
    let (ops, notes) = painted(
        &draw("40pt", "<p>wraps at spaces</p>"),
        &PaintOptions::new(),
    );
    let o = origins(&ops);
    assert_eq!(o.len(), 3, "{ops}");
    assert!(o[0].1 == o[1].1 && o[2].1 < o[1].1, "two lines: {o:?}");
    assert_eq!(o[2].0, 46.0, "the second line starts at the edge: {o:?}");
    assert!(notes.is_empty(), "{notes:?}");
}

#[test]
fn css_colour_alignment_and_size_apply() {
    let body = r#"<p style="text-align:right;color:#ff0000;font-size:20pt">R</p>"#;
    let (ops, notes) = painted(&draw("200pt", body), &PaintOptions::new());
    assert!(notes.is_empty(), "{notes:?}");
    assert!(ops.contains("1 0 0 rg") && ops.contains("20 Tf"), "{ops}");
    let w = crate::vartext::text_width(crate::fontdata::Std14::Helvetica, 20.0, "R");
    assert!((origins(&ops)[0].0 - (246.0 - w)).abs() < 1e-9, "{ops}");
}

#[test]
fn a_floating_field_shows_the_named_value_and_a_missing_one_is_disclosed() {
    let float = r##"<p>Page <span embed="#ff1" embedType="uri" embedMode="raw"/></p>"##;
    let named = r#"<draw name="n" id="ff1" x="300pt" y="0pt" w="20pt" h="20pt"><value><text>7</text></value></draw>"#;
    let (ops, notes) = painted(
        &format!("{}{named}", draw("200pt", float)),
        &PaintOptions::new(),
    );
    assert_eq!(ops.matches("(7) Tj").count(), 2, "{ops}");
    assert!(!kinds(&notes).contains(&PaintNoteKind::Text), "{notes:?}");
    let (_, notes) = painted(&draw("200pt", float), &PaintOptions::new());
    assert_eq!(kinds(&notes), [PaintNoteKind::Text]);
}

#[test]
fn a_rich_caption_is_drawn_and_sized_to_its_text() {
    let body = format!(
        r#"<field name="f1" x="10pt" y="20pt" w="200pt" h="20pt"><ui><textEdit/></ui><font typeface="Helvetica"/><caption><font typeface="Helvetica"/><value><exData contentType="text/html"><body {XHTML}><p>Name</p></body></exData></value></caption><value><text>v</text></value></field>"#
    );
    let (ops, notes) = painted(&body, &PaintOptions::new());
    assert!(notes.is_empty(), "{notes:?}");
    let w = crate::vartext::text_width(crate::fontdata::Std14::Helvetica, 10.0, "Name");
    let o = origins(&ops);
    assert_eq!(o[0].0, 46.0, "{ops}");
    assert!(
        (o[1].0 - (46.0 + w)).abs() < 1e-9,
        "the value follows the caption: {ops}"
    );
}

#[test]
fn a_growable_rich_draw_takes_its_text_height() {
    let body = draw("200pt", "<p>one</p><p>two</p>").replace(r#" h="40pt""#, r#" minH="0pt""#);
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="f"><pageSet><pageArea name="P"><contentArea x="36pt" y="36pt" w="540pt" h="720pt"/></pageArea></pageSet>{body}</subform></template></xdp:xdp>"#
    );
    let t = crate::xfa::read_template(&crate::xfa::XdpDocument::parse(xdp).unwrap())
        .unwrap()
        .unwrap();
    let l = crate::xfa::layout_template(&t, &crate::xfa::LayoutOptions::new()).unwrap();
    let item = l.pages[0]
        .items
        .iter()
        .find(|i| i.name.as_deref() == Some("f1"))
        .unwrap();
    let (_, line_h) = text::metrics(
        &super::face::Face::Std(crate::fontdata::Std14::Helvetica),
        10.0,
        None,
    );
    assert!((item.rect.h - 2.0 * line_h).abs() < 1e-9, "{}", item.rect.h);
}

#[test]
fn a_floating_page_number_comes_from_its_layout_script_and_is_disclosed() {
    let float = r##"<p>Page <span embed="#cp" embedType="uri" embedMode="raw"/> of <span embed="#pc" embedType="uri" embedMode="raw"/></p>"##;
    let field = |id: &str, call: &str| {
        format!(
            r#"<field name="{id}" id="{id}" presence="hidden" x="300pt" y="0pt" w="20pt" h="20pt"><ui><numericEdit/></ui><event activity="ready" ref="$layout"><script contentType="application/x-javascript">this.rawValue = xfa.layout.{call};</script></event></field>"#
        )
    };
    let body = format!(
        "{}{}{}",
        draw("200pt", float),
        field("cp", "page(this)"),
        field("pc", "pageCount()")
    );
    let (ops, notes) = painted(&body, &PaintOptions::new());
    assert!(
        ops.contains("(1) Tj") && ops.matches("(1) Tj").count() == 2,
        "{ops}"
    );
    assert_eq!(kinds(&notes), [PaintNoteKind::Script]);
}

#[test]
fn a_plain_caption_keeps_its_para_margin() {
    let field = |margin: &str| {
        format!(
            r#"<field name="f" x="10pt" y="20pt" w="100pt" h="30pt"><ui><textEdit/></ui><caption placement="top" reserve="10pt"><font typeface="Helvetica"/><para marginLeft="{margin}"/><value><text>Cap</text></value></caption></field>"#
        )
    };
    let x = |m: &str| origins(&painted(&field(m), &PaintOptions::new()).0)[0].0;
    assert_eq!(x("12pt") - x("0pt"), 12.0);
}
