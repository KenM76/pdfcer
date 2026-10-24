//! Compare the text of two PDFs: what was inserted, deleted, replaced or
//! (optionally) reformatted between an old and a new version.
//!
//! Text is extracted from each document (the selected pages, in page
//! order, as one continuous word sequence, so a passage that reflowed onto
//! the next page is not a change), split into words, and diffed with
//! Myers' algorithm. Every change carries the words' text and where they
//! were drawn on each side, merged into one rectangle per line.
//!
//! Word segmentation is pdfcer's inference over the extractor's derived
//! spacing (rule 4): the report says how many words each side had so a
//! caller can disclose it. Invisible text (render mode 3, an OCR layer) is
//! compared like any other text. Artifacts (running heads, folios) are
//! skipped by default. Images, vector art and annotations are not
//! compared here.

mod changes;
mod diff;
mod result;
mod words;

pub use result::{
    DELETED_COLOR, FORMATTING_COLOR, INSERTED_COLOR, REPLACED_COLOR, ResultError, ResultOptions,
    ResultReport, Unmarked, mark_changes,
};

use std::ops::Range;

use pdfcer_model::page_tree::{self, Rect};

use crate::crypto::PermissionBit;
use crate::document::Document;
use crate::text_extract::{self, ExtractError, ExtractOptions, TextColor};
use words::Word;

/// Default for [`CompareOptions::step_budget`].
pub const DEFAULT_STEP_BUDGET: u64 = 100_000_000;

/// Which of the two compared documents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    /// The earlier version.
    Old,
    /// The later version.
    New,
}

/// What to compare and how.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CompareOptions {
    /// Zero-based pages of the old document, in the order to read them.
    /// `None` is every page.
    pub old_pages: Option<Vec<usize>>,
    /// Zero-based pages of the new document. `None` is every page.
    pub new_pages: Option<Vec<usize>>,
    /// Also report words whose text is unchanged but whose size, weight or
    /// fill colour changed. Default `false`.
    pub formatting: bool,
    /// Compare artifact text (running heads, folios, watermarks).
    /// Default `false`.
    pub include_artifacts: bool,
    /// The diff's work ceiling, in inner-loop steps. A stretch whose
    /// shortest edit cannot be found within it is reported as one
    /// [`ChangeKind::Replaced`] and [`CompareReport::truncated`] is set.
    /// Default [`DEFAULT_STEP_BUDGET`].
    pub step_budget: u64,
}

impl Default for CompareOptions {
    fn default() -> Self {
        Self {
            old_pages: None,
            new_pages: None,
            formatting: false,
            include_artifacts: false,
            step_budget: DEFAULT_STEP_BUDGET,
        }
    }
}

impl CompareOptions {
    /// Restrict one side to `pages` (zero-based, in reading order).
    #[must_use]
    pub fn with_pages(mut self, side: Side, pages: Vec<usize>) -> Self {
        match side {
            Side::Old => self.old_pages = Some(pages),
            Side::New => self.new_pages = Some(pages),
        }
        self
    }

    /// Set [`Self::formatting`].
    #[must_use]
    pub const fn with_formatting(mut self, on: bool) -> Self {
        self.formatting = on;
        self
    }

    /// Set [`Self::include_artifacts`].
    #[must_use]
    pub const fn with_artifacts(mut self, on: bool) -> Self {
        self.include_artifacts = on;
        self
    }

    /// Set [`Self::step_budget`].
    #[must_use]
    pub const fn with_step_budget(mut self, steps: u64) -> Self {
        self.step_budget = steps;
        self
    }
}

/// What kind of difference a [`Change`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ChangeKind {
    /// Words only in the new document.
    Inserted,
    /// Words only in the old document.
    Deleted,
    /// Old words replaced by different new words.
    Replaced,
    /// The same words, drawn with a different size, weight or colour.
    Formatting,
}

/// A run of words on one line of one page, in default user space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Place {
    /// Zero-based page index in that document.
    pub page: usize,
    /// The words' bounds. Zero-width for an [`Change::anchor`].
    pub rect: Rect,
}

/// How a formatting change differs. A field is `Some` only when that
/// property changed.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct FormattingDelta {
    /// Old and new effective font size, in points.
    pub size: Option<(f32, f32)>,
    /// Old and new font weight (100–900).
    pub weight: Option<(u16, u16)>,
    /// Old and new fill colour. A colour in a space pdfcer does not decode
    /// is [`TextColor::Other`], and two `Other`s are not reported.
    pub color: Option<(Option<TextColor>, Option<TextColor>)>,
}

/// One difference between the two documents.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Change {
    /// What kind of difference.
    pub kind: ChangeKind,
    /// The old words' indices into the old word sequence (empty for
    /// [`ChangeKind::Inserted`]).
    pub old_words: Range<usize>,
    /// The new words' indices (empty for [`ChangeKind::Deleted`]).
    pub new_words: Range<usize>,
    /// The old words, space-joined.
    pub old_text: String,
    /// The new words, space-joined.
    pub new_text: String,
    /// Where the old words were drawn, one entry per line.
    pub old_places: Vec<Place>,
    /// Where the new words are drawn, one entry per line.
    pub new_places: Vec<Place>,
    /// For a one-sided change, the point on the other side where it
    /// happened: the left edge of the next word there, else the right edge
    /// of the previous one. `None` for two-sided changes, or when the other
    /// side has no positioned word.
    pub anchor: Option<(Side, Place)>,
    /// For [`ChangeKind::Formatting`], what changed.
    pub formatting: Option<FormattingDelta>,
}

/// The result of [`compare_documents`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct CompareReport {
    /// The differences, in new-document order.
    pub changes: Vec<Change>,
    /// Words compared on the old side.
    pub old_word_count: usize,
    /// Words compared on the new side.
    pub new_word_count: usize,
    /// The zero-based pages compared on each side.
    pub old_pages: Vec<usize>,
    /// See [`Self::old_pages`].
    pub new_pages: Vec<usize>,
    /// Pages whose content could not be read, per side: their text is
    /// missing from the comparison.
    pub unreadable_pages: (usize, usize),
    /// Some stretch exceeded [`CompareOptions::step_budget`] and is
    /// reported as one coarse replacement.
    pub truncated: bool,
}

impl CompareReport {
    /// Whether the compared text is identical.
    #[must_use]
    pub fn is_identical(&self) -> bool {
        self.changes.is_empty()
    }

    /// How many changes of `kind` there are.
    #[must_use]
    pub fn count(&self, kind: ChangeKind) -> usize {
        self.changes.iter().filter(|c| c.kind == kind).count()
    }
}

/// Why a comparison could not run.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum CompareError {
    /// The document is encrypted and its permissions withhold content
    /// copying (ISO 32000-2 Table 22, bit 5), which is what extracting
    /// its text for comparison is.
    #[error("the {side:?} document does not permit copying its content")]
    CopyNotPermitted {
        /// Which document.
        side: Side,
    },
    /// A requested page does not exist.
    #[error("page {} of the {side:?} document does not exist ({count} pages)", index + 1)]
    NoSuchPage {
        /// Which document.
        side: Side,
        /// The zero-based index requested.
        index: usize,
        /// The document's page count.
        count: usize,
    },
    /// Text extraction failed.
    #[error("{side:?} document: {source}")]
    Extract {
        /// Which document.
        side: Side,
        /// The cause.
        #[source]
        source: ExtractError,
    },
}

/// Compare the text of `old` against `new`.
///
/// # Errors
///
/// [`CompareError::CopyNotPermitted`] when either document's permissions
/// withhold copying; [`CompareError::NoSuchPage`] for a page past the end;
/// [`CompareError::Extract`] when a page tree cannot be walked.
///
/// # Examples
///
/// ```no_run
/// use pdfcer_core::compare::{compare_documents, ChangeKind, CompareOptions};
/// use pdfcer_core::document::Document;
///
/// let old = Document::load("v1.pdf")?;
/// let new = Document::load("v2.pdf")?;
/// let report = compare_documents(&old, &new, &CompareOptions::default())?;
/// for change in &report.changes {
///     if change.kind == ChangeKind::Replaced {
///         println!("{:?} -> {:?}", change.old_text, change.new_text);
///     }
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn compare_documents(
    old: &Document,
    new: &Document,
    options: &CompareOptions,
) -> Result<CompareReport, CompareError> {
    let a = side_words(old, Side::Old, options.old_pages.as_deref(), options)?;
    let b = side_words(new, Side::New, options.new_pages.as_deref(), options)?;
    let (ia, ib) = changes::intern(&a.words, &b.words);
    let script = diff::diff(&ia, &ib, options.step_budget);
    let changes = changes::build(&script.ops, &a.words, &b.words, options.formatting);
    Ok(CompareReport {
        changes,
        old_word_count: a.words.len(),
        new_word_count: b.words.len(),
        old_pages: a.pages,
        new_pages: b.pages,
        unreadable_pages: (a.unreadable, b.unreadable),
        truncated: script.truncated,
    })
}

/// One side's words and what was read to get them.
struct SideWords {
    words: Vec<Word>,
    pages: Vec<usize>,
    unreadable: usize,
}

fn side_words(
    doc: &Document,
    side: Side,
    pages: Option<&[usize]>,
    options: &CompareOptions,
) -> Result<SideWords, CompareError> {
    if doc
        .encryption()
        .is_some_and(|e| !e.grants(PermissionBit::Copy))
    {
        return Err(CompareError::CopyNotPermitted { side });
    }
    let extract = |source| CompareError::Extract { side, source };
    let count = page_tree::pages(doc)
        .map_err(|e| extract(ExtractError::PageTree(e)))?
        .len();
    let pages = pages.map_or_else(|| (0..count).collect(), <[usize]>::to_vec);
    if let Some(&index) = pages.iter().find(|&&i| i >= count) {
        return Err(CompareError::NoSuchPage { side, index, count });
    }
    let opts = ExtractOptions::default().with_provenance(options.formatting);
    let text = text_extract::extract_pages(doc, &pages, &opts).map_err(extract)?;
    let mut words = Vec::new();
    for page in &text.pages {
        words::page_words(page, options.include_artifacts, &mut words);
    }
    Ok(SideWords {
        words,
        pages,
        unreadable: usize::try_from(text.diagnostics.pages_unreadable).unwrap_or(usize::MAX),
    })
}
