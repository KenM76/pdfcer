//! A run whose show operators differ only in `Tf` size (request G228).
//!
//! The span search joins such operators, so a match across a size change is
//! located and narrowed like any span. A span edit sets its replacement in
//! ONE operator's text state, so a narrowed edit that still crosses a size
//! change would re-set the other piece at the wrong size: it is refused as
//! [`NotFoundReason::SplitRun`], the refusal it had before the join. An edit
//! narrowed into one piece is that piece's edit, at that piece's size.

use crate::text_edit::cause::NotFoundReason;
use crate::text_edit::edit::{Anchor, EditError, OpRec, Rec, ShowData, spannable};

/// `Err(SplitRun)` when two operators of `span` in one text object differ in
/// size; `find` is the request's own text, for the error.
///
/// # Errors
///
/// [`EditError::NoMatch`] with [`NotFoundReason::SplitRun`] naming how many
/// show operators the span touches.
pub(crate) fn refuse_mixed_size(recs: &[OpRec], span: Anchor, find: &str) -> Result<(), EditError> {
    if span.first == span.last {
        return Ok(());
    }
    let mut prev: Option<&ShowData> = None;
    let mut mixed = false;
    let mut shows = 0usize;
    for r in recs.get(span.first..=span.last).unwrap_or_default() {
        match &r.rec {
            Rec::EndText => prev = None,
            Rec::Show(s) => {
                shows += 1;
                mixed |= prev.is_some_and(|p| !spannable(p, s));
                prev = Some(s);
            }
            _ => {}
        }
    }
    if mixed {
        return Err(EditError::NoMatch {
            find: find.to_owned(),
            reason: NotFoundReason::SplitRun { operators: shows },
        });
    }
    Ok(())
}
