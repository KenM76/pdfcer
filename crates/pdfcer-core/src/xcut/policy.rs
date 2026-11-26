//! [`expect`]: the policy, one exhaustive match per operation group.
//!
//! No arm in this module or under `policy/` is a wildcard. A new [`Op`] or
//! [`ObjectKind`] fails to compile here until someone decides its cells.

pub(super) mod clipboard;
mod deferred;

use super::{Expect, ObjectKind, Op};

/// What `op` must do to an object of kind `kind` (decision 228).
///
/// A [`Expect::Pending`] cell names the filed Pass that will drive it;
/// every other cell is driven by the gate test in `xcut/tests`.
///
/// # Example
///
/// ```
/// use pdfcer_core::xcut::{Expect, ObjectKind, Op, expect};
///
/// assert!(matches!(expect(Op::Copy, ObjectKind::Popup), Expect::Refused(_)));
/// let cells = Op::ALL.len() * ObjectKind::ALL.len();
/// let driven = Op::ALL
///     .iter()
///     .flat_map(|op| ObjectKind::ALL.iter().map(move |k| expect(*op, *k)))
///     .filter(Expect::is_driven)
///     .count();
/// assert!(driven > 0 && driven < cells);
/// ```
#[must_use]
pub fn expect(op: Op, kind: ObjectKind) -> Expect {
    match op {
        Op::Copy | Op::Cut | Op::Paste | Op::Duplicate => clipboard::carry(kind),
        Op::ReloadRoundTrip => clipboard::reload(kind),
        Op::Undo | Op::Redo | Op::SaveIncremental | Op::SaveFull => {
            deferred::pending(kind, "782.0")
        }
        Op::Move
        | Op::Transform
        | Op::AlignDistribute
        | Op::Restack
        | Op::Group
        | Op::Ungroup
        | Op::LayerAssign => deferred::pending(kind, "782.1"),
        Op::Delete
        | Op::PageInsert
        | Op::PageExtract
        | Op::PageMerge
        | Op::PageRotate
        | Op::PageReorder => deferred::pending(kind, "782.2"),
        Op::Redact | Op::Sanitize => deferred::pending(kind, "782.3"),
        Op::Flatten
        | Op::ExportSvg
        | Op::ExportDocx
        | Op::ExportImage
        | Op::Print
        | Op::PdfaConvert => deferred::pending(kind, "782.4"),
        Op::StructureTags
        | Op::Search
        | Op::Compare
        | Op::HitTest
        | Op::PropertiesRead
        | Op::PropertiesWrite
        | Op::CliVerb
        | Op::CoreApiDoc => deferred::pending(kind, "782.6"),
    }
}
