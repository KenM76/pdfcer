//! # `cancel` — stopping a render that is no longer wanted
//!
//! A [`RenderCancel`] is a shared flag one thread sets and a rendering
//! thread polls. It exists so a GUI can abandon a render in flight,
//! and it lives in `pdfcer-render` rather than in the shell because
//! `ARCHITECTURE.md` §3 forbids the render crate from knowing anything
//! about a GUI — this is a plain `Arc<AtomicBool>` with no windowing,
//! runtime or executor dependency, and works identically under wasm.
//!
//! ## Why cancellation is load-bearing, not a nicety
//!
//! Rasterization used to run inline on the UI thread. On a real CAD
//! sheet that is ~10 s at 1× and ~58 s at 2× (measured 2026-08-07), so
//! the application did not merely redraw slowly — it stopped answering
//! entirely: no repaint, no progress, no way out. The operator's report
//! was *"it took minutes to try and update the view and hung the entire
//! gui."*
//!
//! Moving the work to a worker fixes the freeze but creates a second
//! problem, and cancellation is the answer to that one:
//!
//! - The worker holds an `Arc<EditSession>` while it renders, so
//!   `Arc::get_mut` fails and **an edit arriving mid-render cannot
//!   mutate the session**.
//! - Making the edit wait re-creates the freeze by another route — a
//!   58-second wait is a hang whichever thread it happens on.
//!
//! So the edit path **cancels the in-flight render and proceeds**, which
//! is only viable if cancellation is fast. That is what this type buys,
//! and why it is not optional.
//!
//! ## Stopping the work, not merely discarding the result
//!
//! Dropping a receiver would discard the *result* while the worker
//! carried on painting for another 58 seconds — still occupying a core,
//! still delaying whatever the operator asked for next. A flag the
//! interpreter actually polls is what makes the work stop.
//!
//! ## Why polling is cheap enough to do per operator
//!
//! The check is one [`Ordering::Relaxed`] load, which on every target
//! pdfcer builds for is an ordinary load with no fence and no bus
//! traffic. The benchmark page has 148,517 operators; at ~1 ns per load
//! that is well under a millisecond against a ~10-second render — below
//! the noise of the measurement that would try to detect it.
//!
//! Relaxed is the correct ordering, not a shortcut: the flag carries no
//! data and guards no memory. The only question asked of it is "has
//! someone set this yet", and a late answer costs at most one more
//! operator's work before the next poll.
//!
//! **The granularity that actually matters is not the operator count.**
//! A single clip operation costs ~360 µs on this page, and the poll sits
//! between operators rather than inside `fill_path`, so worst-case
//! latency is one operation — a third of a millisecond, not the whole
//! render. Pushing the check deeper would buy nothing an operator could
//! perceive and would put a branch inside the hottest loop in the
//! renderer.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// A shared "stop rendering" flag.
///
/// Cloning shares the flag; [`RenderCancel::cancel`] on any clone is
/// observed by all of them. A render given one polls it between
/// operators and returns [`RenderError::Cancelled`] promptly once set.
///
/// [`RenderError::Cancelled`]: crate::RenderError::Cancelled
///
/// # Examples
///
/// ```
/// use pdfcer_render::cancel::RenderCancel;
///
/// let token = RenderCancel::new();
/// assert!(!token.is_cancelled());
///
/// // A clone handed to a worker observes the caller's cancellation.
/// let worker_copy = token.clone();
/// token.cancel();
/// assert!(worker_copy.is_cancelled());
/// ```
#[derive(Debug, Clone, Default)]
pub struct RenderCancel(Arc<AtomicBool>);

impl RenderCancel {
    /// A fresh, un-cancelled token.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Ask every render holding this token (or a clone) to stop.
    ///
    /// Idempotent, and callable from any thread. Returns immediately —
    /// it does not wait for the render to notice, because the caller's
    /// whole reason for cancelling is usually that it does not want to
    /// wait for anything.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether cancellation has been requested.
    ///
    /// One relaxed load — see the module docs for why that ordering is
    /// correct here rather than merely cheap.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A clone must observe the original's cancellation — otherwise the
    /// worker copy is a private flag and nothing can ever stop a render.
    #[test]
    fn a_clone_observes_cancellation() {
        let token = RenderCancel::new();
        let worker_copy = token.clone();
        assert!(!worker_copy.is_cancelled());
        token.cancel();
        assert!(
            worker_copy.is_cancelled(),
            "cancelling one handle must be visible through every clone, or a \
             worker holding a clone would render to completion after the UI \
             thread had given up on it"
        );
    }

    /// The direction that keeps the test above honest (R162): a fresh
    /// token must read `false`, or `a_clone_observes_cancellation` would
    /// pass just as well against a function that always returned `true`.
    #[test]
    fn a_fresh_token_is_not_cancelled() {
        assert!(!RenderCancel::new().is_cancelled());
        assert!(!RenderCancel::default().is_cancelled());
    }

    /// Cancelling twice is not an error and does not un-cancel.
    #[test]
    fn cancellation_is_idempotent() {
        let token = RenderCancel::new();
        token.cancel();
        token.cancel();
        assert!(token.is_cancelled());
    }

    /// The token must cross a thread boundary — that is its entire
    /// purpose, and a compile failure here is the whole feature failing.
    #[test]
    fn the_token_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RenderCancel>();
    }
}
