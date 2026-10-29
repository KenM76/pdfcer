//! Running text: a header or footer line on each page, written as a
//! `/Artifact << /Type /Pagination /Subtype /Header >>` (or `/Footer`)
//! marked-content sequence (ISO 32000-1 §14.8.2.2, Table 330).
//!
//! Pure types; [`crate::edit::EditSession::stamp_running_text`] writes it.
//! Drawn exactly as a Bates label is ([`crate::bates`]): non-embedded
//! Helvetica, `WinAnsiEncoding`, upright on the displayed page.

use crate::bates::{BatesPosition, winansi_code};

/// Replaced by the 1-based page number in [`RunningText::text`].
pub const PAGE_TOKEN: &str = "{page}";
/// Replaced by the document's page count in [`RunningText::text`].
pub const PAGES_TOKEN: &str = "{pages}";

/// Which edge the line runs along.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RunningTextKind {
    /// Top edge (`/Subtype /Header`).
    Header,
    /// Bottom edge (`/Subtype /Footer`).
    #[default]
    Footer,
}

impl RunningTextKind {
    /// The artifact `/Subtype` name.
    #[must_use]
    pub const fn subtype(self) -> &'static str {
        match self {
            Self::Header => "Header",
            Self::Footer => "Footer",
        }
    }
}

/// Where along its edge the line sits.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum RunningTextAlign {
    /// Left end.
    Left,
    /// Centred.
    #[default]
    Center,
    /// Right end.
    Right,
}

/// One header or footer line.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct RunningText {
    /// Header or footer.
    pub kind: RunningTextKind,
    /// Left, centre or right.
    pub align: RunningTextAlign,
    /// The words, with [`PAGE_TOKEN`] and [`PAGES_TOKEN`] filled per page.
    pub text: String,
    /// Distance in points from the displayed page edges; at least 0.
    pub margin: f64,
    /// Font size in points; above 0.
    pub font_size: f64,
    /// 0-based pages to stamp; `None` stamps every page.
    pub pages: Option<Vec<usize>>,
}

impl RunningText {
    /// A centred line of `text`, 36 pt from the edge, 10 pt, every page.
    #[must_use]
    pub fn new(kind: RunningTextKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            align: RunningTextAlign::Center,
            text: text.into(),
            margin: 36.0,
            font_size: 10.0,
            pages: None,
        }
    }

    /// The line as drawn on 1-based page `page` of `pages`.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::running_text::{RunningText, RunningTextKind};
    ///
    /// let footer = RunningText::new(RunningTextKind::Footer, "Page {page} of {pages}");
    /// assert_eq!(footer.label(2, 7), "Page 2 of 7");
    /// ```
    #[must_use]
    pub fn label(&self, page: usize, pages: usize) -> String {
        self.text
            .replace(PAGE_TOKEN, &page.to_string())
            .replace(PAGES_TOKEN, &pages.to_string())
    }

    /// Validate the text, margin and size.
    ///
    /// # Errors
    ///
    /// [`RunningTextError::Empty`], [`RunningTextError::Unencodable`] or
    /// [`RunningTextError::Geometry`].
    pub fn check(&self) -> Result<(), RunningTextError> {
        if self.text.trim().is_empty() {
            return Err(RunningTextError::Empty);
        }
        if let Some(c) = self.text.chars().find(|&c| winansi_code(c).is_none()) {
            return Err(RunningTextError::Unencodable(c));
        }
        let finite = self.margin.is_finite() && self.font_size.is_finite();
        if !finite || self.margin < 0.0 || self.font_size <= 0.0 {
            return Err(RunningTextError::Geometry {
                margin: self.margin.to_string(),
                size: self.font_size.to_string(),
            });
        }
        Ok(())
    }

    /// The Bates slot this kind and alignment draw at.
    pub(crate) const fn position(&self) -> BatesPosition {
        match (self.kind, self.align) {
            (RunningTextKind::Header, RunningTextAlign::Left) => BatesPosition::TopLeft,
            (RunningTextKind::Header, RunningTextAlign::Center) => BatesPosition::TopCenter,
            (RunningTextKind::Header, RunningTextAlign::Right) => BatesPosition::TopRight,
            (RunningTextKind::Footer, RunningTextAlign::Left) => BatesPosition::BottomLeft,
            (RunningTextKind::Footer, RunningTextAlign::Center) => BatesPosition::BottomCenter,
            (RunningTextKind::Footer, RunningTextAlign::Right) => BatesPosition::BottomRight,
        }
    }
}

/// What [`crate::edit::EditSession::stamp_running_text`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunningTextOutcome {
    /// 0-based pages stamped, ascending.
    pub pages: Vec<usize>,
    /// The line on the first stamped page.
    pub first_label: String,
}

/// Why a header or footer could not be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RunningTextError {
    /// The text is empty or only spaces.
    #[error("a header or footer needs some text")]
    Empty,
    /// A character Helvetica's `WinAnsiEncoding` has no code for.
    #[error("the header/footer character {0:?} cannot be drawn in Helvetica (WinAnsiEncoding)")]
    Unencodable(char),
    /// The margin is negative or not finite, or the size is not above 0.
    #[error(
        "a header or footer needs a margin of at least 0 and a font size above 0 (got margin {margin}, size {size})"
    )]
    Geometry {
        /// The margin given, in points.
        margin: String,
        /// The font size given, in points.
        size: String,
    },
    /// The selection names no pages.
    #[error("a header or footer needs at least one page")]
    NoPages,
}

#[cfg(test)]
mod tests;
