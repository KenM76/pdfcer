use super::*;

fn rows(occur: &str, reference: &str) -> String {
    format!(
        r#"<subform name="row">{occur}<bind match="dataRef" ref="{reference}"/><field name="v"/></subform>"#
    )
}

const ITEMS: &str =
    "<data><list><item><v>a</v></item><item><v>b</v></item><item><v>c</v></item></list></data>";

#[test]
fn a_star_dataref_makes_one_instance_per_match() {
    let m = merged(
        &rows(r#"<occur min="0" max="-1"/>"#, "$.list.item[*]"),
        ITEMS,
    );
    assert_eq!(values(&m), ["a", "b", "c"]);
    let root = &m.template.element().children[0];
    assert_eq!(root.children.len(), 3);
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    assert_eq!(m.unbound_values, 0);
}

#[test]
fn dataref_instances_stop_at_max_and_report_excess_by_reference() {
    let m = merged(
        &rows(r#"<occur min="0" max="2"/>"#, "$.list.item[*]"),
        ITEMS,
    );
    assert_eq!(values(&m), ["a", "b"]);
    let excess: Vec<_> = m
        .warnings
        .iter()
        .filter(|w| w.kind == MergeWarningKind::ExcessData)
        .collect();
    assert_eq!(excess.len(), 1);
    assert_eq!(excess[0].reference, "$.list.item[*]");
    assert!(!excess[0].by_name);
}

#[test]
fn a_single_node_dataref_makes_one_instance() {
    let m = merged(
        &rows(r#"<occur min="0" max="-1"/>"#, "$.list.item[1]"),
        ITEMS,
    );
    assert_eq!(values(&m), ["b"]);
}

#[test]
fn an_unresolved_dataref_makes_min_instances_and_a_bad_one_warns() {
    let m = merged(&rows(r#"<occur min="0" max="-1"/>"#, "$.nothing[*]"), ITEMS);
    assert!(m.bound.is_empty());
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    let bad = merged(&rows(r#"<occur min="0" max="-1"/>"#, "$..item"), ITEMS);
    assert_eq!(bad.warnings[0].kind, MergeWarningKind::Unsupported);
}
