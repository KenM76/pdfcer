//! # A tiny cache for clip masks that get rebuilt identically
//!
//! One page of a real CAD drawing applies **24,128 clips** and builds
//! **40 distinct masks** to do it — 603 applications per distinct path,
//! and **99.83% of applications are repeats**. One single path accounts
//! for **97.3%** of every clip operation on the sheet, and 37 of the 40
//! distinct paths are applied exactly once.
//!
//! Before this cache, each of those 24,128 applications paid the full
//! `Mask::new` + `fill_path` + multiply — **362 µs each, 8.4 s of a
//! 10 s render, 86% of the total.** The work was identical every time.
//!
//! ## What is cached is the FINAL mask, not the built one
//!
//! The census measured two identities, and the second is what made this
//! worth building:
//!
//! * the **build key** — geometry, fill rule, CTM, mask size — which
//!   determines the freshly-filled mask *before* intersection. A cache
//!   on this alone serves `Mask::new` + `fill_path`, 259 µs of the 362.
//! * the **full key** — the build key **plus which clip is being
//!   intersected with** — which determines the mask that actually gets
//!   stored in the graphics state.
//!
//! On the reference sheet both come to **40 distinct entries**. Every
//! re-application happens under the *same* incoming clip, so the final
//! mask is identical too, and a hit can hand back the existing
//! [`Arc`] outright: no allocation, no `fill_path`, **and no
//! multiply** — the whole 362 µs rather than 259 µs of it.
//!
//! That equality is a property of this document, not of PDF. A file
//! where the same path is clipped under different accumulated clips
//! would have more full keys than build keys and would get fewer hits.
//! It cannot get *wrong* hits: the incoming clip is part of the key.
//!
//! ## Why the incoming clip is held by strong reference
//!
//! Identity of the incoming clip is **pointer identity**, which is
//! stricter than comparing mask contents — two masks with identical
//! bytes at different addresses miss. That direction is deliberate: it
//! can only lose hits, never invent one.
//!
//! But a raw pointer alone would be unsound. If the incoming `Arc` were
//! dropped, its allocation could be reused by a later mask, and a stale
//! entry would match a pointer that now means something else — the ABA
//! problem, and here it would return **the wrong clip and paint a
//! silently wrong picture**. So each entry keeps a strong `Arc` to the
//! incoming mask, which pins that address for as long as the entry can
//! be matched against it.
//!
//! ## Bounded to [`CAPACITY`], and why so small
//!
//! Two entries serve 99.8% of applications on the reference page, so
//! four is already double the measured need. The cost of being generous
//! is not small: a mask is one byte per device pixel, so at 1× it is
//! ~1 MB and at 2× ~4 MB, and each entry can pin **two** of them (its
//! result and its incoming). Four entries is therefore up to ~8 MB at
//! 1× and ~32 MB at 2×.
//!
//! Caching all 40 distinct masks would be **38.3 MiB at 1×** for a
//! measured benefit over four entries of about 0.2% — 37 of them are
//! used once and would never be hit again.
//!
//! Eviction is **least-recently-used**. The access pattern is one
//! dominant path interleaved with a stream of singletons, and LRU keeps
//! the dominant one resident because it is touched constantly; a
//! singleton entering the cache evicts another singleton, not the hot
//! entry. (Least-frequently-used would also work here. LRU is chosen
//! because it degrades more gracefully on a document whose hot path
//! *changes* partway down the page, which this policy has not been
//! measured against.)
//!
//! ## Lifetime
//!
//! Owned by the [`Interpreter`](crate::interpret) running one content
//! stream, so it dies with the render that created it. It is
//! deliberately **not** global and not a `thread_local`: rendering now
//! happens on a worker thread (Pass 44.0), and a cache outliving one
//! page would be both a leak and a correctness hazard — masks are
//! keyed partly on device size, and nothing outside one render should
//! be able to observe another render's masks.

use std::sync::Arc;

use tiny_skia::{FillRule, Mask, Path, Transform};

/// How many built masks to keep.
///
/// Two serve 99.8% of applications on the reference CAD sheet; this is
/// double that. See the module docs for the memory this implies — a
/// mask is one byte per device pixel and an entry can pin two.
pub(crate) const CAPACITY: usize = 4;

/// A clip's device-space bounding box, `(left, top, right, bottom)` —
/// the same tuple [`GraphicsState::clip_bbox`](crate::gstate) carries,
/// named here so the cache's signatures stay readable.
pub(crate) type ClipBbox = (f32, f32, f32, f32);

/// What a cache hit yields: the already-intersected mask, and the
/// bounding box that accompanies it. Always travel together — see
/// [`ClipCache::get`] for why the bbox is stored rather than
/// recomputed.
pub(crate) type CachedClip = (Arc<Mask>, Option<ClipBbox>);

/// One cached clip: the mask that resulted, and everything that
/// determined it.
struct Entry {
    /// Hash of geometry + fill rule + CTM + mask dimensions.
    build_key: u64,
    /// The clip this was intersected *with*, held by strong reference
    /// so its address cannot be recycled underneath a stale match.
    /// See the module docs on ABA.
    incoming: Option<Arc<Mask>>,
    /// The mask to hand back, already intersected.
    result: Arc<Mask>,
    /// The clip bounding box that accompanied `result` when it was
    /// built. Cached rather than recomputed because it is derived from
    /// the same inputs — see [`ClipCache::get`].
    result_bbox: Option<ClipBbox>,
    /// Monotonic access stamp, for LRU.
    stamp: u64,
}

/// A bounded most-recently-used cache of intersected clip masks.
pub(crate) struct ClipCache {
    entries: Vec<Entry>,
    clock: u64,
}

impl ClipCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            clock: 0,
        }
    }

    /// Hash the inputs that determine the mask **before** intersection.
    ///
    /// Deliberately the same tuple the census measured, so the hit rate
    /// this cache achieves is comparable with the repetition that
    /// justified building it.
    ///
    /// Coordinates hash **bit-exactly** (`to_bits`). Two points
    /// differing in the last ulp produce different coverage, so treating
    /// them as equal would return a mask that is subtly wrong — and
    /// would do it in the direction that *raises* the hit rate, making
    /// the cache look better precisely when it is broken.
    pub(crate) fn build_key(
        path: &Path,
        rule: FillRule,
        ctm: Transform,
        mask_w: u32,
        mask_h: u32,
    ) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for v in path.verbs() {
            (*v as u8).hash(&mut h);
        }
        for p in path.points() {
            p.x.to_bits().hash(&mut h);
            p.y.to_bits().hash(&mut h);
        }
        matches!(rule, FillRule::EvenOdd).hash(&mut h);
        for f in [ctm.sx, ctm.kx, ctm.ky, ctm.sy, ctm.tx, ctm.ty] {
            f.to_bits().hash(&mut h);
        }
        mask_w.hash(&mut h);
        mask_h.hash(&mut h);
        h.finish()
    }

    /// Look up an already-intersected mask.
    ///
    /// Returns the mask **and** the clip bbox that went with it. Both
    /// are returned together because both are functions of the same
    /// inputs: `clip` and `clip_bbox` are only ever written as a pair
    /// (`intersect_clip`'s tail, and the degenerate-path branch that
    /// clips everything out), and `q`/`Q` copy the graphics state
    /// wholesale — so a given `Arc<Mask>` is always accompanied by the
    /// same bbox, and recomputing it on a hit would be arithmetic to
    /// arrive at a value already known.
    ///
    /// A linear scan is right at [`CAPACITY`] entries: four pointer
    /// comparisons against a 362 µs miss is not a cost worth a hash
    /// map's machinery, and the scan keeps `Arc::ptr_eq` available
    /// without needing the pointer to be part of a hash.
    pub(crate) fn get(
        &mut self,
        build_key: u64,
        incoming: Option<&Arc<Mask>>,
    ) -> Option<CachedClip> {
        self.clock += 1;
        let clock = self.clock;
        for e in &mut self.entries {
            if e.build_key != build_key {
                continue;
            }
            let same_incoming = match (&e.incoming, incoming) {
                (None, None) => true,
                (Some(a), Some(b)) => Arc::ptr_eq(a, b),
                _ => false,
            };
            if same_incoming {
                e.stamp = clock;
                crate::profile::note_clip_cache(true);
                return Some((Arc::clone(&e.result), e.result_bbox));
            }
        }
        crate::profile::note_clip_cache(false);
        None
    }

    /// Record a mask that was just built, evicting the least recently
    /// used entry if the cache is full.
    pub(crate) fn insert(
        &mut self,
        build_key: u64,
        incoming: Option<Arc<Mask>>,
        result: Arc<Mask>,
        result_bbox: Option<ClipBbox>,
    ) {
        self.clock += 1;
        let entry = Entry {
            build_key,
            incoming,
            result,
            result_bbox,
            stamp: self.clock,
        };
        if self.entries.len() < CAPACITY {
            self.entries.push(entry);
            return;
        }
        // Evict LRU. `entries` is at capacity and capacity is non-zero,
        // so a minimum exists.
        if let Some(victim) = self.entries.iter_mut().min_by_key(|e| e.stamp) {
            *victim = entry;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiny_skia::PathBuilder;

    fn square(x: f32) -> Path {
        let mut b = PathBuilder::new();
        b.move_to(x, 0.0);
        b.line_to(x + 10.0, 0.0);
        b.line_to(x + 10.0, 10.0);
        b.close();
        b.finish().expect("triangle is a valid path")
    }

    fn mask() -> Arc<Mask> {
        Arc::new(Mask::new(4, 4).expect("4x4 mask allocates"))
    }

    /// The base case: the same inputs hit.
    ///
    /// Without this the negative tests below are unfalsifiable — a
    /// cache that never hits passes every "must miss" assertion.
    #[test]
    fn identical_inputs_hit() {
        let mut c = ClipCache::new();
        let p = square(0.0);
        let k = ClipCache::build_key(&p, FillRule::Winding, Transform::identity(), 4, 4);
        assert!(c.get(k, None).is_none(), "cold cache must miss");
        c.insert(k, None, mask(), Some((1.0, 2.0, 3.0, 4.0)));
        let hit = c.get(k, None).expect("identical inputs must hit");
        assert_eq!(
            hit.1,
            Some((1.0, 2.0, 3.0, 4.0)),
            "bbox travels with the mask"
        );
    }

    /// A different CTM is a different mask, because the mask is in
    /// DEVICE space.
    ///
    /// Dropping the CTM from `build_key` makes this test fail — which
    /// is the point: the same geometry under a different transform
    /// paints somewhere else, and a cache that conflated them would
    /// return a mask for the wrong part of the page.
    #[test]
    fn a_different_ctm_misses() {
        let mut c = ClipCache::new();
        let p = square(0.0);
        let k1 = ClipCache::build_key(&p, FillRule::Winding, Transform::identity(), 4, 4);
        let k2 = ClipCache::build_key(
            &p,
            FillRule::Winding,
            Transform::from_translate(5.0, 0.0),
            4,
            4,
        );
        assert_ne!(
            k1, k2,
            "same path under a different CTM is a different mask"
        );
        c.insert(k1, None, mask(), None);
        assert!(c.get(k2, None).is_none());
    }

    /// A coordinate differing by one ulp is a different mask.
    ///
    /// Comparing coordinates approximately would make this hit, raise
    /// the measured hit rate, and return a mask whose edge coverage is
    /// wrong — the failure that looks like an improvement.
    #[test]
    fn a_one_ulp_coordinate_difference_misses() {
        let p1 = square(1.0);
        let p2 = square(f32::from_bits(1.0f32.to_bits() + 1));
        let k1 = ClipCache::build_key(&p1, FillRule::Winding, Transform::identity(), 4, 4);
        let k2 = ClipCache::build_key(&p2, FillRule::Winding, Transform::identity(), 4, 4);
        assert_ne!(k1, k2, "one ulp apart is not the same coverage");
    }

    /// The fill rule is part of the key: `W` and `W*` differ on a
    /// self-intersecting path.
    #[test]
    fn a_different_fill_rule_misses() {
        let p = square(0.0);
        let k1 = ClipCache::build_key(&p, FillRule::Winding, Transform::identity(), 4, 4);
        let k2 = ClipCache::build_key(&p, FillRule::EvenOdd, Transform::identity(), 4, 4);
        assert_ne!(k1, k2);
    }

    /// Mask dimensions are part of the key, so a render at another
    /// scale cannot reuse this one's masks.
    #[test]
    fn a_different_mask_size_misses() {
        let p = square(0.0);
        let k1 = ClipCache::build_key(&p, FillRule::Winding, Transform::identity(), 4, 4);
        let k2 = ClipCache::build_key(&p, FillRule::Winding, Transform::identity(), 8, 8);
        assert_ne!(k1, k2);
    }

    /// **The one that matters most.** Same build key, different
    /// incoming clip, must miss.
    ///
    /// The cached value is the mask AFTER intersection, so two
    /// applications of the same path under different accumulated clips
    /// produce different results. Comparing only `build_key` would
    /// return the first one for the second, which is a silently wrong
    /// picture — no error, no crash, just the wrong pixels clipped.
    ///
    /// Deleting the `same_incoming` check in `get` makes exactly this
    /// test fail and leaves the rest green.
    #[test]
    fn the_same_path_under_a_different_incoming_clip_misses() {
        let mut c = ClipCache::new();
        let p = square(0.0);
        let k = ClipCache::build_key(&p, FillRule::Winding, Transform::identity(), 4, 4);
        let clip_a = mask();
        let clip_b = mask();
        c.insert(k, Some(Arc::clone(&clip_a)), mask(), None);
        assert!(
            c.get(k, Some(&clip_a)).is_some(),
            "the clip it was built under must hit"
        );
        assert!(
            c.get(k, Some(&clip_b)).is_none(),
            "a different incoming clip is a different result"
        );
        assert!(
            c.get(k, None).is_none(),
            "no incoming clip is also a different result"
        );
    }

    /// Two masks with identical CONTENTS but different addresses miss.
    ///
    /// Pointer identity is stricter than value equality. That loses
    /// hits and can never invent one, which is the direction a cache
    /// returning pictures should err in.
    #[test]
    fn incoming_identity_is_by_pointer_not_by_value() {
        let mut c = ClipCache::new();
        let p = square(0.0);
        let k = ClipCache::build_key(&p, FillRule::Winding, Transform::identity(), 4, 4);
        let a = mask();
        let b = mask(); // same size, same all-zero contents, different Arc
        c.insert(k, Some(a), mask(), None);
        assert!(c.get(k, Some(&b)).is_none());
    }

    /// The cache never exceeds [`CAPACITY`], and evicts the least
    /// recently used rather than the oldest inserted.
    #[test]
    fn eviction_is_least_recently_used_not_first_in() {
        let mut c = ClipCache::new();
        let keys: Vec<u64> = (0..CAPACITY as u64).collect();
        for k in &keys {
            c.insert(*k, None, mask(), None);
        }
        // Touch the FIRST entry so it is the most recently used.
        assert!(c.get(keys[0], None).is_some());
        // Overflow by one: the victim must be entry 1, not entry 0.
        c.insert(999, None, mask(), None);
        assert_eq!(c.entries.len(), CAPACITY, "capacity is never exceeded");
        assert!(
            c.get(keys[0], None).is_some(),
            "the recently used entry survived"
        );
        assert!(
            c.get(keys[1], None).is_none(),
            "the least recently used entry was evicted"
        );
    }
}
