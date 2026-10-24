//! Word segmentation of extracted page text, for comparison.
//!
//! A word is a maximal run of non-whitespace characters. Derived
//! whitespace runs (word spaces and line breaks the extractor inferred)
//! end a word; a run boundary alone does not, so a word whose letters
//! change font mid-way stays one word.

use pdfcer_model::page_tree::Rect;

use crate::text_extract::{PageText, TextColor, TextOrigin, TextRun};

/// One word, with where it was drawn and how it looked.
#[derive(Debug, Clone)]
pub(crate) struct Word {
    pub(crate) text: String,
    /// Zero-based page index in the document.
    pub(crate) page: usize,
    /// Union of the word's glyph cells, or the run's box for an
    /// `/ActualText` run; `None` when the extractor had no geometry.
    pub(crate) rect: Option<Rect>,
    /// The first glyph's look, for formatting comparison.
    pub(crate) style: Option<Style>,
}

/// The look of a word's first glyph.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Style {
    pub(crate) size: f32,
    pub(crate) weight: u16,
    pub(crate) color: Option<TextColor>,
}

/// Builder state across the runs of one page.
struct Builder<'a> {
    words: &'a mut Vec<Word>,
    page: usize,
    current: Option<Word>,
}

impl Builder<'_> {
    fn push_char(&mut self, ch: char, cell: Option<Rect>, style: Option<Style>) {
        if ch.is_whitespace() {
            self.flush();
            return;
        }
        let word = self.current.get_or_insert_with(|| Word {
            text: String::new(),
            page: self.page,
            rect: None,
            style,
        });
        word.text.push(ch);
        word.rect = union(word.rect, cell);
    }

    fn flush(&mut self) {
        if let Some(w) = self.current.take() {
            self.words.push(w);
        }
    }
}

/// Append `page`'s words to `out`. Artifact runs are skipped unless
/// `include_artifacts`.
pub(crate) fn page_words(page: &PageText, include_artifacts: bool, out: &mut Vec<Word>) {
    let mut b = Builder {
        words: out,
        page: page.page_index,
        current: None,
    };
    for run in &page.runs {
        if run.artifact.is_some() && !include_artifacts {
            b.flush();
            continue;
        }
        match run.origin {
            TextOrigin::Glyphs => glyph_run(&mut b, run),
            TextOrigin::DerivedWordSpace | TextOrigin::DerivedLineBreak => b.flush(),
            _ => {
                b.flush();
                for ch in run.text.chars() {
                    b.push_char(ch, run.bbox, None);
                }
                b.flush();
            }
        }
    }
    b.flush();
}

fn glyph_run(b: &mut Builder<'_>, run: &TextRun) {
    for g in &run.glyphs {
        let start = g.text_start as usize;
        let Some(text) = run.text.get(start..start + g.text_len as usize) else {
            continue;
        };
        let style = Style {
            size: g.size,
            weight: g.weight.value,
            color: g.provenance.as_ref().and_then(|p| p.fill_color),
        };
        for ch in text.chars() {
            b.push_char(ch, Some(g.cell()), Some(style));
        }
    }
}

/// The smallest rectangle covering both.
pub(crate) fn union(a: Option<Rect>, b: Option<Rect>) -> Option<Rect> {
    match (a, b) {
        (Some(a), Some(b)) => Some(Rect {
            llx: a.llx.min(b.llx),
            lly: a.lly.min(b.lly),
            urx: a.urx.max(b.urx),
            ury: a.ury.max(b.ury),
        }),
        (a, b) => a.or(b),
    }
}
