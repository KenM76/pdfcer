//! Running a content stream a window at a time ([`Content`]).
//!
//! A stream reaches [`super::run_nested`] either parsed whole or as a
//! [`ContentChunks`], which decodes and tokenizes it a window at a time so
//! a legitimately huge stream is drawn rather than refused. One interpreter
//! runs every window in turn, so its state carries across the seams and
//! the split is invisible. Each window is charged to the
//! [`PageBudget`] only while it runs.

use std::borrow::Cow;

use pdfcer_core::content::{ChunkTruncation, ContentChunks, ContentStream};

use super::{Diagnostics, Interpreter};
use crate::cancel::RenderCancel;
use crate::canvas::Canvas;
use crate::page_budget::{Held, PageBudget};

/// The content one [`super::run_nested`] call runs.
#[derive(Debug)]
// Built once per content run and consumed at once; boxing would add an
// allocation per form and pattern for no memory saved.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Content<'c, 'a> {
    /// Already parsed whole.
    Parsed(&'c ContentStream),
    /// Read a window at a time.
    Chunks(ContentChunks<'a>),
}

impl<'c> From<&'c ContentStream> for Content<'c, '_> {
    fn from(c: &'c ContentStream) -> Self {
        Self::Parsed(c)
    }
}

impl<'a> From<ContentChunks<'a>> for Content<'_, 'a> {
    fn from(c: ContentChunks<'a>) -> Self {
        Self::Chunks(c)
    }
}

/// The first window and what follows it.
type Split<'c, 'a> = (Cow<'c, ContentStream>, Option<ContentChunks<'a>>);

impl<'c, 'a> Content<'c, 'a> {
    /// Split off the first window. A caller that must report a damaged
    /// stream itself primes its [`ContentChunks`] first, so an error here
    /// is only a note.
    pub(crate) fn first_window(self) -> Result<Split<'c, 'a>, &'static [u8]> {
        match self {
            Self::Parsed(c) => Ok((Cow::Borrowed(c), None)),
            Self::Chunks(mut chunks) => match chunks.next_chunk() {
                Ok(first) => Ok((Cow::Owned(first.unwrap_or_default()), Some(chunks))),
                Err(_) => Err(DAMAGED),
            },
        }
    }
}

const DAMAGED: &[u8] = b"ContentTruncated(content stream damaged past its first window)";

/// The bytes a window holds against the [`PageBudget`]: its decoded buffer
/// and its token list.
pub(crate) fn held_bytes(c: &ContentStream) -> usize {
    c.buf.len() + std::mem::size_of_val(c.tokens.as_slice())
}

impl Diagnostics {
    /// The diagnostics of a stream left out, with `note` saying why.
    pub(crate) fn truncated(note: &[u8]) -> Self {
        let mut diag = Self::default();
        diag.content_truncated += 1;
        diag.note(note);
        diag
    }
}

/// Run `first` (charged by `held`), then every window of `rest`.
pub(crate) fn run_windows(
    interp: &mut Interpreter<'_>,
    first: Cow<'_, ContentStream>,
    held: Held<'_>,
    rest: Option<ContentChunks<'_>>,
    canvas: &mut Canvas<'_>,
) {
    let finished = run_window(interp, &first, canvas);
    drop(first);
    drop(held);
    let Some(mut chunks) = rest.filter(|_| finished) else {
        return;
    };
    let budget: &PageBudget = interp.budget;
    loop {
        let window = match chunks.next_chunk() {
            Ok(Some(window)) => window,
            Ok(None) => break,
            Err(_) => return note(interp, DAMAGED),
        };
        let _held = match budget.hold(held_bytes(&window)) {
            Ok(held) => held,
            Err(why) => return note(interp, why.note()),
        };
        if !run_window(interp, &window, canvas) {
            return;
        }
    }
    match chunks.truncation() {
        Some(ChunkTruncation::Budget) => note(
            interp,
            b"ContentTruncated(decoded content past 64x its encoded size)",
        ),
        Some(_) => note(
            interp,
            b"ContentTruncated(no complete operator within 256 MiB)",
        ),
        None => {}
    }
}

fn note(interp: &mut Interpreter<'_>, why: &[u8]) {
    interp.diag.content_truncated += 1;
    interp.diag.note(why);
}

/// Execute one window's operations; `false` once the render is cancelled.
fn run_window(
    interp: &mut Interpreter<'_>,
    window: &ContentStream,
    canvas: &mut Canvas<'_>,
) -> bool {
    for op in window.operations() {
        // THE CANCELLATION POLL: one relaxed load per operator
        // (`crate::cancel` module docs). Stopping leaves `interp.diag`
        // intact; the caller turns a set flag into `RenderError::Cancelled`.
        if interp.cancel.is_some_and(RenderCancel::is_cancelled) {
            return false;
        }
        interp.execute(&op, window, canvas);
    }
    true
}
