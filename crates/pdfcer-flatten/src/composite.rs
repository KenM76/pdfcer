//! §11.4.8 for one region: the tree walked with a fixed set of covering
//! paints, every covered paint at shape 1 and every other at shape 0.

use crate::blend::blend;
use crate::{
    Blend, Ceiling, Colour, Element, FlattenError, Group, Inks, Limits, Paint, Shape, Space,
};

/// A colour and its alpha, not premultiplied.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Px {
    pub(crate) c: Colour,
    pub(crate) a: f32,
}

const CLEAR: Px = Px {
    c: [0.0; 4],
    a: 0.0,
};

/// `Union(b, s) = b + s − b·s` (§11.3.6).
fn union(b: f32, s: f32) -> f32 {
    b + s - b * s
}

fn lerp(a: Colour, b: Colour, t: f32) -> Colour {
    let mut out = a;
    for (o, &v) in out.iter_mut().zip(&b) {
        *o += t * (v - *o);
    }
    out
}

/// The paints' shapes in depth-first order, with the depth ceiling and the
/// unsupported combinations checked on the way.
pub(crate) fn collect<'a>(
    g: &'a Group,
    out: &mut Vec<&'a Shape>,
    depth: usize,
    limits: &Limits,
) -> Result<(), FlattenError> {
    if depth > limits.max_depth {
        return Err(FlattenError::TooComplex(Ceiling::Depth));
    }
    for e in &g.elements {
        match e {
            Element::Paint(p) => {
                if g.knockout && p.overprint.is_some() {
                    return Err(FlattenError::Unsupported(
                        "overprint inside a knockout group",
                    ));
                }
                out.push(&p.shape);
                if out.len() > limits.max_paints {
                    return Err(FlattenError::TooComplex(Ceiling::Paints));
                }
            }
            Element::Group(child) => collect(child, out, depth + 1, limits)?,
        }
    }
    Ok(())
}

fn paints_in(g: &Group) -> usize {
    g.elements
        .iter()
        .map(|e| match e {
            Element::Paint(_) => 1,
            Element::Group(c) => paints_in(c),
        })
        .sum()
}

struct Ctx<'a> {
    space: Space,
    inks: &'a dyn Inks,
    covered: &'a [bool],
}

impl Ctx<'_> {
    fn covers(&self, i: usize) -> bool {
        self.covered.get(i).copied().unwrap_or(false)
    }

    /// §11.4.8's ordinary element step: `α_i = Union(α_b, α_s)` and the
    /// colour weighted by `α_s / α_i`, the blend mixed in by `α_b`.
    fn element(&self, b: Px, s: Px, mode: Blend) -> Px {
        let ab = b.a.clamp(0.0, 1.0);
        let a_s = s.a.clamp(0.0, 1.0);
        let ai = union(ab, a_s);
        if ai <= 0.0 {
            return CLEAR;
        }
        let mixed = if mode == Blend::Normal || ab <= 0.0 {
            s.c
        } else {
            lerp(s.c, blend(mode, self.space, b.c, s.c), ab)
        };
        Px {
            c: lerp(b.c, mixed, a_s / ai),
            a: ai,
        }
    }

    /// An overprinting paint: the named components replaced, the rest kept,
    /// mixed into the backdrop by alpha. The blend mode is not used.
    fn overprint(&self, b: Px, p: &Paint, tints: [Option<f32>; 4], model: usize) -> Px {
        let t = p.alpha.clamp(0.0, 1.0);
        let base = if b.a <= 0.0 { self.space.paper() } else { b.c };
        let rule = |under: [f32; 4]| {
            let mut out = under;
            for (o, tint) in out.iter_mut().zip(tints) {
                if let Some(v) = tint {
                    *o = v.clamp(0.0, 1.0);
                }
            }
            out
        };
        let target = match self.space {
            Space::Cmyk => rule(base),
            Space::Rgb => {
                let under = self.inks.to_cmyk(model, [base[0], base[1], base[2]]);
                let out = rule(under);
                if out == under && b.a >= 1.0 {
                    return b;
                }
                let [r, g, bl] = self.inks.to_rgb(model, out);
                [r, g, bl, 0.0]
            }
        };
        Px {
            c: lerp(base, target, t),
            a: b.a + (1.0 - b.a) * t,
        }
    }

    fn paint(&self, b: Px, p: &Paint) -> Px {
        match p.overprint {
            Some(o) => self.overprint(b, p, o.tints, o.model),
            None => self.element(
                b,
                Px {
                    c: p.colour,
                    a: p.alpha,
                },
                p.blend,
            ),
        }
    }

    /// The members of a non-knockout group painted over `start`, paints
    /// numbered from `base`.
    fn run(&self, g: &Group, start: Px, base: usize) -> Px {
        let mut state = start;
        let mut i = base;
        for e in &g.elements {
            match e {
                Element::Paint(p) => {
                    if self.covers(i) {
                        state = self.paint(state, p);
                    }
                    i += 1;
                }
                Element::Group(child) => {
                    let res = self.group_result(child, state, i);
                    let src = Px {
                        c: res.c,
                        a: res.a * child.alpha.clamp(0.0, 1.0),
                    };
                    state = self.element(state, src, child.blend);
                    i += paints_in(child);
                }
            }
        }
        state
    }

    /// A group's result `⟨C, α_gn⟩` over `backdrop`, after backdrop removal.
    /// Inside a knockout group `backdrop` is that group's initial backdrop
    /// (§11.4.6 NOTE 6).
    fn group_result(&self, g: &Group, backdrop: Px, base: usize) -> Px {
        if g.knockout {
            let initial = if g.isolated { CLEAR } else { backdrop };
            return self.knockout(g, initial, base);
        }
        let iso = self.run(g, CLEAR, base);
        if g.isolated || backdrop.a <= 0.0 || iso.a <= 0.0 {
            return iso;
        }
        let over = self.run(g, backdrop, base);
        Px {
            c: remove_backdrop(over.c, backdrop, iso.a),
            a: iso.a,
        }
    }

    /// A knockout group over `initial` (§11.4.6): every member composites
    /// against `initial`, erasing what earlier members left by its shape.
    fn knockout(&self, g: &Group, initial: Px, base: usize) -> Px {
        let mut accum = initial;
        let mut ag = 0.0_f32;
        let mut i = base;
        for e in &g.elements {
            let (src, shape, mode) = match e {
                Element::Paint(p) => {
                    let hit = self.covers(i);
                    i += 1;
                    if !hit {
                        continue;
                    }
                    (
                        Px {
                            c: p.colour,
                            a: p.alpha,
                        },
                        1.0,
                        p.blend,
                    )
                }
                Element::Group(child) => {
                    let n = paints_in(child);
                    let res = self.group_result(child, initial, i);
                    i += n;
                    if res.a <= 0.0 {
                        continue;
                    }
                    let q = child.alpha.clamp(0.0, 1.0);
                    (
                        Px {
                            c: res.c,
                            a: res.a * q,
                        },
                        res.a,
                        child.blend,
                    )
                }
            };
            (accum, ag) = self.knockout_step(initial, accum, src, shape, ag, mode);
        }
        Px {
            c: remove_backdrop(accum.c, initial, ag),
            a: ag,
        }
    }

    /// §11.4.8's knockout element step; `α_gb` is identically zero.
    fn knockout_step(
        &self,
        initial: Px,
        accum: Px,
        s: Px,
        shape: f32,
        ag_prev: f32,
        mode: Blend,
    ) -> (Px, f32) {
        let a0 = initial.a.clamp(0.0, 1.0);
        let a_s = s.a.clamp(0.0, 1.0);
        let f = shape.clamp(a_s, 1.0);
        let ag = ((1.0 - f) * ag_prev + a_s).clamp(0.0, 1.0);
        let ai = union(a0, ag);
        if ai <= 0.0 {
            return (CLEAR, 0.0);
        }
        let mixed = if a0 <= 0.0 || mode == Blend::Normal {
            s.c
        } else {
            lerp(s.c, blend(mode, self.space, initial.c, s.c), a0)
        };
        let k = (f - a_s) * a0;
        let mut c = [0.0; 4];
        for (((o, &m), &ini), &acc) in c.iter_mut().zip(&mixed).zip(&initial.c).zip(&accum.c) {
            *o = ((1.0 - f) * accum.a * acc + a_s * m + k * ini) / ai;
        }
        (Px { c, a: ai }, ag)
    }
}

/// §11.4.4's backdrop removal: the group colour with the backdrop's
/// contribution taken back out, given the group's own alpha `α_gn`.
fn remove_backdrop(over: Colour, backdrop: Px, agn: f32) -> Colour {
    let a0 = backdrop.a.clamp(0.0, 1.0);
    let agn = agn.clamp(0.0, 1.0);
    if a0 <= 0.0 || agn <= 0.0 {
        return over;
    }
    let k = a0 / agn - a0;
    let mut out = over;
    for (o, &b) in out.iter_mut().zip(&backdrop.c) {
        *o += k * (*o - b);
    }
    out
}

/// The page group's result where exactly the `covered` paints mark, scaled
/// by the root's alpha. Colours are clamped to `0..=1`.
pub(crate) fn evaluate(
    root: &Group,
    space: Space,
    inks: &dyn Inks,
    covered: &[bool],
) -> Result<Px, FlattenError> {
    let ctx = Ctx {
        space,
        inks,
        covered,
    };
    let res = if root.knockout {
        ctx.knockout(root, CLEAR, 0)
    } else {
        ctx.run(root, CLEAR, 0)
    };
    let mut c = res.c;
    for v in c.iter_mut().take(space.components()) {
        *v = v.clamp(0.0, 1.0);
    }
    Ok(Px {
        c,
        a: (res.a * root.alpha.clamp(0.0, 1.0)).clamp(0.0, 1.0),
    })
}

#[cfg(test)]
mod tests {
    // Tests fail by panicking.
    #![allow(clippy::indexing_slicing)]
    use super::*;
    use crate::{NaiveInks, Overprint};

    fn paint(colour: Colour, alpha: f32) -> Element {
        Element::Paint(Paint {
            colour,
            alpha,
            ..Paint::default()
        })
    }

    fn eval(g: &Group, space: Space, covered: &[bool]) -> Px {
        evaluate(g, space, &NaiveInks, covered).unwrap_or(CLEAR)
    }

    fn close(a: Colour, b: Colour) -> bool {
        a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-5)
    }

    fn page(elements: Vec<Element>) -> Group {
        Group {
            elements,
            isolated: true,
            alpha: 1.0,
            ..Group::default()
        }
    }

    #[test]
    fn half_red_over_blue_is_purple() {
        let g = page(vec![
            paint([0.0, 0.0, 1.0, 0.0], 1.0),
            paint([1.0, 0.0, 0.0, 0.0], 0.5),
        ]);
        let px = eval(&g, Space::Rgb, &[true, true]);
        assert!(close(px.c, [0.5, 0.0, 0.5, 0.0]));
        assert!((px.a - 1.0).abs() < 1e-6);
        let alone = eval(&g, Space::Rgb, &[false, true]);
        assert!((alone.a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_knockout_member_erases_the_one_beneath_by_its_shape() {
        // Two half-alpha paints in a knockout group: the upper one sees only
        // the (clear) initial backdrop, so its region is the upper colour at
        // alpha 0.5, not their 0.75 union.
        let ko = Group {
            elements: vec![
                paint([0.0, 0.0, 1.0, 0.0], 0.5),
                paint([1.0, 0.0, 0.0, 0.0], 0.5),
            ],
            isolated: true,
            knockout: true,
            alpha: 1.0,
            ..Group::default()
        };
        let g = page(vec![Element::Group(ko)]);
        let px = eval(&g, Space::Rgb, &[true, true]);
        assert!(close(px.c, [1.0, 0.0, 0.0, 0.0]));
        assert!((px.a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn a_non_isolated_group_blends_with_the_page_but_keeps_its_alpha() {
        let inner = Group {
            elements: vec![Element::Paint(Paint {
                colour: [0.5, 0.5, 0.5, 0.0],
                alpha: 1.0,
                blend: Blend::Multiply,
                ..Paint::default()
            })],
            isolated: false,
            alpha: 1.0,
            ..Group::default()
        };
        let g = page(vec![
            paint([1.0, 0.0, 0.0, 0.0], 1.0),
            Element::Group(inner.clone()),
        ]);
        let px = eval(&g, Space::Rgb, &[true, true]);
        assert!(close(px.c, [0.5, 0.0, 0.0, 0.0]));
        let iso = Group {
            isolated: true,
            ..inner
        };
        let g = page(vec![paint([1.0, 0.0, 0.0, 0.0], 1.0), Element::Group(iso)]);
        // Isolated, the multiply ran over nothing: plain grey on top.
        assert!(close(
            eval(&g, Space::Rgb, &[true, true]).c,
            [0.5, 0.5, 0.5, 0.0]
        ));
    }

    #[test]
    fn cmyk_overprint_keeps_the_backdrop_components_it_does_not_name() {
        let g = page(vec![
            paint([0.0, 0.0, 0.8, 0.0], 1.0),
            Element::Paint(Paint {
                colour: [0.6, 0.0, 0.0, 0.0],
                alpha: 1.0,
                overprint: Some(Overprint {
                    tints: [Some(0.6), None, None, None],
                    model: 0,
                }),
                ..Paint::default()
            }),
        ]);
        let px = eval(&g, Space::Cmyk, &[true, true]);
        assert!(close(px.c, [0.6, 0.0, 0.8, 0.0]));
    }

    #[test]
    fn overprint_in_a_knockout_group_is_refused() {
        let ko = Group {
            elements: vec![Element::Paint(Paint {
                overprint: Some(Overprint::default()),
                ..Paint::default()
            })],
            knockout: true,
            ..Group::default()
        };
        let mut out = Vec::new();
        let r = collect(
            &page(vec![Element::Group(ko)]),
            &mut out,
            0,
            &Limits::default(),
        );
        assert!(matches!(r, Err(FlattenError::Unsupported(_))));
    }
}
