//! Fuzz target: the Pass 9a vector object decomposition + hit-test +
//! centerline derivation (`pdfcer_core::vector`).
//!
//! Decision 011 Appendix A Pass 9a acceptance: *"Fuzz target over the
//! decomposition (malformed/degenerate paths, huge node counts,
//! unbalanced q/Q) 0 crashes."* This drives, over ANY input bytes:
//!
//! 1. `ContentStream::parse(data)` — the tokenizer (already fuzzed by
//!    `content_and_filters`, re-run here so the decomposer gets a real
//!    token stream to walk, including the degenerate shapes libFuzzer
//!    steers toward: `m` with no operands, a `c` with a missing operand,
//!    `re` with a `NaN`-shaped real, a million `l`s, unbalanced `q`/`Q`,
//!    a `cm` mid-path).
//! 2. `vector::decompose(&content, IDENTITY, resolver)` — the object
//!    decomposition, with a resolver that classifies `Do` names as image
//!    OR form (steered by the name's first byte) so both `Do` branches and
//!    the form-`/BBox` transform math are reachable without a `Document`.
//! 3. `vector::hit_test_point` / `hit_test_rect` — over query geometry
//!    derived from the object model's own bounds, so the point-in-polygon
//!    ray cast, the Bézier flattening, and the stroke-proximity distance
//!    run on whatever shapes the decomposition produced.
//! 4. `vector::page_candidates` + `PathObject::page_subpaths` — the
//!    centerline derivation and the user→page transform, over every
//!    object.
//!
//! Invariant (the crate's panic-free policy, ARCHITECTURE.md §10): for ANY
//! input, none of these panics, aborts, or runs unbounded — `MAX_NODES` /
//! `MAX_OBJECTS` cap the work and every access is checked. libFuzzer's
//! `-rss_limit_mb`/`-timeout` convert any regression into a reported crash.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::content::ContentStream;
use pdfcer_core::object::ObjId;
use pdfcer_core::vector::{
    Bounds, MarqueeMode, Matrix, Point, VectorObject, XObjectResolver, XObjectShape, decompose,
    hit_test_point, hit_test_rect, page_candidates,
};

/// A resolver that reaches BOTH `Do` branches without a `Document`: a name
/// whose first byte is odd is a form (with a bbox/matrix derived from the
/// name so the form-transform math sees varied input), else an image.
struct FuzzResolver;

impl XObjectResolver for FuzzResolver {
    fn classify(&self, name: &[u8]) -> Option<XObjectShape> {
        let first = *name.first()?;
        if first % 2 == 1 {
            let s = f64::from(first);
            Some(XObjectShape::Form {
                bbox: Bounds {
                    min: Point::new(0.0, 0.0),
                    max: Point::new(s, s + 1.0),
                },
                matrix: Matrix::new(1.0, 0.0, 0.0, 1.0, s, -s),
                // Derived from the name so the walk sees both `Some` and
                // `None` identities -- a `Do` naming a DIRECT stream carries
                // no object number, and a cycle guard keyed on the identity
                // has to cope with that rather than assume it is always there.
                object: (first % 3 == 0).then(|| ObjId::new(u32::from(first), 0)),
            })
        } else {
            // The sample count is derived from the name too, so the
            // `/Width`/`/Height` carrying path sees both `Some` and `None`
            // (§8.9.5 Table 89 requires both; a resolver may legitimately
            // fail to produce either).
            let pixel_size = (first % 4 == 0).then(|| (u32::from(first), u32::from(first) + 1));
            Some(XObjectShape::Image {
                pixel_size,
                object: (first % 3 == 0).then(|| ObjId::new(u32::from(first), 0)),
            })
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Ok(content) = ContentStream::parse(data.to_vec()) else {
        return;
    };

    let model = decompose(&content, Matrix::IDENTITY, &FuzzResolver);

    // Exercise the transform + centerline math over every object.
    for obj in &model.objects {
        let _ = obj.page_bbox();
        if let VectorObject::Path(p) = obj {
            let _ = p.page_subpaths();
            let _ = p.is_quad();
        }
    }
    let _ = page_candidates(&model);

    // Hit-test at the page's own extent corners + center, plus a couple of
    // deliberately hostile query points, at a small and a large tolerance.
    let bb = model.page_bbox();
    let center = if bb.is_empty() {
        Point::new(0.0, 0.0)
    } else {
        Point::new((bb.min.x + bb.max.x) / 2.0, (bb.min.y + bb.max.y) / 2.0)
    };
    for pt in [
        center,
        Point::new(0.0, 0.0),
        Point::new(f64::NAN, 0.0),
        Point::new(1e300, -1e300),
    ] {
        for tol in [0.0, 1.0, 1e9] {
            let _ = hit_test_point(&model, pt, tol);
        }
    }

    // Marquee over the page extent and a degenerate/huge rectangle.
    for rect in [
        bb,
        Bounds {
            min: Point::new(-1e9, -1e9),
            max: Point::new(1e9, 1e9),
        },
        Bounds {
            min: Point::new(0.0, 0.0),
            max: Point::new(0.0, 0.0),
        },
    ] {
        let _ = hit_test_rect(&model, rect, MarqueeMode::Enclosed);
        let _ = hit_test_rect(&model, rect, MarqueeMode::Touched);
    }
});
