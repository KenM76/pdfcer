//! Fuzz target: the Pass 9c-min vector-edit surgery planners
//! (`pdfcer_core::vector::edit`).
//!
//! Decision 011 Appendix A Pass 9c-min acceptance: *"Fuzz over operand
//! rewriting (degenerate coords, huge operands) 0 crashes."* Over ANY input
//! bytes this drives, for the object model the tokenizer + decomposer
//! produce:
//!
//! 1. `plan_delete` on EVERY object (path / text / image) — the pure
//!    byte-span removal + splice;
//! 2. `plan_move` on every PATH object with a spread of page-space deltas,
//!    including degenerate ones (`NaN`, `±∞`, `1e300`) — exercises the CTM
//!    linear-inverse, the per-operator operand rewrite, the malformed-arity
//!    refusal, and `emit_number` over hostile magnitudes;
//! 3. `plan_move_node` on every path object across a range of node indices
//!    (past the anchor count, so the out-of-range / rectangle / implicit
//!    refusals are all reached) with degenerate target points — exercises
//!    the anchor enumeration (the decompose-mirroring subpath bookkeeping),
//!    the affine inverse, and the single-operator re-emit;
//! 4. `anchor_count` on every path object;
//! 5. `plan_delete_node` across the same overrunning node range (Pass 36.1) —
//!    exercises the subpath-membership walk, the two-anchor guard, the
//!    rectangle/implicit/clip refusals, and the first-anchor path where the
//!    FOLLOWING operator's operands are read and re-emitted as the new `m`;
//! 6. `plan_delete_subpath` and `plan_move_subpath` across an overrunning
//!    subpath range, and `plan_move_handle` across an overrunning node range
//!    on both sides.
//!
//! ## Items 5 and 6 were added together, and 6 is the older gap
//!
//! Pass 36.1 owed this target a `plan_delete_node` arm (ARCHITECTURE.md
//! §10.2). Adding it exposed that the SUBPATH planners had never been fuzzed
//! at all: `plan_delete_subpath` shipped in Pass 25.2, `plan_move_subpath` in
//! Pass 28.0 and `plan_move_handle` in Pass 30.1, and this target still
//! listed only the three Pass 9c-min planners it was written for. Every one
//! of them does index arithmetic and byte splicing over attacker-controlled
//! token ranges, which is precisely what §10.2 asks to be driven. They are
//! added here rather than filed as a follow-up because the loop that reaches
//! them already existed and skipping them would leave the same hole one more
//! Pass.
//!
//! Invariant (ARCHITECTURE.md §10 panic-free policy): for ANY input, none of
//! these panics, aborts, or runs unbounded — the planners either return a
//! `Vec<u8>` or a by-name `VectorEditError`, every access is checked, and
//! `MAX_NODES`/`MAX_OBJECTS` cap the decomposition upstream.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_core::content::ContentStream;
use pdfcer_core::vector::{
    Handle, Matrix, NoXObjects, Point, VectorObject, anchor_count, decompose, plan_delete,
    plan_delete_node, plan_delete_subpath, plan_move, plan_move_handle, plan_move_node,
    plan_move_subpath,
};

/// A spread of page-space deltas, from tame to hostile.
const DELTAS: [(f64, f64); 5] = [
    (0.0, 0.0),
    (10.0, -7.5),
    (1e300, -1e300),
    (f64::NAN, 0.0),
    (f64::INFINITY, f64::NEG_INFINITY),
];

fuzz_target!(|data: &[u8]| {
    let Ok(content) = ContentStream::parse(data.to_vec()) else {
        return;
    };
    let model = decompose(&content, Matrix::IDENTITY, &NoXObjects);

    for obj in &model.objects {
        // Delete works on any object kind.
        let _ = plan_delete(&content, obj);

        let VectorObject::Path(path) = obj else {
            continue;
        };

        // Move with every delta (degenerate coords + huge operands).
        for (dx, dy) in DELTAS {
            let _ = plan_move(&content, path, dx, dy);
        }

        // Node drag across a range that overruns the anchor count, with
        // degenerate targets, so every refusal branch is reachable.
        let n = anchor_count(&content, path);
        for node in 0..n.saturating_add(2) {
            for pt in [
                Point::new(0.0, 0.0),
                Point::new(1e300, -1e300),
                Point::new(f64::NAN, f64::INFINITY),
            ] {
                let _ = plan_move_node(&content, path, node, pt);
            }
            // Pass 36.1. Overrunning the count by two reaches the
            // out-of-range refusal; the in-range indices reach the
            // two-anchor guard, the rectangle and implicit-start refusals,
            // and — at index 0 of any subpath — the branch that reads the
            // FOLLOWING operator's operands and re-emits them as a new `m`,
            // which is the one place this planner indexes into a neighbour.
            let _ = plan_delete_node(&content, path, node);

            // Handle drag, both sides (Pass 30.1 — never fuzzed until now).
            for side in [Handle::Incoming, Handle::Outgoing] {
                let _ = plan_move_handle(&content, path, node, side, Point::new(1e300, f64::NAN));
            }
        }

        // Subpath planners across an overrunning subpath range (Passes 25.2
        // and 28.0 — never fuzzed until now). `+2` so the out-of-range
        // refusal is reached even for an object with no subpaths at all.
        for sp in 0..path.subpaths.len().saturating_add(2) {
            let _ = plan_delete_subpath(&content, path, sp);
            for (dx, dy) in DELTAS {
                let _ = plan_move_subpath(&content, path, sp, dx, dy);
            }
        }
    }
});
