//! Rich text runs to plain text, for the `/V` beside an `/RV`
//! (ISO 32000-1 §12.7.3.4).

use super::{MAX_RICH_BYTES, Run};

/// How [`flat_text`] joins paragraphs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum FlatText {
    /// A line feed between paragraphs, so a reader that shows `/V` keeps
    /// the line structure. The default.
    #[default]
    ParagraphLines,
    /// The character data concatenated with nothing added — XFA 3.x's
    /// derivation of the plain value from rich text.
    Concatenated,
}

/// The plain text of `runs`: their text in order, paragraphs joined as
/// `mode` says. Under [`FlatText::ParagraphLines`] each paragraph index
/// advanced is one line feed, so empty paragraphs — a skipped index, a
/// first run past paragraph 0, a trailing empty run — stay blank lines.
/// Used for the `/V` that accompanies an `/RV` (§12.7.3.4:
/// `/V` holds the field's text without markup).
///
/// # Examples
///
/// ```
/// use pdfcer_core::richtext::{self, FlatText};
///
/// let runs = richtext::parse("<body><p>a <b>b</b></p><p>c</p></body>", None)?;
/// assert_eq!(richtext::flat_text(&runs, FlatText::ParagraphLines), "a b\nc");
/// assert_eq!(richtext::flat_text(&runs, FlatText::Concatenated), "a bc");
/// # Ok::<(), richtext::RichTextError>(())
/// ```
#[must_use]
pub fn flat_text(runs: &[Run], mode: FlatText) -> String {
    let mut out = String::new();
    // Bounds the line feeds a hostile index (`usize::MAX`) can ask for.
    let mut breaks_left = MAX_RICH_BYTES;
    let mut last = 0;
    for run in runs {
        if mode == FlatText::ParagraphLines {
            // A decrease starts a new paragraph, as in `to_xhtml`.
            let breaks = run
                .paragraph
                .checked_sub(last)
                .unwrap_or(1)
                .min(breaks_left);
            breaks_left -= breaks;
            out.extend(std::iter::repeat_n('\n', breaks));
            last = run.paragraph;
        }
        out.push_str(&run.text);
    }
    out
}
