//! The one place the renderer may use threads (decision 080): native
//! targets only, the count a setting, and serial on wasm32.

/// The worker count for a requested `threads`: 0 means every core; wasm32
/// is always 1.
pub(crate) fn workers(threads: usize) -> usize {
    #[cfg(target_arch = "wasm32")]
    let workers = {
        let _ = threads;
        1
    };
    #[cfg(not(target_arch = "wasm32"))]
    let workers = match threads {
        0 => std::thread::available_parallelism().map_or(1, std::num::NonZero::get),
        n => n,
    };
    workers
}

/// Runs `work` on every item, each on its own scoped thread when there is
/// more than one and the target has threads. A panic in `work` propagates.
pub(crate) fn run_each<T: Send>(items: Vec<T>, work: &(dyn Fn(T) + Sync)) {
    #[cfg(not(target_arch = "wasm32"))]
    if items.len() > 1 {
        std::thread::scope(|s| {
            for item in items {
                s.spawn(move || work(item));
            }
        });
        return;
    }
    for item in items {
        work(item);
    }
}
