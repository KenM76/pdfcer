use super::*;

/// A 12pt line pitch, from `para@lineHeight` over 10pt Courier.
const TEXT: &str = r#"<font typeface="Courier" size="10pt"/>"#;
const PARA: &str = r#"<para lineHeight="12pt"/>"#;
const MULTI: &str = r#"<ui><textEdit multiLine="1"/></ui>"#;

/// A `before`-tall subform, then field `f` (`attrs`, holding `inner`, 200pt
/// tall unless `attrs` says otherwise), then 20pt draw `d`.
fn form(before: f64, attrs: &str, inner: &str) -> XfaLayout {
    let para = if inner.contains("<para") { "" } else { PARA };
    lay(&format!(
        r#"<subform name="f0" layout="tb">{PAGE}<subform name="a" w="10pt" h="{before}pt"/>
          <field name="f" w="200pt" {attrs}>{TEXT}{para}{inner}</field>
          <draw name="d" w="10pt" h="20pt"/></subform>"#
    ))
}

fn growable(before: f64, inner: &str) -> XfaLayout {
    form(before, r#"minH="200pt""#, inner)
}

fn item<'a>(page: &'a LayoutPage, name: &str) -> &'a LayoutItem {
    page.items
        .iter()
        .find(|i| i.name.as_deref() == Some(name))
        .unwrap()
}

#[test]
fn a_growable_multi_line_field_splits_on_a_line_into_the_next_area() {
    let l = growable(655.0, MULTI);
    assert_eq!(l.pages.len(), 2);
    let (head, tail) = (item(&l.pages[0], "f"), item(&l.pages[1], "f"));
    assert_eq!(head.rect, r(36.0, 691.0, 200.0, 60.0), "five lines of 65pt");
    assert_eq!(tail.rect, r(36.0, 36.0, 200.0, 140.0));
    let (hf, tf) = (head.fragment.unwrap(), tail.fragment.unwrap());
    assert_eq!((hf.offset, hf.height), (0.0, 200.0));
    assert_eq!((tf.offset, tf.height), (60.0, 200.0));
    assert_eq!(rect(&l.pages[1], "d").y, 176.0, "the draw follows the tail");
    assert!(item(&l.pages[0], "a").fragment.is_none());
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[test]
fn a_field_that_may_not_split_moves_whole() {
    let cases = [
        (
            r#"minH="200pt""#,
            format!(r#"{MULTI}<keep intact="contentArea"/>"#),
        ),
        (r#"minH="200pt""#, String::new()),
        (r#"minH="200pt""#, r#"<ui><textEdit/></ui>"#.to_owned()),
        (r#"h="200pt""#, MULTI.to_owned()),
        (r#"minH="200pt" rotate="90""#, MULTI.to_owned()),
        (
            r#"minH="200pt""#,
            format!(r#"{MULTI}<para lineHeight="12pt" widows="16"/>"#),
        ),
    ];
    for (attrs, inner) in cases {
        let l = form(655.0, attrs, &inner);
        assert_eq!(names(&l.pages[0]), ["a"], "{attrs} {inner}");
        let f = item(&l.pages[1], "f");
        assert_eq!(f.rect.y, 36.0, "{attrs} {inner}");
        assert!(f.fragment.is_none(), "{attrs} {inner}");
    }
}

#[test]
fn less_than_one_line_of_room_moves_the_field_with_its_caption() {
    let caption =
        r#"<caption placement="top" reserve="12pt"><value><text>C</text></value></caption>"#;
    let l = growable(700.0, &format!("{MULTI}{caption}"));
    assert_eq!(names(&l.pages[0]), ["a"], "caption and one line need 24pt");
    assert!(item(&l.pages[1], "f").fragment.is_none());
    let l = growable(690.0, &format!("{MULTI}{caption}"));
    assert_eq!(
        item(&l.pages[0], "f").rect.h,
        24.0,
        "caption, then one line"
    );
}

#[test]
fn a_caption_without_a_reserve_keeps_the_field_whole() {
    let caption = r#"<caption placement="bottom"><value><text>C</text></value></caption>"#;
    let l = growable(655.0, &format!("{MULTI}{caption}"));
    assert!(item(&l.pages[1], "f").fragment.is_none());
}

#[test]
fn a_draw_splits_only_when_its_keep_says_none() {
    let body = |keep: &str| {
        format!(
            r#"<subform name="f0" layout="tb">{PAGE}<subform name="a" w="10pt" h="655pt"/>
              <draw name="t" w="200pt" minH="200pt">{TEXT}{PARA}{keep}<value><text>x</text></value></draw>
              </subform>"#
        )
    };
    let l = lay(&body(""));
    assert!(
        item(&l.pages[1], "t").fragment.is_none(),
        "contentArea by default"
    );
    let l = lay(&body(r#"<keep intact="none"/>"#));
    assert_eq!(item(&l.pages[0], "t").rect.h, 60.0);
    assert_eq!(item(&l.pages[1], "t").fragment.unwrap().offset, 60.0);
}

#[test]
fn a_bordered_subform_and_its_field_split_together() {
    let l = lay(&format!(
        r#"<subform name="f0" layout="tb">{PAGE}<subform name="a" w="10pt" h="655pt"/>
          <subform name="s" layout="tb"><border><edge/></border>
          <field name="f" w="200pt" minH="200pt">{TEXT}{PARA}{MULTI}</field></subform></subform>"#
    ));
    assert_eq!(l.pages.len(), 2);
    for (p, offset) in [(0, 0.0), (1, 60.0)] {
        let s = item(&l.pages[p], "s");
        assert_eq!(s.fragment.unwrap().offset, offset);
        assert_eq!(item(&l.pages[p], "f").fragment.unwrap().offset, offset);
    }
    assert_eq!(rect(&l.pages[0], "s").h, 60.0);
}

#[test]
fn a_very_tall_field_runs_on_line_by_line_without_losing_height() {
    let l = form(0.0, r#"minH="10000pt""#, MULTI);
    let pieces: Vec<_> = l
        .pages
        .iter()
        .flat_map(|p| p.items.iter())
        .filter(|i| i.name.as_deref() == Some("f"))
        .collect();
    assert!(pieces.len() > 10);
    let total: f64 = pieces.iter().map(|i| i.rect.h).sum();
    assert!((total - 10000.0).abs() < 1e-6, "{total}");
    for w in pieces.windows(2) {
        let (a, b) = (w[0].fragment.unwrap(), w[1].fragment.unwrap());
        assert!((a.offset + w[0].rect.h - b.offset).abs() < 1e-6);
        assert_eq!(b.offset % 12.0, 0.0, "every cut on a line");
    }
}
