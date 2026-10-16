//! Edit a recognised paragraph (an OCR layer's invisible text, render mode 3,
//! ISO 32000-2 §9.3.6) word by word, keeping every word where it was: the
//! planning behind
//! [`EditSession::edit_recognised_block_text`](crate::edit::EditSession::edit_recognised_block_text).
//!
//! A recognised word sits over its word in the scan; that position is what
//! search, selection and copy land on. So nothing is re-wrapped. The old and
//! new texts are diffed by word: a kept word keeps its codes and position, a
//! replaced word is set at the old word's origin with its horizontal scaling
//! (`Tz`, §9.3.4) fitted to the old word's width, a removed word goes, and a
//! word with no counterpart joins its neighbour (reported). Only the show
//! operators holding a changed word are rewritten, and each rewrite ends with
//! the text matrix, font and scaling the original left (§9.4.3), so nothing
//! after it moves.

mod diff;
mod rewrite;
mod words;

use crate::content::ContentStream;
use crate::page_tree::{self, Page, Rect};
use crate::text_edit::cause::UnsupportedCause;
use crate::text_edit::edit::{EditOptions, FontWrites, OpRec, TextEditPreview, walk_records};
use crate::text_edit::model::EditableTextModel;
use crate::text_edit::reflow::reflow_recognition_options;
use crate::text_edit::{BlockEditError, BlockEditLine, ReflowApplyError};
use crate::text_extract::{self, ExtractOptions, PageText};
use crate::view::DocumentView;

use diff::Fate;

/// One word of a recognised block, as [`RecognisedBlockHit`] lists it.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RecognisedWord {
    /// The word's characters.
    pub text: String,
    /// The word's box, default user space.
    pub bbox: Rect,
    /// Which of the block's lines (top first) the word is on.
    pub line: usize,
}

/// The recognised block under a page point, and its words.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RecognisedBlockHit {
    /// The index
    /// [`edit_recognised_block_text`](crate::edit::EditSession::edit_recognised_block_text)
    /// takes. Recognised blocks are numbered among the page's invisible
    /// text only, so this differs from `block_at_point`'s numbering.
    pub block_index: usize,
    /// The block's words joined by single spaces, its lines by `\n`. Passing
    /// it back unchanged edits nothing.
    pub text: String,
    /// Every word, in reading order.
    pub words: Vec<RecognisedWord>,
    /// The block's box.
    pub bbox: Rect,
}

/// How one replaced word was fitted to its old box.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct WordFit {
    /// The word as it was.
    pub old_text: String,
    /// What replaced it (several new words joined by spaces when a word with
    /// no counterpart was merged into it).
    pub new_text: String,
    /// The old word's first glyph origin, where the new text starts.
    pub origin: (f64, f64),
    /// The horizontal scaling the new text is set at, percent (`Tz`).
    pub horizontal_scale: f64,
    /// The fit hit the 25%–400% bound of the original scaling, so the new
    /// text is narrower or wider than the old word's box.
    pub clamped: bool,
}

/// What an `edit_recognised_block_text` did and inferred. Every field is
/// disclosure (rule 4); [`Self::disclosures`] carries the sentences.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RecognisedBlockEditReport {
    /// The block edited.
    pub block_index: usize,
    /// Words left exactly as they were.
    pub words_kept: usize,
    /// Old words given new text (see [`Self::fits`]).
    pub words_replaced: usize,
    /// Old words removed.
    pub words_deleted: usize,
    /// New words with no old word to take their place, set inside a
    /// neighbouring replaced word's box.
    pub words_merged: usize,
    /// One per replaced word, in reading order.
    pub fits: Vec<WordFit>,
    /// Characters a font gained a glyph, code or `/ToUnicode` entry for.
    pub glyphs_added: Vec<char>,
    /// The content stream written; `None` when the text was unchanged and
    /// nothing was written.
    pub content_object: Option<u32>,
    /// Every operator-facing disclosure, verbatim.
    pub disclosures: Vec<String>,
}

/// The layout an `edit_recognised_block_text` with the same arguments
/// would leave, for drawing while the operator types: every surviving word
/// positioned, kept words where they are and each replacement at its old
/// word's origin under its [`WordFit`] scaling.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RecognisedBlockEditPreview {
    /// The block's lines after the edit, top to bottom: each line's text,
    /// its first glyph origin, its baseline and its width along the
    /// baseline. A line whose every word is deleted is absent.
    pub lines: Vec<BlockEditLine>,
    /// The block's box before the edit.
    pub old_bbox: Rect,
    /// The box the surviving glyphs occupy (advance by ascent-to-descent,
    /// §9.8 Table 122); `old_bbox` when nothing survives.
    pub new_bbox: Rect,
    /// Every surviving glyph, positioned, for drawing
    /// (`pdfcer_render::edit_preview`): one preview per font resource the
    /// surviving text uses, each with its font, colours, render mode (3 for
    /// an OCR layer) and one matrix per code. The matrices are in reading
    /// order within a preview; a kept and a replaced word in one font share
    /// a preview.
    pub runs: Vec<TextEditPreview>,
    /// What the commit would report (`content_object` as planned, before
    /// any decoupling).
    pub report: RecognisedBlockEditReport,
}

/// A planned recognised-block edit.
pub(crate) struct RecognisedPlan {
    /// The page's new concatenated content; `None` when nothing changes.
    pub(crate) new_content: Option<Vec<u8>>,
    pub(crate) font_writes: FontWrites,
    pub(crate) preview: RecognisedBlockEditPreview,
}

/// The page as both verbs read it: its content, operator records and the
/// invisible-only extraction the model numbers blocks in.
struct Scene {
    page: Page,
    stream: ContentStream,
    recs: Vec<OpRec>,
    text: PageText,
}

impl Scene {
    fn read(doc: &DocumentView<'_>, page_index: usize) -> Result<Self, BlockEditError> {
        let pages = page_tree::pages_in(doc).map_err(ReflowApplyError::from)?;
        let page = pages
            .get(page_index)
            .ok_or(ReflowApplyError::PageIndex(page_index))?
            .clone();
        let options = ExtractOptions::default().with_provenance(true);
        let extracted = text_extract::extract_page_view(doc, &page, page_index, &options)
            .map_err(ReflowApplyError::from)?;
        let stream = ContentStream::from_page(doc, &page).map_err(ReflowApplyError::from)?;
        let recs = walk_records(doc, &page.resources, &stream);
        Ok(Self {
            page,
            stream,
            recs,
            text: words::invisible_only(&extracted),
        })
    }

    fn model(&self) -> EditableTextModel<'_> {
        EditableTextModel::recognize(&self.text, &reflow_recognition_options())
    }
}

/// The recognised block under `(x, y)` on page `page_index`.
///
/// # Errors
///
/// A bad page index or an unreadable page.
pub(crate) fn recognised_hit(
    doc: &DocumentView<'_>,
    page_index: usize,
    x: f64,
    y: f64,
) -> Result<Option<RecognisedBlockHit>, BlockEditError> {
    let scene = Scene::read(doc, page_index)?;
    let model = scene.model();
    let Some(block_index) = model.hit_test(x, y).and_then(|p| model.block_at(p)) else {
        return Ok(None);
    };
    let Some(block) = model.blocks().get(block_index) else {
        return Ok(None);
    };
    let found = words::block_words(&model, &scene.text, block, &scene.recs);
    let text = words::joined(&found);
    let words = found
        .iter()
        .map(|w| RecognisedWord {
            text: w.text.clone(),
            bbox: w.bbox,
            line: w.line,
        })
        .collect();
    Ok(Some(RecognisedBlockHit {
        block_index,
        text,
        words,
        bbox: block.bbox,
    }))
}

/// Plan replacing recognised block `block_index`'s words with `text`.
///
/// # Errors
///
/// [`BlockEditError`]: a bad index, empty text, a refused character (every
/// one named), or an operator the rewrite cannot reproduce.
pub(crate) fn plan_recognised(
    doc: &DocumentView<'_>,
    page_index: usize,
    block_index: usize,
    text: &str,
    opts: &EditOptions,
) -> Result<RecognisedPlan, BlockEditError> {
    let new_words: Vec<&str> = text.split_whitespace().collect();
    if new_words.is_empty() {
        return Err(BlockEditError::EmptyText);
    }
    let scene = Scene::read(doc, page_index)?;
    let model = scene.model();
    let block = model
        .blocks()
        .get(block_index)
        .ok_or(BlockEditError::unsupported(
            UnsupportedCause::ShowOperatorsNotFound,
        ))?;
    let old = words::block_words(&model, &scene.text, block, &scene.recs);
    let fates = diff::fates(&old, &new_words)?;
    let mut report = counted(block_index, &fates, &new_words);
    let ctx = rewrite::Ctx {
        doc,
        page: &scene.page,
        page_index,
        stream: &scene.stream,
        recs: &scene.recs,
        text: &scene.text,
        opts,
    };
    if report.words_replaced + report.words_deleted == 0 {
        let layout = rewrite::layout(&ctx, &old, &fates, &[], &[])?;
        return Ok(RecognisedPlan {
            new_content: None,
            font_writes: FontWrites::default(),
            preview: previewed(layout, block.bbox, report),
        });
    }
    let done = rewrite::rewrite(&ctx, &old, &fates)?;
    let layout = rewrite::layout(&ctx, &old, &fates, &done.reps, &done.encs)?;
    report.fits = done.fits;
    report.glyphs_added = done.glyphs_added;
    report.disclosures = done.disclosures;
    report.content_object = scene.page.contents.first().map(|id| id.num);
    disclose(&mut report);
    Ok(RecognisedPlan {
        new_content: Some(done.content),
        font_writes: done.writes,
        preview: previewed(layout, block.bbox, report),
    })
}

/// The report with its word counts, before any rewrite.
fn counted(block_index: usize, fates: &[Fate], new_words: &[&str]) -> RecognisedBlockEditReport {
    let mut report = RecognisedBlockEditReport {
        block_index,
        words_kept: 0,
        words_replaced: 0,
        words_deleted: 0,
        words_merged: diff::merged(fates, new_words),
        fits: Vec::new(),
        glyphs_added: Vec::new(),
        content_object: None,
        disclosures: Vec::new(),
    };
    for f in fates {
        match f {
            Fate::Keep => report.words_kept += 1,
            Fate::Replace(_) => report.words_replaced += 1,
            Fate::Delete => report.words_deleted += 1,
        }
    }
    report
}

fn previewed(
    layout: rewrite::Layout,
    old_bbox: Rect,
    report: RecognisedBlockEditReport,
) -> RecognisedBlockEditPreview {
    RecognisedBlockEditPreview {
        lines: layout.lines,
        old_bbox,
        new_bbox: layout.bbox.unwrap_or(old_bbox),
        runs: layout.runs,
        report,
    }
}

/// The report's counts and fits as sentences.
fn disclose(r: &mut RecognisedBlockEditReport) {
    if r.words_merged > 0 {
        r.disclosures.push(format!(
            "{} new word(s) had no recognised word to replace and were set inside a neighbouring word's box",
            r.words_merged
        ));
    }
    for f in r.fits.iter().filter(|f| f.clamped) {
        r.disclosures.push(format!(
            "\"{}\" could not be fitted to the box of \"{}\" (scaling held at {:.0}%), so it no longer covers exactly that word",
            f.new_text, f.old_text, f.horizontal_scale
        ));
    }
}
