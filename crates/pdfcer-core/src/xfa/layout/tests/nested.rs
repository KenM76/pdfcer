use super::*;

const NESTED: &str = r#"<proto><subform id="T" name="T" w="10pt" h="30pt"/>
    <subform id="U" name="U" w="10pt" h="20pt"/>
    <subform id="L" name="L" w="10pt" h="20pt"/></proto>"#;

/// `o` (asserting `outer`) holding a 100pt field, then `i` (asserting
/// `inner`) of four 250pt fields: the split cuts `i` after `i2`.
fn nested(outer: &str, inner: &str, opts: &LayoutOptions) -> XfaLayout {
    let fields: String = (1..=4)
        .map(|k| format!(r#"<field name="i{k}" w="10pt" h="250pt"/>"#))
        .collect();
    lay_with(
        &format!(
            r#"<subform name="f" layout="tb">{PAGE}{NESTED}
              <subform name="o" layout="tb">{outer}<field name="o1" w="10pt" h="100pt"/>
              <subform name="i" layout="tb">{inner}{fields}</subform></subform></subform>"#
        ),
        opts,
    )
    .unwrap()
}

#[test]
fn a_cut_nested_subform_lays_its_trailer_and_leader_inside_itself() {
    let l = nested(
        "",
        r##"<overflow leader="#L" trailer="#T"/>"##,
        &LayoutOptions::new(),
    );
    let p0 = &l.pages[0];
    assert_eq!(names(p0).len(), 6, "{:?}", names(p0));
    assert_eq!(rect(p0, "T"), r(36.0, 636.0, 10.0, 30.0), "after i2");
    assert_eq!(rect(p0, "i"), r(36.0, 136.0, 10.0, 530.0), "grown to T");
    assert_eq!(rect(p0, "o").h, 630.0);
    let p1 = &l.pages[1];
    assert_eq!(rect(p1, "L"), r(36.0, 36.0, 10.0, 20.0));
    assert_eq!(rect(p1, "i3").y, 56.0, "below the leader");
    assert_eq!(rect(p1, "i"), r(36.0, 36.0, 10.0, 520.0), "holds L");
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[test]
fn an_inherited_trailer_lays_once_at_the_same_place() {
    let l = nested(r##"<overflow trailer="#T"/>"##, "", &LayoutOptions::new());
    let t: Vec<_> = l.pages[0]
        .items
        .iter()
        .filter(|i| i.name.as_deref() == Some("T"))
        .collect();
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].rect, r(36.0, 636.0, 10.0, 30.0));
}

#[test]
fn nested_overflow_lays_the_innermost_or_each_asserting_subform() {
    let (outer, inner) = (
        r##"<overflow trailer="#U"/>"##,
        r##"<overflow trailer="#T"/>"##,
    );
    let one = nested(outer, inner, &LayoutOptions::new());
    assert_eq!(pages_with(&one, "T"), [0]);
    assert_eq!(pages_with(&one, "U"), Vec::<usize>::new(), "innermost only");
    let each = nested(
        outer,
        inner,
        &LayoutOptions::new().with_nested_overflow(NestedOverflow::EachAsserting),
    );
    let p0 = &each.pages[0];
    assert_eq!(rect(p0, "T"), r(36.0, 636.0, 10.0, 30.0), "innermost first");
    assert_eq!(rect(p0, "U"), r(36.0, 666.0, 10.0, 20.0));
    assert_eq!(rect(p0, "i").h, 530.0, "T inside i");
    assert_eq!(rect(p0, "o").h, 630.0, "the instance lays U after itself");
    assert_eq!(names(&each.pages[1])[2], "i3");
}
