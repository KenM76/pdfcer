//! A from-scratch structural rewrite: drop unreferenced objects, merge
//! identical ones, renumber densely, and optionally pack objects into object
//! streams with a cross-reference stream (ISO 32000-2 §7.5.7, §7.5.8).
//!
//! Unlike [`super::save_full`], nothing here is copied verbatim and the
//! numbering changes, so this is only for an operator-requested size
//! reduction (decision 191). Decoded content is unchanged: stream data is
//! copied as stored, with its filters.
//!
//! What is dropped regardless of the options: object streams and
//! cross-reference streams of the input (they are rebuilt or not needed),
//! and a linearization parameter dictionary, which is rewritten when the
//! output is linearized and dropped otherwise (Annex F.3.3).

mod emit;
mod graph;
mod linearize;

use std::collections::BTreeSet;

use super::{SaveOptions, WriteError};
use crate::PdfVersion;
use crate::document::Document;
use crate::object::ObjId;

/// What [`rebuild`] does beyond renumbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RebuildOptions {
    /// Drop every object not reachable from the trailer's `/Root` or `/Info`.
    pub remove_unreferenced: bool,
    /// Merge objects that are identical after their own references are
    /// merged: streams (except page content streams), fonts, font
    /// descriptors, encodings, graphics states, non-empty arrays and plain
    /// values. Pages, annotations, fields, layers and structure elements
    /// keep their identity even when identical.
    pub merge_duplicates: bool,
    /// Pack every non-stream object into Flate-compressed object streams
    /// and write a cross-reference stream. Raises the header to PDF 1.5
    /// when it is lower (§7.5.7 is a PDF 1.5 feature).
    pub object_streams: bool,
    /// Flate level (0–9) for object streams and the cross-reference stream.
    pub level: u8,
    /// Write a linearized file ("Fast Web View", Annex F) instead: page 1
    /// first, then a hint stream locating every page. Object streams are
    /// not packed when this is on, the header is raised to PDF 1.2 when
    /// lower, and inheritable page attributes move onto every page. Off in
    /// [`Self::all`]: it adds bytes rather than saving them.
    pub linearize: bool,
}

impl RebuildOptions {
    /// Everything on, Flate level 9.
    #[must_use]
    pub const fn all() -> Self {
        Self {
            remove_unreferenced: true,
            merge_duplicates: true,
            object_streams: true,
            level: 9,
            linearize: false,
        }
    }

    /// Everything off: a caller holding this keeps the file's structure and
    /// does not call [`rebuild`] at all (see [`Self::is_none`]).
    #[must_use]
    pub const fn none() -> Self {
        Self {
            remove_unreferenced: false,
            merge_duplicates: false,
            object_streams: false,
            level: 9,
            linearize: false,
        }
    }

    /// Whether no option is on, so a rebuild would only renumber.
    #[must_use]
    pub const fn is_none(&self) -> bool {
        !self.remove_unreferenced
            && !self.merge_duplicates
            && !self.object_streams
            && !self.linearize
    }
}

impl Default for RebuildOptions {
    fn default() -> Self {
        Self::all()
    }
}

/// What a [`rebuild`] did, in the INPUT document's object numbers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RebuildReport {
    /// Indirect objects in the input, structural streams included.
    pub objects_in: usize,
    /// Indirect objects written, new object and cross-reference streams
    /// included.
    pub objects_out: usize,
    /// Objects dropped as unreachable.
    pub unreferenced: Vec<ObjId>,
    /// `(dropped, kept)` pairs: each dropped object was identical to `kept`
    /// and every reference to it now names `kept`.
    pub merged: Vec<(ObjId, ObjId)>,
    /// Input object and cross-reference streams and linearization
    /// dictionaries, which are never carried over.
    pub structural_dropped: Vec<ObjId>,
    /// References to objects the input does not define, written as `null`
    /// (§7.3.10 gives them that meaning already).
    pub dangling: usize,
    /// Object streams written.
    pub object_streams: usize,
    /// The new header version, when packing or linearizing raised it.
    pub version_raised: Option<PdfVersion>,
    /// Whether the output is linearized.
    pub linearized: bool,
}

/// Rewrite `doc` from scratch under `rebuild_options`. `options` supplies
/// the cross-reference entry EOL and the final EOL; `producer` is not
/// applied (the `/Info` dictionary is carried as it is).
///
/// # Errors
///
/// [`WriteError::EncryptedSaveUnsupported`] for an encrypted document, and
/// [`WriteError::Xref`] if a cross-reference section cannot be emitted,
/// [`WriteError::NothingToLinearize`] when linearizing a document with no
/// catalog or no page.
pub fn rebuild(
    doc: &Document,
    rebuild_options: &RebuildOptions,
    options: &SaveOptions,
) -> Result<(Vec<u8>, RebuildReport), WriteError> {
    rebuild_with(doc, rebuild_options, options, None)
}

/// [`rebuild`], writing `%PDF-<version>` as the header instead of the
/// input's version. Object streams still raise it to 1.5; a linearized
/// rebuild keeps the input's version.
///
/// # Errors
///
/// As [`rebuild`].
pub fn rebuild_declaring(
    doc: &Document,
    rebuild_options: &RebuildOptions,
    options: &SaveOptions,
    version: PdfVersion,
) -> Result<(Vec<u8>, RebuildReport), WriteError> {
    rebuild_with(doc, rebuild_options, options, Some(version))
}

fn rebuild_with(
    doc: &Document,
    rebuild_options: &RebuildOptions,
    options: &SaveOptions,
    header: Option<PdfVersion>,
) -> Result<(Vec<u8>, RebuildReport), WriteError> {
    if doc.encryption().is_some() {
        return Err(WriteError::EncryptedSaveUnsupported);
    }
    let mut report = RebuildReport {
        objects_in: doc.object_count(),
        ..RebuildReport::default()
    };
    let mut live = graph::live_objects(doc, &mut report);
    if rebuild_options.remove_unreferenced {
        let reached = graph::reachable(doc.trailer(), &live);
        report.unreferenced = live
            .keys()
            .filter(|id| !reached.contains(id))
            .copied()
            .collect();
        live.retain(|id, _| reached.contains(id));
    }
    let canon = if rebuild_options.merge_duplicates {
        graph::merge_duplicates(doc, &live, &mut report)
    } else {
        std::collections::BTreeMap::new()
    };
    let kept: BTreeSet<ObjId> = live
        .keys()
        .filter(|id| !canon.contains_key(id))
        .copied()
        .collect();
    let bytes = if rebuild_options.linearize {
        report.linearized = true;
        linearize::write(doc, &live, &canon, &kept, &mut report)?
    } else {
        let renum = graph::Renumbering::new(&kept, &canon);
        emit::write(
            doc,
            &live,
            &renum,
            (rebuild_options, header),
            options,
            &mut report,
        )?
    };
    Ok((bytes, report))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test code: a failed lookup should fail the test loudly
mod tests;
