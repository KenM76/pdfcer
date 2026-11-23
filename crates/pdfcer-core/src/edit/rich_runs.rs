//! Filling a rich-text field from styled runs rather than markup.

use super::{EditError, EditSession, RichFillOutcome};
use crate::richtext::{self, FlatText, Run};

impl EditSession {
    /// **Fill a rich-text field from styled runs** (§12.7.3.4): exactly
    /// [`Self::fill_rich_text_field`] with `rv` = [`richtext::to_xhtml`]`(runs)`,
    /// so it is one undoable fill, `/V` is the runs' plain text joined as
    /// `flat` says, and the appearance is drawn from the runs.
    ///
    /// Reading the field's `/RV` back with [`richtext::parse`] returns
    /// `runs` in `to_xhtml`'s normal form, which lets an edit keep the
    /// styles of the characters it did not change.
    ///
    /// # Errors
    ///
    /// As [`Self::fill_rich_text_field`]; [`EditError::InvalidRichText`]
    /// only when the written value exceeds [`richtext::MAX_RICH_BYTES`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use pdfcer_core::edit::EditSession;
    /// use pdfcer_core::richtext::{FlatText, Run, Style};
    /// # fn demo(session: &mut EditSession) -> Result<(), pdfcer_core::edit::EditError> {
    /// let bold = Style { weight: Some(700), ..Style::default() };
    /// let runs = [
    ///     Run { text: "Total: ".into(), style: Style::default(), paragraph: 0 },
    ///     Run { text: "42".into(), style: bold, paragraph: 0 },
    /// ];
    /// let out = session.fill_rich_text_field_runs("notes", &runs, FlatText::ParagraphLines)?;
    /// // Show out.notes off-canvas, as for fill_rich_text_field.
    /// # let _ = out; Ok(()) }
    /// ```
    pub fn fill_rich_text_field_runs(
        &mut self,
        fqn: &str,
        runs: &[Run],
        flat: FlatText,
    ) -> Result<RichFillOutcome, EditError> {
        self.fill_rich_text_field(fqn, &richtext::to_xhtml(runs), flat)
    }
}
