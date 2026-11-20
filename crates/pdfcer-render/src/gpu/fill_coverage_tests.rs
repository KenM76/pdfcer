#![allow(clippy::indexing_slicing, clippy::unwrap_used)] // Tests fail loudly by design.

use super::pixel;
use crate::gpu::fill_edges::build;
use tiny_skia::{FillRule, Mask, Path, PathBuilder, Transform};

std::thread_local! {
    static COVERED: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
    static SPLITS: core::cell::Cell<usize> = const { core::cell::Cell::new(0) };
}

/// A small generator, so the paths are the same every run.
struct Lcg(u64);

impl Lcg {
    fn unit(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }

    fn coord(&mut self, span: f32, grid: f32) -> f32 {
        let v = self.unit() * (span + 40.0) - 20.0;
        if grid > 0.0 {
            (v / grid).round() * grid
        } else {
            v
        }
    }
}

/// Every pixel of `path` filled with `rule` into `w` × `h`, as the model
/// gives it and as tiny-skia writes it: the mismatches.
fn mismatches(path: &Path, rule: FillRule, w: u32, h: u32) -> Vec<(i32, i32, u8, u8)> {
    mismatches_aa(path, rule, (w, h), true)
}

fn mismatches_aa(
    path: &Path,
    rule: FillRule,
    (w, h): (u32, u32),
    aa: bool,
) -> Vec<(i32, i32, u8, u8)> {
    let mut mask = Mask::new(w, h).unwrap();
    mask.fill_path(path, rule, aa, Transform::identity());
    let edges = build(path, rule, (w, h), aa);
    let mut out = Vec::new();
    COVERED.with(|c| c.set(c.get() + mask.data().iter().filter(|v| **v != 0).count()));
    if let Some(e) = &edges {
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                for k in 0..4 {
                    if super::split(e, x << 2, (y << 2) + k) {
                        SPLITS.with(|c| c.set(c.get() + 1));
                    }
                }
            }
        }
    }
    let packed = edges
        .clone()
        .map(|e| crate::gpu::reference::packed_coverage(e, w as i32, h as i32));
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let at = (y as u32 * w + x as u32) as usize;
            let want = mask.data()[at];
            let got = edges.as_ref().map_or(0, |e| pixel(e, x, y));
            let binned = packed.as_ref().map_or(0, |p| p[at]);
            if got != want || binned != want {
                out.push((x, y, binned, want));
            }
        }
    }
    out
}

fn random_path(rng: &mut Lcg, w: f32, h: f32, grid: f32) -> Option<Path> {
    let mut pb = PathBuilder::new();
    let subpaths = 1 + (rng.unit() * 3.0) as usize;
    for _ in 0..subpaths {
        let n = 3 + (rng.unit() * 8.0) as usize;
        pb.move_to(rng.coord(w, grid), rng.coord(h, grid));
        for _ in 1..n {
            pb.line_to(rng.coord(w, grid), rng.coord(h, grid));
        }
        if rng.unit() < 0.5 {
            pb.close();
        }
    }
    pb.finish()
}

fn check(seed: u64, grid: f32, count: usize) {
    check_aa(seed, grid, count, true);
}

fn check_aa(seed: u64, grid: f32, count: usize, aa: bool) {
    let (w, h) = (61, 47);
    let mut rng = Lcg(seed);
    let mut bad = 0;
    for n in 0..count {
        let Some(path) = random_path(&mut rng, w as f32, h as f32, grid) else {
            continue;
        };
        for rule in [FillRule::Winding, FillRule::EvenOdd] {
            let m = mismatches_aa(&path, rule, (w, h), aa);
            if !m.is_empty() {
                if bad < 5 {
                    eprintln!(
                        "path {n} {rule:?}: {} pixels, first {:?}",
                        m.len(),
                        &m[..m.len().min(4)]
                    );
                }
                bad += 1;
            }
        }
    }
    assert_eq!(bad, 0, "{bad} fills differ");
    assert!(COVERED.with(core::cell::Cell::get) > 100_000);
}

#[test]
fn random_polygons_match_tiny_skia() {
    check(1, 0.0, 400);
}

#[test]
fn polygons_on_a_quarter_pixel_grid_match_tiny_skia() {
    check(2, 0.25, 400);
}

#[test]
fn polygons_on_a_whole_pixel_grid_match_tiny_skia() {
    check(3, 1.0, 400);
}

#[test]
fn aliased_polygons_match_tiny_skia() {
    check_aa(5, 0.0, 400, false);
    check_aa(6, 0.5, 400, false);
    check_aa(7, 1.0, 400, false);
}

#[test]
fn abutting_rectangles_match_tiny_skia() {
    let mut rng = Lcg(4);
    let mut bad = 0;
    for _ in 0..300 {
        let mut pb = PathBuilder::new();
        let (y0, y1) = (rng.coord(30.0, 0.125), rng.coord(30.0, 0.125));
        let mut x = rng.coord(40.0, 0.125);
        for _ in 0..3 {
            let x2 = x + 0.25 + rng.unit() * 6.0;
            let x2 = (x2 * 8.0).round() / 8.0;
            let (a, b) = if rng.unit() < 0.5 { (y0, y1) } else { (y1, y0) };
            pb.move_to(x, a);
            pb.line_to(x2, a);
            pb.line_to(x2, b);
            pb.line_to(x, b);
            pb.close();
            x = x2;
        }
        let Some(path) = pb.finish() else { continue };
        for rule in [FillRule::Winding, FillRule::EvenOdd] {
            let m = mismatches(&path, rule, 61, 47);
            if !m.is_empty() {
                eprintln!("{rule:?}: {:?}", &m[..m.len().min(4)]);
                bad += 1;
            }
        }
    }
    assert_eq!(bad, 0, "{bad} fills differ");
    let splits = SPLITS.with(core::cell::Cell::get);
    assert!(splits > 100, "{splits}");
}
