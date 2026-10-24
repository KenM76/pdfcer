//! The annotated comparison result (Pass 645.1): marks a [`CompareReport`]'s
//! changes onto the NEW document as review annotations.
//!
//! Palette and annotation shapes are decision 199: inserted words get a blue
//! highlight, replaced words an orange highlight, formatting changes a purple
//! underline (all `/Highlight` or `/Underline`, §12.5.6.10, with a baked
//! `/AP`), and a deleted stretch a red `/Caret` (§12.5.6.11) where the words
//! used to be. Each mark carries a note: the old text, or what changed.

use crate::annot_author::{CaretSpec, Color, MarkupSpec, Quad, TextMarkupKind};
use crate::edit::{EditError, EditSession, MarkupNote, MarkupOptions};
use crate::pageops::InsertPosition;
use crate::text_edit::{PageTemplate, PlaceTextError, PlaceTextReport};
use pdfcer_model::page_tree::Rect;

use super::{Change, ChangeKind, CompareReport, Place, Side};

/// Highlight colour of an inserted stretch.
pub const INSERTED_COLOR: Color = Color::Rgb(0.25, 0.55, 1.0);
/// Highlight colour of a replaced stretch.
pub const REPLACED_COLOR: Color = Color::Rgb(1.0, 0.6, 0.1);
/// Caret colour of a deletion.
pub const DELETED_COLOR: Color = Color::Rgb(0.9, 0.1, 0.1);
/// Underline colour of a formatting change.
pub const FORMATTING_COLOR: Color = Color::Rgb(0.6, 0.2, 0.8);

/// How [`mark_changes`] builds the result.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ResultOptions {
    /// Insert a summary page (counts and the colour key) before page 1.
    /// Default on.
    pub summary_page: bool,
    /// `/T` of every mark. Default `None` (no author).
    pub author: Option<String>,
    /// How the summary page names the two documents, `(old, new)`.
    pub labels: Option<(String, String)>,
    /// Extra text appended to the summary page (a visual comparison's page
    /// matching, say).
    pub summary_note: Option<String>,
}

impl Default for ResultOptions {
    fn default() -> Self {
        Self {
            summary_page: true,
            author: None,
            labels: None,
            summary_note: None,
        }
    }
}

impl ResultOptions {
    /// Turn the summary page on or off.
    #[must_use]
    pub const fn with_summary_page(mut self, on: bool) -> Self {
        self.summary_page = on;
        self
    }

    /// Set the author written on every mark.
    #[must_use]
    pub fn with_author(mut self, author: impl Into<String>) -> Self {
        self.author = Some(author.into());
        self
    }

    /// Name the two documents on the summary page.
    #[must_use]
    pub fn with_labels(mut self, old: impl Into<String>, new: impl Into<String>) -> Self {
        self.labels = Some((old.into(), new.into()));
        self
    }

    /// Append `text` to the summary page.
    #[must_use]
    pub fn with_summary_note(mut self, text: impl Into<String>) -> Self {
        self.summary_note = Some(text.into());
        self
    }
}

/// A change [`mark_changes`] could not mark, and why.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Unmarked {
    /// Index into [`CompareReport::changes`].
    pub change: usize,
    /// What prevented it.
    pub reason: String,
}

/// What [`mark_changes`] did.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct ResultReport {
    /// Annotations authored (one per change per page it touches).
    pub annotations: usize,
    /// Changes that received at least one mark.
    pub changes_marked: usize,
    /// Changes left unmarked: no place on the new side, or the author verb
    /// refused. The comparison report still lists them.
    pub unmarked: Vec<Unmarked>,
    /// The summary page's placement report, when one was inserted. Its
    /// disclosures (substituted characters and the like) belong in the
    /// caller's output.
    pub summary: Option<PlaceTextReport>,
}

/// Why [`mark_changes`] failed as a whole.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ResultError {
    /// The session's page tree could not be read.
    #[error("the document's pages could not be read: {0}")]
    Pages(#[source] EditError),
    /// The summary page could not be placed. Every mark is already in the
    /// session.
    #[error("the summary page could not be placed: {0}")]
    Summary(#[source] PlaceTextError),
}

/// Mark `report`'s changes on `session`, which must hold the NEW document
/// the report was computed against (page indices are taken as they are).
///
/// Every mark is its own undo entry; the summary page is one more. A change
/// that cannot be marked is listed in [`ResultReport::unmarked`] rather than
/// failing the rest.
///
/// # Errors
///
/// [`ResultError::Pages`] when the page tree cannot be read, before
/// anything is authored; [`ResultError::Summary`] when the summary page
/// fails, after the marks.
pub fn mark_changes(
    session: &mut EditSession,
    report: &CompareReport,
    options: &ResultOptions,
) -> Result<ResultReport, ResultError> {
    let page_count = session
        .pages()
        .map_err(|e| ResultError::Pages(EditError::PageTree(e)))?
        .len();
    let mut out = ResultReport {
        annotations: 0,
        changes_marked: 0,
        unmarked: Vec::new(),
        summary: None,
    };
    for (i, change) in report.changes.iter().enumerate() {
        match mark_one(session, change, options, page_count) {
            Ok(0) => out.unmarked.push(Unmarked {
                change: i,
                reason: "it has no position in the new document".to_owned(),
            }),
            Ok(n) => {
                out.annotations += n;
                out.changes_marked += 1;
            }
            Err(reason) => out.unmarked.push(Unmarked { change: i, reason }),
        }
    }
    if options.summary_page {
        let text = summary_text(report, options);
        let placed = session
            .place_text(&text, &PageTemplate::new(), InsertPosition::Start)
            .map_err(ResultError::Summary)?;
        out.summary = Some(placed);
    }
    Ok(out)
}

/// Author one change's marks; returns how many.
fn mark_one(
    session: &mut EditSession,
    change: &Change,
    options: &ResultOptions,
    page_count: usize,
) -> Result<usize, String> {
    let mut note = MarkupNote::new(note_text(change));
    if let Some(a) = &options.author {
        note = note.by(a.clone());
    }
    let opts = MarkupOptions {
        note: Some(note),
        ..MarkupOptions::default()
    };
    let in_range = |p: &Place| {
        if p.page < page_count {
            Ok(())
        } else {
            Err(format!("page {} is outside the document", p.page + 1))
        }
    };
    if change.kind == ChangeKind::Deleted {
        let Some((Side::New, at)) = change.anchor else {
            return Ok(0);
        };
        in_range(&at)?;
        let spec = CaretSpec {
            color: DELETED_COLOR,
            ..CaretSpec::new(caret_rect(at.rect))
        };
        session
            .add_caret_annotation(at.page, &spec, &opts)
            .map_err(|e| e.to_string())?;
        return Ok(1);
    }
    let (kind, color) = match change.kind {
        ChangeKind::Inserted => (TextMarkupKind::Highlight, INSERTED_COLOR),
        ChangeKind::Formatting => (TextMarkupKind::Underline, FORMATTING_COLOR),
        _ => (TextMarkupKind::Highlight, REPLACED_COLOR),
    };
    let mut made = 0;
    for (page, places) in by_page(&change.new_places) {
        if let Some(first) = places.first() {
            in_range(first)?;
        }
        let spec = MarkupSpec::TextMarkup {
            kind,
            quads: places.iter().map(|p| Quad::from_rect(p.rect)).collect(),
            color,
        };
        session
            .add_markup_with(page, &spec, &opts)
            .map_err(|e| e.to_string())?;
        made += 1;
    }
    Ok(made)
}

/// `places` split into runs that share a page, in order.
fn by_page(places: &[Place]) -> Vec<(usize, Vec<Place>)> {
    let mut out: Vec<(usize, Vec<Place>)> = Vec::new();
    for p in places {
        match out.last_mut() {
            Some((page, v)) if *page == p.page => v.push(*p),
            _ => out.push((p.page, vec![*p])),
        }
    }
    out
}

/// A caret box centred on a zero-width anchor, as wide as half the line is
/// tall.
fn caret_rect(anchor: Rect) -> Rect {
    let h = (anchor.ury - anchor.lly).max(4.0);
    Rect {
        llx: anchor.llx - h / 4.0,
        lly: anchor.lly,
        urx: anchor.llx + h / 4.0,
        ury: anchor.lly + h * 0.6,
    }
}

fn note_text(c: &Change) -> String {
    match c.kind {
        ChangeKind::Inserted => format!("Inserted: {}", c.new_text),
        ChangeKind::Deleted => format!("Deleted: {}", c.old_text),
        ChangeKind::Formatting => format!("Formatting changed{}", formatting_note(c)),
        _ => format!("Replaced. Was: {}", c.old_text),
    }
}

fn formatting_note(c: &Change) -> String {
    let Some(f) = c.formatting else {
        return String::new();
    };
    let mut parts = Vec::new();
    if let Some((a, b)) = f.size {
        parts.push(format!("size {a} pt to {b} pt"));
    }
    if let Some((a, b)) = f.weight {
        parts.push(format!("weight {a} to {b}"));
    }
    if f.color.is_some() {
        parts.push("colour".to_owned());
    }
    format!(": {}", parts.join(", "))
}

/// The summary page's text: what was compared, the counts and the key.
fn summary_text(r: &CompareReport, options: &ResultOptions) -> String {
    let (old, new) = options
        .labels
        .clone()
        .unwrap_or_else(|| ("the old document".to_owned(), "this document".to_owned()));
    let mut s = format!(
        "Comparison summary\n\nOld: {old} ({} words, {} pages compared)\nNew: {new} ({} words, \
         {} pages compared)\n\n",
        r.old_word_count,
        r.old_pages.len(),
        r.new_word_count,
        r.new_pages.len(),
    );
    if r.is_identical() {
        s.push_str("No differences in the text.\n");
    } else {
        s.push_str(&format!(
            "{} changes: {} inserted, {} deleted, {} replaced, {} formatting.\n",
            r.changes.len(),
            r.count(ChangeKind::Inserted),
            r.count(ChangeKind::Deleted),
            r.count(ChangeKind::Replaced),
            r.count(ChangeKind::Formatting),
        ));
    }
    s.push_str(
        "\nKey: blue highlight = inserted text. Orange highlight = replaced text (its note \
         holds the old text). Red caret = deleted text (its note holds the removed words). \
         Purple underline = formatting change.\n\nOnly text is compared, in content order: \
         images, vector art, annotations and form values are not. Text moved within a page's \
         content shows as a deletion and an insertion.\n",
    );
    let (uo, un) = r.unreadable_pages;
    if uo + un > 0 {
        s.push_str(&format!(
            "\n{uo} old and {un} new pages could not be read; their text is missing from this \
             comparison.\n"
        ));
    }
    if r.truncated {
        s.push_str("\nA stretch exceeded the work limit and is reported as one replacement.\n");
    }
    if let Some(note) = &options.summary_note {
        s.push('\n');
        s.push_str(note);
    }
    s
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)] // tests index vectors they just built
mod tests {
    use super::*;

    fn place(page: usize) -> Place {
        Place {
            page,
            rect: Rect {
                llx: 0.0,
                lly: 0.0,
                urx: 1.0,
                ury: 1.0,
            },
        }
    }

    #[test]
    fn places_group_by_consecutive_page() {
        let g = by_page(&[place(0), place(0), place(1), place(0)]);
        let pages: Vec<_> = g.iter().map(|(p, v)| (*p, v.len())).collect();
        assert_eq!(pages, [(0, 2), (1, 1), (0, 1)]);
    }

    #[test]
    fn a_caret_straddles_its_anchor() {
        let r = caret_rect(Rect {
            llx: 100.0,
            lly: 50.0,
            urx: 100.0,
            ury: 62.0,
        });
        assert!(r.llx < 100.0 && r.urx > 100.0);
        assert_eq!(r.lly, 50.0);
    }
}
