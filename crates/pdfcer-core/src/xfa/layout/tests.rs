#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)] // a panic here is the test failing; exact sums of binary-exact points

use super::*;
use crate::xfa::{XdpDocument, read_template};

fn lay_with(body: &str, opts: &LayoutOptions) -> Result<XfaLayout, LayoutError> {
    let xdp = format!(
        r#"<xdp:xdp xmlns:xdp="http://ns.adobe.com/xdp/"><template xmlns="http://www.xfa.org/schema/xfa-template/3.3/">{body}</template></xdp:xdp>"#
    );
    let t = read_template(&XdpDocument::parse(xdp).unwrap())
        .unwrap()
        .unwrap();
    layout_template(&t, opts)
}

fn lay(body: &str) -> XfaLayout {
    lay_with(body, &LayoutOptions::new()).unwrap()
}

const PAGE: &str = r#"<pageSet><pageArea name="P"><contentArea x="36pt" y="36pt" w="540pt" h="720pt"/></pageArea></pageSet>"#;

fn names(page: &LayoutPage) -> Vec<&str> {
    page.items
        .iter()
        .filter_map(|i| i.name.as_deref())
        .collect()
}

fn page_names(l: &XfaLayout) -> Vec<&str> {
    l.pages.iter().filter_map(|p| p.name.as_deref()).collect()
}

fn rect(page: &LayoutPage, name: &str) -> LayoutRect {
    page.items
        .iter()
        .find(|i| i.name.as_deref() == Some(name))
        .unwrap()
        .rect
}

const fn r(x: f64, y: f64, w: f64, h: f64) -> LayoutRect {
    LayoutRect { x, y, w, h }
}

#[test]
fn an_anchor_point_and_a_subform_margin_place_the_child() {
    let l = lay(&format!(
        r#"<subform name="f">{PAGE}
        <subform name="s" x="100pt" y="50pt"><margin leftInset="10pt" topInset="5pt" rightInset="2pt" bottomInset="3pt"/>
          <field name="a" x="40pt" y="20pt" w="40pt" h="20pt" anchorType="middleCenter"/>
        </subform></subform>"#
    ));
    let p = &l.pages[0];
    assert_eq!(names(p), ["s", "a"], "parent before child");
    assert_eq!(rect(p, "a"), r(166.0, 101.0, 40.0, 20.0));
    // Growable: the children's extent plus the far margins.
    assert_eq!(rect(p, "s"), r(136.0, 86.0, 72.0, 38.0));
    assert_eq!(p.items[1].content_area, Some(0));
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[test]
fn growth_is_clamped_and_a_swapped_min_max_is_reported() {
    let l = lay(&format!(
        r#"<subform name="f">{PAGE}
        <subform name="s" minW="100pt" minH="50pt" maxH="10pt"><draw name="d" w="20pt" h="20pt"/></subform></subform>"#
    ));
    assert_eq!(rect(&l.pages[0], "s"), r(36.0, 36.0, 100.0, 20.0));
    assert!(l.warnings.contains(&LayoutWarning::MinGreaterThanMax {
        path: vec![0, 1],
        attribute: "minH"
    }));
}

#[test]
fn page_background_is_flagged_and_drawn_first() {
    let l = lay(
        r#"<subform name="f"><pageSet><pageArea><contentArea x="1in" y="1in" w="6in" h="9in"/>
          <draw name="logo" x="10pt" y="10pt" w="50pt" h="20pt"/><medium stock="a4" orientation="landscape"/></pageArea></pageSet>
          <field name="a" w="1in" h="1in"/></subform>"#,
    );
    let p = &l.pages[0];
    assert_eq!(names(p), ["logo", "a"]);
    assert!(p.items[0].background && !p.items[1].background);
    assert_eq!(p.items[0].content_area, None);
    assert_eq!((p.width, p.height), (841.89, 595.276));
}

#[test]
fn ordered_occurrence_consumes_each_page_area_in_turn() {
    let l = lay(r#"<subform name="f"><pageSet>
          <pageArea name="A"><occur max="1"/><contentArea w="8in" h="10in"/></pageArea>
          <pageArea name="B"><contentArea w="8in" h="10in"/></pageArea></pageSet>
          <field name="x"/>
          <field name="y"><breakBefore targetType="pageArea"/></field>
          <field name="z"><breakBefore targetType="pageArea"/></field></subform>"#);
    assert_eq!(page_names(&l), ["A", "B", "B"]);
    assert_eq!(names(&l.pages[1]), ["y"]);
    assert_eq!(names(&l.pages[2]), ["z"]);
}

#[test]
fn a_break_on_an_empty_first_page_adds_no_blank_page_unless_start_new() {
    let body = |start: &str| {
        format!(
            r#"<subform name="f">{PAGE}<field name="x"><breakBefore targetType="pageArea"{start}/></field></subform>"#
        )
    };
    assert_eq!(lay(&body("")).pages.len(), 1);
    let l = lay(&body(r#" startNew="1""#));
    assert_eq!(l.pages.len(), 2);
    assert!(l.pages[0].items.is_empty());
    assert_eq!(names(&l.pages[1]), ["x"]);
}

#[test]
fn exhausted_page_areas_discard_the_rest_and_say_so() {
    let l = lay(
        r#"<subform name="f"><pageSet><occur max="1"/><pageArea><occur max="1"/><contentArea w="8in" h="10in"/></pageArea></pageSet>
          <field name="x"/><field name="y"><breakBefore targetType="pageArea"/></field><field name="z"/></subform>"#,
    );
    assert_eq!(l.pages.len(), 1);
    assert_eq!(names(&l.pages[0]), ["x"]);
    for path in [vec![0, 2], vec![0, 3]] {
        assert!(
            l.warnings
                .contains(&LayoutWarning::ContentDiscarded { path })
        );
    }
}

#[test]
fn a_page_set_repeats_until_its_own_occur_max() {
    let fields: String = (0..4)
        .map(|i| format!(r#"<field name="f{i}"><breakBefore targetType="pageArea"/></field>"#))
        .collect();
    let l = lay(&format!(
        r#"<subform name="f"><pageSet><occur max="3"/><pageArea><occur max="1"/><contentArea w="8in" h="10in"/></pageArea></pageSet>{fields}</subform>"#
    ));
    assert_eq!(l.pages.len(), 3);
    assert!(
        l.warnings
            .contains(&LayoutWarning::ContentDiscarded { path: vec![0, 4] })
    );
}

#[test]
fn a_cover_without_a_content_area_is_emitted_and_skipped() {
    let l = lay(r#"<subform name="f"><pageSet>
          <pageArea name="cover"><draw name="title" w="1in" h="1in"/></pageArea>
          <pageArea name="body"><occur max="1"/><contentArea w="8in" h="10in"/></pageArea>
          <pageArea name="back"><occur min="1" max="1"/><draw name="end" w="1in" h="1in"/></pageArea></pageSet>
          <field name="x"/></subform>"#);
    assert_eq!(
        page_names(&l),
        ["cover", "body", "back"],
        "back is padded to its min"
    );
    assert!(l.warnings.contains(&LayoutWarning::UnboundedBoilerplate {
        path: vec![0, 0, 0]
    }));
}

#[test]
fn a_target_moves_to_a_named_content_area_on_the_same_page() {
    let l = lay(r##"<subform name="f"><pageSet><pageArea name="P">
          <contentArea name="left" w="3in" h="10in"/><contentArea id="R" x="4in" w="3in" h="10in"/></pageArea></pageSet>
          <field name="x"/>
          <field name="y"><breakBefore targetType="contentArea" target="#R"/></field>
          <field name="z"><breakBefore targetType="contentArea" target="nowhere"/></field></subform>"##);
    assert_eq!(l.pages.len(), 2, "the invalid target goes to the next area");
    assert_eq!(names(&l.pages[0]), ["x", "y"]);
    assert_eq!(rect(&l.pages[0], "y").x, 288.0);
    assert_eq!(l.pages[0].items[1].content_area, Some(1));
    assert!(l.warnings.contains(&LayoutWarning::InvalidBreakTarget {
        path: vec![0, 3],
        target: "nowhere".to_owned()
    }));
}

#[test]
fn a_page_area_target_jumps_back_to_an_earlier_area() {
    let l = lay(r#"<subform name="f"><pageSet>
          <pageArea name="A"><occur max="1"/><contentArea w="8in" h="10in"/></pageArea>
          <pageArea name="B"><contentArea w="8in" h="10in"/></pageArea></pageSet>
          <field name="x"/><field name="y"><breakBefore targetType="pageArea" target="B"/></field>
          <field name="z"><breakBefore targetType="pageArea" target="A"/></field></subform>"#);
    assert_eq!(page_names(&l), ["A", "B", "A"]);
}

#[test]
fn page_odd_pads_to_an_odd_page() {
    let l = lay(&format!(
        r#"<subform name="f">{PAGE}<field name="x"/><field name="y"><breakBefore targetType="pageOdd"/></field></subform>"#
    ));
    assert_eq!(l.pages.len(), 3, "page 2 is even, so y lands on page 3");
    assert_eq!(names(&l.pages[2]), ["y"]);
}

#[test]
fn negative_coordinates_are_kept_or_clamped_and_reported() {
    let body = format!(
        r#"<subform name="f">{PAGE}<field name="a" x="-10pt" y="5pt" w="1in" h="1in"/></subform>"#
    );
    let kept = lay(&body);
    assert_eq!(rect(&kept.pages[0], "a").x, 26.0);
    let clamp = LayoutOptions::new().with_negative_coordinates(NegativeCoordinates::Clamp);
    let clamped = lay_with(&body, &clamp).unwrap();
    assert_eq!(rect(&clamped.pages[0], "a").x, 36.0);
    for l in [kept, clamped] {
        assert!(
            l.warnings
                .contains(&LayoutWarning::NegativeCoordinate { path: vec![0, 1] })
        );
    }
}

#[test]
fn presence_and_initial_occurrences_decide_what_is_placed() {
    let l = lay(&format!(
        r#"<subform name="f">{PAGE}
          <field name="hid" presence="hidden"/><field name="inv" presence="invisible"/>
          <subform name="none"><occur min="0"/></subform>
          <subform name="two"><occur initial="2" max="5"/></subform>
          <subformSet><draw name="inset"/></subformSet></subform>"#
    ));
    let p = &l.pages[0];
    assert_eq!(names(p), ["inv", "two", "two", "inset"]);
    assert!(!p.items[0].visible && p.items[1].visible);
}

#[test]
fn a_table_root_is_laid_positioned_and_reported() {
    let l = lay(&format!(
        r#"<subform name="f" layout="table">{PAGE}<field name="a"/></subform>"#
    ));
    assert_eq!(names(&l.pages[0]), ["a"]);
    assert_eq!(
        l.warnings,
        [LayoutWarning::FlowedLayoutPending {
            path: vec![0],
            strategy: LayoutStrategy::Table
        }]
    );
}

#[test]
fn a_tb_root_stacks_moves_to_the_next_area_and_reports_overflow() {
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}
          <field name="a" x="-100pt" y="100pt" w="100pt" h="300pt"/>
          <field name="b" w="50pt" h="300pt"/><field name="c" w="50pt" h="300pt"/>
          <field name="d" w="50pt" h="800pt"/></subform>"#
    ));
    assert_eq!(l.pages.len(), 3);
    assert_eq!(rect(&l.pages[0], "a"), r(36.0, 36.0, 100.0, 300.0));
    assert_eq!(rect(&l.pages[0], "b"), r(36.0, 336.0, 50.0, 300.0));
    assert_eq!(rect(&l.pages[1], "c"), r(36.0, 36.0, 50.0, 300.0));
    assert_eq!(rect(&l.pages[2], "d"), r(36.0, 36.0, 50.0, 800.0));
    assert_eq!(
        l.warnings,
        [LayoutWarning::ContentOverflow { path: vec![0, 4] }]
    );
}

fn tiled(strategy: &str) -> XfaLayout {
    lay(&format!(
        r#"<subform name="f">{PAGE}<subform name="s" layout="{strategy}" w="100pt">
          <margin leftInset="10pt" rightInset="10pt"/>
          <field name="a" w="50pt" h="20pt"/><field name="b" w="50pt" h="30pt"/>
          <field name="c" w="30pt" h="10pt"/></subform></subform>"#
    ))
}

#[test]
fn lr_tb_tiles_and_wraps_at_the_inner_width() {
    let l = tiled("lr-tb");
    let p = &l.pages[0];
    assert_eq!(rect(p, "s"), r(36.0, 36.0, 100.0, 50.0));
    assert_eq!(rect(p, "a"), r(46.0, 36.0, 50.0, 20.0));
    assert_eq!(rect(p, "b"), r(46.0, 56.0, 50.0, 30.0));
    assert_eq!(rect(p, "c"), r(96.0, 56.0, 30.0, 10.0));
    assert!(l.warnings.is_empty());
}

#[test]
fn rl_tb_tiles_from_the_right() {
    let p = &tiled("rl-tb").pages[0];
    assert_eq!(rect(p, "a"), r(76.0, 36.0, 50.0, 20.0));
    assert_eq!(rect(p, "b"), r(76.0, 56.0, 50.0, 30.0));
    assert_eq!(rect(p, "c"), r(46.0, 56.0, 30.0, 10.0));
}

#[test]
fn a_tb_subform_grows_to_its_stack_and_ignores_child_coordinates() {
    let l = lay(&format!(
        r#"<subform name="f">{PAGE}<subform name="s" layout="tb" x="10pt" y="10pt">
          <subformSet><field name="a" x="-5pt" w="40pt" h="20pt"/></subformSet>
          <field name="b" w="60pt" h="5pt"/></subform></subform>"#
    ));
    let p = &l.pages[0];
    assert_eq!(rect(p, "s"), r(46.0, 46.0, 60.0, 25.0));
    assert_eq!(rect(p, "a"), r(46.0, 46.0, 40.0, 20.0));
    assert_eq!(rect(p, "b"), r(46.0, 66.0, 60.0, 5.0));
    assert!(l.warnings.is_empty());
}

#[test]
fn a_tb_subform_splits_between_its_children_and_cuts_a_nested_one() {
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}<subform name="s" layout="tb">
          <field name="a" w="100pt" h="300pt"/><subform name="t" layout="tb">
          <field name="b" w="100pt" h="300pt"/><field name="c" w="100pt" h="300pt"/>
          </subform></subform></subform>"#
    ));
    assert_eq!(l.pages.len(), 2);
    assert_eq!(names(&l.pages[0]), ["s", "a", "t", "b"]);
    assert_eq!(names(&l.pages[1]), ["s", "t", "c"]);
    assert_eq!(rect(&l.pages[0], "s"), r(36.0, 36.0, 100.0, 600.0));
    assert_eq!(rect(&l.pages[0], "t"), r(36.0, 336.0, 100.0, 300.0));
    assert_eq!(rect(&l.pages[1], "s"), r(36.0, 36.0, 100.0, 300.0));
    assert_eq!(rect(&l.pages[1], "c"), r(36.0, 36.0, 100.0, 300.0));
    assert!(l.warnings.is_empty());
}

#[test]
fn keep_intact_moves_the_whole_subform_and_reports_its_overflow() {
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}<field name="p" w="10pt" h="100pt"/>
          <subform name="s" layout="tb"><keep intact="contentArea"/>
          <field name="a" w="100pt" h="450pt"/><field name="b" w="100pt" h="450pt"/>
          </subform></subform>"#
    ));
    assert_eq!(names(&l.pages[1]), ["s", "a", "b"]);
    assert_eq!(
        l.warnings,
        [LayoutWarning::ContentOverflow { path: vec![0, 2] }]
    );
}

#[test]
fn no_page_area_or_no_root_subform_is_an_error() {
    let o = LayoutOptions::new();
    let none = r#"<subform name="f"><field name="a"/></subform>"#;
    assert_eq!(lay_with(none, &o), Err(LayoutError::NoPageArea));
    let zero =
        r#"<subform name="f"><pageSet><pageArea><occur max="0"/></pageArea></pageSet></subform>"#;
    assert_eq!(lay_with(zero, &o), Err(LayoutError::NoPageArea));
    assert_eq!(lay_with("<config/>", &o), Err(LayoutError::NoRootSubform));
}

#[test]
fn an_unbounded_page_set_of_boilerplate_stops_at_the_page_limit() {
    let l = lay(
        r#"<subform name="f"><pageSet><pageArea><occur max="1"/><draw name="bg"/></pageArea></pageSet><field name="a"/></subform>"#,
    );
    assert_eq!(l.pages.len(), MAX_LAYOUT_PAGES);
    assert!(l.warnings.contains(&LayoutWarning::PageLimit));
    assert!(
        l.warnings
            .contains(&LayoutWarning::ContentDiscarded { path: vec![0, 1] })
    );
}

#[test]
fn huge_occurrence_counts_stop_at_the_limits() {
    let l = lay(&format!(
        r#"<subform name="f">{PAGE}<subformSet><occur initial="4000000000" max="-1"/></subformSet></subform>"#
    ));
    assert!(l.warnings.contains(&LayoutWarning::ItemLimit));
    let l = lay(
        r#"<subform name="f"><pageSet><pageArea><occur max="1"/><contentArea w="1in" h="1in"/></pageArea>
          <pageArea><occur min="4000000000" max="-1"/></pageArea></pageSet></subform>"#,
    );
    assert_eq!(l.pages.len(), MAX_LAYOUT_PAGES);
    assert!(l.warnings.contains(&LayoutWarning::PageLimit));
}

#[test]
fn an_adhering_chain_that_does_not_fit_moves_together() {
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}<field name="p" w="10pt" h="500pt"/>
          <field name="c" w="10pt" h="100pt"><keep next="contentArea"/></field>
          <field name="d" w="10pt" h="100pt"><keep next="contentArea"/></field>
          <field name="e" w="10pt" h="100pt"/></subform>"#
    ));
    assert_eq!(names(&l.pages[0]), ["p"]);
    assert_eq!(names(&l.pages[1]), ["c", "d", "e"]);
    assert!(l.warnings.is_empty());
}

#[test]
fn keep_previous_on_a_splittable_partner_keeps_only_its_top_fragment() {
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}<field name="p" w="10pt" h="500pt"/>
          <field name="c" w="10pt" h="100pt"/>
          <subform name="d" layout="tb"><keep previous="contentArea"/>
          <field name="d1" w="10pt" h="100pt"/><field name="d2" w="10pt" h="200pt"/>
          </subform></subform>"#
    ));
    assert_eq!(names(&l.pages[0]), ["p", "c", "d", "d1"]);
    assert_eq!(names(&l.pages[1]), ["d", "d2"]);
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}<field name="p" w="10pt" h="500pt"/>
          <field name="c" w="10pt" h="100pt"/>
          <subform name="d" layout="tb"><keep previous="contentArea"/>
          <field name="d1" w="10pt" h="150pt"/></subform></subform>"#
    ));
    assert_eq!(names(&l.pages[0]), ["p"]);
    assert_eq!(names(&l.pages[1]), ["c", "d", "d1"]);
}

#[test]
fn an_explicit_break_wins_over_adhesion() {
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}<field name="p" w="10pt" h="500pt"/>
          <field name="c" w="10pt" h="100pt"><keep next="contentArea"/></field>
          <subform name="e" layout="tb"><breakBefore targetType="contentArea"/>
          <field name="e1" w="10pt" h="200pt"/></subform></subform>"#
    ));
    assert_eq!(names(&l.pages[0]), ["p", "c"]);
    assert_eq!(names(&l.pages[1]), ["e", "e1"]);
}

#[test]
fn an_oversize_adhering_pair_runs_past_an_empty_area() {
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}
          <field name="c" w="10pt" h="500pt"><keep next="contentArea"/></field>
          <field name="d" w="10pt" h="500pt"/></subform>"#
    ));
    assert_eq!(l.pages.len(), 1);
    assert_eq!(rect(&l.pages[0], "d"), r(36.0, 536.0, 10.0, 500.0));
    assert_eq!(
        l.warnings,
        [LayoutWarning::ContentOverflow { path: vec![0, 2] }]
    );
}
