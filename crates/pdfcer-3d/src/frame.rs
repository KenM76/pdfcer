//! Drawing one frame's triangles into the image, on one thread or many.
//!
//! The draw order is fixed: opaque meshes, then translucent ones, each in
//! mesh order. With several workers the image is cut into row bands; a
//! first pass bins every triangle (by its projected rows) into the bands it
//! may touch, then each band draws its triangles in that same order. Every
//! pixel therefore sees the same writes in the same order at any worker
//! count, so the image is byte-identical (decision 080).

use crate::TriangleMesh;
use crate::parallel;
use crate::raster::{Paint, Target};
use crate::render::Scene;

/// Bands per worker: more than one so a model filling only the middle rows
/// still spreads across every worker (bands are dealt out in turn).
const BANDS_PER_WORKER: usize = 4;
/// The fewest rows a band holds.
const MIN_BAND_ROWS: usize = 8;

/// Every triangle of `meshes` a frame draws, in draw order (opaque meshes,
/// then translucent ones, each in mesh order): mesh, triangle index, paint.
/// A mesh with no paint or a transparent one is not drawn.
pub(crate) fn drawn<'a, 'p>(
    meshes: &'a [TriangleMesh],
    paints: &'a [Paint<'p>],
    opaque: &'a [bool],
) -> impl Iterator<Item = (&'a TriangleMesh, usize, Paint<'p>)> + 'a {
    [true, false].into_iter().flat_map(move |pass| {
        meshes.iter().enumerate().flat_map(move |(i, mesh)| {
            let paint = paints.get(i).copied();
            let drawn = paint
                .filter(|p| p.colour[3] != 0 && opaque.get(i).copied().unwrap_or(false) == pass);
            drawn
                .into_iter()
                .flat_map(move |p| (0..mesh.triangles.len()).map(move |t| (mesh, t, p)))
        })
    })
}

/// What a frame draws: meshes, their paints, and which are opaque.
pub(crate) struct Frame<'a, 'p> {
    pub(crate) scene: &'a Scene<'a>,
    pub(crate) meshes: &'a [TriangleMesh],
    pub(crate) paints: &'a [Paint<'p>],
    pub(crate) opaque: &'a [bool],
}

impl Frame<'_, '_> {
    /// Draws into the whole image (`rgba`, `depth`, `width` × `height`)
    /// with `workers` threads.
    pub(crate) fn draw(&self, image: (&mut [u8], &mut [f64]), size: (u32, u32), workers: usize) {
        let (rgba, depth) = image;
        let (width, height) = size;
        let row = width as usize;
        let bands = (workers * BANDS_PER_WORKER).min((height as usize).div_ceil(MIN_BAND_ROWS));
        if workers <= 1 || bands <= 1 {
            let mut target = Target {
                width,
                height,
                top: 0,
                rgba,
                depth,
            };
            for (mesh, t, paint) in self.sequence() {
                self.scene.draw(&mut target, mesh, t, paint);
            }
            return;
        }
        let rows = (height as usize).div_ceil(bands);
        let seq: Vec<_> = self.sequence().collect();
        let bins = self.bin(&seq, workers, rows, bands);
        let mut per_worker: Vec<Vec<(usize, Target)>> = (0..workers).map(|_| Vec::new()).collect();
        for (k, (rgba, depth)) in rgba
            .chunks_mut(rows * row * 4)
            .zip(depth.chunks_mut(rows * row))
            .enumerate()
        {
            let target = Target {
                width,
                height,
                top: (k * rows) as u32,
                rgba,
                depth,
            };
            if let Some(slot) = per_worker.get_mut(k % workers) {
                slot.push((k, target));
            }
        }
        parallel::run_each(per_worker, &|targets| {
            for (k, mut target) in targets {
                for chunk in &bins {
                    for &i in chunk.get(k).into_iter().flatten() {
                        if let Some(&(mesh, t, paint)) = seq.get(i as usize) {
                            self.scene.draw(&mut target, mesh, t, paint);
                        }
                    }
                }
            }
        });
    }

    /// Every triangle drawn, in draw order: mesh, triangle index, paint.
    fn sequence(&self) -> impl Iterator<Item = (&TriangleMesh, usize, Paint<'_>)> + '_ {
        drawn(self.meshes, self.paints, self.opaque)
    }

    /// For each of `workers` contiguous chunks of `seq`, the `seq` indices
    /// each band may need, in order.
    fn bin(
        &self,
        seq: &[(&TriangleMesh, usize, Paint<'_>)],
        workers: usize,
        rows: usize,
        bands: usize,
    ) -> Vec<Vec<Vec<u32>>> {
        let per = seq.len().div_ceil(workers).max(1);
        let mut out: Vec<Vec<Vec<u32>>> = (0..seq.len().div_ceil(per))
            .map(|_| vec![Vec::new(); bands])
            .collect();
        let jobs: Vec<_> = out.iter_mut().enumerate().collect();
        parallel::run_each(jobs, &|(c, bins)| {
            let start = c * per;
            for (i, &(mesh, t, _)) in seq.iter().enumerate().skip(start).take(per) {
                let Some((y0, y1)) = self.scene.rows(mesh, t) else {
                    continue;
                };
                let first = (y0.floor().max(0.0) as usize) / rows;
                let last = ((y1.ceil().max(0.0) as usize) / rows).min(bands - 1);
                for band in bins.iter_mut().take(last + 1).skip(first) {
                    band.push(i as u32);
                }
            }
        });
        out
    }
}
