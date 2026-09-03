//! Fuzz target: the Pass 12.M1 snapping engine query
//! (`pdfcer_core::vector::snap_candidates` + the H/V constraint).
//!
//! Decision 011 Appendix A Pass 12.M1 is a read-only interaction service with
//! the crate's panic-free posture (ARCHITECTURE.md §10). This drives, over ANY
//! input bytes:
//!
//! 1. `ContentStream::parse(data)` → `vector::decompose` — a real object model
//!    over whatever degenerate/hostile geometry libFuzzer steers toward.
//! 2. `snap_candidates(query, config, model)` — the snap query, at several
//!    hostile query points (a page-interior point derived from the model's own
//!    bounds, the origin, a non-finite point, a huge point), each with
//!    intersection snapping OFF (default) and ON (the neighbourhood-bounded,
//!    capped scan — the Z4 path), and with axes/grid on. The invariant: it
//!    never panics, never runs unbounded (`MAX_NEIGHBOURHOOD_SEGMENTS` /
//!    `MAX_CANDIDATES` cap the work), and the returned list is well-formed
//!    (sorted by priority, then distance — re-checked here).
//! 3. `constrained_second_point` / `measured_length` over the same points —
//!    the H/V projection math, panic-free for any finite/non-finite input.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::content::ContentStream;
use pdfcer_core::vector::{
    AxisConstraint, Matrix, NoXObjects, Point, SnapConfig, constrained_second_point, decompose,
    measured_length, snap_candidates,
};

fuzz_target!(|data: &[u8]| {
    let Ok(content) = ContentStream::parse(data.to_vec()) else {
        return;
    };
    let model = decompose(&content, Matrix::IDENTITY, &NoXObjects);

    // A page-interior query derived from the model's own extent, plus hostile
    // points.
    let bb = model.page_bbox();
    let center = if bb.is_empty() {
        Point::new(0.0, 0.0)
    } else {
        Point::new((bb.min.x + bb.max.x) / 2.0, (bb.min.y + bb.max.y) / 2.0)
    };
    let queries = [
        center,
        Point::new(0.0, 0.0),
        Point::new(f64::NAN, 0.0),
        Point::new(1e300, -1e300),
    ];

    for q in queries {
        for intersections in [false, true] {
            let cfg = SnapConfig::new(12.0)
                .with_intersections(intersections)
                .with_grid(Some(10.0));
            let cands = snap_candidates(q, &cfg, &model);
            // The result must be priority-then-distance ordered (the contract
            // the GUI cycles through): re-check monotonic non-decreasing key.
            for w in cands.windows(2) {
                let (a, b) = (w[0], w[1]);
                let pa = a.kind.priority();
                let pb = b.kind.priority();
                assert!(pa <= pb, "candidates must be priority-ordered");
                if pa == pb {
                    let da = a.point.distance(q);
                    let db = b.point.distance(q);
                    // Within a priority band, distance is non-decreasing (a
                    // stray non-finite is ordered deterministically by
                    // total_cmp, so this holds for finite queries; skip the
                    // NaN-query case where distances are all NaN).
                    if da.is_finite() && db.is_finite() {
                        assert!(da <= db + 1e-6, "ties must be nearest-first");
                    }
                }
            }
        }
    }

    // H/V constraint math over the hostile points — panic-free for any input.
    for a in queries {
        for b in queries {
            for c in [
                AxisConstraint::Aligned,
                AxisConstraint::Horizontal,
                AxisConstraint::Vertical,
            ] {
                let _ = constrained_second_point(a, b, c);
                let _ = measured_length(a, b, c);
            }
        }
    }
});
