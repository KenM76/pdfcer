use super::*;

/// A short header, then an `lr-tb` subform of four rows (one of two
/// side-by-side fields) taller than the room left on the page.
fn body() -> String {
    format!(
        r#"<subform name="f" layout="tb">{PAGE}<subform name="p" layout="tb">
          <field name="h" w="540pt" h="160pt"/>
          <subform name="d" layout="lr-tb" w="540pt">
            <field name="a" w="540pt" h="200pt"/>
            <field name="b1" w="270pt" h="200pt"/><field name="b2" w="270pt" h="200pt"/>
            <field name="c" w="540pt" h="200pt"/><field name="e" w="540pt" h="200pt"/>
          </subform></subform></subform>"#
    )
}

#[test]
fn an_lr_tb_subform_splits_between_its_rows() {
    let l = lay(&body());
    assert_eq!(l.pages.len(), 2);
    assert_eq!(names(&l.pages[0]), ["p", "h", "d", "a", "b1", "b2"]);
    assert_eq!(names(&l.pages[1]), ["p", "d", "c", "e"]);
    assert_eq!(rect(&l.pages[0], "d"), r(36.0, 196.0, 540.0, 400.0));
    assert_eq!(rect(&l.pages[0], "b2"), r(306.0, 396.0, 270.0, 200.0));
    assert_eq!(rect(&l.pages[1], "c"), r(36.0, 36.0, 540.0, 200.0));
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[test]
fn tb_only_moves_the_tiled_subform_whole() {
    let opts = LayoutOptions::new().with_tiled_split(TiledSplit::TbOnly);
    let l = lay_with(&body(), &opts).unwrap();
    assert_eq!(names(&l.pages[0]), ["p", "h"]);
    assert_eq!(names(&l.pages[1]), ["p", "d", "a", "b1", "b2", "c", "e"]);
}

#[test]
fn keep_intact_holds_a_tiled_subform_together() {
    let held = body().replace(
        r#"layout="lr-tb" w="540pt">"#,
        r#"layout="lr-tb" w="540pt"><keep intact="contentArea"/>"#,
    );
    let l = lay(&held);
    assert_eq!(names(&l.pages[0]), ["p", "h"]);
}

/// An `lr-tb` root holding a header and a `tb` subform of three fields
/// taller than the room left on the page (gap G-L50, p.280 Example 8.7).
fn tiled_root() -> String {
    format!(
        r#"<subform name="f" layout="lr-tb" w="540pt">{PAGE}
          <field name="h" w="540pt" h="160pt"/>
          <subform name="d" layout="tb" w="540pt">
            <field name="a" w="540pt" h="300pt"/>
            <field name="b" w="540pt" h="300pt"/>
            <field name="c" w="540pt" h="300pt"/>
          </subform></subform>"#
    )
}

#[test]
fn a_tiled_root_splits_its_content_across_pages() {
    let l = lay(&tiled_root());
    assert_eq!(names(&l.pages[0]), ["h", "d", "a"]);
    assert_eq!(names(&l.pages[1]), ["d", "b", "c"]);
    assert!(l.warnings.is_empty(), "{:?}", l.warnings);
}

#[test]
fn tb_only_moves_content_under_a_tiled_root_whole() {
    let opts = LayoutOptions::new().with_tiled_split(TiledSplit::TbOnly);
    let l = lay_with(&tiled_root(), &opts).unwrap();
    assert_eq!(names(&l.pages[0]), ["h"]);
    assert_eq!(names(&l.pages[1]), ["d", "a", "b", "c"]);
}
