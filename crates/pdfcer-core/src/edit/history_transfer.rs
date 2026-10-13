//! Moving an edit session, undo history included, to another process:
//! [`EditSession::export_history`] and [`EditSession::import_history`]
//! (pdfcer-gui request G184).
//!
//! The export is the session's overlay — every touched object's current
//! value, deletions, the working trailer, authored stream bytes — plus the
//! undo and redo stacks, keyed to the base revision by SHA-256. It does not
//! contain the base document: the receiver opens the same base bytes itself.
//! Stream values keep their byte spans, which index the base buffer and the
//! staging bytes the export carries, so the base must be byte-identical.
//!
//! Format: `PDFCERHIST` magic, a version, the base length and digest, then the
//! fields in [`EditSession::export_history`]'s order (see `history_codec`).

use std::collections::{BTreeMap, BTreeSet};

use sha2::{Digest, Sha256};

use super::checkpoint::Entry;
use super::history_codec::{Malformed, Reader, Writer};
use super::history_kind::{ReadKind, read_kind, write_kind};
use super::{Command, EditSession, ObjectWrite, Removal};
use crate::object::{Dict, ObjId, Object};

const MAGIC: &[u8] = b"PDFCERHIST";
const VERSION: u64 = 1;

/// Why [`EditSession::import_history`] refused. A refusal changes nothing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HistoryError {
    /// The bytes are not an exported history.
    #[error("not a pdfcer edit history")]
    NotAHistory,
    /// Written by a format version this build does not read.
    #[error("edit history format version {found} is not supported (this build reads {supported})")]
    UnsupportedVersion {
        /// The version in the bytes.
        found: u64,
        /// The version this build writes and reads.
        supported: u64,
    },
    /// The receiving session was opened from different bytes than the one
    /// that exported the history.
    #[error("the edit history belongs to a different revision of the document")]
    BaseMismatch,
    /// The receiving session already has edits or history of its own.
    #[error("the receiving edit session already has edits; import into a freshly opened one")]
    SessionNotPristine,
    /// The history names a command kind this build does not have (a build
    /// newer or older than the exporter's).
    #[error("the edit history contains a command this build does not know: {name}")]
    UnknownCommand {
        /// The command kind's name as written.
        name: String,
    },
    /// The bytes are damaged or truncated.
    #[error("the edit history is damaged at byte {offset}")]
    Malformed {
        /// Where decoding stopped.
        offset: usize,
    },
}

impl From<Malformed> for HistoryError {
    fn from(m: Malformed) -> Self {
        Self::Malformed { offset: m.offset }
    }
}

/// What [`EditSession::import_history`] restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct HistoryImport {
    /// Commands now on the undo stack.
    pub undo_steps: usize,
    /// Commands now on the redo stack.
    pub redo_steps: usize,
}

/// Everything `import_history` decodes before it touches the session.
struct Decoded {
    staging: Vec<u8>,
    trailer: Dict,
    next_number: Option<u32>,
    state: BTreeMap<ObjId, Object>,
    deleted: BTreeSet<ObjId>,
    redacted: bool,
    redaction_pending: bool,
    undo: Vec<Command>,
    redo: Vec<Command>,
}

impl EditSession {
    /// **The session's unsaved edits and undo/redo history, as bytes** another
    /// process can restore with [`import_history`](Self::import_history) onto
    /// a session opened from the same base bytes.
    ///
    /// The bytes do not include the base document. They do include every
    /// value the history can restore, so content a later edit removed is
    /// still in them while it is undoable; treat them as you would the
    /// unsaved document. Commands the undo-depth bound already evicted are not
    /// history any more, but their effects are part of the exported state.
    /// Checkpoints do not travel.
    #[must_use]
    pub fn export_history(&self) -> Vec<u8> {
        let mut w = Writer::default();
        w.out.extend_from_slice(MAGIC);
        w.uint(VERSION);
        let base = self.base.bytes();
        w.usize(base.len());
        w.out.extend_from_slice(&Sha256::digest(base));
        w.bytes(&self.staging);
        w.dict(&self.trailer);
        w.bool(self.next_number.is_some());
        w.uint(u64::from(self.next_number.unwrap_or(0)));
        w.usize(self.state.len());
        for (id, value) in &self.state {
            w.id(*id);
            w.object(value);
        }
        w.usize(self.deleted.len());
        for id in &self.deleted {
            w.id(*id);
        }
        w.bool(self.redacted);
        w.bool(self.redaction_pending);
        for stack in [&self.undo, &self.redo] {
            w.usize(stack.len());
            for entry in stack {
                write_command(&mut w, entry);
            }
        }
        w.out
    }

    /// **Restore a history from [`export_history`](Self::export_history)**
    /// onto this session, which must be freshly opened from the same base
    /// bytes. Afterwards the session is the exporter's: same content, same
    /// unsaved state, same undo and redo steps, and the same
    /// [`has_pending_redaction`](Self::has_pending_redaction) save guard.
    ///
    /// # Errors
    ///
    /// [`HistoryError`]: `SessionNotPristine` if this session has any edit or
    /// history; `BaseMismatch` if its base bytes differ from the exporter's;
    /// `UnknownCommand` across builds whose command sets differ;
    /// `NotAHistory`, `UnsupportedVersion` or `Malformed` for other bytes.
    /// Nothing changes on refusal.
    pub fn import_history(&mut self, bytes: &[u8]) -> Result<HistoryImport, HistoryError> {
        if !self.is_pristine() {
            return Err(HistoryError::SessionNotPristine);
        }
        let decoded = self.decode_history(bytes)?;
        let report = HistoryImport {
            undo_steps: decoded.undo.len(),
            redo_steps: decoded.redo.len(),
        };
        self.staging = decoded.staging;
        self.trailer = decoded.trailer;
        self.next_number = decoded.next_number;
        self.state = decoded.state;
        self.deleted = decoded.deleted;
        self.redacted = decoded.redacted;
        self.redaction_pending = decoded.redaction_pending;
        self.undo = decoded.undo.into_iter().map(Entry::new).collect();
        self.redo = decoded.redo.into_iter().map(Entry::new).collect();
        self.page_objects_cache = None;
        *self
            .text_walk_cache
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        Ok(report)
    }

    fn is_pristine(&self) -> bool {
        self.undo.is_empty()
            && self.redo.is_empty()
            && self.state.is_empty()
            && self.deleted.is_empty()
            && self.staging.is_empty()
            && self.evictions == 0
            && !self.redacted
            && !self.redaction_pending
            && &self.trailer == self.base.trailer()
    }

    fn decode_history(&self, bytes: &[u8]) -> Result<Decoded, HistoryError> {
        let Some(body) = bytes.strip_prefix(MAGIC) else {
            return Err(HistoryError::NotAHistory);
        };
        let mut r = Reader::new(body);
        let version = r.uint()?;
        if version != VERSION {
            return Err(HistoryError::UnsupportedVersion {
                found: version,
                supported: VERSION,
            });
        }
        let base = self.base.bytes();
        let base_len = r.usize()?;
        let digest = r.take(32)?;
        if base_len != base.len() || digest != Sha256::digest(base).as_slice() {
            return Err(HistoryError::BaseMismatch);
        }
        let staging = r.bytes()?;
        r.span_limit = base.len().saturating_add(staging.len());
        let trailer = r.dict(0)?;
        let has_next = r.bool()?;
        let next = u32::try_from(r.uint()?).or_else(|_| r.fail())?;
        let mut state = BTreeMap::new();
        for _ in 0..r.count()? {
            let id = r.id()?;
            state.insert(id, r.object()?);
        }
        let mut deleted = BTreeSet::new();
        for _ in 0..r.count()? {
            deleted.insert(r.id()?);
        }
        let redacted = r.bool()?;
        let redaction_pending = r.bool()?;
        let undo = read_stack(&mut r)?;
        let redo = read_stack(&mut r)?;
        if !r.at_end() {
            return r.fail::<Decoded>().map_err(HistoryError::from);
        }
        Ok(Decoded {
            staging,
            trailer,
            next_number: has_next.then_some(next),
            state,
            deleted,
            redacted,
            redaction_pending,
            undo,
            redo,
        })
    }
}

fn write_command(w: &mut Writer, c: &Command) {
    write_kind(w, c.kind);
    w.usize(c.objects.len());
    for o in &c.objects {
        w.id(o.id);
        w.opt_object(o.before.as_ref());
        w.opt_object(o.after.as_ref());
    }
    w.usize(c.removals.len());
    for rm in &c.removals {
        w.id(rm.id);
        w.bool(rm.was_deleted);
        w.bool(rm.is_deleted);
    }
    w.bool(c.trailer.is_some());
    if let Some((before, after)) = &c.trailer {
        w.dict(before);
        w.dict(after);
    }
}

fn read_stack(r: &mut Reader<'_>) -> Result<Vec<Command>, HistoryError> {
    let n = r.count()?;
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(read_command(r)?);
    }
    Ok(out)
}

fn read_command(r: &mut Reader<'_>) -> Result<Command, HistoryError> {
    let kind = match read_kind(r)? {
        ReadKind::Known(k) => k,
        ReadKind::Unknown(name) => return Err(HistoryError::UnknownCommand { name }),
    };
    let mut objects = Vec::new();
    for _ in 0..r.count()? {
        objects.push(ObjectWrite {
            id: r.id()?,
            before: r.opt_object()?,
            after: r.opt_object()?,
        });
    }
    let mut removals = Vec::new();
    for _ in 0..r.count()? {
        removals.push(Removal {
            id: r.id()?,
            was_deleted: r.bool()?,
            is_deleted: r.bool()?,
        });
    }
    let trailer = if r.bool()? {
        Some((r.dict(0)?, r.dict(0)?))
    } else {
        None
    };
    Ok(Command {
        kind,
        objects,
        removals,
        trailer,
    })
}
