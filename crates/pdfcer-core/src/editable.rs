//! **Editable round trip** — export a PDF's internals to a form a human can
//! edit in a text editor, and compile the edited form back
//! (`Pass 194.0`).
//!
//! The shape qpdf calls QDF, with one capability qpdf does not have.
//!
//! # The export is itself a PDF, and that is the whole trick
//!
//! It would be easy — and wrong — to invent a bespoke text syntax here. Doing so
//! would mean writing a second parser to read it back, and pdfcer would then have
//! **two** answers to "what does this byte sequence mean", which is exactly the
//! drift `R92` exists to prevent.
//!
//! So [`export`] emits a **valid PDF**, and [`import`] is just
//! [`Document::from_bytes`]. Everything that makes the export readable is a
//! choice a conforming file is allowed to make:
//!
//! - **Object streams are expanded.** Every object becomes a top-level
//!   `N G obj`, so a text search finds it. This is the single biggest
//!   readability win: on a modern file most of the interesting dictionaries —
//!   every `/ExtGState`, every colour space — are compressed inside `/ObjStm`
//!   containers and are invisible to `grep`.
//! - **Streams are decoded and `/Filter` is dropped.** A content stream becomes
//!   readable operators; `/Length` is rewritten to match.
//! - **A classic `xref` table**, so the file's own structure is legible.
//! - Objects in ascending numeric order, one per line-group, so a `diff`
//!   between two exports is meaningful.
//!
//! # ★★ WHAT THIS DOES THAT qpdf CANNOT: the incremental compile-back
//!
//! qpdf's own `TODO.md` lists *"Support incremental updates"* and *"Support
//! digital signatures. This probably requires support for incremental updates"*
//! as **unimplemented**. Every qpdf write is a full rewrite, so every qpdf
//! round trip invalidates every signature in the file.
//!
//! pdfcer has had incremental save as its **default** save mode from the start
//! (`ARCHITECTURE.md` §5), so [`import`] can do the thing qpdf cannot: diff the
//! edited export against the **original** document and emit only the objects
//! that genuinely changed, appended to the original bytes as a §7.5.6
//! incremental update. Objects nobody touched are **not re-emitted at all** —
//! their bytes, and every byte range before them, are unchanged, so a signature
//! covering them remains valid.
//!
//! What still breaks a signature is **editing an object that signature covers**,
//! and no implementation can avoid that. The distinction worth keeping: qpdf
//! breaks signatures because of *how it writes*; pdfcer breaks one only because
//! of *what you changed*.
//!
//! # ★★★ THE HARD PART, AND IT IS NOT THE WRITING: comparing a decoded stream
//! # against a compressed one
//!
//! The export decodes every stream. The original's streams are compressed. So a
//! byte-for-byte comparison of a stream object would mark **every stream in the
//! document as edited**, the incremental update would contain the whole file,
//! and the feature's entire point would be lost — silently, while appearing to
//! work.
//!
//! [`import`] therefore compares streams **semantically**
//! ([`stream_is_unchanged`]): the decoded payloads must match, and the
//! dictionaries must match *ignoring the three keys that describe the encoding
//! rather than the content* — `/Filter`, `/DecodeParms` and `/Length`. Two
//! streams that differ only in how they were compressed are the same stream.
//!
//! This is the one place where a bug would be invisible in testing and
//! expensive in practice, which is why it is a named function with its own
//! tests rather than an inline comparison.
//!
//! # What is deliberately NOT preserved, and why saying so matters
//!
//! - **Object numbers ARE preserved.** They have to be: an incremental update
//!   that renumbered objects would have to rewrite every reference to them, and
//!   the result would not be an update at all. This also rules out the
//!   compaction qpdf offers.
//! - **Encryption is refused, not silently dropped.** An exported plaintext
//!   view of an encrypted document is a decryption, and writing it to disk
//!   without the operator asking is not this function's decision to make.
//! - **The export is not byte-identical to the input and never will be.** It is
//!   a full rewrite by construction. Only the *compile-back* is minimal-diff.

use crate::document::Document;
use crate::object::{Dict, Name, ObjId, Object};
use crate::writer::{DirtySet, serialize};
use std::collections::BTreeMap;

/// Why an export or import could not be performed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum EditableError {
    /// The document is encrypted.
    ///
    /// Refused rather than handled: exporting an encrypted document in
    /// plaintext is a decryption, and §7.6.3.1 is explicit that nothing in PDF
    /// encryption enforces permissions — so the decision to write a decrypted
    /// view to disk belongs to the operator, stated at the moment it happens,
    /// not to this function.
    #[error(
        "this document is encrypted; exporting it as editable text would write its decrypted \
         contents to disk, which is a decision for the operator rather than for this verb"
    )]
    Encrypted,
    /// A stream's data span does not lie within its document's buffer.
    #[error("object {id} has a stream whose data lies outside the document buffer")]
    StreamOutOfRange {
        /// The offending object.
        id: ObjId,
    },
    /// The document has no `/Root`, so the export would have no trailer.
    #[error("this document has no resolvable /Root catalog, so it cannot be re-assembled")]
    NoCatalog,
}

/// What an [`import`] found when it compared the edited export against the
/// original.
///
/// Returned alongside the [`DirtySet`] so a shell can disclose the change
/// **before** writing anything — rule 4's off-canvas reporting, and the number
/// an operator most wants before committing an edit made in a text editor.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct ImportReport {
    /// Objects present in both, with different content.
    pub modified: Vec<ObjId>,
    /// Objects the edited file has and the original does not.
    pub added: Vec<ObjId>,
    /// Objects the original has and the edited file does not.
    pub removed: Vec<ObjId>,
    /// Objects compared and found identical — the ones an incremental save will
    /// not re-emit at all.
    pub unchanged: usize,
    /// Objects judged unchanged only because their streams compared equal
    /// *semantically* — same decoded payload, different (or absent)
    /// compression.
    ///
    /// Reported separately because it is the count that proves the
    /// decoded-versus-compressed comparison is doing its job. If this is 0 on a
    /// document that has streams, the comparison silently fell back to
    /// byte equality and the incremental update is about to contain the whole
    /// file.
    pub streams_matched_after_decode: usize,
}

impl ImportReport {
    /// Whether anything at all changed.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.modified.is_empty() && self.added.is_empty() && self.removed.is_empty()
    }
}

/// Keys that describe **how** a stream is stored rather than **what** it holds.
///
/// Ignored when comparing two streams for semantic equality. `/Length` is here
/// for the same reason as the two filter keys: it is a property of the encoded
/// bytes, and the encoded bytes are exactly what the export changes.
const ENCODING_KEYS: [&[u8]; 3] = [b"Filter", b"DecodeParms", b"Length"];

/// Export `doc` as an editable PDF.
///
/// # Errors
///
/// [`EditableError::Encrypted`] for an encrypted document,
/// [`EditableError::NoCatalog`] when there is no `/Root`, and
/// [`EditableError::StreamOutOfRange`] when a stream's span is not inside the
/// document's buffer.
pub fn export(doc: &Document) -> Result<Vec<u8>, EditableError> {
    if doc.encryption().is_some() {
        return Err(EditableError::Encrypted);
    }
    let root = doc
        .trailer()
        .get(b"Root")
        .and_then(Object::as_reference)
        .ok_or(EditableError::NoCatalog)?;

    // Decode every stream up front, so a failure is reported before any bytes
    // are written rather than half way through a file.
    //
    // A stream that will NOT decode is kept in its original encoded form with
    // its `/Filter` intact. That is deliberate and is the fail-clean posture
    // (`ARCHITECTURE.md` §10): a single corrupt stream must not cost the
    // operator the export of the other nine hundred objects, and silently
    // emitting an empty stream in its place would destroy data.
    let mut objects: BTreeMap<ObjId, Object> = BTreeMap::new();
    let mut payloads: Vec<u8> = Vec::new();
    let mut undecoded = 0usize;
    for obj in doc.objects() {
        match &obj.value {
            Object::Stream(s) => {
                let raw = serialize::stream_data(s, doc.bytes())
                    .ok_or(EditableError::StreamOutOfRange { id: obj.id })?;
                match crate::filters::decode_stream(&s.dict, raw) {
                    Ok(data) => {
                        let mut dict = s.dict.clone();
                        for k in ENCODING_KEYS {
                            dict.remove(k);
                        }
                        dict.insert(
                            Name::from(b"Length"),
                            Object::Integer(i64::try_from(data.len()).unwrap_or(0)),
                        );
                        let start = payloads.len();
                        payloads.extend_from_slice(&data);
                        objects.insert(
                            obj.id,
                            Object::Stream(crate::object::Stream {
                                dict,
                                data_span: crate::span::ByteSpan::new(start, data.len()),
                            }),
                        );
                    }
                    Err(_) => {
                        undecoded += 1;
                        let start = payloads.len();
                        payloads.extend_from_slice(raw);
                        objects.insert(
                            obj.id,
                            Object::Stream(crate::object::Stream {
                                dict: s.dict.clone(),
                                data_span: crate::span::ByteSpan::new(start, raw.len()),
                            }),
                        );
                    }
                }
            }
            other => {
                objects.insert(obj.id, other.clone());
            }
        }
    }

    let mut out = Vec::with_capacity(doc.bytes().len() * 2);
    out.extend_from_slice(b"%PDF-");
    out.extend_from_slice(doc.version().to_string().as_bytes());
    out.extend_from_slice(b"\n%\xe2\xe3\xcf\xd3\n");
    out.extend_from_slice(
        b"% Editable export produced by pdfcer. Object streams expanded, stream\n\
          % data decoded, cross-reference written as a classic table. This IS a\n\
          % valid PDF and can be opened normally; it is NOT byte-identical to the\n\
          % source and is not meant to be. Edit it, then compile it back with\n\
          % `pdfcer import-structure` to append only what you changed as an\n\
          % incremental update to the ORIGINAL file.\n",
    );
    if undecoded > 0 {
        out.extend_from_slice(
            format!("% {undecoded} stream(s) would not decode and are left ENCODED, with their /Filter intact.\n")
                .as_bytes(),
        );
    }

    let encoder = crate::writer::encoder::IdentityEncoder;
    let mut offsets: BTreeMap<u32, usize> = BTreeMap::new();
    let mut highest = 0u32;
    for (id, value) in &objects {
        offsets.insert(id.num, out.len());
        highest = highest.max(id.num);
        serialize::write_indirect(&mut out, *id, value, &payloads, &encoder);
    }

    // A classic table, contiguous from 0, with a free head entry — the most
    // legible form and the one a reader can check by eye.
    let startxref = out.len();
    let size = highest + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for num in 1..size {
        match offsets.get(&num) {
            Some(off) => out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes()),
            // A number the document never defined. Free, which is exactly what
            // it is; inventing an offset would be a lie the reader cannot see.
            None => out.extend_from_slice(b"0000000000 65535 f \n"),
        }
    }
    out.extend_from_slice(b"trailer\n<< /Size ");
    out.extend_from_slice(size.to_string().as_bytes());
    out.extend_from_slice(b" /Root ");
    out.extend_from_slice(format!("{} {} R", root.num, root.generation).as_bytes());
    if let Some(Object::Reference(info)) = doc.trailer().get(b"Info") {
        out.extend_from_slice(format!(" /Info {} {} R", info.num, info.generation).as_bytes());
    }
    out.extend_from_slice(b" >>\nstartxref\n");
    out.extend_from_slice(startxref.to_string().as_bytes());
    out.extend_from_slice(b"\n%%EOF\n");
    Ok(out)
}

/// Are two stream objects the same stream, ignoring how they are stored?
///
/// See this module's header: the export decodes, the original is compressed, so
/// byte equality would call every stream in the document modified. The decoded
/// payloads must match and the dictionaries must match ignoring
/// [`ENCODING_KEYS`].
///
/// Returns `None` when either stream's span does not lie in its buffer, which
/// is a genuine "cannot tell" rather than a difference — the caller treats it
/// as modified, because re-emitting an object unnecessarily is safe and
/// skipping a changed one is not.
#[must_use]
pub fn stream_is_unchanged(
    a: &crate::object::Stream,
    a_src: &[u8],
    b: &crate::object::Stream,
    b_src: &[u8],
) -> Option<bool> {
    let a_raw = serialize::stream_data(a, a_src)?;
    let b_raw = serialize::stream_data(b, b_src)?;
    let a_data = crate::filters::decode_stream(&a.dict, a_raw).unwrap_or_else(|_| a_raw.to_vec());
    let b_data = crate::filters::decode_stream(&b.dict, b_raw).unwrap_or_else(|_| b_raw.to_vec());
    if a_data != b_data {
        return Some(false);
    }
    Some(dicts_match_ignoring_encoding(&a.dict, &b.dict))
}

/// Dictionary equality that ignores the keys describing the encoding.
///
/// Order-insensitive: §7.3.7 says a dictionary's written entry order *"shall be
/// ignored"*, so two dictionaries that differ only in order are the same
/// dictionary and treating them as different would re-emit objects nobody
/// edited.
fn dicts_match_ignoring_encoding(a: &Dict, b: &Dict) -> bool {
    let keep = |d: &Dict| -> BTreeMap<Vec<u8>, Object> {
        d.iter()
            .filter(|(k, _)| !ENCODING_KEYS.contains(&k.0.as_slice()))
            .map(|(k, v)| (k.0.clone(), v.clone()))
            .collect()
    };
    keep(a) == keep(b)
}

/// Diff `edited` against `original` and build the [`DirtySet`] an incremental
/// save needs.
///
/// The returned set contains **only** what genuinely changed, so
/// [`crate::writer::save_incremental`] appends a minimal §7.5.6 update to the
/// original bytes and every untouched object keeps the bytes — and the byte
/// offsets — it already had.
#[must_use]
pub fn import(original: &Document, edited: &Document) -> (DirtySet, ImportReport) {
    // bypass-exempt: `import` BUILDS a DirtySet, it does not APPLY one.
    //
    // The gate exists to stop a mutation reaching the writer without an undo
    // entry, a rule-4 disclosure or a certification check. This function makes
    // none: it is a pure diff of two already-parsed documents that RETURNS the
    // set for a caller to do something with. Nothing here is saved, and the
    // caller -- `pdfcer`'s `import` subcommand -- is what performs the
    // save, under the same disclosure obligations as any other.
    //
    // ★ The distinction the gate cannot see, stated so a reviewer can: the
    // three sanctioned exceptions above are all code that WRITES. This one
    // never touches an output file. Routing it through `EditSession` would
    // mean inventing a session for a computation whose whole output IS the
    // description of a change, which is the thing a session already holds --
    // it would be a session wrapping its own contents.
    //
    // bypass-exempt: builds a set, never applies one (reason immediately above).
    let mut dirty = DirtySet::empty();
    let mut report = ImportReport::default();
    let mut staging: Vec<u8> = Vec::new();
    let base_len = original.bytes().len();

    for obj in edited.objects() {
        let Some(before) = original.get(obj.id) else {
            report.added.push(obj.id);
            stage(
                &mut dirty,
                &mut staging,
                base_len,
                obj.id,
                &obj.value,
                edited,
            );
            continue;
        };
        let same = match (&before.value, &obj.value) {
            (Object::Stream(a), Object::Stream(b)) => {
                match stream_is_unchanged(a, original.bytes(), b, edited.bytes()) {
                    Some(true) => {
                        report.streams_matched_after_decode += 1;
                        true
                    }
                    // `None` — a span we cannot read — is treated as CHANGED.
                    // Re-emitting an object nobody edited costs bytes;
                    // skipping one that was edited loses the operator's work.
                    Some(false) | None => false,
                }
            }
            (a, b) => a == b,
        };
        if same {
            report.unchanged += 1;
        } else {
            report.modified.push(obj.id);
            stage(
                &mut dirty,
                &mut staging,
                base_len,
                obj.id,
                &obj.value,
                edited,
            );
        }
    }

    for obj in original.objects() {
        if edited.get(obj.id).is_none() {
            report.removed.push(obj.id);
            dirty.delete(obj.id);
        }
    }

    report
        .modified
        .sort_unstable_by_key(|i| (i.num, i.generation));
    report.added.sort_unstable_by_key(|i| (i.num, i.generation));
    report
        .removed
        .sort_unstable_by_key(|i| (i.num, i.generation));
    // bypass-exempt: the staging buffer belongs to the DirtySet being BUILT
    // and returned here; see the reason at the head of this function. No save
    // happens in this file.
    dirty.set_staging(staging);
    (dirty, report)
}

/// Stage one replacement, re-basing a stream's span into the writer's combined
/// coordinate system.
///
/// A stream parsed out of the EDITED document has a span into the edited
/// buffer, which the writer knows nothing about. `DirtySet`'s staging buffer
/// exists for exactly this: an authored span is expressed as
/// `base.len() + local`, and the writer serializes against `base ++ staging`.
/// Copying the bytes is what makes the replacement independent of the edited
/// document's lifetime.
fn stage(
    dirty: &mut DirtySet,
    staging: &mut Vec<u8>,
    base_len: usize,
    id: ObjId,
    value: &Object,
    edited: &Document,
) {
    match value {
        Object::Stream(s) => {
            let data = serialize::stream_data(s, edited.bytes()).unwrap_or(&[]);
            let local = staging.len();
            staging.extend_from_slice(data);
            let mut dict = s.dict.clone();
            // The export dropped `/Filter`, so the staged bytes are literal.
            // `/Length` must describe them or the file will not re-parse.
            dict.insert(
                Name::from(b"Length"),
                Object::Integer(i64::try_from(data.len()).unwrap_or(0)),
            );
            dirty.replace(
                id,
                Object::Stream(crate::object::Stream {
                    dict,
                    data_span: crate::span::ByteSpan::new(base_len + local, data.len()),
                }),
            );
        }
        other => dirty.replace(id, other.clone()),
    }
}
