//! Fields and draws sized by their text (Pass 647.2c, request X026).

use super::*;

/// Courier 10pt: 6pt a character; `lineHeight` makes the pitch 12pt.
const TEXT: &str = r#"<font typeface="Courier" size="10pt"/><para lineHeight="12pt"/>"#;
const MULTI: &str = r#"<ui><textEdit multiLine="1"/></ui>"#;

/// `n` nine-letter words: three to a 200pt line (3 × 54 + 2 × 6 = 174).
fn words(n: usize) -> String {
    let v = vec!["abcdefghi"; n].join(" ");
    format!("<value><text>{v}</text></value>")
}

/// `before`-tall subform `a`, then `<{tag} name="f" {attrs}>` holding
/// `inner`, then 20pt draw `d`.
fn form(before: f64, tag: &str, attrs: &str, inner: &str) -> XfaLayout {
    lay(&format!(
        r#"<subform name="f0" layout="tb">{PAGE}<subform name="a" w="10pt" h="{before}pt"/>
          <{tag} name="f" {attrs}>{TEXT}{inner}</{tag}>
          <draw name="d" w="10pt" h="20pt"/></subform>"#
    ))
}

#[test]
fn a_multi_line_field_takes_the_height_of_its_wrapped_text() {
    let l = form(
        0.0,
        "field",
        r#"w="200pt" minH="20pt""#,
        &format!("{MULTI}{}", words(60)),
    );
    let p = &l.pages[0];
    assert_eq!(
        rect(p, "f"),
        r(36.0, 36.0, 200.0, 240.0),
        "20 lines of 12pt"
    );
    assert_eq!(rect(p, "d").y, 276.0, "the next draw follows");
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[test]
fn a_draw_takes_the_height_of_its_wrapped_text() {
    let l = form(0.0, "draw", r#"w="200pt" minH="20pt""#, &words(60));
    assert_eq!(rect(&l.pages[0], "f").h, 240.0);
    assert_eq!(rect(&l.pages[0], "d").y, 276.0);
}

#[test]
fn max_h_caps_the_height_and_zero_means_unbounded() {
    let inner = format!("{MULTI}{}", words(60));
    let l = form(
        0.0,
        "field",
        r#"w="200pt" minH="20pt" maxH="100pt""#,
        &inner,
    );
    assert_eq!(rect(&l.pages[0], "f").h, 100.0);
    let l = form(0.0, "field", r#"w="200pt" maxH="0""#, &inner);
    assert_eq!(rect(&l.pages[0], "f").h, 240.0, "maxH 0 is omitted (3.0)");
}

#[test]
fn an_empty_field_keeps_its_min_h() {
    let l = form(0.0, "field", r#"w="200pt" minH="20pt""#, MULTI);
    assert_eq!(rect(&l.pages[0], "f").h, 20.0);
    let l = form(
        0.0,
        "field",
        r#"w="200pt" minH="20pt""#,
        &format!("{MULTI}<value><text/></value>"),
    );
    assert_eq!(rect(&l.pages[0], "f").h, 20.0);
}

#[test]
fn a_grown_field_splits_across_pages_without_a_min_h() {
    // 40 lines = 480pt; 60pt left on page one.
    let l = form(
        660.0,
        "field",
        r#"w="200pt""#,
        &format!("{MULTI}{}", words(120)),
    );
    assert_eq!(l.pages.len(), 2, "the fewest pages");
    assert_eq!(rect(&l.pages[0], "f").h, 60.0);
    assert_eq!(rect(&l.pages[1], "f").h, 420.0);
    assert_eq!(rect(&l.pages[1], "d").y, 456.0);
}

#[test]
fn a_single_line_field_grows_in_width_only() {
    let l = form(
        0.0,
        "field",
        "",
        &format!("<ui><textEdit/></ui>{}", words(10)),
    );
    // 10 × 54 + 9 × 6 = 594: one unwrapped line.
    assert_eq!(rect(&l.pages[0], "f"), r(36.0, 36.0, 594.0, 12.0));
}

#[test]
fn margins_and_a_caption_add_to_the_text() {
    let inner = format!(
        r#"{MULTI}<margin topInset="2pt" bottomInset="3pt" leftInset="4pt" rightInset="6pt"/>
        <caption placement="top" reserve="15pt"><value><text>Name</text></value></caption>{}"#,
        words(60)
    );
    // Wrap at 200 - 10 = 190: still three words a line.
    let l = form(0.0, "field", r#"w="200pt""#, &inner);
    assert_eq!(rect(&l.pages[0], "f").h, 240.0 + 15.0 + 5.0);
    let left = inner.replace(
        r#"placement="top" reserve="15pt""#,
        r#"placement="left" reserve="20pt""#,
    );
    // Wrap at 200 - 10 - 20 = 170: two words a line, 30 lines.
    let l = form(0.0, "field", r#"w="200pt""#, &left);
    assert_eq!(rect(&l.pages[0], "f").h, 360.0 + 5.0);
}

#[test]
fn a_zero_font_size_or_a_narrow_box_stays_bounded() {
    let l = lay(&format!(
        r#"<subform name="f0" layout="tb">{PAGE}<draw name="f" w="200pt"><font size="0pt"/>{}</draw></subform>"#,
        words(3)
    ));
    assert!(rect(&l.pages[0], "f").h > 0.0, "the 10pt default");
    let l = form(0.0, "field", r#"w="1pt""#, &format!("{MULTI}{}", words(5)));
    assert_eq!(rect(&l.pages[0], "f").h, 60.0, "a word a line");
}
