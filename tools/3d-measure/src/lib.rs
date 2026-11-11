//! The ray-speed half of `measure-3d`, shared by the native binary and a
//! wasm32 build timed from node (`wasm/time.mjs`): wasm32-unknown-unknown has
//! no clock, so the host times [`trace`].

use pdfcer_3d::{AssembledModel, PickIndex, Ray};

/// Deterministic uniform numbers in [0, 1).
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

/// The model's axis-aligned bounds, or `None` when it has no vertices.
pub fn bounds(m: &AssembledModel) -> Option<([f64; 3], [f64; 3])> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in m.meshes.iter().flat_map(|t| t.positions.iter()) {
        for a in 0..3 {
            lo[a] = lo[a].min(p[a]);
            hi[a] = hi[a].max(p[a]);
        }
    }
    lo[0].is_finite().then_some((lo, hi))
}

/// Casts `n` closest-hit rays from random points on a sphere around the
/// bounds towards random points inside them; returns the hit count.
pub fn trace(index: &PickIndex, lo: [f64; 3], hi: [f64; 3], n: u64) -> u64 {
    let c = [0, 1, 2].map(|a| (lo[a] + hi[a]) / 2.0);
    let r = (0..3)
        .map(|a| (hi[a] - lo[a]).powi(2))
        .sum::<f64>()
        .sqrt()
        .max(1e-9);
    let mut g = Lcg(0x705);
    let mut hits = 0u64;
    for _ in 0..n {
        let (z, phi) = (2.0 * g.next() - 1.0, std::f64::consts::TAU * g.next());
        let s = (1.0 - z * z).sqrt();
        let o = [
            c[0] + r * s * phi.cos(),
            c[1] + r * s * phi.sin(),
            c[2] + r * z,
        ];
        let q = [0, 1, 2].map(|a| lo[a] + (hi[a] - lo[a]) * g.next());
        let d = [0, 1, 2].map(|a| q[a] - o[a]);
        let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        let ray = Ray {
            origin: o,
            direction: d.map(|v| v / len),
            min_distance: 0.0,
        };
        hits += u64::from(index.pick(&ray).is_some());
    }
    hits
}

#[cfg(target_arch = "wasm32")]
mod wasm {
    use super::*;

    struct Loaded {
        index: PickIndex,
        lo: [f64; 3],
        hi: [f64; 3],
    }
    static mut LOADED: Option<Loaded> = None;

    /// A buffer of `len` bytes for the host to copy a `.prc` file into.
    #[unsafe(no_mangle)]
    pub extern "C" fn alloc(len: usize) -> *mut u8 {
        Box::leak(vec![0u8; len].into_boxed_slice()).as_mut_ptr()
    }

    /// Assembles the file at `ptr..ptr+len` and builds its index; 1 on success.
    #[unsafe(no_mangle)]
    pub extern "C" fn load(ptr: *const u8, len: usize) -> u32 {
        // SAFETY: the host wrote `len` bytes at `ptr`, a buffer from `alloc`.
        let data = unsafe { std::slice::from_raw_parts(ptr, len) };
        let Ok(m) = pdfcer_3d::assemble(data) else {
            return 0;
        };
        let Some((lo, hi)) = bounds(&m) else { return 0 };
        let index = PickIndex::new(&m.meshes);
        // SAFETY: single-threaded wasm; the host calls these in sequence.
        unsafe { LOADED = Some(Loaded { index, lo, hi }) };
        1
    }

    /// [`trace`] over the loaded model.
    #[unsafe(no_mangle)]
    pub extern "C" fn rays(n: u32) -> u32 {
        // SAFETY: as in `load`.
        #[allow(static_mut_refs)] // single-threaded wasm, see above
        let Some(l) = (unsafe { LOADED.as_ref() }) else {
            return 0;
        };
        trace(&l.index, l.lo, l.hi, u64::from(n)) as u32
    }
}
