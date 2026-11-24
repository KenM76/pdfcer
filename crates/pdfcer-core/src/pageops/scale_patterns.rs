//! Placing a scaled page's patterns.
//!
//! A pattern's `/Matrix` maps pattern space to the default coordinate
//! space of the page whose resources name it, not to the CTM in force when
//! it is painted (ISO 32000-2 §8.7.2, §8.7.3.1). The `cm` that
//! [`super::scale`] wraps the content in therefore moves the shapes but not
//! their fills, so each page-level pattern gets a copy whose `/Matrix` is
//! the old one followed by the placement. Patterns inside form XObjects map
//! to the form's space, which the wrapper already scales, and need nothing.

use crate::object::{Dict, Name, ObjId, Object};

use super::scale::PagePlacement;

/// A pattern's `/Matrix` (Tables 75 and 76): identity when absent, `None`
/// when present but not six numbers.
pub(crate) fn pattern_matrix(
    resolve: &dyn Fn(&Object) -> Object,
    pattern: &Dict,
) -> Option<[f64; 6]> {
    let Some(m) = pattern.get(b"Matrix") else {
        return Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);
    };
    let arr = resolve(m);
    let items = arr.as_array()?;
    let mut out = [0.0; 6];
    if items.len() != 6 {
        return None;
    }
    for (slot, item) in out.iter_mut().zip(items) {
        *slot = resolve(item).as_number().filter(|v| v.is_finite())?;
    }
    Some(out)
}

/// `m` followed by the placement — `[a·s b·s c·s d·s e·s+dx f·s+dy]` — so a
/// point of pattern space lands where the content under it went.
#[must_use]
pub(crate) fn placed_matrix(m: [f64; 6], p: &PagePlacement) -> [f64; 6] {
    let s = p.scale;
    let [a, b, c, d, e, f] = m;
    [a * s, b * s, c * s, d * s, p.x(e), p.y(f)]
}

/// A copy of the pattern object (a tiling stream or a shading dictionary)
/// with its `/Matrix` placed; `None` when it is not a pattern or its matrix
/// cannot be read, so the caller keeps the original.
pub(crate) fn placed_pattern(
    resolve: &dyn Fn(&Object) -> Object,
    pattern: &Object,
    p: &PagePlacement,
) -> Option<Object> {
    let mut out = pattern.clone();
    let dict = match &mut out {
        Object::Dict(d) => d,
        Object::Stream(s) => &mut s.dict,
        _ => return None,
    };
    let m = placed_matrix(pattern_matrix(resolve, dict)?, p);
    dict.insert(
        Name::from(b"Matrix"),
        Object::Array(m.iter().copied().map(Object::Real).collect()),
    );
    Some(out)
}

/// A scaled page's patterns, planned before any object is allocated.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct PagePatterns {
    /// The page's resources (own or inherited), resolved, to be written
    /// onto the page with `/Pattern` naming the copies.
    pub resources: Dict,
    /// The resolved `/Pattern` subdictionary.
    pub patterns: Dict,
    /// Per placed pattern: its name, the object it was read from (`None`
    /// when held directly) and the placed copy.
    pub copies: Vec<(Name, Option<ObjId>, Object)>,
    /// Patterns whose `/Matrix` could not be read, left as they were.
    pub unreadable: usize,
}

/// Plan the placed copies for one page whose resources are `resources`.
/// `None` when the page names no pattern.
pub(crate) fn plan(
    resolve: &dyn Fn(&Object) -> Object,
    resources: &Object,
    p: &PagePlacement,
) -> Option<PagePatterns> {
    let resources = resolve(resources).as_dict()?.clone();
    let patterns = resolve(resources.get(b"Pattern")?).as_dict()?.clone();
    let mut copies = Vec::new();
    let mut unreadable = 0;
    for (name, entry) in patterns.iter() {
        match placed_pattern(resolve, &resolve(entry), p) {
            Some(copy) => copies.push((name.clone(), entry.as_reference(), copy)),
            None => unreadable += 1,
        }
    }
    (!copies.is_empty() || unreadable > 0).then_some(PagePatterns {
        resources,
        patterns,
        copies,
        unreadable,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;
    use crate::page_tree::Rect;

    fn place() -> PagePlacement {
        PagePlacement {
            target: Rect::from_corners(0.0, 0.0, 306.0, 396.0),
            scale: 0.5,
            offset_x: 10.0,
            offset_y: 20.0,
            orientation_flipped: false,
        }
    }

    fn id(o: &Object) -> Object {
        o.clone()
    }

    fn with(m: Object) -> Dict {
        let mut d = Dict::new();
        d.insert(Name::from(b"PatternType"), Object::Integer(2));
        d.insert(Name::from(b"Matrix"), m);
        d
    }

    #[test]
    fn a_pattern_point_follows_the_content() {
        let m = [2.0, 0.0, 0.0, 3.0, 5.0, 7.0];
        let placed = placed_matrix(m, &place());
        let (u, v) = (4.0, -1.0);
        let apply = |m: [f64; 6]| (u * m[0] + v * m[2] + m[4], u * m[1] + v * m[3] + m[5]);
        let (x, y) = apply(m);
        assert_eq!(apply(placed), (place().x(x), place().y(y)));
    }

    #[test]
    fn an_absent_matrix_is_identity() {
        let mut d = Dict::new();
        d.insert(Name::from(b"PatternType"), Object::Integer(2));
        let out = placed_pattern(&id, &Object::Dict(d), &place()).unwrap();
        let m = out
            .as_dict()
            .unwrap()
            .get(b"Matrix")
            .unwrap()
            .as_array()
            .unwrap()
            .to_vec();
        assert_eq!(m, [0.5, 0.0, 0.0, 0.5, 10.0, 20.0].map(Object::Real));
    }

    #[test]
    fn an_unreadable_matrix_is_left_alone() {
        for bad in [
            Object::Integer(1),
            Object::Array(vec![Object::Integer(1); 5]),
            Object::Array(vec![Object::Name(Name::from(b"x")); 6]),
            Object::Array(vec![Object::Real(f64::NAN); 6]),
        ] {
            assert_eq!(
                placed_pattern(&id, &Object::Dict(with(bad)), &place()),
                None
            );
        }
        assert_eq!(placed_pattern(&id, &Object::Integer(3), &place()), None);
    }
}
