//! An unspliced format plan, and the batch form that splices many of them
//! into one content stream (request G227).

use std::collections::BTreeMap;

use crate::text_edit::edit::{emit_tm, splice};
use crate::text_edit::format::{CreatedFont, FormatError, FormatPlan, FormatReport};
use crate::text_edit::opacity::CreatedAlpha;
use crate::writer::content::emit_number;

/// One reflowed follower: the absolute `Tm` at `start..end`, moved along
/// its `e` operand by `delta`.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TmShift {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) m: [f64; 6],
    pub(crate) delta: f64,
}

/// A format plan before splicing: the anchor's replacement edits, the
/// follower shifts kept apart so a batch can sum two requests' shifts of
/// one `Tm`, and whether the plan read only the bytes it replaces.
#[derive(Debug)]
pub(crate) struct PlannedFormat {
    pub(crate) edits: Vec<(usize, usize, Vec<u8>)>,
    pub(crate) shifts: Vec<TmShift>,
    /// `false` when planning read stream bytes outside its own edits (a
    /// decoration scan, a synthetic-italic matrix), so the plan is only
    /// valid against the exact stream it was made on.
    pub(crate) local: bool,
    pub(crate) report: FormatReport,
    pub(crate) created_font: Option<CreatedFont>,
    pub(crate) created_alphas: Vec<CreatedAlpha>,
}

impl PlannedFormat {
    /// Splice this one plan into `buf`.
    pub(crate) fn into_plan(self, buf: &[u8]) -> FormatPlan {
        let mut edits = self.edits;
        edits.extend(self.shifts.iter().map(|s| {
            let [a, b, c, d, e, f] = s.m;
            (s.start, s.end, emit_tm([a, b, c, d, e + s.delta, f]))
        }));
        FormatPlan {
            new_content: splice(buf, &mut edits),
            report: self.report,
            created_font: self.created_font,
            created_alphas: self.created_alphas,
        }
    }
}

/// Splice `plans` — made against the SAME stream `buf`, given in the order a
/// sequential run would apply them (descending pinned start) — into one
/// buffer equal to applying them one at a time.
///
/// Two plans shifting one follower `Tm` compose: each shift is applied to
/// the value the previous one wrote, rounded as the stream would store it.
/// Returns `None` when any two edits overlap; the caller then applies the
/// requests sequentially instead.
pub(crate) fn splice_batch(buf: &[u8], plans: &mut [PlannedFormat]) -> Option<Vec<u8>> {
    let mut edits = Vec::new();
    let mut tms: BTreeMap<usize, (usize, [f64; 6])> = BTreeMap::new();
    for plan in plans.iter_mut() {
        edits.append(&mut plan.edits);
        for s in &plan.shifts {
            let (end, m) = tms.entry(s.start).or_insert((s.end, s.m));
            if *end != s.end {
                return None;
            }
            m[4] = stored(m[4] + s.delta);
        }
    }
    edits.extend(
        tms.into_iter()
            .map(|(start, (end, m))| (start, end, emit_tm(m))),
    );
    edits.sort_by_key(|e| e.0);
    let overlap = edits.windows(2).any(|w| match w {
        [x, y] => x.1 > y.0,
        _ => false,
    });
    (!overlap).then(|| splice(buf, &mut edits))
}

/// `v` as it reads back after being written into a content stream.
fn stored(v: f64) -> f64 {
    let mut out = Vec::new();
    emit_number(&mut out, v);
    std::str::from_utf8(&out)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(v)
}

/// The outcome of [`EditSession::format_texts`](crate::edit::EditSession::format_texts).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct FormatBatch {
    /// One report per request, in the caller's order.
    pub reports: Vec<FormatReport>,
    /// Disclosures about the batch as a whole (a content stream copied off
    /// a shared one, structure-tree notes), not repeated per report.
    pub disclosures: Vec<String>,
    /// `true` when every request was planned against one walk of the stream
    /// and written in one rewrite; `false` when the batch fell back to
    /// applying the requests one at a time (still one undo entry).
    pub planned_once: bool,
}

/// Why [`EditSession::format_texts`](crate::edit::EditSession::format_texts)
/// applied nothing.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FormatBatchError {
    /// Request `index` (into the caller's slice) was refused.
    #[error("request {index}: {error}")]
    Request {
        /// The refused request's position in the caller's slice.
        index: usize,
        /// Why it was refused.
        error: Box<FormatError>,
    },
    /// The batch could not be planned at once and is longer than the undo
    /// stack can fold into one entry.
    #[error(
        "{requests} requests cannot be applied one at a time as one undo entry \
         (the limit is {limit})"
    )]
    TooManyForOneUndo {
        /// How many requests were given.
        requests: usize,
        /// [`MAX_UNDO_DEPTH`](crate::edit::MAX_UNDO_DEPTH).
        limit: usize,
    },
}

impl FormatBatchError {
    /// The refused request's index, when one request was the cause.
    #[must_use]
    pub const fn index(&self) -> Option<usize> {
        match self {
            Self::Request { index, .. } => Some(*index),
            Self::TooManyForOneUndo { .. } => None,
        }
    }
}
