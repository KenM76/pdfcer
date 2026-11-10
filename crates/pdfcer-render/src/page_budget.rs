//! Work ceilings shared by every content stream that one render runs.
//!
//! The per-stream guards (the depth limit and the cycle set) stop unbounded
//! recursion, but they do not stop fan-out. Eight distinct forms that each
//! draw the next one twice are 2^8 paints, and a Type 3 glyph that shows
//! eight of its own glyphs is 8^64 paints, at every depth the limit allows.
//! A budget counted across the whole render bounds that. Past it, the
//! remaining nested content is left out and
//! [`crate::interpret::Diagnostics::content_truncated`] counts each
//! stream that was skipped (ARCHITECTURE.md §10.1).

use std::cell::Cell;

/// The number of nested content streams one render may execute at depth 2
/// or below. A depth-2 stream is one invoked from inside a form XObject, a
/// Type 3 glyph procedure, a pattern cell or a soft mask.
///
/// Depths 0 and 1 are free: the page's own content, and the forms or Type 3
/// glyphs it invokes directly. That keeps a long document set entirely in a
/// Type 3 font inside the budget however many glyphs it shows.
pub const MAX_NESTED_PAINTS: usize = 50_000;

/// The bytes of decoded content that all streams one render is executing
/// may hold at once: the page's content plus every form, pattern cell and
/// glyph procedure further up the stack. Both the decoded buffer and the
/// token list count. A finished stream gives its bytes back.
pub const MAX_PAGE_CONTENT: usize = 512 << 20;

/// Why [`PageBudget::enter`] refused a stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Refusal {
    /// [`MAX_NESTED_PAINTS`] is spent.
    Paints,
    /// Holding this stream would pass [`MAX_PAGE_CONTENT`].
    Content,
}

impl Refusal {
    /// The diagnostic note for this refusal.
    pub(crate) const fn note(self) -> &'static [u8] {
        match self {
            Self::Paints => b"ContentTruncated(nested paints past MAX_NESTED_PAINTS)",
            Self::Content => b"ContentTruncated(decoded content held past MAX_PAGE_CONTENT)",
        }
    }
}

/// One render's running totals. Created once at each root entry point and
/// borrowed by every nested interpreter.
#[derive(Debug, Default)]
pub(crate) struct PageBudget {
    nested_paints: Cell<usize>,
    held: Cell<usize>,
}

impl PageBudget {
    /// Whether a stream at `depth` would be refused for paints alone. The
    /// callers ask before decoding, so a spent budget skips the inflate
    /// and the parse as well as the run.
    pub(crate) fn paints_spent(&self, depth: usize) -> bool {
        depth >= 2 && self.nested_paints.get() >= MAX_NESTED_PAINTS
    }

    /// Charge one stream at XObject nesting `depth` that holds `bytes` of
    /// decoded content. The bytes stay charged until the returned guard
    /// drops.
    ///
    /// # Errors
    ///
    /// A [`Refusal`] if the stream would pass either ceiling. Nothing is
    /// charged when it is refused.
    pub(crate) fn enter(&self, depth: usize, bytes: usize) -> Result<Held<'_>, Refusal> {
        let paints = self.nested_paints.get();
        if self.paints_spent(depth) {
            return Err(Refusal::Paints);
        }
        let held = self.held.get().saturating_add(bytes);
        if held > MAX_PAGE_CONTENT {
            return Err(Refusal::Content);
        }
        if depth >= 2 {
            self.nested_paints.set(paints + 1);
        }
        self.held.set(held);
        Ok(Held {
            budget: self,
            bytes,
        })
    }
}

/// The bytes a running stream holds against its [`PageBudget`].
#[derive(Debug)]
pub(crate) struct Held<'a> {
    budget: &'a PageBudget,
    bytes: usize,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        let held = &self.budget.held;
        held.set(held.get().saturating_sub(self.bytes));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shallow_paints_are_free_and_deep_ones_are_capped() {
        let budget = PageBudget::default();
        for _ in 0..MAX_NESTED_PAINTS + 10 {
            assert!(budget.enter(1, 0).is_ok());
        }
        for _ in 0..MAX_NESTED_PAINTS {
            assert!(budget.enter(2, 0).is_ok());
        }
        assert_eq!(budget.enter(5, 0).err(), Some(Refusal::Paints));
        assert!(budget.enter(0, 0).is_ok());
    }

    #[test]
    fn held_bytes_are_returned_when_the_stream_finishes() {
        let budget = PageBudget::default();
        let outer = budget.enter(0, MAX_PAGE_CONTENT - 10);
        assert!(outer.is_ok());
        assert_eq!(budget.enter(1, 11).err(), Some(Refusal::Content));
        {
            let inner = budget.enter(1, 10);
            assert!(inner.is_ok());
        }
        drop(outer);
        assert!(budget.enter(1, MAX_PAGE_CONTENT).is_ok());
    }
}
