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

impl EditSession {
    /// **Fork for print**: an independent session showing exactly what this
    /// one shows now, for a printed copy that is decorated and dropped.
    ///
    /// The fork shares the parsed base (no reparse, no re-serialise) and
    /// copies the overlay, so it costs the edits made so far, not the
    /// document. Edits to the fork never reach `self`, and `self` is only
    /// borrowed: its undo/redo history, caches and checkpoints are
    /// untouched. The fork starts with an empty history and its own
    /// checkpoint identity. It keeps this session's settings and its
    /// redaction state, so a fork of a session with a deferred redaction
    /// refuses ordinary saves just as this one does.
    #[must_use]
    pub fn fork_for_print(&self) -> EditSession {
        Self {
            base: std::sync::Arc::clone(&self.base),
            state: self.state.clone(),
            deleted: self.deleted.clone(),
            trailer: self.trailer.clone(),
            next_number: self.next_number,
            staging: self.staging.clone(),
            structure_notes: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            session_serial: super::checkpoint::next_serial(),
            evictions: 0,
            evicted_tail: std::collections::VecDeque::new(),
            undo_group: None,
            page_import: None,
            quad_point_order: self.quad_point_order,
            redact_options: self.redact_options,
            widget_tab_tail: self.widget_tab_tail,
            tab_row_tolerance: self.tab_row_tolerance,
            page_objects_cache: None,
            text_walk_cache: std::sync::Mutex::new(None),
            redacted: self.redacted,
            redaction_pending: self.redaction_pending,
            #[cfg(debug_assertions)]
            base_page_tree_walked: self.base_page_tree_walked,
        }
    }

    /// [`Self::fork_for_print`] then [`Self::decorate_for_print`] on the
    /// fork: the decorated printed copy, leaving `self` untouched.
    ///
    /// # Errors
    ///
    /// As [`Self::decorate_for_print`]; the fork is dropped.
    pub fn decorated_for_print(
        &self,
        decoration: &PrintDecoration,
    ) -> Result<(EditSession, PrintDecorationReport), EditError> {
        let mut fork = self.fork_for_print();
        let report = fork.decorate_for_print(decoration)?;
        Ok((fork, report))
    }
}
