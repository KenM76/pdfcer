use super::*;

/// A member subform `name` with `occur` and one field `n`.
fn member(name: &str, occur: &str) -> String {
    format!(r#"<subform name="{name}">{occur}<field name="n"/></subform>"#)
}

fn set(relation: &str, occur: &str, members: &[String]) -> String {
    format!(
        r#"<subformSet name="topping" relation="{relation}">{occur}{}</subformSet>"#,
        members.concat()
    )
}

fn datum(name: &str, value: &str) -> String {
    format!("<{name}><n>{value}</n></{name}>")
}

/// The names of the laid member instances (`occur@initial` not 0) of each
/// set copy under the root subform, in order.
fn copies(m: &MergedForm) -> Vec<Vec<String>> {
    let laid = |n: &XfaNode| {
        n.child("occur")
            .and_then(|o| o.attr("initial"))
            .is_none_or(|i| i != "0")
    };
    m.template.element().children[0]
        .children
        .iter()
        .filter(|c| c.tag == "subformSet" && laid(c))
        .map(|s| {
            s.children
                .iter()
                .filter(|c| c.tag == "subform" && laid(c))
                .map(|c| c.name().unwrap_or_default().to_owned())
                .collect()
        })
        .collect()
}

fn three(relation: &str, occurs: [&str; 3]) -> String {
    set(
        relation,
        "",
        &[
            member("pepperoni", occurs[0]),
            member("greenPeppers", occurs[1]),
            member("olives", occurs[2]),
        ],
    )
}

/// Ex 9.13 (pp.351-352): the matched member first, the rest at their min.
#[test]
fn unordered_matched_members_lead_and_the_rest_take_their_min() {
    let t = three(
        "unordered",
        [r#"<occur min="0"/>"#, r#"<occur min="2"/>"#, "<occur/>"],
    );
    let m = merged(&t, &format!("<data>{}</data>", datum("pepperoni", "p")));
    assert_eq!(
        copies(&m),
        [["pepperoni", "greenPeppers", "greenPeppers", "olives"]]
    );
    assert_eq!(values(&m), ["p"]);
}

/// Members that bind come first in data order, not template order.
#[test]
fn unordered_members_follow_the_data_order() {
    let t = three("unordered", [r#"<occur min="0"/>"#; 3]);
    let data = format!(
        "<data>{}{}</data>",
        datum("olives", "o"),
        datum("greenPeppers", "g")
    );
    let m = merged(&t, &data);
    assert_eq!(copies(&m), [["olives", "greenPeppers"]]);
    assert_eq!(values(&m), ["o", "g"]);
    let ordered = merged(&three("ordered", [r#"<occur min="0"/>"#; 3]), &data);
    assert_eq!(copies(&ordered), [["greenPeppers", "olives"]]);
}

/// A choice set keeps the one member whose data comes first (p.349).
#[test]
fn choice_keeps_the_member_matched_first() {
    let t = three("choice", [r#"<occur min="0"/>"#; 3]);
    let data = format!(
        "<data>{}{}</data>",
        datum("olives", "o"),
        datum("pepperoni", "p")
    );
    let m = merged(&t, &data);
    assert_eq!(copies(&m), [["olives"]]);
    assert_eq!(values(&m), ["o"]);
}

/// No member binds: the set stays, with no member (p.349, G-SS7).
#[test]
fn choice_with_no_match_keeps_an_empty_set() {
    let m = merged(&three("choice", ["", "", ""]), "<data><other/></data>");
    assert_eq!(copies(&m), [Vec::<String>::new()]);
}

/// The chosen member's min is ignored (p.344); its max still counts unless
/// [`ChoiceInstances::One`] (G-SS5).
#[test]
fn choice_ignores_min_and_honours_max_by_default() {
    let t = three(
        "choice",
        [r#"<occur min="3" max="-1"/>"#, r#"<occur min="0"/>"#, ""],
    );
    let one = format!("<data>{}</data>", datum("pepperoni", "a"));
    assert_eq!(copies(&merged(&t, &one)), [["pepperoni"]]);
    let two = format!(
        "<data>{}{}</data>",
        datum("pepperoni", "a"),
        datum("pepperoni", "b")
    );
    assert_eq!(copies(&merged(&t, &two)), [["pepperoni", "pepperoni"]]);
    let opts = MergeOptions::new().with_choice_instances(ChoiceInstances::One);
    assert_eq!(copies(&merged_with(&t, &two, &opts)), [["pepperoni"]]);
}

/// Without data a choice set keeps only its first member (p.349, G-SS6).
#[test]
fn choice_without_data_keeps_the_first_member() {
    let body = three("choice", [r#"<occur initial="2"/>"#, "", ""]);
    let x = XdpDocument::parse(format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template><subform name="data">{body}</subform></template></xdp:xdp>"#
    ))
    .unwrap();
    let t = read_template(&x).unwrap().unwrap();
    let m = merge_data(&t, &x, &MergeOptions::new()).unwrap();
    assert_eq!(copies(&m), [["pepperoni"]]);
}

/// A set with `occur@max` gets one more copy per copy that binds data
/// (p.341); without `occur` it is one copy and the rest is left unbound.
#[test]
fn a_repeating_set_copies_while_its_members_bind() {
    let item = member("item", "");
    let data = format!(
        "<data>{}{}{}</data>",
        datum("item", "a"),
        datum("item", "b"),
        datum("item", "c")
    );
    let many = set(
        "ordered",
        r#"<occur min="1" max="-1"/>"#,
        std::slice::from_ref(&item),
    );
    let m = merged(&many, &data);
    assert_eq!(copies(&m), [["item"], ["item"], ["item"]]);
    assert_eq!(values(&m), ["a", "b", "c"]);
    let once = merged(&set("ordered", "", &[item]), &data);
    assert_eq!(copies(&once), [["item"]]);
    assert_eq!(once.unbound_values, 2);
}
