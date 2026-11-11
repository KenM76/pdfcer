//! Bounce light from model-fixed lamps through a probe volume, the CPU path
//! of decision 224: a lattice of probes, each holding an octahedral map of
//! the irradiance arriving round it and of the distance to what it sees,
//! built a bounded step at a time so a caller can spread the cost over
//! frames.
//!
//! Units: irradiance is the multiplier the renderer adds to a surface's
//! direct shade term, so a white surface surrounded by surfaces lit to 1
//! receives 1. Only the lamps given are sources; ambient, camera lights and
//! emissive terms never are (decision 224: emissive carries surface colour
//! on the measured corpus).
//!
//! Results are identical at any thread count and any step budget: each probe
//! of a bounce is built from the previous bounce alone.

mod grid;
mod inside;
mod octa;
#[cfg(test)]
mod tests;
mod trace;

use crate::Bounds;
use crate::TriangleMesh;
use crate::camera::pick::{Hit, Ray, intersect_weighted};
use crate::camera::pick_index::PickIndex;
use crate::lighting::shade::Local;
use crate::render::corners;
use crate::render::parallel::{run_each, workers};
use grid::Grid;
use octa::TEXELS;
use trace::Published;

/// The most probes a volume holds.
pub(crate) const MAX_PROBES: usize = 4096;
/// Rays per probe per bounce.
pub(crate) const RAYS: usize = 64;
/// Bounces built; the volume is complete after this many.
pub(crate) const BOUNCES: usize = 3;
/// The cost charged for classifying one probe, in rays.
const CLASSIFY_COST: usize = 14;

/// A light source for the bounce.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Lamp {
    /// A directional light: `to_light` is the unit direction towards it,
    /// `colour` its colour times intensity.
    Far {
        to_light: [f64; 3],
        colour: [f64; 3],
    },
    /// A point or spot light.
    Near(Local),
}

/// What a ray meets: its diffuse colour and how much light passes through.
pub(crate) trait Surfaces: Sync {
    /// Linear diffuse reflectance at a hit with barycentric `weights`, each
    /// channel 0 to 1.
    fn albedo(&self, hit: &Hit, weights: [f64; 3]) -> [f64; 3];
    /// The share of light passing straight through, 0 (opaque) to 1 (a cut
    /// out hole, which rays pass as if it were not there).
    fn passes(&self, hit: &Hit, weights: [f64; 3]) -> f64;
}

/// Everything a bounce reads.
pub(crate) struct Scene<'a> {
    pub(crate) meshes: &'a [TriangleMesh],
    pub(crate) index: &'a PickIndex,
    pub(crate) surfaces: &'a dyn Surfaces,
    pub(crate) lamps: &'a [Lamp],
    /// The radiance of a ray that meets nothing.
    pub(crate) sky: [f64; 3],
}

impl Scene<'_> {
    /// The barycentric weights of `hit`, found by `ray`, on its triangle.
    fn weights(&self, hit: &Hit, ray: &Ray) -> [f64; 3] {
        let c = self
            .meshes
            .get(hit.mesh)
            .and_then(|m| corners(m, m.triangles.get(hit.triangle)?));
        c.and_then(|c| intersect_weighted(&c, ray))
            .map_or([1.0 / 3.0; 3], |(_, w)| w)
    }
}

/// One probe's maps.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Probe {
    irradiance: [[f32; 3]; TEXELS],
    /// Mean distance and mean squared distance, per texel.
    depth: [[f32; 2]; TEXELS],
}

impl Probe {
    /// No light, and everything `far` away.
    fn dark(far: f64) -> Probe {
        Probe {
            irradiance: [[0.0; 3]; TEXELS],
            depth: [[far as f32, (far * far) as f32]; TEXELS],
        }
    }

    fn irradiance_at(&self, n: [f64; 3]) -> [f64; 3] {
        grid::blend(&self.irradiance, n)
    }

    fn depth_at(&self, d: [f64; 3]) -> [f32; 2] {
        self.depth
            .get(octa::nearest(d))
            .copied()
            .unwrap_or([0.0; 2])
    }
}

/// Where a build has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stage {
    /// Classifying probes, from this one on.
    Classify(usize),
    /// Building bounce `bounce`, from this probe on.
    Trace {
        bounce: usize,
        next: usize,
    },
    Done,
}

/// The probe volume over one model, built by [`Self::step`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Volume {
    grid: Grid,
    closed: Vec<bool>,
    active: Vec<bool>,
    /// The last complete bounce; what [`Self::irradiance`] reads.
    current: Vec<Probe>,
    next: Vec<Probe>,
    stage: Stage,
}

impl Volume {
    /// An unbuilt volume over `meshes`; `None` when they have no extent.
    pub(crate) fn new(meshes: &[TriangleMesh]) -> Option<Volume> {
        Self::with_ceiling(meshes, MAX_PROBES)
    }

    /// [`Self::new`] with at most `max_probes` probes (8 or more).
    pub(crate) fn with_ceiling(meshes: &[TriangleMesh], max_probes: usize) -> Option<Volume> {
        let grid = Grid::around(&Bounds::of(meshes)?, max_probes.clamp(8, MAX_PROBES))?;
        let dark = Probe::dark(2.0 * grid.diagonal());
        Some(Volume {
            closed: inside::closed_meshes(meshes),
            active: vec![false; grid.len()],
            current: vec![dark.clone(); grid.len()],
            next: vec![dark; grid.len()],
            stage: Stage::Classify(0),
            grid,
        })
    }

    /// Whether every bounce is built.
    pub(crate) fn is_done(&self) -> bool {
        self.stage == Stage::Done
    }

    /// The bounces complete so far.
    pub(crate) fn bounces(&self) -> usize {
        match self.stage {
            Stage::Classify(_) => 0,
            Stage::Trace { bounce, .. } => bounce,
            Stage::Done => BOUNCES,
        }
    }

    /// The irradiance at `p` on the side its unit normal `n` faces, from the
    /// last complete bounce (zero before the first).
    pub(crate) fn irradiance(&self, p: [f64; 3], n: [f64; 3]) -> [f64; 3] {
        grid::gather(&self.grid, &self.current, &self.active, p, n)
    }

    /// Advances the build by about `budget` rays (at least one probe), on up
    /// to `threads` threads (0: every core); returns [`Self::is_done`].
    pub(crate) fn step(&mut self, scene: &Scene, budget: usize, threads: usize) -> bool {
        match self.stage {
            Stage::Classify(from) => self.classify(scene, from, budget, threads),
            Stage::Trace { bounce, next } => self.trace(scene, bounce, next, budget, threads),
            Stage::Done => {}
        }
        self.is_done()
    }

    /// Builds the whole volume.
    pub(crate) fn build(&mut self, scene: &Scene, threads: usize) {
        // Each step advances at least one probe, so this ends.
        while !self.step(scene, usize::MAX, threads) {}
    }

    fn classify(&mut self, scene: &Scene, from: usize, budget: usize, threads: usize) {
        let end = batch_end(from, self.grid.len(), budget / CLASSIFY_COST);
        let (grid, closed) = (&self.grid, &self.closed);
        let far = 2.0 * grid.diagonal();
        if let Some(slots) = self.active.get_mut(from..end) {
            for_chunks(slots, from, threads, &|i, slot: &mut bool| {
                *slot = inside::usable(
                    scene.index,
                    closed,
                    grid.position(i),
                    grid.min_spacing(),
                    far,
                );
            });
        }
        self.stage = if end >= self.grid.len() {
            Stage::Trace { bounce: 0, next: 0 }
        } else {
            Stage::Classify(end)
        };
    }

    fn trace(&mut self, scene: &Scene, bounce: usize, from: usize, budget: usize, threads: usize) {
        let end = batch_end(from, self.grid.len(), budget / RAYS);
        let last = Published {
            grid: &self.grid,
            probes: &self.current,
            active: &self.active,
        };
        if let (Some(slots), Some(active)) =
            (self.next.get_mut(from..end), self.active.get(from..end))
        {
            let work = |i: usize, slot: &mut Probe| {
                if active.get(i - from).copied().unwrap_or(false) {
                    *slot = trace::trace_probe(scene, &last, last.grid.position(i));
                }
            };
            for_chunks(slots, from, threads, &work);
        }
        self.stage = if end < self.grid.len() {
            Stage::Trace { bounce, next: end }
        } else {
            std::mem::swap(&mut self.current, &mut self.next);
            if bounce + 1 >= BOUNCES {
                Stage::Done
            } else {
                Stage::Trace {
                    bounce: bounce + 1,
                    next: 0,
                }
            }
        };
    }
}

/// The end of a batch of `count` items from `from`, at least one, at most
/// `len`.
fn batch_end(from: usize, len: usize, count: usize) -> usize {
    from.saturating_add(count.max(1)).min(len)
}

/// Runs `work(index, slot)` over `slots` (item `first` onwards), split into
/// one contiguous chunk per worker.
fn for_chunks<T: Send>(
    slots: &mut [T],
    first: usize,
    threads: usize,
    work: &(dyn Fn(usize, &mut T) + Sync),
) {
    let size = slots.len().div_ceil(workers(threads).max(1)).max(1);
    let chunks: Vec<(usize, &mut [T])> = slots
        .chunks_mut(size)
        .enumerate()
        .map(|(c, chunk)| (first + c * size, chunk))
        .collect();
    run_each(chunks, &|(start, chunk): (usize, &mut [T])| {
        for (k, slot) in chunk.iter_mut().enumerate() {
            work(start + k, slot);
        }
    });
}

/// `f` of each pair of axis values.
fn zip3<A, B, T>(a: [A; 3], b: [B; 3], f: impl Fn(A, B) -> T) -> [T; 3] {
    let ([a0, a1, a2], [b0, b1, b2]) = (a, b);
    [f(a0, b0), f(a1, b1), f(a2, b2)]
}

/// `count` directions spread evenly over the sphere (a spherical Fibonacci
/// set), the first `offset` by half the golden angle so a second set
/// interleaves with the first. The angle advances by rotation rather than
/// trigonometry, so every target gets the same bits.
pub(crate) fn sphere_directions(count: usize, offset: bool) -> Vec<[f64; 3]> {
    // cos and sin of the golden angle, pi * (3 - sqrt 5).
    const COS: f64 = -0.737_368_878_078_319_7;
    const SIN: f64 = 0.675_490_294_261_523_8;
    let (mut c, mut s) = if offset {
        (((1.0 + COS) * 0.5).sqrt(), ((1.0 - COS) * 0.5).sqrt())
    } else {
        (1.0, 0.0)
    };
    let n = count.max(1) as f64;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let z = 1.0 - (2.0 * i as f64 + 1.0) / n;
        let r = (1.0 - z * z).max(0.0).sqrt();
        out.push([r * c, r * s, z]);
        (c, s) = (c * COS - s * SIN, s * COS + c * SIN);
    }
    out
}
