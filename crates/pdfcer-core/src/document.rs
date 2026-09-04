//! # The Document — pdfcer's single object model (load side)
//!
//! Ties the layers together: retained source buffer → header probe →
//! xref chain → eagerly parsed indirect objects → reference resolution.
//! Spec sources: `iso32000__s__7.5.5.md` (load algorithm),
//! `iso32000__s__7.3.10.md` (resolution semantics, dangling-ref rule),
//! `iso32000__s__7.7.2.md` (catalog) in the PDF-spec RAG. Clause
//! numbers are ISO 32000-1:2008.
//!
//! ## THE named invariant: one `Document`, both directions
//!
//! [`Document`] is simultaneously the parse result AND (from the first
//! editing Pass on) the write source
//! (docs/decisions/001-oxidize-pdf-adopt-vs-build.md §6.1 item 3). No
//! separate builder/generation model may ever be introduced — the
//! audited prior art shows exactly how that bifurcation forecloses
//! round-trip editing. Write-side machinery (dirty tracking, the
//! §7.5.6 incremental writer) lands in the first editing Pass and
//! operates on *this* type.
//!
//! ## The retained buffer
//!
//! The document owns the complete source bytes for its lifetime. Every
//! file-level [`IndirectObject`] records a
//! [`ByteSpan`](crate::span::ByteSpan) into this buffer (its full
//! `N G obj … endobj` definition), which is what makes
//! minimal-diff/incremental re-emission mechanical (ARCHITECTURE.md §5;
//! `crate::span`). Memory cost is the file size — the deliberate price
//! of the round-trip invariant, and small next to any renderer's
//! working set.
//!
//! Objects that lived **inside an object stream** (§7.5.7) have no span
//! of their own; they carry [`Provenance::ObjectStream`] instead,
//! naming the container and index. See [`Provenance`] for why that
//! distinction is a type and not a sentinel.
//!
//! ## Eager parse, strict failure (Pass 1 posture)
//!
//! `load`/`from_bytes` parse **every in-use object up front**; any
//! malformed object fails the whole load with a precise error. Rationale:
//! fail-clean over partial success (ARCHITECTURE.md §10) — a viewer
//! that opens a file "successfully" and then errors object-by-object
//! gives worse diagnostics than one clean refusal naming the offset.
//! Lazy/tolerant loading (open-what-you-can) is a deliberate later
//! feature for damaged-file recovery, driven by corpus evidence, not a
//! default.
//!
//! ### Two phases, in this order (§7.5.7 + §7.5.8.3)
//!
//! 1. **File-level objects** — every type-1 (`InUse`) xref entry is
//!    parsed at its offset.
//! 2. **Compressed objects** — every type-2 (`InStream`) entry is
//!    resolved through its container.
//!
//! The order is forced, not stylistic: a container object stream is
//! itself an ordinary file-level stream object reached through a
//! type-1 entry, so phase 2 needs phase 1 to have finished. §7.5.7
//! guarantees this terminates at two levels — an object stream may not
//! contain a stream, so a container can never live inside another
//! container, and the object supplying a container's `/Length` may not
//! be compressed either. There is no recursion to guard, only an
//! ordering to respect.
//!
//! Each container is decoded **once** and cached for the duration of
//! the load ([`ObjectStream`]); a container holding 200 objects would
//! otherwise be inflated 200 times.
//!
//! ## Resolution semantics (§7.3.10, §7.5.4)
//!
//! [`Document::get`] / [`Document::resolve`] implement, exactly:
//! - **dangling reference → null** ("shall not be considered an
//!   error") — including object numbers with no xref entry at all;
//! - **generation mismatch → null** (a stale reference; the xref
//!   entry's generation is part of the identity);
//! - **free entry → null** (§7.5.4 free-list mechanics — how deleted
//!   and hybrid-hidden objects read as absent);
//! - **reference chains are depth-guarded** ([`MAX_RESOLVE_DEPTH`],
//!   pdfcer policy — `5 0 obj 5 0 R endobj` is legal syntax and must
//!   not loop).

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::path::Path;

use crate::linearization::{self, Linearization};
use crate::object::{Dict, IndirectObject, ObjId, Object, Provenance};
use crate::objstm::{ObjStmError, ObjectStream};
use crate::parser::{ParseError, Parser, StreamLengthPolicy, TerminatorPolicy};
use crate::recover::{self, RecoveryReport};
use crate::view::DocumentView;
use crate::xref::{self, SectionShape, XrefEntry, XrefError, XrefErrorKind, XrefTable};
use crate::{PdfError, PdfVersion};

/// Maximum reference-chain hops [`Document::resolve`] follows before
/// declaring a cycle and yielding null.
///
/// pdfcer policy (ARCHITECTURE.md §10): the spec permits
/// reference-to-reference chains and does not bound them; legitimate
/// files use at most a handful of hops. Null (not an error) on
/// exhaustion keeps the §7.3.10 "references never hard-fail" posture
/// consistent even for hostile cycles.
pub const MAX_RESOLVE_DEPTH: usize = 32;

/// Errors from loading a document.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DocError {
    /// File I/O failed.
    #[error("I/O error reading PDF: {0}")]
    Io(#[from] std::io::Error),
    /// The document is encrypted with a configuration pdfcer will not decrypt
    /// (§7.6). The inner error names *which* configuration and why — an
    /// unimplemented cipher, an unsourced algorithm, a different security
    /// handler, or a document no conforming reader may open at all.
    ///
    /// Those are four different facts with four different next actions, which
    /// is why this is not one flat "encrypted files are unsupported".
    #[error("{0}")]
    Encryption(#[from] crate::crypto::EncryptionUnsupported),
    /// The document is encrypted with a configuration pdfcer **can** decrypt,
    /// but no password opened it.
    ///
    /// §7.6.3.1 requires trying the empty user password first and silently, so
    /// reaching this error means the empty password was already tried and
    /// failed: the document genuinely has a non-empty user password. The
    /// caller should prompt and retry with
    /// [`Document::from_bytes_with_password`].
    ///
    /// **N6**: ISO 32000-1 states no error model here at all — no retry limit,
    /// no reporting requirement, nothing about distinguishing a wrong password
    /// from a malformed one. All of that is pdfcer policy.
    #[error("this document is password-protected; supply the user or owner password")]
    PasswordRequired,
    /// Nothing authenticated, **and** the supplied password contains
    /// characters `/R` 5's password preprocessing may have changed before
    /// hashing — so "wrong password" is not the only explanation.
    ///
    /// The Adobe supplement's step 1 applies **SASLprep** (RFC 4013) with the
    /// Normalize and BIDI options before the UTF-8 conversion. pdfcer
    /// implements the UTF-8 conversion and the 127-byte truncation exactly and
    /// does **not** implement SASLprep; neither RFC is staged in the project's
    /// spec corpus, and a stringprep dependency was not taken for a read-only
    /// increment.
    ///
    /// For an all-ASCII password SASLprep is the identity, so this error can
    /// never arise from one. pdfcer still *attempts* a non-ASCII password —
    /// SASLprep is the identity for much more than ASCII, and getting it wrong
    /// cannot open a document with a wrong key, only fail to open one with the
    /// right password. This variant exists so that failure does not
    /// masquerade as [`Self::PasswordRequired`]'s "you typed it wrong", which
    /// would send the operator to re-check a password that was correct.
    ///
    /// Reported only at `/R` 5. Password encoding below that is PDFDocEncoding
    /// (**T8**), a different unimplemented question.
    #[error(
        "this document is password-protected; the password supplied contains non-ASCII characters, \
         and pdfcer does not apply the RFC 4013 (SASLprep) normalisation that /R 5 specifies before \
         hashing — so a correct password can be rejected here"
    )]
    PasswordRequiresNormalisation,
    /// Authentication failed at handler revision **6** (AES-256 hardened),
    /// where the failure is **not** unambiguously "wrong password" (`Pass
    /// 5.4`).
    ///
    /// `/R` 6's key derivation (Algorithm 2.B) contains a genuine spec
    /// ambiguity — **A13**, the loop-exit test, which is internally
    /// inconsistent between steps (e) and (f) and produces a different digest
    /// (hence a different key, hence a failed authentication) depending on the
    /// reading. pdfcer defaults to the reading pypdf and Acrobat write
    /// ([`crate::crypto::r6::A13Reading::PerformThenTest`]); a document written
    /// by an implementation that took the other reading would fail here with a
    /// correct password. This variant names A13 so that failure is not
    /// reported as "you typed it wrong" (`R169`: a spec ambiguity is disclosed,
    /// never silently resolved). Raised only at `/R` 6.
    #[error(
        "this document is password-protected (AES-256 /R 6); authentication failed. If the password is correct, the cause may be ambiguity A13 in ISO 32000-2 Algorithm 2.B's loop-exit test -- pdfcer uses the reading pypdf and Acrobat write, and a file written under the other reading would not open"
    )]
    PasswordRequiredR6,
    /// The `%PDF-` header probe failed — not a PDF.
    #[error(transparent)]
    Header(PdfError),
    /// The cross-reference machinery failed — a damaged/unreadable
    /// xref chain, or the deliberate `/Encrypt` capability-gap refusal
    /// (`XrefErrorKind::EncryptionUnsupported`).
    #[error(transparent)]
    Xref(#[from] XrefError),
    /// An indirect object at an xref-declared offset failed to parse.
    #[error("object {id} (xref offset {offset}): {source}")]
    BadObject {
        /// The object the xref table pointed at.
        id: ObjId,
        /// The offset the xref entry gave.
        offset: u64,
        /// The underlying structural error.
        source: ParseError,
    },
    /// The object parsed at an offset doesn't match the xref entry
    /// that pointed there (wrong number or generation) — the table and
    /// body disagree; strict Pass 1 refuses rather than guessing which
    /// is right.
    #[error("object at xref offset {offset} declares {found}, xref expected {expected}")]
    ObjectIdMismatch {
        /// What the xref entry promised.
        expected: ObjId,
        /// What the bytes at the offset declared.
        found: ObjId,
        /// The offset in question.
        offset: u64,
    },
    /// A type-2 cross-reference entry (§7.5.8.3) named an object stream
    /// that is not present as a loadable file-level object.
    ///
    /// §7.5.7: "there shall be an entry for it in a cross-reference
    /// table or cross-reference stream" — so a missing container means
    /// the file's own xref contradicts itself.
    #[error("object {num} is compressed in object stream {container}, which is not present")]
    ObjectStreamMissing {
        /// The container the type-2 entry named (generation always 0).
        container: ObjId,
        /// The object that was to be read from it.
        num: u32,
    },
    /// An object stream (§7.5.7) could not be decoded or read.
    #[error("object stream {container}: {source}")]
    ObjectStream {
        /// The container object stream.
        container: ObjId,
        /// What went wrong inside it.
        source: ObjStmError,
    },
    /// The object number stored in an object stream's pair table
    /// disagrees with the object number its type-2 cross-reference
    /// entry promised — the xref and the container contradict each
    /// other, and the strict loader refuses rather than picking one
    /// (the same posture as [`DocError::ObjectIdMismatch`] for
    /// file-level objects).
    #[error(
        "object stream {container} index {index} holds object {found}, xref expected object {expected}"
    )]
    ObjectStreamIdMismatch {
        /// The container object stream.
        container: ObjId,
        /// The index within it that was read.
        index: u32,
        /// The object number the type-2 entry promised.
        expected: u32,
        /// The object number the pair table actually stored.
        found: u32,
    },
    /// The trailer has no usable `/Root` (Table 15 requires it; a
    /// conformant update trailer always repeats it — see the RAG's
    /// gotcha on why fallback-to-older-trailers is a repair heuristic,
    /// not Pass 1 behavior).
    #[error("trailer /Root missing or not a reference to a dictionary")]
    NoCatalog,
    /// Cross-reference **recovery** (decision 013) was attempted after the
    /// strict load failed, but could not produce a document — no
    /// confirmable objects, no `/Catalog`, or the entry cap. A named
    /// fail-clean refusal, never a partial/garbage document. (An encrypted
    /// recovered file is refused as [`XrefErrorKind::EncryptionUnsupported`]
    /// via [`DocError::Xref`] instead, matching the clean-path capability
    /// gap.)
    ///
    /// [`XrefErrorKind::EncryptionUnsupported`]: crate::xref::XrefErrorKind::EncryptionUnsupported
    #[error("cross-reference recovery failed: {0}")]
    Recovery(#[from] crate::recover::RecoverError),
}

/// A loaded PDF document: retained bytes + parsed structure.
///
/// See the module docs for the three design commitments this type
/// embodies (one model, retained buffer, strict eager load).
#[derive(Debug)]
pub struct Document {
    /// The complete, untouched source bytes (the provenance substrate —
    /// every span in `objects` indexes into this).
    buf: Vec<u8>,
    /// Header-declared version (`%PDF-M.N`). Catalog `/Version`
    /// override (§7.5.5) is reconciled by [`Document::version`].
    header_version: PdfVersion,
    /// Merged, `/Size`-filtered cross-reference table.
    xref: XrefTable,
    /// The newest trailer dictionary.
    trailer: Dict,
    /// Every in-use object, parsed. Keyed by full `(num, generation)`
    /// identity.
    objects: HashMap<ObjId, IndirectObject>,
    /// The byte offset this file's own `startxref` names — the value an
    /// appended update's `/Prev` must carry (§7.5.6). Retained at load
    /// because it is not recoverable from the merged view.
    base_startxref: u64,
    /// The physical form of the newest cross-reference section, so a
    /// save can match it instead of normalizing (R33).
    section_shape: SectionShape,
    /// Annex F linearization state, detected at load (R36).
    linearization: Linearization,
    /// Highest object number the cross-reference chain mentions, before
    /// the `/Size` filter — the number a writer must allocate above.
    highest_object_number: u32,
    /// How many cross-reference entries `/Size` suppressed.
    suppressed_by_size: usize,
    /// `Some` when this document was loaded via **cross-reference
    /// recovery** (decision 013): the stored xref could not be parsed and
    /// the table was rebuilt by scanning. The writer reads this to force a
    /// full rewrite and refuse incremental save (the recovered-base rule);
    /// the CLI/GUI read it to disclose. `None` for every cleanly-loaded
    /// file — recovery lives entirely on the strict-load error path, so a
    /// clean file's flow never sets it.
    recovery: Option<RecoveryReport>,
    /// `Some` when this document was loaded from an **encrypted** file and
    /// successfully decrypted (§7.6, [`crate::crypto`]).
    ///
    /// Holds the configuration and which password opened it — deliberately
    /// **not** the file encryption key. Nothing after load needs the key
    /// (every object is already plaintext in memory), and keeping key
    /// material alive for the document's lifetime buys nothing. The
    /// re-encrypt-on-save path, when it exists, will re-derive it from a
    /// password the operator supplies at that moment.
    encryption: Option<DocumentEncryption>,
}

/// What a successfully-decrypted document was protected with.
///
/// Reported so the shells can tell the operator the document is encrypted,
/// which cipher, and what the author's permissions ask for. It is a
/// **disclosure**, not a gate: §7.6.3.1 states plainly that "there is nothing
/// inherent in PDF encryption that enforces the document permissions", so
/// anywhere pdfcer chooses to act on a permission bit it must say so (rule 4).
#[derive(Debug, Clone)]
pub struct DocumentEncryption {
    /// The parsed `/Encrypt` dictionary.
    pub config: crate::crypto::EncryptionConfig,
    /// Which password opened it. [`AuthKind::EmptyUser`] is the no-prompt
    /// case — the document had an empty user password, which every
    /// conforming reader tries silently before prompting.
    ///
    /// [`AuthKind::EmptyUser`]: crate::crypto::AuthKind::EmptyUser
    pub auth: crate::crypto::AuthKind,
    /// The Algorithm 3.13 `/Perms` verdict — **the only integrity check in
    /// PDF encryption**, and a `should`, not a `shall`.
    ///
    /// [`PermsCheck::NotApplicable`] for every `/R` ≤ 4 document, where the
    /// entry does not exist. That is the *ordinary* answer, not a failed
    /// check, and a front end must not render it as one.
    ///
    /// # Why it is here rather than acted on
    ///
    /// `/Perms` holds an encrypted copy of `/P` and `/EncryptMetadata`. The
    /// plaintext copies sit in the `/Encrypt` dictionary where anyone can edit
    /// them, with no integrity protection anywhere else in clause 7.6
    /// (**N7**). So a disagreement is the one signal PDF gives that a
    /// document's stated permissions are not the ones its encryptor recorded —
    /// and no clause says what to do about it.
    ///
    /// pdfcer reports it and **prefers neither value**:
    /// [`EncryptionConfig::permissions`] keeps returning the dictionary's
    /// `/P`, because that is what the file declares and what every other
    /// viewer shows, and this field carries the disagreement beside it.
    /// Silently substituting the decrypted copy would be pdfcer deciding, on an
    /// inference, what the operator is told — the exact shape project rule 4
    /// forbids. Refusing the document would reject files nothing else objects
    /// to, on the strength of a check the standard declines to require. See
    /// [`PermsCheck`]'s own docs (**T27**).
    ///
    /// [`PermsCheck`]: crate::crypto::PermsCheck
    /// [`PermsCheck::NotApplicable`]: crate::crypto::PermsCheck::NotApplicable
    /// [`EncryptionConfig::permissions`]: crate::crypto::EncryptionConfig::permissions
    pub perms: crate::crypto::PermsCheck,
}

impl Document {
    /// Load a document from a file on disk.
    ///
    /// # Errors
    ///
    /// [`DocError`] — I/O, header, xref, or object-level failure; every
    /// variant carries the offending offset/object for diagnostics.
    pub fn load(path: &Path) -> Result<Self, DocError> {
        Self::from_bytes(std::fs::read(path)?)
    }

    /// Load a document from disk, supplying a password for an encrypted file.
    ///
    /// `password` is `None` to mean "no password known" — which is **not** the
    /// same as an empty password. §7.6.3.1 requires a reader to try the empty
    /// user password first and silently in *either* case, so `None` still
    /// opens a permissions-only document with no prompt; what `None` means is
    /// that if that fails, there is nothing else to try and the load returns
    /// [`DocError::PasswordRequired`].
    ///
    /// Either the user or the owner password opens the document
    /// (§7.6.3.1). Which one it was is reported by
    /// [`Document::encryption`].
    ///
    /// # Errors
    ///
    /// [`DocError`] — as [`Document::load`], plus [`DocError::Encryption`] for
    /// a configuration pdfcer will not decrypt and
    /// [`DocError::PasswordRequired`] when no supplied password authenticated.
    pub fn load_with_password(path: &Path, password: Option<&[u8]>) -> Result<Self, DocError> {
        Self::from_bytes_with_password(std::fs::read(path)?, password)
    }

    /// Load a document from bytes (takes ownership — the buffer is
    /// retained for the document's lifetime; see module docs).
    ///
    /// # Errors
    ///
    /// [`DocError`] — see [`Document::load`].
    pub fn from_bytes(buf: Vec<u8>) -> Result<Self, DocError> {
        Self::from_bytes_with_password(buf, None)
    }

    /// Load a document from bytes, supplying a password for an encrypted file.
    ///
    /// See [`Document::load_with_password`] for what `None` means (it is not
    /// the empty password).
    ///
    /// # Errors
    ///
    /// [`DocError`] — see [`Document::load_with_password`].
    pub fn from_bytes_with_password(
        buf: Vec<u8>,
        password: Option<&[u8]>,
    ) -> Result<Self, DocError> {
        // 1. Header (§7.5.2 via the Pass 0 probe).
        match crate::probe_header(&buf) {
            // Header OK: try the strict §7.5.5 cross-reference load.
            Ok(header_version) => match xref::load_xref_chain(&buf) {
                Ok(loaded) => Self::assemble(
                    buf,
                    header_version,
                    loaded.table,
                    loaded.trailer,
                    loaded.startxref,
                    loaded.newest_shape,
                    loaded.highest_object_number,
                    loaded.suppressed_by_size,
                    None,
                    password,
                ),
                // The strict path failed. Decision 013: attempt
                // rebuild-by-scan recovery, but ONLY on the specific
                // failure kinds that mean the cross-reference machinery is
                // unparseable — never on the deliberate `/Encrypt`
                // capability gap, and never on a future unknown kind
                // (`recovery_reason_for` returns `None` for both).
                Err(e) => match recover::recovery_reason_for(&e.kind) {
                    None => Err(DocError::Xref(e)),
                    Some(reason) => match recover::recover(&buf, reason) {
                        Ok(rec) => Self::assemble_recovered(buf, Some(header_version), rec),
                        // A broken-xref-AND-encrypted file fails clean for
                        // the RIGHT reason (§7.6), not as a recovery error.
                        Err(recover::RecoverError::Encrypted) => Err(DocError::Xref(XrefError {
                            offset: 0,
                            kind: XrefErrorKind::EncryptionUnsupported,
                        })),
                        Err(other) => Err(DocError::Recovery(other)),
                    },
                },
            },
            // Header probe failed ("not a PDF", offset-start, leading junk).
            // Rebuild-by-scan is header-independent (absolute offsets), so
            // it also opens an offset-start file (decision 007 §10 item 6).
            // Attempt recovery, but succeed ONLY if the scan finds objects
            // AND a `/Catalog`; otherwise the ORIGINAL header error stands
            // ("not a PDF").
            Err(header_err) => match recover::recover(&buf, recover::RecoveryReason::MissingHeader)
            {
                Ok(rec) => Self::assemble_recovered(buf, None, rec),
                Err(_) => Err(DocError::Header(header_err)),
            },
        }
    }

    /// Assemble a [`Document`] from a cross-reference view (table plus
    /// trailer plus physical facts), running the §7.5.7 two-phase eager
    /// parse. Shared by the strict load path and the recovery path so the
    /// object-parse, object-stream resolution, and linearization detection
    /// are written exactly once.
    ///
    /// `recovery` is `Some` only on the rebuild-by-scan path; the writer
    /// and front ends read it (decision 013).
    #[allow(clippy::too_many_arguments)] // a private assembly seam; the
    // alternative (a builder struct) buys nothing for two call sites.
    fn assemble(
        mut buf: Vec<u8>,
        header_version: PdfVersion,
        table: XrefTable,
        trailer: Dict,
        base_startxref: u64,
        section_shape: SectionShape,
        highest_object_number: u32,
        suppressed_by_size: usize,
        recovery: Option<RecoveryReport>,
        password: Option<&[u8]>,
    ) -> Result<Self, DocError> {
        // Phase 1. Eagerly parse every file-level in-use object. Strict on
        // the clean path; on the recovery path the SAME `/Length`-vs-
        // `endstream` leniency `crate::recover`'s confirmation step used
        // must apply here too, or an object recovery accepted would be
        // rejected on re-parse and cost the whole document (the exact
        // failure this policy exists to close). Free entries resolve to
        // null, not to bytes; type-2 entries wait for phase 2 (module docs).
        let length_policy = if recovery.is_some() {
            StreamLengthPolicy::RecoverFromEndstream
        } else {
            StreamLengthPolicy::Strict
        };
        // The missing-`endobj` leniency travels with the length leniency,
        // and for exactly the same stated reason: an object `recover`'s
        // confirmation step accepted must not be rejected on re-parse
        // here, or the recovery costs the whole document.
        let terminator_policy = if recovery.is_some() {
            TerminatorPolicy::RecoverAtNextHeader
        } else {
            TerminatorPolicy::Strict
        };
        let mut objects: HashMap<ObjId, IndirectObject> = HashMap::new();
        let mut compressed: Vec<(u32, u32, u32)> = Vec::new();
        for (num, entry) in table.iter() {
            match entry {
                XrefEntry::InUse { offset, generation } => {
                    let id = ObjId::new(num, generation);
                    let (io, _repairs) =
                        parse_object_at(&buf, &table, offset, length_policy, terminator_policy)
                            .map_err(|source| DocError::BadObject { id, offset, source })?;
                    if io.id != id {
                        return Err(DocError::ObjectIdMismatch {
                            expected: id,
                            found: io.id,
                            offset,
                        });
                    }
                    objects.insert(id, io);
                }
                XrefEntry::InStream { stream_num, index } => {
                    compressed.push((num, stream_num, index));
                }
                // `XrefEntry` is #[non_exhaustive] (§7.5.8.3 reserves
                // future types); an entry kind this build does not know
                // resolves to null, which is exactly what the spec
                // prescribes for unknown types.
                _ => {}
            }
        }

        // Phase 1.5. Decrypt (7.6), if the trailer says so.
        //
        // The position of this step is load-bearing, not stylistic. It runs
        // AFTER phase 1 -- which needs only spans and `/Length`, both of which
        // are plaintext integers -- and BEFORE phase 2, which inflates object
        // streams. Decrypting here makes every object stream's data plaintext
        // by the time phase 2 reads it, and the objects phase 2 parses out of
        // that data are then correctly left alone: strings inside an object
        // stream are NOT separately encrypted (TRAP T4). Moving this after
        // phase 2 would re-apply Algorithm 1 per contained object and corrupt
        // every string in every modern file.
        let encryption = Self::decrypt_in_place(&mut buf, &trailer, &mut objects, password)?;

        // Phase 2. Resolve compressed objects through their containers,
        // decoding each container at most once. Deterministic order so
        // that a file with several broken containers always reports the
        // same one (`iter()` over a HashMap is not ordered).
        compressed.sort_unstable();
        Self::load_compressed(&buf, &compressed, &mut objects)?;

        // Annex F detection runs on the raw buffer and needs no xref
        // (F.3.3: the parameter dictionary's values are all direct and
        // it is unreferenced from the document graph), but it is done
        // here rather than in `load_xref_chain` because it is a
        // property of the *file*, not of its cross-reference machinery.
        let linearization = linearization::detect(&buf);

        Ok(Self {
            buf,
            header_version,
            xref: table,
            trailer,
            objects,
            base_startxref,
            section_shape,
            linearization,
            highest_object_number,
            suppressed_by_size,
            recovery,
            encryption,
        })
    }

    /// Authenticate and decrypt the loaded objects in place (7.6).
    ///
    /// Returns `Ok(None)` for an unencrypted document -- the overwhelmingly
    /// common case, and a single `contains_key` away.
    ///
    /// # What "in place" means, and its consequence for saving
    ///
    /// **Stream data is decrypted in the retained buffer.** RC4 is a stream
    /// cipher and preserves length exactly, so the plaintext fits precisely
    /// where the ciphertext was and every span, `/Length` and provenance
    /// record stays true. That is what lets this increment land without
    /// touching [`Stream`](crate::object::Stream), which holds a span rather
    /// than owning bytes. AES will not have that property -- its plaintext is
    /// shorter than its ciphertext -- so the AES increment has to solve a
    /// problem this one did not.
    ///
    /// **Strings are decrypted in the parsed objects**, because
    /// [`Object::String`] owns its bytes and a decrypted string cannot
    /// generally be re-escaped into the same number of source bytes.
    ///
    /// So after this runs, the buffer and the parsed objects **disagree**:
    /// streams are plaintext in both, strings are plaintext only in the
    /// objects. That is precisely why [`Document::save_full`] and
    /// [`Document::save_incremental`] refuse a decrypted document -- the
    /// writer re-emits untouched objects verbatim from their source span, and
    /// doing that here would produce a file whose `/Encrypt` claims
    /// encryption while half its content is plaintext. A file like that is
    /// not "partly saved"; it is unreadable by everything, including pdfcer.
    ///
    /// # Errors
    ///
    /// [`DocError::Encryption`] for a configuration pdfcer will not decrypt;
    /// [`DocError::PasswordRequired`] when nothing authenticated.
    fn decrypt_in_place(
        buf: &mut [u8],
        trailer: &Dict,
        objects: &mut HashMap<ObjId, IndirectObject>,
        password: Option<&[u8]>,
    ) -> Result<Option<DocumentEncryption>, DocError> {
        use crate::crypto::{EncryptionConfig, EncryptionUnsupported, apply};

        let Some(entry) = trailer.get(b"Encrypt") else {
            return Ok(None);
        };

        // The `/Encrypt` dictionary is usually direct in the trailer, but may
        // be indirect. If it is, its object number must be remembered: its
        // `/O` and `/U` are the INPUTS to the key derivation, so decrypting
        // them with a key derived from themselves would authenticate
        // successfully and then produce a document of noise (E2/E3).
        let (encrypt_dict, encrypt_dict_id) = match entry {
            Object::Dict(d) => (d.clone(), None),
            Object::Reference(id) => match objects.get(id).map(|o| &o.value) {
                Some(Object::Dict(d)) => (d.clone(), Some(id.num)),
                _ => {
                    return Err(DocError::Encryption(EncryptionUnsupported::Malformed(
                        "/Encrypt names an object that is not a dictionary",
                    )));
                }
            },
            _ => {
                return Err(DocError::Encryption(EncryptionUnsupported::Malformed(
                    "/Encrypt is neither a dictionary nor a reference",
                )));
            }
        };

        // Resolution for indirect `/O`, `/U` and `/CF` entries. Snapshotting
        // the values a closure needs is simpler than fighting the borrow
        // checker over `objects`, which is about to be mutated.
        let snapshot: HashMap<ObjId, Object> = objects
            .iter()
            .map(|(id, io)| (*id, io.value.clone()))
            .collect();
        let resolve = move |id: ObjId| snapshot.get(&id).cloned();

        let config = EncryptionConfig::parse(&encrypt_dict, &resolve)?;

        // Algorithm 2 step (e) and Algorithm 5 step (c) hash `/ID[0]`.
        // 7.6.1 E1: trailer `/ID` strings are never encrypted and are direct
        // objects, so this reads them as-is. A file with no `/ID` hashes
        // nothing there, which an empty slice expresses exactly -- the
        // algorithm has no branch for "no ID" (SPEC AMBIGUITY A4).
        let id0: Vec<u8> = match trailer.get(b"ID") {
            Some(Object::Array(items)) => match items.first() {
                Some(Object::String(s)) => s.clone(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };

        let Some((key, auth)) = config.authenticate(password, &id0) else {
            // A failed authentication is normally just a wrong password. At
            // `/R` 5 with a non-ASCII password it is ambiguous, because the
            // SASLprep step pdfcer does not implement may have been the thing
            // that mattered -- and telling the operator "wrong password" for a
            // password that was right sends them to re-check the one thing
            // that is not the problem.
            return Err(match password {
                Some(pw) if config.password_may_need_normalisation(pw) => {
                    DocError::PasswordRequiresNormalisation
                }
                // At /R 6 a failure is not unambiguously "wrong password": the
                // A13 loop-exit ambiguity can reject a correct password for a
                // file written under the other reading. Name it (Pass 5.4).
                _ if config.revision == 6 => DocError::PasswordRequiredR6,
                _ => DocError::PasswordRequired,
            });
        };

        // Algorithm 3.13, run once, immediately after the key is recovered and
        // before a single object is decrypted -- the check needs nothing but
        // the key, and running it here means the verdict is available to the
        // shells for the whole life of the document rather than being
        // recomputed on demand from state they would have to keep.
        let perms = config.check_perms(&key);

        // Decrypt every file-level object. Order within the map does not
        // matter: each object's key depends only on its own identity
        // (Algorithm 1), never on another object's contents.
        for (id, obj) in objects.iter_mut() {
            if apply::skip(obj, encrypt_dict_id, config.encrypt_metadata).is_some() {
                continue;
            }
            if key.streams_encrypted()
                && let Object::Stream(stream) = &mut obj.value
            {
                let span = stream.data_span;
                let end = span.start.saturating_add(span.len);
                // Checked access, not an in-bounds proof: `data_span` comes
                // from a `/Length` in an untrusted file, and a stream whose
                // declared length runs past the buffer is a real thing a
                // malformed (or deliberately hostile) document contains. A
                // span that does not fit is left alone -- the object then
                // fails to decode later, with an error about the object, which
                // is the honest place for it.
                if let Some(cipher_text) = buf.get(span.start..end) {
                    let plain = key.decrypt_stream(*id, cipher_text);
                    // ** The plaintext may be SHORTER than the ciphertext. **
                    //
                    // Increment 1 asserted the opposite here -- `RC4 must
                    // preserve length` -- and guarded the copy on
                    // `plain.len() == span.len`. That was true and correct for
                    // RC4, and it is the R186 shape: a guard keyed on a
                    // property that quietly stopped holding. Under `/AESV2`
                    // the ciphertext carries a 16-byte IV plus padding (T5), so
                    // the equality is ALWAYS false, the copy would ALWAYS be
                    // skipped, and every stream in an AES document would stay
                    // ciphertext -- with no test red and no error raised.
                    //
                    // Shorter is the easy direction: the plaintext still fits
                    // at `span.start`, so only the recorded length changes.
                    // Nothing re-reads the dictionary's `/Length` after parse
                    // (it is consumed once, at parser.rs's stream read), and
                    // `data_span` is what every downstream reader actually
                    // slices -- content.rs, attachments.rs, the object-stream
                    // path, edit.rs. So shortening the span is sufficient and
                    // `Stream` needs no new field.
                    if plain.len() <= span.len
                        && let Some(slot) = buf.get_mut(span.start..span.start + plain.len())
                    {
                        slot.copy_from_slice(&plain);
                        stream.data_span.len = plain.len();
                    }
                    // A plaintext LONGER than its ciphertext is impossible for
                    // both implemented ciphers, so there is no branch for it:
                    // it would have to overwrite the following object, and
                    // silently declining is the only safe response.
                }
            }
            if key.strings_encrypted() {
                apply::decrypt_strings(&mut obj.value, *id, &key);
            }
        }

        Ok(Some(DocumentEncryption {
            config,
            auth,
            perms,
        }))
    }

    /// How this document was encrypted, if it was -- `None` for a plain file.
    ///
    /// A **disclosure**, not a gate. See [`DocumentEncryption`].
    #[must_use]
    pub fn encryption(&self) -> Option<&DocumentEncryption> {
        self.encryption.as_ref()
    }

    /// Drop this document's encryption, in memory, without re-parsing.
    ///
    /// Load already decrypted every object's strings (in the parsed values) and
    /// every stream's data (in the retained buffer) — see
    /// [`Document::decrypt_in_place`]. So a document that WAS encrypted is,
    /// object-for-object, already a plaintext document; the only things still
    /// asserting encryption are the [`encryption`](Self::encryption) field and
    /// the trailer's `/Encrypt` reference. Clearing both is all that
    /// "remove encryption" needs at the model level — the writer then emits
    /// plaintext because the bytes it re-serialises already are.
    ///
    /// `Pass 5.4`: used by [`crate::EditSession::remove_encryption`] and
    /// [`crate::EditSession::set_permissions`] (which re-keys, so it first
    /// clears the old state, then the writer freshly encrypts).
    pub(crate) fn clear_encryption(&mut self) {
        self.encryption = None;
        self.trailer.remove(b"Encrypt");
    }

    /// Assemble a [`Document`] from a [`recover::RecoveredXref`].
    ///
    /// The recovered document is fixed into the shape a save must take
    /// (decision 013 §3.3.4, the recovered-base rule): its section shape is
    /// a plain classic table (`xref_stm: None`) so a full rewrite emits a
    /// fresh valid classic cross-reference, and `base_startxref` is 0
    /// because incremental append onto a broken base is refused by the
    /// writer. `/Size` suppression is 0 — recovery's synthesized `/Size`
    /// already covers every recovered object.
    ///
    /// `header_override` is the header-probe version when the header parsed
    /// (the xref-failure path); `None` on the header-failure path, where
    /// the recovery's own whole-buffer version scan is authoritative.
    fn assemble_recovered(
        buf: Vec<u8>,
        header_override: Option<PdfVersion>,
        rec: recover::RecoveredXref,
    ) -> Result<Self, DocError> {
        let version = header_override.unwrap_or(rec.version);
        Self::assemble(
            buf,
            version,
            rec.table,
            rec.trailer,
            0,
            SectionShape::Classic { xref_stm: None },
            rec.highest_object_number,
            0,
            Some(rec.report),
            // The recovery path never carries a password: `recover` refuses an
            // encrypted file outright (`RecoverError::Encrypted`), because
            // rebuild-by-scan looks for `N G obj` headers in bytes that would
            // be ciphertext. So a recovered document is never encrypted, and
            // threading a password here would be dead weight that read as
            // support.
            None,
        )
    }

    /// Load phase 2: resolve every type-2 (`InStream`) cross-reference
    /// entry through its object stream, inserting the parsed objects
    /// into `objects`.
    ///
    /// `compressed` is `(object number, container object number, index
    /// within container)`, sorted. Containers are looked up in
    /// `objects` — which is why this runs strictly after phase 1 (module
    /// docs) — decoded once, and cached for the rest of the load.
    ///
    /// Every compressed object is inserted with generation **0**:
    /// §7.5.7 fixes the generation of a compressed object at zero, and a
    /// type-2 entry carries no generation field to disagree with.
    fn load_compressed(
        buf: &[u8],
        compressed: &[(u32, u32, u32)],
        objects: &mut HashMap<ObjId, IndirectObject>,
    ) -> Result<(), DocError> {
        let mut cache: HashMap<u32, ObjectStream> = HashMap::new();

        for &(num, stream_num, index) in compressed {
            // §7.5.7: "The generation number of an object stream and of
            // any compressed object shall be zero."
            let container = ObjId::new(stream_num, 0);

            let objstm = match cache.entry(stream_num) {
                Entry::Occupied(slot) => slot.into_mut(),
                Entry::Vacant(slot) => {
                    let io = objects
                        .get(&container)
                        .ok_or(DocError::ObjectStreamMissing { container, num })?;
                    let Object::Stream(stream) = &io.value else {
                        return Err(DocError::ObjectStream {
                            container,
                            source: ObjStmError::NotAStream,
                        });
                    };
                    let raw = stream.data_span.slice(buf).ok_or(DocError::ObjectStream {
                        container,
                        source: ObjStmError::DataOutOfRange,
                    })?;
                    let parsed = ObjectStream::parse(&stream.dict, raw)
                        .map_err(|source| DocError::ObjectStream { container, source })?;
                    slot.insert(parsed)
                }
            };

            let idx = usize::try_from(index).unwrap_or(usize::MAX);
            let (found, value) = objstm
                .object_at(idx)
                .map_err(|source| DocError::ObjectStream { container, source })?;
            if found != num {
                return Err(DocError::ObjectStreamIdMismatch {
                    container,
                    index,
                    expected: num,
                    found,
                });
            }

            let id = ObjId::new(num, 0);
            objects.insert(
                id,
                IndirectObject {
                    id,
                    value,
                    provenance: Provenance::ObjectStream { container, index },
                },
            );
        }
        Ok(())
    }

    /// The retained source bytes (the coordinate system every span in
    /// this document indexes into).
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.buf
    }

    /// A read view of this document for the rasterizer, the vector object
    /// model and `pageops` — the file exactly as loaded.
    ///
    /// The mirror of [`EditSession::view`](crate::edit::EditSession::view),
    /// and the reason every read path can take one parameter type
    /// ([`DocumentView`]) instead of two overloads. A `Document` has no
    /// overlay and no staging buffer, so its view carries a
    /// [`StreamSource::Contiguous`](crate::view::StreamSource::Contiguous)
    /// over [`Document::bytes`] — meaning "render the file as it is on
    /// disk", which is exactly what `pdfcer` and the round-trip tools
    /// want — and why `pdfcer-render`'s `&Document` back-compat wrappers can
    /// build one implicitly without changing any caller's behaviour.
    ///
    /// Cheap: two borrows plus a version probe. Building one per call is
    /// the intended usage; there is nothing to cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::document::Document;
    /// use pdfcer_core::graph::ObjectGraph;
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let doc = Document::from_bytes(
    ///     include_bytes!("../../../fixtures/synthetic/hello.pdf").to_vec(),
    /// )?;
    /// let view = doc.view();
    /// assert_eq!(view.version(), doc.version());
    /// assert_eq!(view.catalog_id(), doc.catalog_id());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn view(&self) -> DocumentView<'_> {
        DocumentView::new(self, self.bytes(), self.version())
    }

    /// The newest trailer dictionary (§7.5.5 Table 15).
    #[must_use]
    pub const fn trailer(&self) -> &Dict {
        &self.trailer
    }

    /// The merged cross-reference table.
    #[must_use]
    pub const fn xref(&self) -> &XrefTable {
        &self.xref
    }

    /// The document's effective version: the header's `%PDF-M.N`,
    /// upgraded by the catalog's `/Version` name if present and higher
    /// (§7.5.5: version = max of the two; the catalog entry exists so
    /// an incremental update can raise the version without touching
    /// byte 0).
    #[must_use]
    pub fn version(&self) -> PdfVersion {
        let catalog_version = self
            .catalog()
            .ok()
            .and_then(|cat| cat.get(b"Version"))
            .and_then(Object::as_name)
            .and_then(|n| parse_version_name(n.as_bytes()));
        match catalog_version {
            Some(v) if v > self.header_version => v,
            _ => self.header_version,
        }
    }

    /// Look up an indirect object by full identity, applying the
    /// §7.3.10/§7.5.4 rules (module docs): dangling, stale-generation,
    /// and free all yield `None` here — callers wanting the null-object
    /// semantics use [`Document::resolve`], which maps `None` to
    /// [`Object::Null`].
    #[must_use]
    pub fn get(&self, id: ObjId) -> Option<&IndirectObject> {
        self.objects.get(&id)
    }

    /// Resolve `obj` to a non-reference value: follow reference chains
    /// (depth-guarded), mapping every unresolvable case to the null
    /// object per §7.3.10. Non-reference objects return themselves.
    #[must_use]
    pub fn resolve<'a>(&'a self, obj: &'a Object) -> &'a Object {
        const NULL: &Object = &Object::Null;
        let mut current = obj;
        for _ in 0..MAX_RESOLVE_DEPTH {
            match current {
                Object::Reference(id) => match self.get(*id) {
                    Some(io) => current = &io.value,
                    None => return NULL,
                },
                other => return other,
            }
        }
        // Chain too deep / cyclic: null, not an error (module docs).
        NULL
    }

    /// The document catalog (§7.7.2) — the dictionary the trailer's
    /// required `/Root` reference points at.
    ///
    /// # Errors
    ///
    /// [`DocError::NoCatalog`] if `/Root` is missing, not a reference,
    /// or doesn't resolve to a dictionary.
    pub fn catalog(&self) -> Result<&Dict, DocError> {
        self.trailer
            .get(b"Root")
            .map(|root| self.resolve(root))
            .and_then(Object::as_dict)
            .ok_or(DocError::NoCatalog)
    }

    /// Number of parsed (in-use) objects.
    #[must_use]
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// Iterate all parsed indirect objects (unordered).
    pub fn objects(&self) -> impl Iterator<Item = &IndirectObject> {
        self.objects.values()
    }

    /// The byte offset this file's own `startxref` names (§7.5.5).
    ///
    /// This is the value an appended update's `/Prev` must carry —
    /// §7.5.6: *"a `Prev` entry giving the location of the previous
    /// cross-reference section"*, which is the section `startxref`
    /// currently points at. Taking the base trailer's own `/Prev`
    /// instead is the classic off-by-one-revision bug: it would skip a
    /// revision and silently resurrect superseded objects.
    #[must_use]
    pub const fn base_startxref(&self) -> u64 {
        self.base_startxref
    }

    /// The physical form of the newest cross-reference section.
    ///
    /// The writer matches this rather than choosing (R33): emitting a
    /// cross-reference stream where the base file had a classic table
    /// silently raises a PDF 1.4 document's effective version to 1.5.
    /// §7.5.6 does not require the match — that is a recorded spec
    /// silence — which is exactly why pdfcer imposes it.
    #[must_use]
    pub const fn section_shape(&self) -> SectionShape {
        self.section_shape
    }

    /// Whether this file is a §7.5.8.4 **hybrid-reference** file: a
    /// classic cross-reference section whose trailer carries
    /// `/XRefStm`.
    ///
    /// Hybrid files are readable by pre-1.5 readers *and* carry the
    /// PDF 1.5 features those readers cannot see. Incremental save
    /// preserves that duality (form A: a classic update section
    /// carrying `/XRefStm` forward); a full rewrite cannot, and refuses
    /// by name rather than flattening it.
    #[must_use]
    pub const fn is_hybrid(&self) -> bool {
        matches!(
            self.section_shape,
            SectionShape::Classic { xref_stm: Some(_) }
        )
    }

    /// The cross-reference **recovery** record, or `None` for a cleanly
    /// loaded file (decision 013).
    ///
    /// `Some` means the stored cross-reference table could not be parsed
    /// and pdfcer rebuilt it by scanning the file for `N G obj` headers
    /// (rebuild-by-scan). The returned [`RecoveryReport`] carries the
    /// counted disclosure (why the strict path failed, how many objects
    /// came from the file-level scan vs. object streams, collisions,
    /// trailer source, offset-start). This is a **reviewable fact**
    /// (fuzzy-never-sneaky, R20): the CLI prints it and forces a distinct
    /// exit status, and the GUI shows a banner. A recovered document also
    /// cannot be saved incrementally — see
    /// [`Document::save_incremental`]'s refusal.
    #[must_use]
    pub const fn recovery(&self) -> Option<&RecoveryReport> {
        self.recovery.as_ref()
    }

    /// Whether this document was loaded via cross-reference recovery
    /// (decision 013) — the shorthand the writer's incremental-save refusal
    /// and the front ends' disclosure branch on.
    #[must_use]
    pub const fn loaded_via_recovery(&self) -> bool {
        self.recovery.is_some()
    }

    /// Annex F linearization ("Fast Web View") state, detected at load.
    ///
    /// Reported so a save can warn before spending the property
    /// (F.1: an incremental update *"shall"* de-linearize). pdfcer never
    /// repairs it and never strips a stale `/Linearized` dictionary —
    /// see [`crate::linearization`].
    #[must_use]
    pub const fn linearization(&self) -> Linearization {
        self.linearization
    }

    /// Save an incremental update (§7.5.6) to `path`.
    ///
    /// This is pdfcer's **default** save mode (`ARCHITECTURE.md` §5):
    /// the existing bytes are copied untouched and a new revision is
    /// appended, which is what keeps any pre-existing digital
    /// signature's `/ByteRange` digest valid (§12.8.1 NOTE 1).
    ///
    /// `dirty` is the **save-time diff against the base revision**, and
    /// the only thing that should compute one is
    /// [`crate::edit::EditSession::dirty_set`] — see
    /// `ARCHITECTURE.md` §11.1 for the "union of every command ever run"
    /// bug that a hand-built dirty set reintroduces. An empty one gives
    /// byte-identical output.
    ///
    /// ⚠️ **Incremental save structurally preserves superseded
    /// content.** The old bytes of every replaced object remain in the
    /// file by construction. Any operation whose contract is *removal*
    /// — redaction above all — must therefore use
    /// [`Document::save_full`] and must refuse this mode (R35).
    ///
    /// # Errors
    ///
    /// [`crate::writer::WriteError`] — I/O, a broken provenance span,
    /// or a cross-reference form that cannot express an entry.
    pub fn save_incremental(
        &self,
        path: &Path,
        dirty: &crate::writer::DirtySet,
        options: &crate::writer::SaveOptions,
    ) -> Result<crate::writer::SaveReport, crate::writer::WriteError> {
        let (bytes, report) = crate::writer::save_incremental(self, dirty, options)?;
        std::fs::write(path, &bytes)?;
        Ok(report)
    }

    /// Rewrite the whole document to `path` as a single revision.
    ///
    /// Every `File`-provenance object is re-emitted from its retained
    /// source bytes verbatim; only the header prefix, object offsets,
    /// the cross-reference section and the trailer are regenerated. So
    /// byte identity holds **per object definition**, never for the
    /// file as a whole (R32) — offsets legitimately move.
    ///
    /// ⚠️ **A full rewrite invalidates every existing digital
    /// signature**, because a signature covers a byte range that this
    /// mode necessarily disturbs (§12.8.1). That collides with R35's
    /// requirement that redaction use this mode; "redact a signed
    /// document" is a genuine either/or for the operator to resolve,
    /// never something pdfcer decides silently (R36, decision 007 W7).
    ///
    /// # Errors
    ///
    /// [`crate::writer::WriteError`], notably
    /// [`WriteError::HybridFullRewrite`](crate::writer::WriteError::HybridFullRewrite)
    /// for a §7.5.8.4 hybrid-reference input.
    pub fn save_full(
        &self,
        path: &Path,
        dirty: &crate::writer::DirtySet,
        options: &crate::writer::SaveOptions,
    ) -> Result<crate::writer::SaveReport, crate::writer::WriteError> {
        let (bytes, report) = crate::writer::save_full(self, dirty, options)?;
        std::fs::write(path, &bytes)?;
        Ok(report)
    }

    /// The lowest object number this document does **not** already
    /// account for — where a newly created object goes.
    ///
    /// Four sources are consulted, and taking the maximum of all four
    /// is the point:
    ///
    /// 1. **the highest number the cross-reference chain mentions before
    ///    the `/Size` filter runs.** The unfiltered number, deliberately:
    ///    `/Size` is a hard reader-side filter, so a file that
    ///    under-reports it loads with real entries invisible, and
    ///    allocating from the *filtered* view picks a number the file
    ///    already defines. That produces an update section whose entry
    ///    collides with a live object — a file that looks fine and
    ///    resolves to the wrong bytes. Found by the `writer_roundtrip`
    ///    fuzz target, on a corpus file carrying `/Size 3` over six
    ///    entries.
    ///
    ///    Free entries count too. A free number is *reusable* in
    ///    principle (§7.5.4's free list exists to be consumed), but
    ///    reusing one correctly means honouring the generation rules —
    ///    a resurrected object takes the free entry's generation — and
    ///    pdfcer has no deletion path to have created those entries in
    ///    the first place. Allocating past them is always safe;
    ///    consuming them is Pass 3.2 work.
    /// 2. every parsed object, in case the two disagree;
    /// 3. the trailer's `/Size`, so a *stale, oversized* `/Size` cannot
    ///    hand out a number that a `/Prev` revision defines and this
    ///    merged view dropped.
    /// 4. **★ the newest cross-reference STREAM's own object number**, when
    ///    the file has one ([`Document::section_shape`]).
    ///
    /// ## ★ Why source 4 exists, and the bug it closes
    ///
    /// A cross-reference stream is an indirect object, and the writer
    /// **reuses its number** for the update section it emits (`R33`: match
    /// the base's section shape). But it is *the section*, not a body
    /// object — the parser never files it in `objects`, and there is no
    /// requirement anywhere that it appear in its own `/Index` or be
    /// covered by its own `/Size`.
    ///
    /// So a file can carry `75 0 obj << /Type /XRef /Size 75 /Index [9 1 29
    /// 45] >>`: object 75 exists, and every one of sources 1–3 answers 74.
    /// This function then handed out **75**, the session wrote its new
    /// object there, and the writer wrote *its own cross-reference stream*
    /// over the top of it — same object number, later in the file, so the
    /// session's object simply vanished and the reader resolved the
    /// reference to the xref stream.
    ///
    /// The failure is silent in the worst way: the file parses, opens, and
    /// renders. Only the one thing the edit added is missing. Found by
    /// `tools/embed-sweep` over the pdfium corpus
    /// (`testing/resources/annotation_stamp_with_ap.pdf`), where an embedded
    /// font program came back as a 44-byte cross-reference stream and the
    /// text it should have drawn was silently skipped.
    ///
    /// **This was never specific to font embedding.** Any command that
    /// creates an object — adding text, an image, an annotation, a form
    /// field — hits it on any file shaped this way. It survived because a
    /// collision needs a file whose xref stream is outside its own `/Size`,
    /// which no producer pdfcer's fixtures came from emits.
    ///
    /// Returns `None` only if the document already reaches
    /// [`u32::MAX`], which Annex C's implementation limits put far out
    /// of reach for any real file — reported rather than wrapped,
    /// because silently allocating object 0 would corrupt the free list.
    ///
    /// ⚠️ A caller must **also** check
    /// [`Document::suppressed_object_count`] before creating an object:
    /// a valid number is not by itself a licence to write one.
    #[must_use]
    pub fn next_object_number(&self) -> Option<u32> {
        let from_objects = self.objects.keys().map(|id| id.num).max().unwrap_or(0);
        let from_size = self
            .trailer
            .get(b"Size")
            .and_then(Object::as_int)
            .and_then(|s| u32::try_from(s.saturating_sub(1)).ok())
            .unwrap_or(0);
        // Source 4 — see this function's docs. The writer reuses this number
        // for the section it emits, so handing it out would guarantee a
        // collision rather than merely risk one.
        let from_section = match self.section_shape {
            SectionShape::Stream { id, .. } => id.num,
            // A classic table is not an object and spends no number. The
            // `_` arm is deliberate: `SectionShape` is `#[non_exhaustive]`,
            // and a shape this build does not know must not silently be
            // treated as spending a number it might.
            _ => 0,
        };
        self.highest_object_number
            .max(from_objects)
            .max(from_size)
            .max(from_section)
            .checked_add(1)
            .filter(|n| *n != 0)
    }

    /// How many cross-reference entries this file's `/Size` is hiding.
    ///
    /// ## Why anyone cares (and why it blocks object creation)
    ///
    /// Table 15 / §7.5.5 make `/Size` a hard filter: an object numbered
    /// at or above it *"shall be ignored and defined to be missing"*. A
    /// file whose `/Size` under-reports is therefore **relying on the
    /// filter** — the entries above it are invisible, and the document
    /// may only be loadable at all because they are (they can point at
    /// bytes that do not parse).
    ///
    /// Creating an object raises `/Size`. That does not merely add the
    /// new object: it **exposes every suppressed entry below the new
    /// number**, resurrecting objects the operator never touched. Two
    /// separate rules say no — §5's minimal-diff invariant (an edit must
    /// not change objects it did not name) and R27's fail-clean posture
    /// (name the refusal rather than produce a plausible wrong file).
    ///
    /// So a non-zero count means *"an edit may modify existing objects
    /// in this file, but must not create one"*. The eventual fix is to
    /// emit explicit free entries (§7.5.4, generation 65535 — the very
    /// mechanism §7.5.8.4 uses to hide an object) for the exposed range,
    /// which needs free-list writing that Pass 3.2 brings.
    ///
    /// Zero for essentially every well-formed file, which is why this is
    /// a refusal rather than a feature gap worth blocking on.
    #[must_use]
    pub const fn suppressed_object_count(&self) -> usize {
        self.suppressed_by_size
    }
}

/// Parse the indirect object at `offset`, resolving indirect `/Length`
/// values through the xref table (the §7.3.10 EXAMPLE 3 pattern: the
/// length object is itself fetched and parsed on demand — it is always
/// a plain integer object, so this nested parse cannot recurse).
///
/// `pub(crate)` so the cross-reference recovery path (`crate::recover`) can
/// reuse the exact same object-parse + indirect-`/Length` resolution the
/// normal loader uses (rule 13: no new tokenizer) when confirming scanned
/// `N G obj` candidates.
///
/// `policy` selects the `/Length`-vs-`endstream` strictness
/// ([`StreamLengthPolicy`]). It is [`StreamLengthPolicy::Strict`] on every
/// clean-load call site and [`StreamLengthPolicy::RecoverFromEndstream`]
/// only under rebuild-by-scan recovery. Returns the parsed object together
/// with the number of stream extents that had to be re-derived (always `0`
/// under `Strict`), so the caller can fold it into the counted disclosure.
///
/// Note the nested `resolve_length` parse stays **strict** regardless: a
/// `/Length` object is a plain integer (§7.3.10 EXAMPLE 3), never a stream,
/// so the policy has nothing to act on there and holding it strict keeps
/// the nested parse from recursing into extent recovery.
pub(crate) fn parse_object_at(
    buf: &[u8],
    table: &XrefTable,
    offset: u64,
    policy: StreamLengthPolicy,
    terminator: TerminatorPolicy,
) -> Result<(IndirectObject, ParseRepairs), ParseError> {
    let offset = usize::try_from(offset).unwrap_or(usize::MAX);
    let mut resolve_length = |id: ObjId| -> Option<i64> {
        let XrefEntry::InUse {
            offset: len_offset,
            generation,
        } = table.get(id.num)?
        else {
            return None;
        };
        if generation != id.generation {
            return None;
        }
        let len_offset = usize::try_from(len_offset).ok()?;
        let io = Parser::at(buf, len_offset)
            .parse_indirect_object(&mut |_| None)
            .ok()?;
        (io.id == id).then_some(())?;
        io.value.as_int()
    };
    let mut parser = Parser::at(buf, offset)
        .with_stream_length_policy(policy)
        .with_terminator_policy(terminator);
    let io = parser.parse_indirect_object(&mut resolve_length)?;
    Ok((
        io,
        ParseRepairs {
            stream_lengths: parser.stream_lengths_recovered(),
            missing_endobj: parser.missing_endobj_recovered(),
        },
    ))
}

/// How much repair a single object's parse needed.
///
/// Every field counts a place where the file contradicted itself or
/// omitted something §7.3.10 requires, and pdfcer chose to continue rather
/// than refuse. **All of them are zero under the strict policies**, so a
/// non-zero total is proof the file was damaged — which is why these are
/// carried out to the recovery report and disclosed (R20,
/// fuzzy-never-sneaky) instead of being absorbed silently.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ParseRepairs {
    /// Stream extents re-derived from `endstream` because `/Length` was
    /// unusable ([`StreamLengthPolicy::RecoverFromEndstream`]).
    pub stream_lengths: usize,
    /// Definitions accepted with no `endobj` keyword
    /// ([`TerminatorPolicy::RecoverAtNextHeader`]).
    pub missing_endobj: usize,
}

/// Parse a catalog `/Version` name (`1.7`, `2.0`) into a version pair.
fn parse_version_name(name: &[u8]) -> Option<PdfVersion> {
    let text = std::str::from_utf8(name).ok()?;
    let (major, minor) = text.split_once('.')?;
    Some(PdfVersion {
        major: major.parse().ok()?,
        minor: minor.parse().ok()?,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    /// Build a small, offset-consistent classic PDF from parts: each
    /// entry in `objects` is (object number, body text); generation 0.
    /// Returns the complete file bytes.
    fn build_pdf(objects: &[(u32, &str)], trailer_extra: &str) -> Vec<u8> {
        let mut buf = b"%PDF-1.4\n".to_vec();
        let mut offsets: Vec<(u32, usize)> = Vec::new();
        for (num, body) in objects {
            offsets.push((*num, buf.len()));
            buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref_at = buf.len();
        let max_num = objects.iter().map(|(n, _)| *n).max().unwrap_or(0);
        buf.extend_from_slice(format!("xref\n0 {}\n", max_num + 1).as_bytes());
        buf.extend_from_slice(b"0000000000 65535 f\r\n");
        for num in 1..=max_num {
            match offsets.iter().find(|(n, _)| *n == num) {
                Some((_, off)) => {
                    buf.extend_from_slice(format!("{off:010} 00000 n\r\n").as_bytes());
                }
                None => buf.extend_from_slice(b"0000000000 65535 f\r\n"),
            }
        }
        buf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R {trailer_extra}>>\nstartxref\n{xref_at}\n%%EOF\n",
                max_num + 1
            )
            .as_bytes(),
        );
        buf
    }

    fn minimal_doc() -> Vec<u8> {
        build_pdf(
            &[
                (1, "<< /Type /Catalog /Pages 2 0 R >>"),
                (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            ],
            "",
        )
    }

    #[test]
    fn loads_minimal_document() {
        let doc = Document::from_bytes(minimal_doc()).unwrap();
        assert_eq!(doc.object_count(), 2);
        assert_eq!(doc.version().to_string(), "1.4");
        let cat = doc.catalog().unwrap();
        assert_eq!(
            cat.get(b"Type").unwrap().as_name().unwrap().as_bytes(),
            b"Catalog"
        );
    }

    #[test]
    fn resolve_follows_references_and_nulls_dangling() {
        let doc = Document::from_bytes(minimal_doc()).unwrap();
        let cat = doc.catalog().unwrap();
        // /Pages resolves through the reference to the Pages dict.
        let pages = doc.resolve(cat.get(b"Pages").unwrap());
        assert_eq!(
            pages.as_dict().unwrap().get(b"Count").unwrap().as_int(),
            Some(0)
        );
        // §7.3.10: dangling → null, not an error.
        let dangling = Object::Reference(ObjId::new(17, 0));
        assert_eq!(*doc.resolve(&dangling), Object::Null);
        // Stale generation → null too.
        let stale = Object::Reference(ObjId::new(2, 9));
        assert_eq!(*doc.resolve(&stale), Object::Null);
    }

    #[test]
    fn reference_cycle_resolves_to_null() {
        // `3 0 obj 3 0 R endobj` — legal syntax, must not loop.
        let doc = Document::from_bytes(build_pdf(
            &[
                (1, "<< /Type /Catalog /Pages 2 0 R >>"),
                (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
                (3, "3 0 R"),
            ],
            "",
        ))
        .unwrap();
        let cyc = Object::Reference(ObjId::new(3, 0));
        assert_eq!(*doc.resolve(&cyc), Object::Null);
    }

    #[test]
    fn provenance_spans_recover_source_bytes() {
        // The whole point of the retained buffer: every object's span
        // slices back to its exact definition bytes.
        let doc = Document::from_bytes(minimal_doc()).unwrap();
        let io = doc.get(ObjId::new(1, 0)).unwrap();
        let raw = io.file_span().unwrap().slice(doc.bytes()).unwrap();
        assert!(raw.starts_with(b"1 0 obj"));
        assert!(raw.ends_with(b"endobj"));
    }

    #[test]
    fn catalog_version_upgrades_header() {
        // §7.5.5: effective version = max(header, catalog /Version).
        let doc = Document::from_bytes(build_pdf(
            &[
                (1, "<< /Type /Catalog /Pages 2 0 R /Version /1.7 >>"),
                (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            ],
            "",
        ))
        .unwrap();
        assert_eq!(doc.version().to_string(), "1.7");
    }

    #[test]
    fn catalog_version_cannot_downgrade_header() {
        let mut bytes = build_pdf(
            &[
                (1, "<< /Type /Catalog /Pages 2 0 R /Version /1.2 >>"),
                (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            ],
            "",
        );
        // Bump the header to 1.6; catalog says 1.2; max wins.
        let pos = bytes.windows(3).position(|w| w == b"1.4").unwrap();
        bytes[pos..pos + 3].copy_from_slice(b"1.6");
        let doc = Document::from_bytes(bytes).unwrap();
        assert_eq!(doc.version().to_string(), "1.6");
    }

    #[test]
    fn object_id_mismatch_is_strict_error() {
        // Corrupt the xref so object 2's entry points at object 1's
        // offset — the strict loader must refuse, naming both ids.
        let bytes = minimal_doc();
        let text = String::from_utf8(bytes.clone()).unwrap();
        let obj1_off = text.find("1 0 obj").unwrap();
        // xref entry lines: find object 2's and overwrite its offset
        // with object 1's.
        let obj2_off = text.find("2 0 obj").unwrap();
        let entry = format!("{obj2_off:010} 00000 n");
        let entry_pos = text.find(&entry).unwrap();
        let mut corrupted = bytes;
        corrupted[entry_pos..entry_pos + 10].copy_from_slice(format!("{obj1_off:010}").as_bytes());
        let err = Document::from_bytes(corrupted).unwrap_err();
        assert!(matches!(err, DocError::ObjectIdMismatch { .. }));
    }

    #[test]
    fn missing_root_is_no_catalog() {
        let mut buf = b"%PDF-1.4\n".to_vec();
        let xref_at = buf.len();
        buf.extend_from_slice(b"xref\n0 1\n0000000000 65535 f\r\n");
        buf.extend_from_slice(
            format!("trailer\n<< /Size 1 >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
        );
        let doc = Document::from_bytes(buf).unwrap();
        assert!(matches!(doc.catalog(), Err(DocError::NoCatalog)));
    }

    #[test]
    fn stream_with_indirect_length_loads_end_to_end() {
        // §7.3.10 EXAMPLE 3 through the full Document path.
        let doc = Document::from_bytes(build_pdf(
            &[
                (1, "<< /Type /Catalog /Pages 2 0 R >>"),
                (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
                (3, "<< /Length 4 0 R >>\nstream\nhello wo\nendstream"),
                (4, "8"),
            ],
            "",
        ))
        .unwrap();
        let io = doc.get(ObjId::new(3, 0)).unwrap();
        let Object::Stream(s) = &io.value else {
            panic!("not a stream");
        };
        assert_eq!(s.data_span.slice(doc.bytes()).unwrap(), b"hello wo");
    }
}
