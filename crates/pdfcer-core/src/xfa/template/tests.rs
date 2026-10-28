#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // a panic here is the test failing

use super::*;
use crate::xfa::packets::parse_xdp;

const FORM: &str = r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/">
<subform name="form1" layout="tb" w="8.5in">
  <pageSet><pageArea name="Page1"><contentArea x="0.25in" y="0.25in" w="8in" h="10.5in"/><medium stock="letter" short="8.5in" long="11in"/></pageArea></pageSet>
  <subform name="row" layout="lr-tb" presence="hidden" colSpan="-1"><occur min="0" max="-1"/>
    <field name="a" x="10mm" y="2cm" w="72pt" maxW="0" anchorType="middleCenter"/>
    <draw name="label"><value><text>Hello</text></value></draw>
  </subform>
</subform></template></xdp:xdp>"#;

fn template() -> Template {
    read_template(&parse_xdp(FORM.to_owned()).unwrap())
        .unwrap()
        .unwrap()
}

#[test]
fn the_root_subform_and_its_page_set_are_found() {
    let t = template();
    let root = t.root_subform().unwrap();
    assert_eq!(root.name(), Some("form1"));
    assert_eq!(root.layout(), LayoutStrategy::Tb);
    let page = root.child("pageSet").unwrap().child("pageArea").unwrap();
    assert_eq!(page.container_kind(), Some(ContainerKind::PageArea));
    let ca = page.child("contentArea").unwrap();
    assert_eq!(ca.measurement("w"), Some(Measurement::Points(576.0)));
}

#[test]
fn attributes_read_with_their_defaults() {
    let t = template();
    let row = t.root_subform().unwrap().child("subform").unwrap();
    assert_eq!(row.layout(), LayoutStrategy::LrTb);
    assert_eq!(row.presence(), Presence::Hidden);
    assert_eq!(row.col_span(), None);
    assert_eq!(
        row.occur(),
        Some(Occur {
            min: 0,
            max: None,
            initial: 0
        })
    );
    let field = row.child("field").unwrap();
    assert_eq!(field.anchor_type(), AnchorType::MiddleCenter);
    assert_eq!(field.col_span(), Some(1));
    assert_eq!(field.occur(), None);
    let draw = row.child("draw").unwrap();
    assert_eq!(draw.layout(), LayoutStrategy::Position);
    assert_eq!(draw.presence(), Presence::Visible);
    assert_eq!(
        draw.child("value").unwrap().child("text").unwrap().text,
        "Hello"
    );
}

#[test]
fn an_absent_size_is_not_zero() {
    let t = template();
    let field = t
        .root_subform()
        .unwrap()
        .child("subform")
        .unwrap()
        .child("field")
        .unwrap();
    let pt = |k| field.measurement(k).and_then(Measurement::points);
    assert!((pt("x").unwrap() - 28.346_456).abs() < 1e-5);
    assert!((pt("y").unwrap() - 56.692_913).abs() < 1e-5);
    assert_eq!(pt("w"), Some(72.0));
    assert_eq!(field.measurement("h"), None);
    assert_eq!(field.measurement("maxW"), None, "maxW=0 means unbounded");
}

#[test]
fn occur_defaults_differ_by_container() {
    let doc = parse_xdp("<template><subform/><pageSet/></template>".to_owned()).unwrap();
    let t = read_template(&doc).unwrap().unwrap();
    let one = Occur {
        min: 1,
        max: Some(1),
        initial: 1,
    };
    assert_eq!(t.element().child("subform").unwrap().occur(), Some(one));
    let page = t.element().child("pageSet").unwrap().occur().unwrap();
    assert_eq!((page.min, page.max), (0, None));
}

#[test]
fn measurements_parse_units_and_refuse_junk() {
    assert_eq!(Measurement::parse("2"), Some(Measurement::Points(144.0)));
    assert_eq!(
        Measurement::parse(" -1.5pt "),
        Some(Measurement::Points(-1.5))
    );
    assert_eq!(Measurement::parse("2em"), Some(Measurement::Em(2.0)));
    assert_eq!(Measurement::parse("50%"), Some(Measurement::Percent(50.0)));
    assert_eq!(Measurement::parse("3px"), None);
    assert_eq!(Measurement::parse("in"), None);
    assert_eq!(Measurement::parse(""), None);
}

#[test]
fn no_template_packet_reads_as_none() {
    let doc = parse_xdp(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><datasets/></xdp:xdp>"#.to_owned(),
    )
    .unwrap();
    assert_eq!(read_template(&doc), Ok(None));
}
