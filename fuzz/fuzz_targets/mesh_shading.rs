//! Fuzz target: the mesh-shading stream decoder
//! (`pdfcer_render::mesh::parse`; `Pass 125.0`, ISO 32000-1 §8.7.4.5.5–.8).
//!
//! # Why this decoder needs its own target rather than riding `load_document`
//!
//! `ARCHITECTURE.md` §10.2 asks for a fuzz target on any new
//! untrusted-input parser, and this one is unusually exposed even by the
//! standards of a PDF parser:
//!
//! 1. **It is a bit-level parser with file-controlled field widths.**
//!    `/BitsPerCoordinate`, `/BitsPerComponent` and `/BitsPerFlag` come out
//!    of the dictionary, and the *record size* is computed from them
//!    together with the colour space's component count. A byte-oriented
//!    parser mis-reads; a bit-oriented one whose stride is wrong
//!    desynchronises and keeps producing plausible garbage until the stream
//!    ends.
//! 2. **Its output size is not bounded by its input size.** Two-bit
//!    coordinates and a one-bit component make a type 5 vertex record eight
//!    bits long, so a megabyte of stream is a million vertices and two
//!    million triangles — before subdivision. `MAX_RECORDS` and
//!    `MAX_TRIANGLES` exist for that, and a fuzz target is how one learns
//!    whether they are actually reached.
//! 3. **Types 6 and 7 have no fixed stride at all.** The record length
//!    depends on an edge flag read from the stream, so a malformed flag
//!    changes how many bytes the *next* record starts at. That is the
//!    classic shape for a length-confusion bug.
//! 4. **The patch inheritance chain reads back into previously-parsed
//!    state.** A continued patch copies four control points and two colours
//!    out of its predecessor, and the chain can be arbitrarily long.
//!
//! # The invariants asserted
//!
//! * **No panic, on any input, under any parameter combination.** The
//!    crate's panic-free policy (X5/X6). Every index into the fuzz bytes
//!    and every array index derived from an edge flag is in this file's
//!    blast radius.
//! * **The declared ceilings hold.** A successful parse must not report
//!   more primitives than `MAX_RECORDS` allows, and the returned geometry
//!   must be finite in size. A ceiling that a fuzzer can walk past is not a
//!   ceiling.
//! * **A refusal is a refusal.** `Err` must carry a reason string, because
//!   project rule 4 makes every refusal a disclosure and a nameless one
//!   cannot be disclosed.
//!
//! # What it deliberately does not do
//!
//! It does not *paint*. Rasterisation is bounded by the destination pixmap
//! and takes a transform, and fuzzing it would mostly explore
//! floating-point transforms rather than the parser. The parser is where
//! file-controlled lengths live, and lengths are what break.

#![no_main]

use libfuzzer_sys::fuzz_target;
use pdfcer_render::color::{ColorDiagnostics, ColorSpace};
use pdfcer_render::mesh::{MeshData, ParseInput, parse};

/// Widths the standard allows, so that most of the fuzzer's budget goes on
/// stream content rather than on the four-fifths of the byte space that is
/// rejected in the first ten lines of `parse`.
///
/// The illegal widths are still reached — the selector below can produce
/// one — but they are not the default draw.
const COORD_BITS: [u32; 8] = [1, 2, 4, 8, 12, 16, 24, 32];
const COMP_BITS: [u32; 6] = [1, 2, 4, 8, 12, 16];
const FLAG_BITS: [u32; 3] = [2, 4, 8];

fuzz_target!(|data: &[u8]| {
    // The first six bytes steer the dictionary; the rest is the stream.
    // Six is enough to reach every branch and small enough that a
    // one-record stream is still expressible.
    if data.len() < 6 {
        return;
    }
    let (head, stream) = data.split_at(6);

    let shading_type = 4 + (head[0] & 3);
    let bpco = COORD_BITS[usize::from(head[1]) % COORD_BITS.len()];
    let bpcp = COMP_BITS[usize::from(head[2]) % COMP_BITS.len()];
    let bpf = FLAG_BITS[usize::from(head[3]) % FLAG_BITS.len()];
    // Every fourth draw uses an ILLEGAL width, so the validation path is
    // exercised too rather than only the happy one.
    let bpco = if head[1] & 0xC0 == 0xC0 { 7 } else { bpco };

    let space = match head[4] & 3 {
        0 => ColorSpace::DeviceGray,
        1 => ColorSpace::DeviceRgb,
        _ => ColorSpace::DeviceCmyk,
    };
    let parametric = head[4] & 4 != 0;
    let patch_padding = if head[4] & 8 == 0 {
        pdfcer_core::settings::MeshPatchPadding::PerRecord
    } else {
        pdfcer_core::settings::MeshPatchPadding::None
    };

    // `/Decode` is REQUIRED and its length is checked against the component
    // count, so a fuzzer that only ever supplied a correct one would never
    // reach the arity branch. Half the draws are deliberately short.
    let ncomp = if parametric { 1 } else { space.components() };
    let full: Vec<f32> = (0..4 + 2 * ncomp)
        .map(|i| if i % 2 == 0 { 0.0 } else { 100.0 })
        .collect();
    let decode: &[f32] = if head[5] & 1 == 0 {
        &full
    } else {
        &full[..full.len().saturating_sub(2)]
    };

    let input = ParseInput {
        shading_type,
        data: stream,
        decode: Some(decode),
        bits_per_coordinate: Some(bpco),
        bits_per_component: Some(bpcp),
        bits_per_flag: Some(bpf),
        vertices_per_row: Some(u32::from(head[5] >> 1).max(1)),
        space: &space,
        // No page, no bridges: the fuzzer exercises the stream decoder, and
        // the colour route is the fixture tests' business.
        bridges: &pdfcer_render::icc::ColorBridges::none(),
        parametric,
        patch_padding,
        intent: pdfcer_core::settings::CmykIntent::default(),
    };

    let mut diag = ColorDiagnostics::default();
    match parse(&input, &mut diag) {
        Ok(mesh) => {
            // The ceiling, checked rather than trusted. `MAX_RECORDS` is
            // private, so this asserts the property it exists to give:
            // geometry proportional to the stream, not to the widths the
            // dictionary happens to name.
            let primitives = mesh.primitive_count();
            assert!(
                primitives <= 4_000_000,
                "{primitives} primitives from a {}-byte stream exceeds the \
                 declared ceiling",
                stream.len()
            );
            assert_eq!(primitives > 0, true, "an Ok parse must carry geometry");
            match &mesh.data {
                MeshData::Triangles(t) => assert_eq!(t.len(), primitives),
                MeshData::Patches(p) => assert_eq!(p.len(), primitives),
            }
            // A type 5 mesh must report the row count it inferred, because
            // the dictionary does not carry it and an operator who cannot
            // see the inference cannot check it (project rule 4).
            if shading_type == 5 {
                assert!(
                    mesh.rows_inferred.is_some(),
                    "a type 5 parse must disclose the row count it inferred"
                );
            }
        }
        Err(reason) => {
            assert!(
                !reason.reason().is_empty(),
                "every refusal must be nameable; rule 4 forbids a silent one"
            );
        }
    }
});
