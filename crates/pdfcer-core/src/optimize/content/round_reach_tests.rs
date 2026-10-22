#![allow(clippy::unwrap_used, clippy::panic, clippy::indexing_slicing)]

use super::*;
use crate::optimize::OptimizeSettings;
use crate::optimize::tests::{build, id, session};
use crate::space_audit::tests::stream;

/// 1.004 rounds to 1 within 0.01 at scale 1 (20 numbers); at scale 10
/// that is 0.04 and nothing shorter fits.
fn body() -> Vec<u8> {
    b"1.004  2.5  m  S\n".repeat(20)
}

const FORM: &str = "/Type /XObject /Subtype /Form /BBox [0 0 200 200]";

/// Page 3 draws `content` with `resources`; 5 is `five`; `extra` from 6.
fn doc(
    page_extra: &str,
    content: &[u8],
    resources: &str,
    five: Vec<u8>,
    extra: &[Vec<u8>],
) -> Vec<u8> {
    let contents = if page_extra.contains("/Contents") {
        ""
    } else {
        "/Contents 4 0 R"
    };
    let mut bodies = vec![
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 200] {contents} \
             /Resources << {resources} >> {page_extra} >>"
        )
        .into_bytes(),
        stream("", content),
        five,
    ];
    bodies.extend_from_slice(extra);
    build(&bodies)
}

fn form(extra: &str, content: &[u8]) -> Vec<u8> {
    stream(&format!("{FORM} {extra}"), content)
}

/// What `round=0.01` does to object `n`: numbers rounded and why not.
fn rounding(bytes: Vec<u8>, n: u32) -> (u64, Option<Unrounded>) {
    let mut settings = OptimizeSettings::new();
    settings.content = Some(ContentPatch::parse("content(round=0.01)").unwrap());
    let plan = session(bytes).plan_optimize(&settings).unwrap();
    let c = plan
        .changes
        .iter()
        .find(|c| c.id == id(n))
        .and_then(|c| c.content)
        .unwrap_or_else(|| panic!("no content change for {n}: {:?}", plan.skipped));
    (c.numbers_rounded, c.unrounded)
}

fn drawn(content: &[u8], matrix: &str) -> (u64, Option<Unrounded>) {
    let fm = "/XObject << /Fm0 5 0 R >>";
    rounding(doc("", content, fm, form(matrix, &body()), &[]), 5)
}

#[test]
fn a_form_rounds_at_the_scale_of_its_do() {
    assert_eq!(drawn(b"/Fm0 Do", ""), (20, None));
    assert_eq!(drawn(b"q 10 0 0 10 0 0 cm /Fm0 Do Q", ""), (0, None));
    // The q/Q is balanced before the Do: back at scale 1.
    assert_eq!(drawn(b"q 10 0 0 10 0 0 cm Q /Fm0 Do", ""), (20, None));
}

#[test]
fn a_form_matrix_scales_its_space() {
    assert_eq!(drawn(b"/Fm0 Do", "/Matrix [10 0 0 10 0 0]"), (0, None));
    // A rotation stretches nothing.
    assert_eq!(drawn(b"/Fm0 Do", "/Matrix [0 1 -1 0 50 50]"), (20, None));
}

#[test]
fn a_form_drawn_twice_takes_the_larger_scale() {
    assert_eq!(
        drawn(b"/Fm0 Do q 10 0 0 10 0 0 cm /Fm0 Do Q", ""),
        (0, None)
    );
}

#[test]
fn a_form_after_an_unknown_matrix_or_with_a_bad_matrix_does_not_round() {
    let unknown = (0, Some(Unrounded::UnknownStart));
    assert_eq!(drawn(b"/M cm /Fm0 Do", ""), unknown);
    assert_eq!(drawn(b"/Fm0 Do", "/Matrix [1 0 0]"), unknown);
}

#[test]
fn a_form_an_annotation_also_draws_does_not_round() {
    let annot = b"<< /Type /Annot /Subtype /Stamp /Rect [0 0 9 9] /AP << /N 5 0 R >> >>".to_vec();
    let fm = "/XObject << /Fm0 5 0 R >>";
    let bytes = doc(
        "/Annots [6 0 R]",
        b"/Fm0 Do",
        fm,
        form("", &body()),
        &[annot],
    );
    assert_eq!(rounding(bytes, 5), (0, Some(Unrounded::UnknownStart)));
}

#[test]
fn a_nested_form_multiplies_every_matrix_on_the_way() {
    // Page scale 1, Fm0's /Matrix 2, a cm of 5 inside it: Fm1 is at 10.
    let outer = form(
        "/Matrix [2 0 0 2 0 0] /Resources << /XObject << /Fm1 6 0 R >> >>",
        b"q 5 0 0 5 0 0 cm /Fm1 Do Q",
    );
    let fm = "/XObject << /Fm0 5 0 R >>";
    let bytes = |inner: &str| doc("", b"/Fm0 Do", fm, outer.clone(), &[form(inner, &body())]);
    assert_eq!(rounding(bytes(""), 6), (0, None));
    // A /Matrix of 1/10 brings it back to scale 1.
    assert_eq!(rounding(bytes("/Matrix [.1 0 0 .1 0 0]"), 6), (20, None));
}

#[test]
fn a_form_that_draws_itself_is_walked_once() {
    assert_eq!(
        rounding(
            doc(
                "",
                b"/Fm0 Do",
                "/XObject << /Fm0 5 0 R >>",
                form("", &[body(), b"/Fm0 Do".to_vec()].concat()),
                &[]
            ),
            5
        )
        .1,
        None
    );
}

#[test]
fn a_tiling_pattern_rounds_at_its_matrix_scale() {
    let pattern = |matrix: &str| {
        stream(
            &format!(
                "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 200 200] \
                 /XStep 200 /YStep 200 /Resources << >> {matrix}"
            ),
            &body(),
        )
    };
    let res = "/Pattern << /P0 5 0 R >>";
    let p = |m| {
        rounding(
            doc(
                "",
                b"/Pattern cs /P0 scn 0 0 9 9 re f",
                res,
                pattern(m),
                &[],
            ),
            5,
        )
    };
    assert_eq!(p(""), (20, None));
    assert_eq!(p("/Matrix [10 0 0 10 0 0]"), (0, None));
    // The page's cm does not reach a pattern: its matrix is to default space.
    let cm = rounding(
        doc(
            "",
            b"10 0 0 10 0 0 cm /Pattern cs /P0 scn 0 0 9 9 re f",
            res,
            pattern(""),
            &[],
        ),
        5,
    );
    assert_eq!(cm, (20, None));
}

#[test]
fn a_form_named_beside_an_unreadable_stream_does_not_round() {
    // Stream 6 does not decode, so it may draw Fm0 at any scale.
    let page = "/Contents [4 0 R 6 0 R]";
    let broken = stream("/Filter /FlateDecode", b"not flate");
    let fm = "/XObject << /Fm0 5 0 R >>";
    let bytes = doc(page, b"/Fm0 Do", fm, form("", &body()), &[broken]);
    assert_eq!(rounding(bytes, 5), (0, Some(Unrounded::UnknownStart)));
}

#[test]
fn a_pattern_a_form_names_maps_to_the_form_space() {
    let pattern = stream(
        "/Type /Pattern /PatternType 1 /PaintType 1 /TilingType 1 /BBox [0 0 200 200] \
         /XStep 200 /YStep 200 /Resources << >>",
        &body(),
    );
    let outer = |matrix: &str| {
        form(
            &format!("{matrix} /Resources << /Pattern << /P0 6 0 R >> >>"),
            b"/Pattern cs /P0 scn 0 0 9 9 re f",
        )
    };
    let fm = "/XObject << /Fm0 5 0 R >>";
    let p = |m| {
        rounding(
            doc("", b"/Fm0 Do", fm, outer(m), std::slice::from_ref(&pattern)),
            6,
        )
    };
    assert_eq!(p(""), (20, None));
    assert_eq!(p("/Matrix [10 0 0 10 0 0]"), (0, None));
}
