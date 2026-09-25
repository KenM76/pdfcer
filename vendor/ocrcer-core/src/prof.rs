//! Optional runtime stage counters, read only when `OCRCER_PROFILE=1` is set
//! in the process environment. A bench-only diagnostic: no reading path
//! consumes anything this module produces, and every timer is a single
//! `Option<Instant>` check when the flag is off, not a branch inside the
//! hot loops themselves.
//!
//! `std::time::Instant::now()` has no clock on `wasm32-unknown-unknown` and
//! panics if called there; [`start`] never calls it unless [`enabled`] is
//! true, and `enabled` is only true when the flag was set, which a `cargo
//! build` for that target never does. This module and its call sites compile
//! for wasm32 unchanged; nothing here is gated to another module tree.

use std::sync::atomic::{AtomicU64, Ordering};

/// One process's running totals. Nanosecond buckets for the four pipeline
/// shares `ocrcer-runtime`'s chunk-15 profile reports on, plus the counts a
/// caller needs to explain them (edges per word, `nearest()` calls,
/// prototypes visited and abandoned early).
#[derive(Default)]
pub struct Counters {
    pub binarize_layout_ns: AtomicU64,
    pub segment_ns: AtomicU64,
    pub extract_ns: AtomicU64,
    pub match_ns: AtomicU64,
    pub decode_ns: AtomicU64,
    pub words: AtomicU64,
    pub edges: AtomicU64,
    pub match_calls: AtomicU64,
    pub prototypes_visited: AtomicU64,
    pub prototypes_abandoned: AtomicU64,
}

impl Counters {
    const fn new() -> Self {
        Counters {
            binarize_layout_ns: AtomicU64::new(0),
            segment_ns: AtomicU64::new(0),
            extract_ns: AtomicU64::new(0),
            match_ns: AtomicU64::new(0),
            decode_ns: AtomicU64::new(0),
            words: AtomicU64::new(0),
            edges: AtomicU64::new(0),
            match_calls: AtomicU64::new(0),
            prototypes_visited: AtomicU64::new(0),
            prototypes_abandoned: AtomicU64::new(0),
        }
    }
}

pub static COUNTERS: Counters = Counters::new();

/// Adds `n` to a counter. Callers already gate this on [`enabled`]; it does
/// not gate itself, so a caller that wants an unconditional count (there are
/// none today) is free to add one.
pub fn add(slot: &AtomicU64, n: u64) {
    slot.fetch_add(n, Ordering::Relaxed);
}

pub fn get(slot: &AtomicU64) -> u64 {
    slot.load(Ordering::Relaxed)
}

/// Zeroes every counter, for a driver that profiles one page at a time.
pub fn reset() {
    let c = &COUNTERS;
    for slot in [
        &c.binarize_layout_ns,
        &c.segment_ns,
        &c.extract_ns,
        &c.match_ns,
        &c.decode_ns,
    ] {
        slot.store(0, Ordering::Relaxed);
    }
    for slot in [&c.words, &c.edges, &c.match_calls, &c.prototypes_visited, &c.prototypes_abandoned] {
        slot.store(0, Ordering::Relaxed);
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod timing {
    use super::*;
    use std::sync::OnceLock;
    use std::time::Instant;

    static ENABLED: OnceLock<bool> = OnceLock::new();

    pub fn enabled() -> bool {
        *ENABLED.get_or_init(|| std::env::var("OCRCER_PROFILE").map(|v| v == "1").unwrap_or(false))
    }

    pub struct Timer(Option<Instant>);

    pub fn start() -> Timer {
        Timer(if enabled() { Some(Instant::now()) } else { None })
    }

    impl Timer {
        pub fn stop(self, slot: &AtomicU64) {
            if let Some(t) = self.0 {
                add(slot, t.elapsed().as_nanos() as u64);
            }
        }
    }
}

// No clock on wasm32-unknown-unknown: `enabled` is always false and `start`
// never touches `Instant`, so nothing here can panic at runtime and nothing
// upstream needs a `cfg` of its own to call it.
#[cfg(target_arch = "wasm32")]
mod timing {
    #[inline]
    pub fn enabled() -> bool {
        false
    }
    pub struct Timer;
    pub fn start() -> Timer {
        Timer
    }
    impl Timer {
        #[inline]
        pub fn stop(self, _slot: &super::AtomicU64) {}
    }
}

pub use timing::{enabled, start, Timer};
