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

const PAGE_KEEP: &str = r#"<keep next="pageArea"/>"#;
const TWO_AREAS: &str = r#"<pageSet><pageArea name="P"><contentArea x="36pt" y="36pt" w="540pt" h="360pt"/>
    <contentArea x="36pt" y="400pt" w="540pt" h="360pt"/></pageArea></pageSet>"#;

/// `a1`, `a2` (keep next by page) and `a3`, top-level or inside `o`.
fn page_kept(page: &str, a1: u32, nested: bool, opts: &LayoutOptions) -> XfaLayout {
    let fields = [
        field("a1", a1, ""),
        field("a2", 100, PAGE_KEEP),
        field("a3", 100, ""),
    ]
    .concat();
    let body = if nested {
        format!(r#"<subform name="o" layout="tb">{fields}</subform>"#)
    } else {
        fields
    };
    lay_with(
        &format!(r#"<subform name="f" layout="tb">{page}{body}</subform>"#),
        opts,
    )
    .unwrap()
}

fn area_of(l: &XfaLayout, name: &str) -> (usize, Option<usize>) {
    l.pages
        .iter()
        .enumerate()
        .find_map(|(n, p)| {
            p.items
                .iter()
                .find(|i| i.name.as_deref() == Some(name))
                .map(|i| (n, i.content_area))
        })
        .unwrap()
}

#[test]
fn a_page_area_keep_holds_at_a_page_end_unless_laid_as_adobe() {
    let adobe = LayoutOptions::new().with_page_area_keep(PageAreaKeep::Adobe);
    for nested in [false, true] {
        let spec = page_kept(PAGE, 600, nested, &LayoutOptions::new());
        assert_eq!(pages_with(&spec, "a2"), [1], "nested={nested}");
        let a2 = path_of(&spec, "a2");
        assert_eq!(
            spec.warnings,
            [LayoutWarning::PageAreaKeep { path: a2 }],
            "nested={nested}"
        );
        let ours = page_kept(PAGE, 600, nested, &adobe);
        assert_eq!(pages_with(&ours, "a2"), [0], "nested={nested}");
        assert_eq!(pages_with(&ours, "a3"), [1], "nested={nested}");
        assert!(!ours.warnings.is_empty(), "disclosed either way");
    }
}

#[test]
fn a_page_area_keep_lets_the_pair_part_between_areas_of_one_page() {
    for nested in [false, true] {
        let l = page_kept(TWO_AREAS, 200, nested, &LayoutOptions::new());
        assert_eq!(area_of(&l, "a2"), (0, Some(0)), "nested={nested}");
        assert_eq!(area_of(&l, "a3"), (0, Some(1)), "nested={nested}");
    }
}

fn path_of(l: &XfaLayout, name: &str) -> Vec<usize> {
    l.pages
        .iter()
        .flat_map(|p| &p.items)
        .find(|i| i.name.as_deref() == Some(name))
        .unwrap()
        .path
        .clone()
}
