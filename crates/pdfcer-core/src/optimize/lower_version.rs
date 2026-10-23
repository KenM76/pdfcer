//! Lower a document's declared PDF version to one its content fits under.
//!
//! Opt-in only: ISO 32000-2 Annex I.2 says a writer should not change a
//! document to an older version, so this runs only when asked and only when
//! [`version_floor_with`] proves the content needs nothing newer. The header
//! is rewritten in place (same length, so no offset moves) and a catalog
//! `/Version` (§7.7.2) is removed by an incremental update. Below 1.5 a file
//! using object streams or cross-reference streams (§7.5.7, §7.5.8) is
//! rebuilt with a classic cross-reference table instead, which renumbers
//! its objects.

use std::fmt;

use super::{FloorOptions, VersionFloor, version_floor_with};
use crate::PdfVersion;
use crate::document::{DocError, Document};
use crate::edit::EditSession;
use crate::object::Provenance;
use crate::writer::{self, RebuildOptions, RebuildReport, SaveOptions, WriteError};
use crate::xref::SectionShape;

/// How far into the file the `%PDF-` marker is looked for (§7.5.2 lets
/// readers accept leading bytes; real files put it within the first KiB).
const HEADER_WINDOW: usize = 1024;

/// Settings for [`lower_version`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct LowerOptions {
    /// How the floor is measured.
    pub floor: FloorOptions,
    /// Lower a signed document anyway. The header is inside every
    /// signature's byte range, so every signature stops verifying.
    pub allow_signed: bool,
}

impl LowerOptions {
    /// Set [`floor`](Self::floor).
    #[must_use]
    pub fn with_floor(mut self, floor: FloorOptions) -> Self {
        self.floor = floor;
        self
    }

    /// Set [`allow_signed`](Self::allow_signed).
    #[must_use]
    pub fn with_allow_signed(mut self, allow: bool) -> Self {
        self.allow_signed = allow;
        self
    }
}

/// How the version was lowered.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum LowerRoute {
    /// Only the header's version digits changed (plus an incremental update
    /// removing the catalog `/Version`, when there was one).
    InPlace,
    /// The file was rebuilt with a classic cross-reference table, because
    /// it used object or cross-reference streams below 1.5, or its header
    /// could not be patched in place. Objects are renumbered.
    Rebuilt(RebuildReport),
}

/// A lowered file and what was done to it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct LoweredVersion {
    /// The new file.
    pub bytes: Vec<u8>,
    /// The effective version before (greater of header and catalog).
    pub from: PdfVersion,
    /// The declared version now.
    pub to: PdfVersion,
    /// Whether a catalog `/Version` was removed.
    pub catalog_version_removed: bool,
    /// How the file was written.
    pub route: LowerRoute,
    /// The floor that allowed it.
    pub floor: VersionFloor,
}

/// Why [`lower_version`] refused or failed. Nothing is written in any case.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LowerError {
    /// The input does not load.
    #[error("the file does not load: {0}")]
    Load(#[source] DocError),
    /// The document is encrypted; the rewrite would not re-encrypt it.
    #[error("the document is encrypted; decrypt it, lower the copy, then encrypt the result")]
    Encrypted,
    /// The document is signed and [`LowerOptions::allow_signed`] is not set.
    #[error(
        "the document has {signatures} signature(s); changing its header invalidates all of \
         them. Allow it explicitly to proceed"
    )]
    Signed {
        /// Signature dictionaries found.
        signatures: usize,
    },
    /// A stream the scan needs could not be read, so the floor is only a
    /// lower bound and lowering could hide a newer construct.
    #[error(
        "{unreadable} stream(s) could not be read, so the content may need more than PDF \
         {floor}; the version is not lowered"
    )]
    Unproven {
        /// The lower bound found.
        floor: PdfVersion,
        /// Streams that could not be read.
        unreadable: usize,
    },
    /// The content needs more than the target.
    #[error("the content needs PDF {floor} ({needs}), above the target PDF {target}")]
    BelowFloor {
        /// The requested version.
        target: PdfVersion,
        /// What the content needs.
        floor: PdfVersion,
        /// The construct that sets the floor.
        needs: String,
    },
    /// The target is not below the declared version.
    #[error("the document already declares PDF {declared}, not above the target PDF {target}")]
    NotLower {
        /// The requested version.
        target: PdfVersion,
        /// The effective declared version.
        declared: PdfVersion,
    },
    /// Writing the result failed.
    #[error("writing the lowered file failed: {0}")]
    Write(#[from] WriteError),
    /// The written result does not reload, or does not declare the target.
    #[error("the lowered file does not reload as PDF {target}: {reason}")]
    Verify {
        /// The requested version.
        target: PdfVersion,
        /// What went wrong.
        reason: String,
    },
}

/// Lower the file `bytes` to declare `target`.
///
/// Refuses an encrypted document, a signed one unless
/// [`LowerOptions::allow_signed`], a floor that is unproven or above
/// `target`, and a `target` not below the declared version. The result is
/// reloaded and checked before it is returned.
///
/// # Errors
///
/// [`LowerError`], one variant per refusal or failure.
///
/// # Example
///
/// ```
/// # use pdfcer_core::{PdfVersion, optimize::{LowerOptions, lower_version}};
/// # use pdfcer_core::writer::SaveOptions;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/synthetic/hello.pdf"))?;
/// let to = PdfVersion { major: 1, minor: 3 };
/// match lower_version(bytes, to, &LowerOptions::default(), &SaveOptions::default()) {
///     Ok(done) => println!("lowered {} -> {}", done.from, done.to),
///     Err(why) => println!("kept: {why}"),
/// }
/// # Ok(()) }
/// ```
pub fn lower_version(
    bytes: Vec<u8>,
    target: PdfVersion,
    options: &LowerOptions,
    save: &SaveOptions,
) -> Result<LoweredVersion, LowerError> {
    let doc = Document::from_bytes(bytes).map_err(LowerError::Load)?;
    let from = doc.version();
    let mut session = EditSession::new(doc);
    let floor = check(&session, target, options)?;
    let rebuild = target < STREAMS && uses_streams(session.document());
    let removed = session.drop_catalog_version();
    let mut out = if removed {
        session
            .to_incremental_bytes(save)
            .or_else(|_| session.to_full_bytes(save))?
            .0
    } else {
        session.document().bytes().to_vec()
    };
    let route = if rebuild || !patch_header(&mut out, target) {
        let doc = Document::from_bytes(out).map_err(|e| verify(target, &e.to_string()))?;
        let (rebuilt, report) =
            writer::rebuild_declaring(&doc, &RebuildOptions::none(), save, target)?;
        out = rebuilt;
        LowerRoute::Rebuilt(report)
    } else {
        LowerRoute::InPlace
    };
    check_result(&out, target)?;
    Ok(LoweredVersion {
        bytes: out,
        from,
        to: target,
        catalog_version_removed: removed,
        route,
        floor,
    })
}

/// Object and cross-reference streams arrive in PDF 1.5.
const STREAMS: PdfVersion = PdfVersion { major: 1, minor: 5 };

fn check(
    session: &EditSession,
    target: PdfVersion,
    options: &LowerOptions,
) -> Result<VersionFloor, LowerError> {
    let doc = session.document();
    if doc.encryption().is_some() {
        return Err(LowerError::Encrypted);
    }
    let declared = doc.version();
    if target >= declared {
        return Err(LowerError::NotLower { target, declared });
    }
    let signatures = crate::signature::census(session).signatures;
    if signatures > 0 && !options.allow_signed {
        return Err(LowerError::Signed { signatures });
    }
    let floor = version_floor_with(doc, options.floor);
    if !floor.proven() {
        return Err(LowerError::Unproven {
            floor: floor.floor,
            unreadable: floor.unprovable.len(),
        });
    }
    if floor.floor > target {
        let needs = floor
            .requirements
            .first()
            .map_or_else(String::new, |r| r.construct.clone());
        return Err(LowerError::BelowFloor {
            target,
            floor: floor.floor,
            needs,
        });
    }
    Ok(floor)
}

/// Whether `doc` stores objects in object streams or has a
/// cross-reference stream (a hybrid file counts: its hidden objects are
/// only in the stream).
fn uses_streams(doc: &Document) -> bool {
    doc.is_hybrid()
        || matches!(doc.section_shape(), SectionShape::Stream { .. })
        || doc
            .objects()
            .any(|o| matches!(o.provenance, Provenance::ObjectStream { .. }))
}

/// Rewrite the `M.N` after the first `%PDF-` to `target`, only when it is
/// exactly three bytes so no offset moves.
fn patch_header(bytes: &mut [u8], target: PdfVersion) -> bool {
    let window = bytes
        .get(..bytes.len().min(HEADER_WINDOW))
        .unwrap_or_default();
    let Some(at) = window.windows(5).position(|w| w == b"%PDF-") else {
        return false;
    };
    let v = at + 5;
    let after = bytes.get(v + 3).copied();
    if after.is_some_and(|b| b.is_ascii_digit() || b == b'.')
        || target.major > 9
        || target.minor > 9
    {
        return false;
    }
    let Some([a, b'.', c]) = bytes.get_mut(v..v + 3) else {
        return false;
    };
    if !a.is_ascii_digit() || !c.is_ascii_digit() {
        return false;
    }
    *a = b'0' + target.major;
    *c = b'0' + target.minor;
    true
}

fn check_result(bytes: &[u8], target: PdfVersion) -> Result<(), LowerError> {
    let doc = Document::from_bytes(bytes.to_vec()).map_err(|e| verify(target, &e.to_string()))?;
    if doc.version() != target {
        return Err(verify(
            target,
            &format!("it declares PDF {}", doc.version()),
        ));
    }
    if target < STREAMS && uses_streams(&doc) {
        return Err(verify(target, "it still uses PDF 1.5 file structure"));
    }
    Ok(())
}

fn verify(target: PdfVersion, reason: &str) -> LowerError {
    LowerError::Verify {
        target,
        reason: reason.to_owned(),
    }
}

impl fmt::Display for LowerRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InPlace => f.write_str("header rewritten in place"),
            Self::Rebuilt(r) => write!(
                f,
                "rebuilt with a classic cross-reference table, {} object(s) renumbered",
                r.objects_out
            ),
        }
    }
}

#[cfg(test)]
#[path = "lower_version_tests.rs"]
mod tests;
