//! [`srgb_to_cmyk_with`] at [`BlackGeneration::Medium`], fast enough to run
//! per pixel: the solver's output on a regular sRGB grid is baked into the
//! crate, and a colour is interpolated from its cell.
//!
//! The grids are generated data. Regenerate with
//! `cargo run --release -p pdfcer-color --example inverse_grid_gen` after any
//! change to the forward table or the solver; the tests check sampled nodes
//! against the solver.

use super::{BlackGeneration, CmykIntent, srgb_to_cmyk_with};

/// Nodes per sRGB axis. 17 puts a node every 16 8-bit counts, the usual
/// density for an ICC `B2A` table.
pub const INVERSE_GRID_NODES: usize = 17;
const N: usize = INVERSE_GRID_NODES;

/// Each node is C, M, Y, K as little-endian `u16` (0..=65535 → 0..1), nodes
/// row-major in (r, g, b) with b fastest.
static CALIBRATED: &[u8] = include_bytes!("inverse_grid_calibrated.bin");
static NEUTRAL_BLACK: &[u8] = include_bytes!("inverse_grid_neutral_black.bin");

/// sRGB (0..1 each) → the `DeviceCMYK` that [`super::cmyk_to_srgb_with`] at
/// `intent` draws as that colour, with [`BlackGeneration::Medium`] black.
///
/// Interpolated from a baked grid of [`srgb_to_cmyk_with`] results: exact at
/// the nodes (white is no ink), within a few 8-bit counts of the solver
/// between them. A NaN component reads as 0.
///
/// # Examples
///
/// ```
/// use pdfcer_color::color::{CmykIntent, cmyk_to_srgb, srgb_to_cmyk_fast};
///
/// let [c, m, y, k] = srgb_to_cmyk_fast(CmykIntent::Calibrated, 0.5, 0.5, 0.5);
/// // A neutral is separated into C, M and Y with a little black.
/// assert!(c > 0.3 && k < 0.2);
/// let back = cmyk_to_srgb(c, m, y, k);
/// assert!(back.iter().all(|v| (v - 0.5).abs() < 3.0 / 255.0));
/// ```
#[must_use]
pub fn srgb_to_cmyk_fast(intent: CmykIntent, r: f32, g: f32, b: f32) -> [f32; 4] {
    interpolate(grid(intent), [r, g, b])
}

fn grid(intent: CmykIntent) -> &'static [u8] {
    match intent {
        CmykIntent::Calibrated => CALIBRATED,
        CmykIntent::NeutralBlack => NEUTRAL_BLACK,
    }
}

/// The solver's output at every node, in the baked layout. Used by the
/// generator example; slow (seconds in a release build).
#[doc(hidden)]
#[must_use]
pub fn solve_inverse_grid(intent: CmykIntent) -> Vec<u8> {
    let mut out = Vec::with_capacity(N * N * N * 8);
    for r in 0..N {
        for g in 0..N {
            for b in 0..N {
                let [c, m, y, k] =
                    srgb_to_cmyk_with(intent, BlackGeneration::Medium, at(r), at(g), at(b)).cmyk;
                for v in [c, m, y, k] {
                    out.extend_from_slice(&quantize(v).to_le_bytes());
                }
            }
        }
    }
    out
}

fn at(i: usize) -> f32 {
    i as f32 / (N - 1) as f32
}

/// Ink under [`NO_INK`] is stored as none: the solver's black-range search
/// stops at its tolerance, so paper white comes back with a trace of K that
/// would mark white paint as ink.
fn quantize(v: f32) -> u16 {
    if v < NO_INK {
        return 0;
    }
    // In 0..=65535 after the clamp, so the cast is exact.
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}

const NO_INK: f32 = 1.0 / 1024.0;

fn node(grid: &[u8], i: usize) -> [f32; 4] {
    let mut out = [0.0; 4];
    for (ch, o) in out.iter_mut().enumerate() {
        let at = (i * 4 + ch) * 2;
        if let Some(&[lo, hi]) = grid
            .get(at..at + 2)
            .and_then(|s| <&[u8; 2]>::try_from(s).ok())
        {
            *o = f32::from(u16::from_le_bytes([lo, hi])) / 65535.0;
        }
    }
    out
}

/// The base node index of the cell holding `v`, and the fraction into it.
fn axis(v: f32) -> (usize, f32) {
    let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
    let t = v * (N - 1) as f32;
    let i = (t as usize).min(N - 2);
    (i, t - i as f32)
}

/// Trilinear interpolation over the cell's eight nodes.
fn interpolate(grid: &[u8], [r, g, b]: [f32; 3]) -> [f32; 4] {
    let (ir, fr) = axis(r);
    let (ig, fg) = axis(g);
    let (ib, fb) = axis(b);
    let at = |dr: usize, dg: usize, db: usize| node(grid, ((ir + dr) * N + ig + dg) * N + ib + db);
    let lerp = |a: [f32; 4], z: [f32; 4], t: f32| {
        let mut out = a;
        for (o, (x, y)) in out.iter_mut().zip(a.iter().zip(z.iter())) {
            *o = x + (y - x) * t;
        }
        out
    };
    let edge = |dr: usize, dg: usize| lerp(at(dr, dg, 0), at(dr, dg, 1), fb);
    let face = |dr: usize| lerp(edge(dr, 0), edge(dr, 1), fg);
    lerp(face(0), face(1), fr)
}

#[cfg(test)]
#[path = "inverse_grid_tests.rs"]
mod tests;
