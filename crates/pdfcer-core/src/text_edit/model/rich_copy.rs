//! Formatted copy: a selection (or a page) as paragraphs of styled runs, for
//! a clipboard a word processor pastes with its formatting.
//!
//! Each recognised block is one paragraph; inside it a wrapped line joins
//! with a space and a broken one ([`LineEnd::Break`]) with a line break, as
//! [`EditableTextModel::block_text_with_breaks`] spells them. Every run
//! carries the size, weight, slant, family, decoration and colour of its
//! glyphs. All of it is DERIVED from layout and font data, and
//! [`RichTextOutcome`] counts what was guessed or could not be carried.

mod fonts;
mod write;

pub use fonts::{FontFace, PageFonts};

use super::{EditableTextModel, GlyphRef, LineEnd, TextPosition, lists::derived_space_between};
use crate::text_edit::decoration::PageDecorations;
use crate::text_extract::{TextColor, WeightSource};

/// Optional page data that sharpens a formatted copy. Without it, runs still
/// carry size and weight; family, slant, underline and colour need it.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct RichTextSources<'s> {
    /// Families and slant ([`PageFonts::read`]). Needs an extraction with
    /// `capture_provenance`.
    pub fonts: Option<&'s PageFonts>,
    /// Underline and strikethrough
    /// ([`page_decorations`](crate::text_edit::decoration::page_decorations)).
    pub decorations: Option<&'s PageDecorations>,
}

impl<'s> RichTextSources<'s> {
    /// Use `fonts` for family and slant.
    #[must_use]
    pub const fn with_fonts(mut self, fonts: &'s PageFonts) -> Self {
        self.fonts = Some(fonts);
        self
    }

    /// Use `decorations` for underline and strikethrough.
    #[must_use]
    pub const fn with_decorations(mut self, decorations: &'s PageDecorations) -> Self {
        self.decorations = Some(decorations);
        self
    }
}

/// How one run looks.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct SpanStyle {
    /// Font family, when [`RichTextSources::fonts`] named one.
    pub family: Option<String>,
    /// Drawn size in points, to the nearest half point.
    pub size: f32,
    /// Weight 600 or more ([`FontWeight::is_bold`](crate::text_extract::FontWeight::is_bold)).
    pub bold: bool,
    /// Italic or oblique ([`FontFace::italic`]).
    pub italic: bool,
    /// Underlined.
    pub underline: bool,
    /// Struck through.
    pub strikethrough: bool,
    /// Fill colour as 8-bit RGB, for DeviceGray and DeviceRGB text only;
    /// `None` leaves the paste target's default (black).
    pub color: Option<[u8; 3]>,
}

/// A stretch of text in one style. `text` may contain `\n`, a line break
/// inside the paragraph.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RichSpan {
    /// The characters.
    pub text: String,
    /// Their style.
    pub style: SpanStyle,
}

/// One recognised block. A blank line on the page is a paragraph with no
/// spans.
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct RichParagraph {
    /// The runs, in reading order.
    pub spans: Vec<RichSpan>,
    /// The block's line size in points (the height an empty paragraph keeps).
    pub size: f32,
    /// Extra space below, in points: the gap to the next block in the same
    /// column beyond ordinary leading. DERIVED.
    pub space_after: f32,
}

/// What a formatted copy guessed or dropped (rule 4: disclosed, not silent).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RichTextOutcome {
    /// Paragraphs written; each is a recognised block. DERIVED.
    pub paragraphs: usize,
    /// Glyphs whose bold/normal came from a font name or fill-and-stroke
    /// rather than a declared weight. DERIVED.
    pub weights_derived: usize,
    /// Glyphs written without a family (no provenance, or an unnamed font).
    pub families_missing: usize,
    /// Glyphs in a colour space this export does not carry (CMYK and
    /// others); they paste in the target's default colour.
    pub colours_dropped: usize,
    /// [`PageDecorations::forms_unread`] of the decorations supplied: form
    /// XObjects whose underline and strikethrough could not be read, so their
    /// text pastes undecorated. Zero when no decorations were supplied.
    pub decoration_forms_unread: usize,
}

/// A formatted copy. Serialise it with [`Self::to_html`], [`Self::to_rtf`],
/// [`Self::to_cf_html`] or [`Self::plain_text`].
#[derive(Debug, Clone, PartialEq, Default)]
#[non_exhaustive]
pub struct RichText {
    /// The paragraphs, in block order.
    pub paragraphs: Vec<RichParagraph>,
    /// The disclosure.
    pub outcome: RichTextOutcome,
}

impl EditableTextModel<'_> {
    /// The text between `range`'s two ends (either order), or the whole page
    /// when `range` is `None`, as formatted paragraphs. A glyph is included
    /// when it lies wholly inside the range.
    ///
    /// ```
    /// # use pdfcer_core::text_edit::model::{EditableTextModel, BlockRecognitionOptions, RichTextSources};
    /// # use pdfcer_core::text_extract::PageText;
    /// let page = PageText::default();
    /// let model = EditableTextModel::recognize(&page, &BlockRecognitionOptions::default());
    /// let rich = model.rich_text(None, &RichTextSources::default());
    /// assert!(rich.to_html().contains("<!--StartFragment-->"));
    /// ```
    #[must_use]
    pub fn rich_text(
        &self,
        range: Option<(TextPosition, TextPosition)>,
        sources: &RichTextSources<'_>,
    ) -> RichText {
        let bounds = range.map(|(a, b)| {
            let (a, b) = (a.key(), b.key());
            (a.min(b), a.max(b))
        });
        let mut out = RichText::default();
        let mut columns: Vec<(usize, f32, usize)> = Vec::new();
        for block in &self.blocks {
            let mut para = Paragraph::default();
            let ends = self.line_ends(block);
            for (k, &li) in block.line_indices.iter().enumerate() {
                let Some(line) = self.lines.get(li) else {
                    continue;
                };
                para.size = para.size.max(line.size);
                let mut prev: Option<GlyphRef> = None;
                for &g in &line.glyphs {
                    if !self.inside(g, bounds) {
                        continue;
                    }
                    para.touched = true;
                    if prev.is_some_and(|p| derived_space_between(self.page, p, g)) {
                        para.push_text(" ");
                    }
                    prev = Some(g);
                    self.push_glyph(&mut para, g, sources, &mut out.outcome);
                }
                if prev.is_some() {
                    para.pending = match ends.get(k) {
                        Some(LineEnd::Break) => Some("\n"),
                        Some(LineEnd::Wrap) => Some(" "),
                        _ => None,
                    };
                }
            }
            if para.touched {
                columns.push((block.column, block.bbox.lly as f32, out.paragraphs.len()));
                set_space_after(
                    &mut out.paragraphs,
                    &columns,
                    block.column,
                    block.bbox.ury as f32,
                );
                out.paragraphs.push(para.finish());
            }
        }
        out.outcome.paragraphs = out.paragraphs.len();
        out.outcome.decoration_forms_unread = sources.decorations.map_or(0, |d| d.forms_unread);
        out
    }

    fn inside(&self, g: GlyphRef, bounds: Option<((usize, usize), (usize, usize))>) -> bool {
        let Some((lo, hi)) = bounds else {
            return true;
        };
        let Some(glyph) = self.glyph(g) else {
            return false;
        };
        let start = glyph.text_start as usize;
        (g.run, start) >= lo && (g.run, start + glyph.text_len as usize) <= hi
    }

    fn push_glyph(
        &self,
        para: &mut Paragraph,
        g: GlyphRef,
        sources: &RichTextSources<'_>,
        outcome: &mut RichTextOutcome,
    ) {
        let (Some(glyph), Some(run)) = (self.glyph(g), self.page.runs.get(g.run)) else {
            return;
        };
        let start = glyph.text_start as usize;
        let Some(text) = run.text.get(start..start + glyph.text_len as usize) else {
            return;
        };
        if matches!(
            glyph.weight.source,
            WeightSource::FontName | WeightSource::Synthetic
        ) {
            outcome.weights_derived += 1;
        }
        let prov = glyph.provenance.as_ref();
        let face = prov.and_then(|p| {
            let fonts = sources.fonts?;
            fonts.face(p.content_stream, p.font_resource.as_deref()?)
        });
        if face.is_none() {
            outcome.families_missing += 1;
        }
        let deco = prov.zip(sources.decorations).map(|(p, d)| d.of(p));
        let color = match prov.and_then(|p| p.fill_color) {
            Some(TextColor::Gray(v)) => Some([to_u8(v); 3]),
            Some(TextColor::Rgb(r, g, b)) => Some([to_u8(r), to_u8(g), to_u8(b)]),
            Some(_) => {
                outcome.colours_dropped += 1;
                None
            }
            None => None,
        };
        let style = SpanStyle {
            family: face.map(|f| f.family.clone()),
            size: (glyph.size * 2.0).round() / 2.0,
            bold: glyph.weight.is_bold(),
            italic: face.is_some_and(|f| f.italic),
            underline: deco.is_some_and(|d| d.underline),
            strikethrough: deco.is_some_and(|d| d.strikethrough),
            color,
        };
        para.push(text, style);
    }
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // clamped to 0..=255
fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Set the space below the last paragraph written in `column`, now that the
/// next block in it starts at `top`. Ordinary leading is about 0.2 em of
/// gap between two line boxes; anything past half an em is paragraph space.
fn set_space_after(
    paragraphs: &mut [RichParagraph],
    columns: &[(usize, f32, usize)],
    column: usize,
    top: f32,
) {
    let Some(&(_, bottom, index)) = columns.iter().rev().skip(1).find(|c| c.0 == column) else {
        return;
    };
    if let Some(p) = paragraphs.get_mut(index) {
        let gap = bottom - top;
        if gap > 0.5 * p.size {
            p.space_after = (gap - 0.2 * p.size).round();
        }
    }
}

/// A paragraph being built.
#[derive(Default)]
struct Paragraph {
    spans: Vec<RichSpan>,
    size: f32,
    /// The separator owed before the next included glyph.
    pending: Option<&'static str>,
    touched: bool,
}

impl Paragraph {
    fn push(&mut self, text: &str, style: SpanStyle) {
        if let Some(sep) = self.pending.take() {
            if let Some(last) = self.spans.last_mut() {
                last.text.truncate(last.text.trim_end().len());
            }
            self.push_text(sep);
        }
        match self.spans.last_mut() {
            Some(last) if last.style == style => last.text.push_str(text),
            _ => self.spans.push(RichSpan {
                text: text.to_owned(),
                style,
            }),
        }
    }

    /// Text in the previous run's style.
    fn push_text(&mut self, text: &str) {
        if let Some(last) = self.spans.last_mut() {
            last.text.push_str(text);
        }
    }

    fn finish(mut self) -> RichParagraph {
        if let Some(first) = self.spans.first_mut() {
            first.text = first.text.trim_start().to_owned();
        }
        if let Some(last) = self.spans.last_mut() {
            last.text.truncate(last.text.trim_end().len());
        }
        self.spans.retain(|s| !s.text.is_empty());
        if self.spans.iter().all(|s| s.text.trim().is_empty()) {
            self.spans.clear();
        }
        RichParagraph {
            spans: self.spans,
            size: self.size,
            space_after: 0.0,
        }
    }
}
