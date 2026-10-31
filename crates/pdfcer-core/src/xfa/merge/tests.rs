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

fn values(m: &MergedForm) -> Vec<&str> {
    m.bound.iter().map(|b| b.value.as_str()).collect()
}

/// X022 Form F: an exclusion group with default binding selects by name.
#[test]
fn an_exclgroup_binds_by_name_and_its_members_do_not() {
    let body = r#"<subform name="Section1"><exclGroup name="LikesPizza"><field name="Yes"><items><text>Yes</text></items></field><field name="No"><items><text>No</text></items></field></exclGroup></subform>"#;
    let m = merged(
        body,
        "<data><Section1><LikesPizza>Yes</LikesPizza><Yes>member</Yes></Section1></data>",
    );
    assert_eq!(values(&m), ["Yes"]);
    assert_eq!(m.value_at(&[0, 0, 0]), Some("Yes"));
}

/// X023 Form A: a `match="none"` subform opens no data scope, a bound
/// subform inside it still does.
#[test]
fn a_none_subform_is_not_a_data_scope() {
    let body = r#"<subform name="sec"><bind match="none"/><field name="colour"/><subform name="inner"><field name="pet"/></subform></subform><subform name="bound"><field name="town"/></subform>"#;
    let m = merged(
        body,
        "<data><colour>Teal</colour><inner><pet>Biscuit</pet></inner><bound><town>Springfield</town></bound></data>",
    );
    assert_eq!(values(&m), ["Teal", "Biscuit", "Springfield"]);
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
}

/// X023 Form B: the control, a bound subform with nested data.
#[test]
fn a_named_subform_binds_its_group() {
    let body = r#"<subform name="sec"><field name="colour"/></subform>"#;
    let m = merged(
        body,
        "<data><sec><colour>Teal</colour></sec><colour>outer</colour></data>",
    );
    assert_eq!(values(&m), ["Teal"]);
}

#[test]
fn transparent_containers_pass_their_scope_through() {
    let body = r#"<subform><field name="a"/></subform><subform name="s" scope="none"><field name="b"/></subform><area name="x"><field name="c"/></area>"#;
    let m = merged(body, "<data><a>1</a><b>2</b><c>3</c></data>");
    assert_eq!(values(&m), ["1", "2", "3"]);
}

/// p.185: a field may match data a generation up; never data inside another
/// group (p.189).
#[test]
fn a_scope_match_looks_up_but_never_into_another_group() {
    let body = r#"<subform name="g"><field name="up"/><field name="hidden"/></subform>"#;
    let m = merged(
        body,
        "<data><g><z>0</z></g><up>u</up><other><hidden>h</hidden></other></data>",
    );
    assert_eq!(values(&m), ["u"]);
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
}

#[test]
fn match_template_allows_direct_matches_only() {
    let x = XdpDocument::parse(format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template><subform name="data" mergeMode="matchTemplate"><subform name="g"><field name="up"/></subform></subform></template><xfa:datasets xmlns:xfa="{DATA_NS}"><xfa:data><data><g><z>0</z></g><up>u</up></data></xfa:data></xfa:datasets></xdp:xdp>"#
    ))
    .unwrap();
    let t = read_template(&x).unwrap().unwrap();
    let m = merge_data(&t, &x, &MergeOptions::new()).unwrap();
    assert!(m.bound.is_empty(), "{:?}", m.bound);
}

/// p.191, p.200: same-named fields take same-named values in document
/// order, one each; a spare field keeps its template value.
#[test]
fn same_named_fields_bind_in_document_order_one_node_each() {
    let body = r#"<field name="n"/><field name="n"/><field name="n"/>"#;
    let m = merged(body, "<data><n>1</n><n>2</n></data>");
    assert_eq!(values(&m), ["1", "2"]);
    assert_eq!(m.value_at(&[0, 2]), None);
}

#[test]
fn a_dataref_bound_node_is_not_taken_again_by_name() {
    let body = format!(r#"{}<field name="n"/>"#, field("r", "$.n"));
    let m = merged(&body, "<data><n>1</n><n>2</n></data>");
    assert_eq!(values(&m), ["1", "2"]);
}

/// p.181: a field never binds a group; it is skipped with a warning.
#[test]
fn a_same_named_group_is_a_wrong_type_for_a_field() {
    let m = merged(r#"<field name="g"/>"#, "<data><g><x>1</x></g></data>");
    assert!(m.bound.is_empty());
    assert_eq!(m.warnings[0].kind, MergeWarningKind::WrongType);
    assert_eq!(m.warnings[0].reference, "g");
}

#[test]
fn page_set_fields_do_not_bind_by_name() {
    let body =
        r#"<pageSet><pageArea name="p"><field name="a"/></pageArea></pageSet><field name="a"/>"#;
    let m = merged(body, "<data><a>1</a></data>");
    assert_eq!(m.bound.len(), 1);
    assert_eq!(m.bound[0].path, [0, 1]);
}

#[test]
fn a_thousand_same_named_fields_and_deep_none_chains_stay_sane() {
    let mut body = "<field name=\"n\"/>".repeat(1_000);
    for _ in 0..200 {
        body = format!(r#"<subform name="s"><bind match="none"/>{body}</subform>"#);
    }
    let data = format!("<data>{}</data>", "<n>v</n>".repeat(1_500));
    let m = merged(&body, &data);
    assert_eq!(m.bound.len(), 1_000);
}

const PIZZA: &str = r#"<exclGroup name="LikesPizza"><bind match="global"/><field name="Yes"><items><text>Yes</text></items></field><field name="No"><items><text>No</text></items></field></exclGroup>"#;

/// X022 Form E: a global exclusion group, data outside its subform's group.
#[test]
fn x022_form_e_selects_yes() {
    let body = format!(r#"<subform name="Section1">{PIZZA}</subform>"#);
    let m = merged(
        &body,
        "<data><Section1><note>n</note></Section1><LikesPizza>Yes</LikesPizza></data>",
    );
    assert_eq!(values(&m), ["Yes"]);
    assert_eq!(m.value_at(&[0, 0, 0]), Some("Yes"));
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
}

/// Global reaches into another subform's group, where `once` cannot, and
/// `AsOnce` turns it off for exclusion groups only.
#[test]
fn global_reaches_another_group_unless_as_once() {
    let body = format!(
        r#"<subform name="Section1">{PIZZA}<field name="who"><bind match="global"/></field></subform>"#
    );
    let data = "<data><Section1><note>n</note></Section1><Other><LikesPizza>Yes</LikesPizza><who>Ann</who></Other></data>";
    assert_eq!(values(&merged(&body, data)), ["Yes", "Ann"]);
    let as_once = MergeOptions::new().with_global_exclgroup(GlobalExclGroup::AsOnce);
    assert_eq!(values(&merged_with(&body, data, &as_once)), ["Ann"]);
    let once = body.replace(r#"<bind match="global"/>"#, "");
    assert!(values(&merged(&once, data)).is_empty());
}

/// X022 point 2: every copy shows the one value, protected copies and
/// repeated subforms included, and they all name one data node.
#[test]
fn every_global_copy_shares_one_value() {
    let copy = r#"<field name="userName"><bind match="global"/></field>"#;
    let body = format!(
        r#"<subform name="page1">{copy}</subform><subform name="later"><field name="userName" access="protected"><bind match="global"/></field></subform><subform name="rep">{copy}</subform><subform name="rep">{copy}</subform>"#
    );
    let m = merged(&body, "<data><userName>A. Tester</userName></data>");
    assert_eq!(values(&m), ["A. Tester"; 4]);
    assert!(m.warnings.is_empty(), "{:?}", m.warnings);
}

#[test]
fn a_thousand_global_copies_bind() {
    let copy = r#"<subform name="s"><field name="n"><bind match="global"/></field></subform>"#;
    let m = merged(&copy.repeat(1000), "<data><n>v</n></data>");
    assert_eq!(m.bound.len(), 1000);
    assert!(m.bound.iter().all(|b| b.value == "v"));
}

/// `Records` looks only outside every record (the data root's values).
#[test]
fn the_records_scope_skips_values_inside_a_record() {
    let body = r#"<subform name="t"><field name="n"><bind match="global"/></field></subform>"#;
    let inside = "<data><s><n>A</n></s><t><q>0</q></t></data>";
    let records = MergeOptions::new().with_global_scope(GlobalScope::Records);
    assert_eq!(values(&merged(body, inside)), ["A"]);
    assert!(values(&merged_with(body, inside, &records)).is_empty());
    let outside = "<data><t><q>0</q></t></data><n>B</n>";
    assert_eq!(values(&merged_with(body, outside, &records)), ["B"]);
}

/// A global copy of a normally bound value shares it, with a warning.
#[test]
fn mixing_global_and_once_warns_and_shares() {
    let body = r#"<field name="n"/><field name="n"><bind match="global"/></field>"#;
    let m = merged(body, "<data><n>v</n></data>");
    assert_eq!(values(&m), ["v", "v"]);
    assert_eq!(m.warnings.len(), 1);
    assert_eq!(m.warnings[0].kind, MergeWarningKind::MixedGlobal);
    assert_eq!(m.warnings[0].path, [0, 1]);
}

/// A global field on a master page takes the shared value; a global
/// subform binds as `once`.
#[test]
fn page_set_globals_bind_and_global_subforms_bind_once() {
    let body = r#"<pageSet><pageArea name="p"><field name="n"><bind match="global"/></field><field name="m"/></pageArea></pageSet><subform name="g"><bind match="global"/><field name="x"/></subform>"#;
    let m = merged(
        body,
        "<data><n>v</n><m>no</m><g><x>1</x></g><x>2</x></data>",
    );
    assert_eq!(values(&m), ["v", "1"]);
}

#[test]
fn a_global_name_with_only_a_group_warns_wrong_type() {
    let body = r#"<subform name="s"><field name="n"><bind match="global"/></field></subform>"#;
    let m = merged(body, "<data><s><n><k>1</k></n></s></data>");
    assert!(m.bound.is_empty());
    assert_eq!(m.warnings[0].kind, MergeWarningKind::WrongType);
}

#[test]
fn every_value_records_its_data_node_and_span() {
    let data = r#"<data k="7"><a>one</a><b/><c></c><a>two</a></data>"#;
    let x = xdp("", data);
    let doc = parse_xml(x.text()).unwrap();
    let root = data::read_data(&doc, DataAttributes::Load)
        .unwrap()
        .unwrap();
    let rec = &root.children[0];
    let text = x.text();
    let span_text = |s: &data::ValueSpan| match s {
        data::ValueSpan::Attribute(r) | data::ValueSpan::Content(r) => text[r.clone()].to_owned(),
        data::ValueSpan::Empty { element, qname } => format!("{qname}|{}", &text[element.clone()]),
        data::ValueSpan::None => "none".to_owned(),
    };
    let got: Vec<String> = rec.children.iter().map(|c| span_text(&c.span)).collect();
    assert_eq!(got, ["7", "one", "b|<b/>", "", "two"]);
    assert_eq!(span_text(&rec.span), "none");
    if let data::ValueSpan::Content(r) = &rec.children[3].span {
        assert_eq!(&text[r.start..r.start + 4], "</c>");
    }

    let body = r#"<field name="a"/><field name="a"/><field name="c"/>"#;
    let m = merged(body, data);
    let paths: Vec<&[usize]> = m.bound.iter().map(|b| b.data.as_slice()).collect();
    assert_eq!(paths, [&[0, 1][..], &[0, 4], &[0, 3]]);
}
