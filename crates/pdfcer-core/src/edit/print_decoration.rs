//! [`EditSession::decorate_for_print`]: the headers, footers and printer's
//! marks that go on a printed copy only.

use super::{EditError, EditSession};
use crate::printer_marks::{PrinterMarks, PrinterMarksOutcome};
use crate::running_text::{RunningText, RunningTextOutcome};

/// What a printed copy carries beyond the document itself.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct PrintDecoration {
    /// Header and footer lines, stamped in order.
    pub running_text: Vec<RunningText>,
    /// Printer's marks, added after the running text.
    pub marks: Option<PrinterMarks>,
}

impl PrintDecoration {
    /// `true` when there is nothing to add.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.running_text.is_empty() && self.marks.is_none()
    }
}

/// What [`EditSession::decorate_for_print`] added.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct PrintDecorationReport {
    /// One outcome per [`PrintDecoration::running_text`] entry, same order.
    pub running_text: Vec<RunningTextOutcome>,
    /// The printer's marks outcome, when marks were asked for.
    pub marks: Option<PrinterMarksOutcome>,
}

impl EditSession {
    /// **Decorate for print**: stamp `decoration`'s headers and footers
    /// ([`Self::stamp_running_text`]) and then its printer's marks
    /// ([`Self::add_printer_marks`]).
    ///
    /// Meant for a session that is printed and then dropped, never saved:
    /// the decorations are a printed-copy-only change. All or nothing: if
    /// one step is refused, the steps already applied are undone before the
    /// error returns. One undo entry per step on success.
    ///
    /// # Errors
    ///
    /// The first refusal of [`Self::stamp_running_text`] or
    /// [`Self::add_printer_marks`].
    pub fn decorate_for_print(
        &mut self,
        decoration: &PrintDecoration,
    ) -> Result<PrintDecorationReport, EditError> {
        let mut report = PrintDecorationReport::default();
        let mut applied = 0;
        let result = (|| {
            for stamp in &decoration.running_text {
                report.running_text.push(self.stamp_running_text(stamp)?);
                applied += 1;
            }
            if let Some(marks) = &decoration.marks {
                report.marks = Some(self.add_printer_marks(marks)?);
            }
            Ok(())
        })();
        match result {
            Ok(()) => Ok(report),
            Err(err) => {
                for _ in 0..applied {
                    self.undo();
                }
                Err(err)
            }
        }
    }
}
