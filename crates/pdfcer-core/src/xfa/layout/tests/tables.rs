#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)] // a panic here is the test failing; exact sums of binary-exact points

use super::*;

fn r(x: f64, y: f64, w: f64, h: f64) -> LayoutRect {
    LayoutRect { x, y, w, h }
}

#[test]
fn columns_take_their_width_or_the_widest_cell_and_rows_the_tallest() {
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}<subform name="t" layout="table" columnWidths="100pt -1">
        <subform name="r1" layout="row"><draw name="a" w="30pt" h="10pt"/><draw name="b" w="50pt" h="20pt"/><draw name="c" w="10pt" h="5pt"/></subform>
        <subform name="r2" layout="row"><draw name="d" w="60pt" h="15pt"/><draw name="e" w="70pt" h="12pt"/></subform>
        </subform></subform>"#
    ));
    let p = &l.pages[0];
    assert_eq!(rect(p, "a"), r(36.0, 36.0, 100.0, 20.0));
    assert_eq!(rect(p, "b"), r(136.0, 36.0, 70.0, 20.0));
    assert_eq!(rect(p, "c"), r(206.0, 36.0, 10.0, 20.0));
    assert_eq!(rect(p, "d"), r(36.0, 56.0, 100.0, 15.0));
    assert_eq!(rect(p, "e"), r(136.0, 56.0, 70.0, 15.0));
    assert_eq!(rect(p, "r2"), r(36.0, 56.0, 180.0, 15.0));
    assert_eq!(rect(p, "t"), r(36.0, 36.0, 180.0, 35.0));
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[test]
fn a_rest_span_hides_later_cells_and_rl_row_starts_at_the_right() {
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}<subform name="t" layout="table" columnWidths="10pt 20pt 30pt">
        <subform name="r" layout="rl-row"><draw name="a" w="5pt" h="5pt"/><draw name="b" colSpan="-1" w="5pt" h="5pt"/><draw name="z" w="5pt" h="5pt"/></subform>
        <subform name="s" layout="row"><draw name="c" colSpan="2" w="5pt" h="5pt"/><draw name="d" w="5pt" h="5pt"/></subform>
        </subform></subform>"#
    ));
    let p = &l.pages[0];
    assert_eq!(rect(p, "a"), r(86.0, 36.0, 10.0, 5.0));
    assert_eq!(rect(p, "b"), r(36.0, 36.0, 50.0, 5.0));
    assert!(!names(p).contains(&"z"), "{:?}", names(p));
    assert_eq!(rect(p, "c"), r(36.0, 41.0, 30.0, 5.0));
    assert_eq!(rect(p, "d"), r(66.0, 41.0, 30.0, 5.0));
}

#[test]
fn a_flowed_cell_is_content_tall_unless_declared_heights_are_asked_for() {
    let body = format!(
        r#"<subform name="f" layout="tb">{PAGE}<subform name="t" layout="table"><subform name="r" layout="row">
        <subform name="c1" layout="tb" w="50pt" h="100pt"><draw name="x" w="10pt" h="10pt"/></subform>
        <subform name="c2" w="50pt" h="20pt"/>
        </subform></subform></subform>"#
    );
    let l = lay(&body);
    assert_eq!(rect(&l.pages[0], "r").h, 20.0);
    assert_eq!(rect(&l.pages[0], "c1").h, 20.0);
    let declared = LayoutOptions::new().with_table_cell_height(TableCellHeight::Declared);
    let l = lay_with(&body, &declared).unwrap();
    assert_eq!(rect(&l.pages[0], "r").h, 100.0);
    assert_eq!(rect(&l.pages[0], "c2").h, 100.0);
}

#[test]
fn a_row_outside_a_table_is_positioned_and_reported() {
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}<subform name="r" layout="row" w="100pt" h="20pt"><draw name="a" x="40pt" w="5pt" h="5pt"/></subform></subform>"#
    ));
    assert_eq!(rect(&l.pages[0], "a").x, 76.0);
    assert_eq!(
        l.warnings,
        [LayoutWarning::InappropriateLayout {
            path: vec![0, 1],
            strategy: LayoutStrategy::Row
        }]
    );
}

#[test]
fn a_table_splits_between_rows_and_a_table_root_aligns_its_rows() {
    let rows: String = (0..10)
        .map(|i| {
            format!(r#"<subform name="r{i}" layout="row"><draw w="10pt" h="100pt"/></subform>"#)
        })
        .collect();
    let l = lay(&format!(
        r#"<subform name="f" layout="tb">{PAGE}<subform name="t" layout="table">{rows}</subform></subform>"#
    ));
    assert_eq!(l.pages.len(), 2);
    assert!(
        names(&l.pages[1]).contains(&"r7"),
        "{:?}",
        names(&l.pages[1])
    );
    assert_eq!(rect(&l.pages[1], "r7").y, 36.0);
    let l = lay(&format!(
        r#"<subform name="f" layout="table">{PAGE}<subform name="r" layout="row"><draw name="a" w="10pt" h="10pt"/></subform><subform name="s" layout="row"><draw name="b" w="30pt" h="10pt"/></subform></subform>"#
    ));
    assert_eq!(rect(&l.pages[0], "a").w, 30.0);
    assert_eq!(rect(&l.pages[0], "s").y, 46.0);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}
