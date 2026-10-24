//! Region marks for a visual comparison (Pass 645.2): each changed region
//! a renderer found becomes an orange revision cloud on the NEW document,
//! and a page with no counterpart a blue frame (decision 200).
//!
//! Core cannot render, so the regions arrive already placed in each page's
//! user space; this module only authors them.

use crate::annot_author::{Color, MarkupSpec};
use crate::edit::{EditError, EditSession, MarkupNote, MarkupOptions};
use pdfcer_model::page_tree::Rect;

use super::result::{INSERTED_COLOR, REPLACED_COLOR, ResultError, ResultOptions, Unmarked};

/// Cloud intensity of a changed-region mark (`/BE /I`, §12.5.4).
const CLOUD: f64 = 1.0;
/// How far outside the changed pixels a cloud is drawn, points.
const MARGIN: f64 = 3.0;
/// How far inside the page edge an inserted page's frame is drawn, points.
const INSET: f64 = 4.0;

/// What a region mark says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RegionKind {
    /// The page's appearance changed here: an orange revision cloud.
    Changed,
    /// The whole page has no counterpart in the old document: a blue frame
    /// just inside `rect`.
    InsertedPage,
}

/// One region to mark.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RegionMark {
    /// 0-based page of the session's document.
    pub page: usize,
    /// The region (or, for [`RegionKind::InsertedPage`], the page box) in
    /// the page's default user space.
    pub rect: Rect,
    /// Which mark to draw.
    pub kind: RegionKind,
    /// The annotation's note.
    pub note: String,
}

impl RegionMark {
    /// A changed region.
    #[must_use]
    pub fn changed(page: usize, rect: Rect, note: impl Into<String>) -> Self {
        Self {
            page,
            rect,
            kind: RegionKind::Changed,
            note: note.into(),
        }
    }

    /// A page only the new document has; `page_box` is its crop box.
    #[must_use]
    pub fn inserted_page(page: usize, page_box: Rect, note: impl Into<String>) -> Self {
        Self {
            page,
            rect: page_box,
            kind: RegionKind::InsertedPage,
            note: note.into(),
        }
    }
}

/// What [`mark_regions`] did.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RegionReport {
    /// Annotations authored, one per mark.
    pub annotations: usize,
    /// Marks not authored (index into the slice given) and why.
    pub unmarked: Vec<Unmarked>,
}

/// Author `marks` on `session`. Uses [`ResultOptions::author`]; the summary
/// page option is ignored here.
///
/// Call it **before** [`super::mark_changes`] when that inserts a summary
/// page, because the summary page shifts every page index by one.
///
/// Each mark is its own undo entry. A mark that cannot be authored is
/// listed in [`RegionReport::unmarked`] rather than failing the rest.
///
/// # Errors
///
/// [`ResultError::Pages`] when the page tree cannot be read, before
/// anything is authored.
pub fn mark_regions(
    session: &mut EditSession,
    marks: &[RegionMark],
    options: &ResultOptions,
) -> Result<RegionReport, ResultError> {
    let count = session
        .pages()
        .map_err(|e| ResultError::Pages(EditError::PageTree(e)))?
        .len();
    let mut out = RegionReport {
        annotations: 0,
        unmarked: Vec::new(),
    };
    for (i, m) in marks.iter().enumerate() {
        match mark_one(session, m, options, count) {
            Ok(()) => out.annotations += 1,
            Err(reason) => out.unmarked.push(Unmarked { change: i, reason }),
        }
    }
    Ok(out)
}

fn mark_one(
    session: &mut EditSession,
    m: &RegionMark,
    options: &ResultOptions,
    count: usize,
) -> Result<(), String> {
    if m.page >= count {
        return Err(format!("page {} is outside the document", m.page + 1));
    }
    let mut note = MarkupNote::new(m.note.clone());
    if let Some(a) = &options.author {
        note = note.by(a.clone());
    }
    let opts = MarkupOptions {
        note: Some(note),
        ..MarkupOptions::default()
    };
    let (rect, color, effect) = match m.kind {
        RegionKind::Changed => (grow(m.rect, MARGIN), REPLACED_COLOR, Some(CLOUD)),
        RegionKind::InsertedPage => (grow(m.rect, -INSET), INSERTED_COLOR, None),
    };
    let spec = square(rect, color, effect);
    session
        .add_markup_with(m.page, &spec, &opts)
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn square(rect: Rect, color: Color, border_effect: Option<f64>) -> MarkupSpec {
    MarkupSpec::Square {
        rect,
        border: Some(color),
        interior: None,
        border_width: 1.5,
        border_effect,
    }
}

/// `r` grown by `d` on every side (shrunk when negative, never inverted).
fn grow(r: Rect, d: f64) -> Rect {
    let (cx, cy) = ((r.llx + r.urx) / 2.0, (r.lly + r.ury) / 2.0);
    Rect {
        llx: (r.llx - d).min(cx),
        lly: (r.lly - d).min(cy),
        urx: (r.urx + d).max(cx),
        ury: (r.ury + d).max(cy),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrinking_past_the_centre_collapses_rather_than_inverts() {
        let r = grow(
            Rect {
                llx: 0.0,
                lly: 0.0,
                urx: 4.0,
                ury: 4.0,
            },
            -10.0,
        );
        assert!(r.llx <= r.urx && r.lly <= r.ury);
    }
}
