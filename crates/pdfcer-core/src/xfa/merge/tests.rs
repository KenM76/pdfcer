#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use super::*;
use crate::xfa::read_template;

const DATA_NS: &str = "http://www.xfa.org/schema/xfa-data/1.0/";

fn xdp(template_body: &str, data: &str) -> XdpDocument {
    XdpDocument::parse(format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/"><subform name="data">{template_body}</subform></template><xfa:datasets xmlns:xfa="{DATA_NS}"><xfa:data>{data}</xfa:data></xfa:datasets></xdp:xdp>"#
    ))
    .unwrap()
}

fn merged_with(body: &str, data: &str, opts: &MergeOptions) -> MergedForm {
    let x = xdp(body, data);
    let t = read_template(&x).unwrap().unwrap();
    merge_data(&t, &x, opts).unwrap()
}

fn merged(body: &str, data: &str) -> MergedForm {
    merged_with(body, data, &MergeOptions::new())
}

fn field(name: &str, reference: &str) -> String {
    format!(r#"<field name="{name}"><bind match="dataRef" ref="{reference}"/></field>"#)
}

/// The X020 shape: dataRef fields inside an unbound page subform.
const X020_DATA: &str = "<data><GV_LINE1>First line</GV_LINE1><GV_LINE2>Second</GV_LINE2><IS_CREATE_VENDOR><REQUEST_ID>9000000123</REQUEST_ID></IS_CREATE_VENDOR></data>";

#[test]
fn dollar_refs_resolve_through_an_unbound_page_subform() {
    let body = format!(
        r#"<subform name="DemoPage">{}{}{}</subform>"#,
        field("a", "$.GV_LINE1"),
        field("b", "$.GV_LINE2"),
        field("c", "$.IS_CREATE_VENDOR.REQUEST_ID"),
    );
    let m = merged(&body, X020_DATA);
    let values: Vec<_> = m.bound.iter().map(|b| b.value.as_str()).collect();
    assert_eq!(values, ["First line", "Second", "9000000123"]);
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
    // template > subform data > subform DemoPage > field c.
    assert_eq!(m.value_at(&[0, 0, 2]), Some("9000000123"));
    let c = &m.template.element().children[0].children[0].children[2];
    assert_eq!(c.child("value").unwrap().children[0].text, "9000000123");
}

#[test]
fn a_bound_subform_becomes_the_scope_for_its_fields() {
    let body = format!(
        r#"<subform name="v"><bind match="dataRef" ref="$.IS_CREATE_VENDOR"/>{}</subform>"#,
        field("id", "$.REQUEST_ID")
    );
    let m = merged(&body, X020_DATA);
    assert_eq!(m.bound[0].value, "9000000123");
}

#[test]
fn an_existing_value_is_replaced_in_place() {
    let body = r#"<field name="a"><value><integer>7</integer></value><bind match="dataRef" ref="$.N"/></field>"#;
    let m = merged(body, "<data><N>42</N></data>");
    let a = &m.template.element().children[0].children[0];
    assert_eq!(a.children[0].children[0].tag, "integer");
    assert_eq!(a.children[0].children[0].text, "42");
}

#[test]
fn absolute_starts_and_indexes_resolve() {
    let data = "<data><R><V>a</V><V>b</V></R></data>";
    for (r, want) in [
        ("$data.data.R.V[1]", "b"),
        ("!data.data.R.V", "a"),
        ("xfa.datasets.data.data.R.V[0]", "a"),
        ("$record.R.V[1]", "b"),
    ] {
        let m = merged(&field("f", r), data);
        assert_eq!(m.bound.first().map(|b| b.value.as_str()), Some(want), "{r}");
    }
}

#[test]
fn star_binds_the_first_unbound_match() {
    let body = format!("{}{}", field("x", "$.V[*]"), field("y", "$.V[*]"));
    let m = merged(&body, "<data><V>1</V><V>2</V></data>");
    let values: Vec<_> = m.bound.iter().map(|b| b.value.as_str()).collect();
    assert_eq!(values, ["1", "2"]);
}

#[test]
fn an_unqualified_ref_searches_upward_unless_told_not_to() {
    let body = format!(
        r#"<subform name="s"><bind match="dataRef" ref="$.G"/>{}</subform>"#,
        field("f", "TOP")
    );
    let data = "<data><TOP>up</TOP><G><X>1</X></G></data>";
    assert_eq!(merged(&body, data).bound[0].value, "up");
    let opts = MergeOptions::new().with_scope_search(ScopeSearch::Children);
    let m = merged_with(&body, data, &opts);
    assert!(m.bound.is_empty());
    assert_eq!(m.warnings[0].kind, MergeWarningKind::Unresolved);
}

#[test]
fn attributes_load_as_values_by_default() {
    let data = r#"<data><P id="77"><Q>q</Q></P></data>"#;
    assert_eq!(merged(&field("f", "$.P.id"), data).bound[0].value, "77");
    let opts = MergeOptions::new().with_data_attributes(DataAttributes::Ignore);
    let m = merged_with(&field("f", "$.P.id"), data, &opts);
    assert_eq!(m.warnings[0].kind, MergeWarningKind::Unresolved);
}

#[test]
fn a_bare_ref_is_ignored_unless_set_to_bind() {
    let body = r#"<field name="f"><bind ref="$.A"/></field>"#;
    let data = "<data><A>v</A></data>";
    let m = merged(body, data);
    assert!(m.bound.is_empty() && m.warnings.is_empty());
    let opts = MergeOptions::new().with_bare_ref(BareRef::DataRef);
    assert_eq!(merged_with(body, data, &opts).bound[0].value, "v");
}

#[test]
fn an_escaped_dot_is_part_of_the_name() {
    let m = merged(&field("f", r"$.a\.b"), "<data><a.b>dot</a.b></data>");
    assert_eq!(m.bound[0].value, "dot");
}

#[test]
fn bad_refs_warn_and_keep_the_template_value() {
    let data = "<data><G><X>1</X></G><V>v</V></data>";
    for (r, kind) in [
        ("$.MISSING", MergeWarningKind::Unresolved),
        ("$.G", MergeWarningKind::WrongType),
        ("$..X", MergeWarningKind::Unsupported),
        ("$.#dataGroup", MergeWarningKind::Unsupported),
        ("$.V[+1]", MergeWarningKind::Unsupported),
        ("$.V[-1]", MergeWarningKind::Unsupported),
        ("$.V[x]", MergeWarningKind::Unsupported),
        ("$.*", MergeWarningKind::Unsupported),
        ("$.V.[a==1]", MergeWarningKind::Unsupported),
        ("", MergeWarningKind::Unsupported),
        ("$.V[5]", MergeWarningKind::Unresolved),
    ] {
        let body = format!(
            r#"<field name="f"><value><text>keep</text></value><bind match="dataRef" ref="{r}"/></field>"#
        );
        let m = merged(&body, data);
        assert!(m.bound.is_empty(), "{r}");
        assert_eq!(m.warnings.len(), 1, "{r}");
        assert_eq!(m.warnings[0].kind, kind, "{r}");
        assert_eq!(m.warnings[0].reference, r);
        let f = &m.template.element().children[0].children[0];
        assert_eq!(f.child("value").unwrap().children[0].text, "keep", "{r}");
    }
}

#[test]
fn a_subform_bound_to_a_value_is_a_wrong_type_and_passes_scope_through() {
    let body = format!(
        r#"<subform name="s"><bind match="dataRef" ref="$.V"/>{}</subform>"#,
        field("f", "$.V")
    );
    let m = merged(&body, "<data><V>v</V></data>");
    assert_eq!(m.warnings[0].kind, MergeWarningKind::WrongType);
    assert_eq!(m.bound[0].value, "v");
}

#[test]
fn hostile_data_warns_and_never_panics() {
    let deep = format!("$.{}", vec!["A"; 5000].join("."));
    let m = merged(&field("f", &deep), "<data><A>1</A></data>");
    assert_eq!(m.warnings[0].kind, MergeWarningKind::TooLong);
    let path = format!("$.{}", vec!["A"; 200].join("."));
    let nested = format!("{}v{}", "<A>".repeat(200), "</A>".repeat(200));
    let m = merged(&field("f", &path), &format!("<data>{nested}</data>"));
    assert_eq!(m.bound[0].value, "v");
    let repeated = "<V>1</V>".repeat(1000);
    let m = merged(&field("f", "$.V[999]"), &format!("<data>{repeated}</data>"));
    assert_eq!(m.bound[0].value, "1");
}

#[test]
fn no_datasets_reports_every_ref_and_changes_nothing() {
    let x = XdpDocument::parse(format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template><subform name="data">{}</subform></template></xdp:xdp>"#,
        field("f", "$.A")
    ))
    .unwrap();
    let t = read_template(&x).unwrap().unwrap();
    let m = merge_data(&t, &x, &MergeOptions::new()).unwrap();
    assert!(!m.has_data);
    assert_eq!(m.template, t);
    assert_eq!(m.warnings[0].kind, MergeWarningKind::Unresolved);
}

#[test]
fn an_exclgroup_value_turns_on_the_member_with_that_on_value() {
    let member = |n: &str, on: &str| {
        format!(
            r#"<field name="{n}"><ui><checkButton/></ui><items><text>{on}</text></items></field>"#
        )
    };
    let body = format!(
        r#"<exclGroup name="g"><bind match="dataRef" ref="$.YN"/>{}{}</exclGroup>"#,
        member("Yes", "1"),
        member("No", "2")
    );
    let m = merged(&body, "<data><YN>2</YN></data>");
    let g = &m.template.element().children[0].children[0];
    for f in g.children_named("field") {
        assert_eq!(f.child("value").unwrap().children[0].text, "2");
    }
    assert_eq!(m.bound.len(), 1);
}
