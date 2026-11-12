//! Integration tests for `pdfcer-flatten`, through its public API only.
// Tests fail by panicking.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use pdfcer_flatten::{
    Blend, Ceiling, Element, FillRule, FlattenError, Group, Limits, NaiveInks, Overprint, Paint,
    Region, Shape, Space, flatten,
};

fn rect(x: f64, y: f64, w: f64, h: f64) -> Shape {
    Shape {
        contours: vec![vec![[x, y], [x + w, y], [x + w, y + h], [x, y + h]]],
        rule: FillRule::NonZero,
    }
}

fn paint(shape: Shape, colour: [f32; 4], alpha: f32, blend: Blend) -> Element {
    Element::Paint(Paint {
        shape,
        colour,
        alpha,
        blend,
        overprint: None,
    })
}

fn page(elements: Vec<Element>) -> Group {
    Group {
        elements,
        isolated: true,
        knockout: false,
        alpha: 1.0,
        blend: Blend::Normal,
    }
}

fn region<'a>(regions: &'a [Region], paints: &[usize]) -> &'a Region {
    regions
        .iter()
        .find(|r| r.paints == paints)
        .unwrap_or_else(|| panic!("no region covered by {paints:?}"))
}

fn close(a: [f32; 4], b: [f32; 4]) -> bool {
    a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4)
}

#[test]
fn a_multiply_over_a_fill_is_the_product_where_they_overlap() {
    let g = page(vec![
        paint(
            rect(0.0, 0.0, 10.0, 10.0),
            [1.0, 0.5, 0.0, 0.0],
            1.0,
            Blend::Normal,
        ),
        paint(
            rect(5.0, 0.0, 10.0, 10.0),
            [0.5, 0.5, 1.0, 0.0],
            1.0,
            Blend::Multiply,
        ),
    ]);
    let r = flatten(&g, Space::Rgb, &NaiveInks, &Limits::default()).unwrap();
    assert_eq!(r.len(), 3);
    assert!(close(region(&r, &[0, 1]).colour, [0.5, 0.25, 0.0, 0.0]));
    assert!(close(region(&r, &[1]).colour, [0.5, 0.5, 1.0, 0.0]));
}

#[test]
fn a_knockout_group_with_translucent_members_knocks_out_by_shape() {
    let ko = Group {
        elements: vec![
            paint(
                rect(0.0, 0.0, 10.0, 10.0),
                [0.0, 0.0, 1.0, 0.0],
                0.6,
                Blend::Normal,
            ),
            paint(
                rect(5.0, 0.0, 10.0, 10.0),
                [1.0, 0.0, 0.0, 0.0],
                0.6,
                Blend::Normal,
            ),
        ],
        isolated: true,
        knockout: true,
        alpha: 1.0,
        blend: Blend::Normal,
    };
    let r = flatten(
        &page(vec![Element::Group(ko)]),
        Space::Rgb,
        &NaiveInks,
        &Limits::default(),
    )
    .unwrap();
    let both = region(&r, &[0, 1]);
    assert!(close(both.colour, [1.0, 0.0, 0.0, 0.0]));
    assert!((both.alpha - 0.6).abs() < 1e-5);
}

#[test]
fn an_additive_overprint_through_naive_inks_adds_ink() {
    // Cyan overprinted onto yellow with the other tints kept: green.
    let g = page(vec![
        paint(
            rect(0.0, 0.0, 10.0, 10.0),
            [1.0, 1.0, 0.0, 0.0],
            1.0,
            Blend::Normal,
        ),
        Element::Paint(Paint {
            shape: rect(0.0, 0.0, 10.0, 10.0),
            colour: [0.0, 1.0, 1.0, 0.0],
            alpha: 1.0,
            blend: Blend::Normal,
            overprint: Some(Overprint {
                tints: [Some(1.0), None, None, None],
                model: 0,
            }),
        }),
    ]);
    let r = flatten(&g, Space::Rgb, &NaiveInks, &Limits::default()).unwrap();
    assert_eq!(r.len(), 1);
    assert!(close(r[0].colour, [0.0, 1.0, 0.0, 0.0]));
}

#[test]
fn the_paint_ceiling_refuses_instead_of_running_long() {
    let elements = (0..5)
        .map(|i| {
            paint(
                rect(f64::from(i), 0.0, 3.0, 3.0),
                [0.0; 4],
                0.5,
                Blend::Normal,
            )
        })
        .collect();
    let limits = Limits {
        max_paints: 4,
        ..Limits::default()
    };
    assert_eq!(
        flatten(&page(elements), Space::Rgb, &NaiveInks, &limits),
        Err(FlattenError::TooComplex(Ceiling::Paints))
    );
}

#[test]
fn the_page_alpha_scales_every_region() {
    let mut g = page(vec![paint(
        rect(0.0, 0.0, 4.0, 4.0),
        [0.2, 0.4, 0.6, 0.0],
        1.0,
        Blend::Normal,
    )]);
    g.alpha = 0.25;
    let r = flatten(&g, Space::Rgb, &NaiveInks, &Limits::default()).unwrap();
    assert!((r[0].alpha - 0.25).abs() < 1e-6);
}

#[test]
fn a_cmyk_hue_blend_keeps_the_backdrop_black() {
    let g = page(vec![
        paint(
            rect(0.0, 0.0, 4.0, 4.0),
            [0.1, 0.5, 0.2, 0.3],
            1.0,
            Blend::Normal,
        ),
        paint(
            rect(0.0, 0.0, 4.0, 4.0),
            [0.9, 0.1, 0.4, 0.8],
            1.0,
            Blend::Hue,
        ),
    ]);
    let r = flatten(&g, Space::Cmyk, &NaiveInks, &Limits::default()).unwrap();
    assert!((r[0].colour[3] - 0.3).abs() < 1e-6);
}
