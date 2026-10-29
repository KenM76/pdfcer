use super::*;

const KEEP: &str = r#"<keep next="contentArea"/>"#;

/// `o` holding `body` in a `tb` flow on one 720pt content area.
fn inside(body: &str) -> XfaLayout {
    lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}<subform name="o" layout="tb">{body}</subform></subform>"#
    ))
}

fn field(name: &str, h: u32, keep: &str) -> String {
    format!(r#"<field name="{name}" w="10pt" h="{h}pt">{keep}</field>"#)
}

#[test]
fn an_unsplittable_adhering_pair_moves_together() {
    let l = inside(
        &[
            field("a1", 400, ""),
            field("a2", 250, KEEP),
            field("a3", 200, ""),
        ]
        .concat(),
    );
    assert_eq!(pages_with(&l, "a2"), [1], "moves with a3");
    assert_eq!(rect(&l.pages[1], "a2").y, 36.0);
    assert_eq!(rect(&l.pages[1], "a3").y, 286.0);
}

#[test]
fn a_splittable_partner_splits_and_its_rest_moves_with_it() {
    let b = [field("b1", 125, ""), field("b2", 125, "")].concat();
    let l = inside(&format!(
        r#"{}<subform name="a2" layout="tb">{KEEP}{b}</subform>{}"#,
        field("a1", 400, ""),
        field("a3", 200, "")
    ));
    assert_eq!(pages_with(&l, "b1"), [0]);
    assert_eq!(pages_with(&l, "b2"), [1]);
    assert_eq!(rect(&l.pages[1], "b2").y, 36.0);
    assert_eq!(rect(&l.pages[1], "a3").y, 161.0);
}

#[test]
fn instances_of_one_subform_adhere_to_each_other() {
    let l = inside(&format!(
        r#"{}<subform name="a" layout="tb">{KEEP}<occur min="3" initial="3" max="3"/>{}</subform>"#,
        field("o1", 100, ""),
        field("x", 250, "")
    ));
    let on = |p: usize| names(&l.pages[p]).iter().filter(|n| **n == "a").count();
    assert_eq!((on(0), on(1)), (0, 3));
}

#[test]
fn adhesion_does_not_cross_a_parent_boundary() {
    let l = inside(&format!(
        r#"<subform name="p" layout="tb">{}{}</subform>{}"#,
        field("p1", 400, ""),
        field("p2", 250, KEEP),
        field("q", 200, "")
    ));
    assert_eq!(pages_with(&l, "p2"), [0]);
    assert_eq!(rect(&l.pages[1], "q").y, 36.0);
}
