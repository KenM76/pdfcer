//! [`EditSession::snapshot`]: an independent copy of what a session shows now.

use super::EditSession;

impl EditSession {
    /// **Snapshot**: an independent session showing exactly what this one
    /// shows now, unsaved edits included, for reading elsewhere (a worker
    /// thread running preflight, comparison or export, or a printed copy).
    ///
    /// The snapshot shares the parsed base (no reparse, no re-serialise) and
    /// copies the overlay, so it costs the edits made so far, not the
    /// document. `self` is only borrowed: its undo/redo history, caches and
    /// checkpoints are untouched. Edits to the snapshot never reach `self`,
    /// and edits to `self` after the call never reach the snapshot. The
    /// snapshot starts with an empty history and its own checkpoint
    /// identity. It keeps this session's settings and its redaction state,
    /// so a snapshot of a session with a deferred redaction refuses ordinary
    /// saves just as this one does.
    #[must_use]
    pub fn snapshot(&self) -> EditSession {
        Self {
            base: std::sync::Arc::clone(&self.base),
            state: self.state.clone(),
            deleted: self.deleted.clone(),
            trailer: self.trailer.clone(),
            next_number: self.next_number,
            staging: self.staging.clone(),
            structure_notes: Vec::new(),
            anchor_report: Vec::new(),
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
            rich_style_precedence: self.rich_style_precedence,
            tab_row_tolerance: self.tab_row_tolerance,
            xmp_sync: self.xmp_sync,
            page_objects_cache: None,
            text_walk_cache: std::sync::Mutex::new(None),
            redacted: self.redacted,
            redaction_pending: self.redaction_pending,
            #[cfg(debug_assertions)]
            base_page_tree_walked: self.base_page_tree_walked,
        }
    }
}
