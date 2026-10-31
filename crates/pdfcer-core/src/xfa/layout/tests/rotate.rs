use super::*;

fn item<'a>(l: &'a XfaLayout, name: &str) -> &'a LayoutItem {
    l.pages[0]
        .items
        .iter()
        .find(|i| i.name.as_deref() == Some(name))
        .unwrap()
}

#[test]
fn a_positioned_draw_turns_about_its_anchor() {
    let l = lay(&format!(
        r#"<subform name="f">{PAGE}
        <draw name="a" x="10pt" y="60pt" w="40pt" h="10pt" rotate="90"/>
        <draw name="b" x="10pt" y="60pt" w="40pt" h="10pt" rotate="-90"/>
        <subform name="s" x="10pt" y="60pt" w="40pt" h="10pt" rotate="90"/></subform>"#
    ));
    let a = item(&l, "a");
    assert_eq!((a.rotate, a.rect), (90, r(46.0, 56.0, 10.0, 40.0)), "rises");
    let b = item(&l, "b");
    assert_eq!((b.rotate, b.rect), (270, r(36.0, 96.0, 10.0, 40.0)));
    let s = item(&l, "s");
    assert_eq!((s.rotate, s.rect), (0, r(46.0, 96.0, 40.0, 10.0)), "G-L30");
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[test]
fn a_flowed_parent_reserves_the_turned_extent() {
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}
        <draw name="a" w="40pt" h="10pt" rotate="90"/><draw name="b" w="10pt" h="20pt"/></subform>"#
    ));
    assert_eq!(item(&l, "a").rect, r(36.0, 36.0, 10.0, 40.0));
    assert_eq!(item(&l, "b").rect.y, 76.0);
}

#[test]
fn an_odd_angle_is_ignored_or_rounded() {
    let body = format!(
        r#"<subform name="f">{PAGE}<draw name="a" w="40pt" h="10pt" rotate="45"/></subform>"#
    );
    assert_eq!(item(&lay(&body), "a").rotate, 0);
    let near = LayoutOptions::new().with_odd_rotate(OddRotate::Nearest);
    assert_eq!(item(&lay_with(&body, &near).unwrap(), "a").rotate, 90);
}
