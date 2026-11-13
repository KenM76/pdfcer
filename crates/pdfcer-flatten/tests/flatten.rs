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
        ..Paint::default()
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
                ..Overprint::default()
            }),
            ..Paint::default()
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

#[test]
fn spot_over_spot_overprint_regions_carry_both_planes() {
    // Plane 0 solid on the left square, plane 1 at 0.5 overprinted on a
    // square overlapping it: the overlap keeps plane 0 and gains plane 1.
    let g = page(vec![
        Element::Paint(Paint {
            shape: rect(0.0, 0.0, 4.0, 4.0),
            alpha: 1.0,
            spots: [1.0, 0.0, 0.0, 0.0],
            ..Paint::default()
        }),
        Element::Paint(Paint {
            shape: rect(2.0, 0.0, 4.0, 4.0),
            alpha: 1.0,
            overprint: Some(Overprint {
                tints: [None; 4],
                spots: [None, Some(0.5), None, None],
                ..Overprint::default()
            }),
            ..Paint::default()
        }),
    ]);
    let r = flatten(&g, Space::Cmyk, &NaiveInks, &Limits::default()).unwrap();
    assert_eq!(region(&r, &[0]).spots, [1.0, 0.0, 0.0, 0.0]);
    assert_eq!(region(&r, &[0, 1]).spots, [1.0, 0.5, 0.0, 0.0]);
    assert_eq!(region(&r, &[1]).spots, [0.0, 0.5, 0.0, 0.0]);
}

/// The area an even-odd region covers, by a 0.25-unit sample grid over
/// `0..w` squared: independent of how the contours are wound or grouped.
fn sampled_area(r: &Region, w: f64) -> f64 {
    let n = (w * 4.0) as usize;
    let mut hits = 0usize;
    for ix in 0..n {
        for iy in 0..n {
            let p = [(ix as f64 + 0.5) / 4.0, (iy as f64 + 0.5) / 4.0];
            let mut odd = false;
            for c in &r.contours {
                let mut prev = c[c.len() - 1];
                for &q in c {
                    if (q[1] > p[1]) != (prev[1] > p[1])
                        && p[0] < q[0] + (p[1] - q[1]) / (prev[1] - q[1]) * (prev[0] - q[0])
                    {
                        odd = !odd;
                    }
                    prev = q;
                }
            }
            hits += usize::from(odd);
        }
    }
    hits as f64 / 16.0
}

/// The signed area of a region's contours: equal to its even-odd area only
/// when holes wind against their outer and islands with it.
fn wound_area(r: &Region) -> f64 {
    r.contours
        .iter()
        .map(|c| {
            let mut s = 0.0;
            let mut prev = c[c.len() - 1];
            for &p in c {
                s += prev[0] * p[1] - p[0] * prev[1];
                prev = p;
            }
            s / 2.0
        })
        .sum::<f64>()
        .abs()
}

/// A background and 300 translucent glyphs, one of them a ring: more
/// paints than a 64-paint ceiling, cut without a single boolean because
/// each glyph lies inside the background clear of its edge.
#[test]
fn a_page_of_glyphs_over_a_background_flattens_and_keeps_every_hole() {
    let mut elements = vec![paint(
        rect(0.0, 0.0, 80.0, 80.0),
        [0.0, 0.0, 1.0, 0.0],
        1.0,
        Blend::Normal,
    )];
    for i in 0..300 {
        let (x, y) = (f64::from(i % 20) * 4.0 + 1.0, f64::from(i / 20) * 4.0 + 1.0);
        let mut glyph = rect(x, y, 2.0, 2.0);
        if i == 0 {
            glyph.contours.push(vec![
                [x + 0.5, y + 0.5],
                [x + 1.5, y + 0.5],
                [x + 1.5, y + 1.5],
                [x + 0.5, y + 1.5],
            ]);
            glyph.rule = FillRule::EvenOdd;
        }
        elements.push(paint(glyph, [1.0, 0.0, 0.0, 0.0], 0.5, Blend::Multiply));
    }
    let limits = Limits {
        max_work: 0,
        ..Limits::default()
    };
    let regions = flatten(&page(elements), Space::Rgb, &NaiveInks, &limits).unwrap();
    assert_eq!(regions.len(), 301);
    // The background less 300 glyphs of 4, plus the ring's hole of 1.
    let background = region(&regions, &[0]);
    assert!((sampled_area(background, 80.0) - 5201.0).abs() < 1e-9);
    assert!((wound_area(background) - 5201.0).abs() < 1e-9);
    let ring = region(&regions, &[0, 1]);
    assert!((sampled_area(ring, 80.0) - 3.0).abs() < 1e-9);
    assert!((wound_area(ring) - 3.0).abs() < 1e-9);
    assert!(close(
        region(&regions, &[0, 2]).colour,
        [0.0, 0.0, 0.5, 0.0]
    ));
}

#[test]
fn the_work_ceiling_refuses_overlapping_paints() {
    let elements = (0..3)
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
        max_work: 20,
        ..Limits::default()
    };
    assert_eq!(
        flatten(&page(elements), Space::Rgb, &NaiveInks, &limits),
        Err(FlattenError::TooComplex(Ceiling::Work))
    );
}
