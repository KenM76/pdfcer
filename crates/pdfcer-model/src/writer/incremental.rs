//! Incremental save (§7.5.6): append one revision to the stored bytes.
//!
//! An encrypted document appends under its own handler and key (§7.6.2):
//! dirty objects are encrypted, untouched ones are not re-emitted, and the
//! trailer carries `/Encrypt` and `/ID[0]` unchanged (§7.6.3).

use std::collections::BTreeMap;

use crate::crypto::Cipher;
use crate::crypto::apply::skip_value;
use crate::document::{Document, DocumentEncryption};
use crate::object::{Dict, Name, ObjId, Object};
use crate::xref::{SectionShape, XrefEntry};

use super::encoder::{IdentityEncoder, KeyEncoder, ObjectEncoder};
use super::save::{
    Emission, SaveReport, apply_free_list, bump_size, copy_trailer_without_prev, emit_object,
    refresh_changing_identifier, relist_shadowed_by_xref_stm,
};
use super::{DirtySet, SaveOptions, WriteError, serialize, xref_out};

/// Append a revision to `doc` and return the complete new file bytes
/// (§7.5.6).
///
/// With an empty `dirty` set the output is **byte-identical to the
/// input** — see [`super`]'s contract table.
///
/// # Errors
///
/// [`WriteError`] — a broken provenance span, a dirty object that is
/// not in the document, or a cross-reference form that cannot express
/// an entry it was handed.
///
/// # Examples
///
/// ```
/// use pdfcer_model::document::Document;
/// use pdfcer_model::writer::{DirtySet, SaveOptions, save_incremental};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// // Embedded at compile time so the example does not depend on the
/// // working directory a doctest happens to run in.
/// let bytes: Vec<u8> =
///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec();
/// let doc = Document::from_bytes(bytes.clone())?;
///
/// // Zero edits means zero bytes: the output IS the input.
/// let (out, report) =
///     save_incremental(&doc, &DirtySet::empty(), &SaveOptions::identity())?;
/// assert_eq!(out, bytes);
/// assert!(report.byte_identical);
/// assert_eq!(report.bytes_appended, 0);
/// # Ok(())
/// # }
/// ```
pub fn save_incremental(
    doc: &Document,
    dirty: &DirtySet,
    // `producer` is ignored here: it is about `/Info`, which an append must
    // never touch (§7.6.5 forbids re-encrypting it, too). The two EOL knobs
    // describe bytes this path writes.
    options: &SaveOptions,
) -> Result<(Vec<u8>, SaveReport), WriteError> {
    // Decision 013 (the recovered-base rule): an append onto an invalid base
    // xref would write a `/Prev` pointing at a section that does not exist.
    // Checked FIRST so no later path can append to a broken base.
    if doc.loaded_via_recovery() {
        return Err(WriteError::RecoveredBaseForbidsIncremental);
    }

    // Parsed spans index the document's buffer, which load decrypted in
    // place; the appended file must start from the bytes as stored.
    let values = doc.bytes();
    let base = doc
        .encryption()
        .map_or(values, DocumentEncryption::ciphertext);
    // Resolve `EOL-A1` against the file being saved, once, so an incremental
    // save and a full rewrite of one document never disagree about its form.
    let entry_eol = options.xref_entry_eol.resolve(base);
    let mut out = base.to_vec();
    // R45: authored appearance streams carry spans past the base into the
    // session's staging buffer; `combined` is `values ++ staging`.
    let combined = dirty.combined_source(values);

    // Zero edits means zero bytes, checked before anything else so no later
    // code path can append to an unchanged document.
    if dirty.is_empty() {
        return Ok((out, SaveReport::unchanged(base.len())));
    }
    let encoder = encoder_for(doc, dirty)?;

    // Separate the appended region from an unterminated final line (§7.2.3).
    if !matches!(out.last(), Some(b'\n' | b'\r')) {
        out.push(b'\n');
    }
    // §14.4's changing identifier is digested over exactly the appended
    // object definitions (see `super::fileid`).
    let body_start = out.len();
    let mut bodies = write_bodies(&mut out, doc, dirty, (base, &combined), &*encoder)?;
    let body_end = out.len();
    let deleted = close_entries(&mut bodies.entries, doc, dirty, base);
    let trailer = section_trailer(
        doc,
        dirty,
        &bodies.entries,
        base.len(),
        out.get(body_start..body_end).unwrap_or(&[]),
    );
    write_section(
        &mut out,
        doc,
        &mut bodies.entries,
        trailer,
        entry_eol,
        options,
    )?;

    let report = SaveReport {
        bytes_written: out.len(),
        bytes_appended: out.len().saturating_sub(base.len()),
        objects_written: bodies.verbatim + bodies.reserialized,
        objects_verbatim: bodies.verbatim,
        objects_reserialized: bodies.reserialized,
        byte_identical: false,
        delinearized: doc.linearization().save_invalidates_fast_web_view(),
        promoted: bodies.promoted,
        objects_deleted: deleted,
    };
    Ok((out, report))
}

/// The encoder appended objects go through: identity for a plain document;
/// for an encrypted one, its own handler and key (§7.6.2), exempting what the
/// read side does not decrypt.
///
/// RC4 is refused by name: pdfcer never writes RC4 (standing rule W14).
fn encoder_for<'d>(
    doc: &'d Document,
    dirty: &DirtySet,
) -> Result<Box<dyn ObjectEncoder + 'd>, WriteError> {
    let Some(enc) = doc.encryption() else {
        return Ok(Box::new(IdentityEncoder));
    };
    let key = enc.file_key();
    if [key.string_cipher(), key.stream_cipher()].contains(&Cipher::Rc4) {
        return Err(WriteError::Rc4AppendRefused);
    }
    // A fresh IV per payload is a `shall` (§7.6.2); prove entropy works
    // before writing a byte.
    crate::crypto::rng::array::<16>().map_err(|_| WriteError::EntropyUnavailable)?;
    let clear = dirty
        .iter()
        .filter(|id| {
            let value = dirty
                .replacement(*id)
                .or_else(|| doc.get(*id).map(|io| &io.value));
            value.is_some_and(|v| {
                skip_value(
                    id.num,
                    v,
                    enc.encrypt_dict_id(),
                    enc.config.encrypt_metadata,
                )
                .is_some()
            })
        })
        .map(|id| id.num)
        .collect();
    Ok(Box::new(KeyEncoder::new(key, clear)))
}

/// What the object-definition pass wrote.
struct Bodies {
    entries: BTreeMap<u32, XrefEntry>,
    verbatim: usize,
    reserialized: usize,
    promoted: Vec<ObjId>,
}

/// Write every non-deleted dirty object in ascending order. `sources` is
/// `(verbatim, values)`: the stored bytes an untouched definition is copied
/// from, and the bytes a value's spans index.
fn write_bodies(
    out: &mut Vec<u8>,
    doc: &Document,
    dirty: &DirtySet,
    sources: (&[u8], &[u8]),
    encoder: &dyn ObjectEncoder,
) -> Result<Bodies, WriteError> {
    let (verbatim_src, values) = sources;
    let mut b = Bodies {
        entries: BTreeMap::new(),
        verbatim: 0,
        reserialized: 0,
        promoted: Vec::new(),
    };
    for id in dirty.iter() {
        // A deletion's whole expression is a type-0 entry (`apply_free_list`).
        if dirty.is_deleted(id) {
            continue;
        }
        let offset = out.len() as u64;
        if let Some(value) = dirty.replacement(id) {
            // §5 promises byte identity only for what was NOT touched.
            if doc
                .get(id)
                .is_some_and(|io| io.provenance.container().is_some())
            {
                b.promoted.push(id);
            }
            serialize::write_indirect(out, id, value, values, encoder);
            b.reserialized += 1;
        } else {
            // A re-emission of an unknown id has no value to write.
            let io = doc.get(id).ok_or(WriteError::UnknownDirtyObject { id })?;
            match emit_object(out, io, (verbatim_src, values), encoder)? {
                Emission::Verbatim => b.verbatim += 1,
                // Re-serialized because its recovered extent contradicts its
                // bytes: NOT a promotion.
                Emission::RecoveredReserialized => b.reserialized += 1,
                Emission::Promoted => {
                    b.reserialized += 1;
                    b.promoted.push(id);
                }
            }
        }
        b.entries.insert(
            id.num,
            XrefEntry::InUse {
                offset,
                generation: id.generation,
            },
        );
    }
    Ok(b)
}

/// The object-0 free-list head (carried forward from the base, Annex H.7),
/// the deletions (Pass 3.2) and the hybrid re-listing. Returns the deletion
/// count.
fn close_entries(
    entries: &mut BTreeMap<u32, XrefEntry>,
    doc: &Document,
    dirty: &DirtySet,
    base: &[u8],
) -> usize {
    entries.entry(0).or_insert_with(|| {
        doc.xref().get(0).unwrap_or(XrefEntry::Free {
            next_free: 0,
            generation: 65_535,
        })
    });
    let deleted = apply_free_list(entries, doc, dirty);
    relist_shadowed_by_xref_stm(entries, doc, base);
    deleted
}

/// The appended section's trailer: §7.5.6 requirement 3 — every previous
/// entry except `/Prev`, then a new `/Prev`.
///
/// A hybrid file's `/XRefStm` is carried forward here (§7.5.8.4 form A). An
/// encrypted file's `/Encrypt` and `/ID[0]` are carried unchanged (§7.6.3).
fn section_trailer(
    doc: &Document,
    dirty: &DirtySet,
    entries: &BTreeMap<u32, XrefEntry>,
    base_len: usize,
    appended: &[u8],
) -> Dict {
    let highest = entries.keys().copied().max().unwrap_or(0);
    let mut trailer = copy_trailer_without_prev(doc.trailer());
    // Patches go on FIRST so the writer's own `/Prev` and `/Size` can never be
    // displaced by one (§7.5.5).
    for (key, value) in dirty.trailer_patch().iter() {
        trailer.insert(key.clone(), value.clone());
    }
    trailer.insert(
        Name::from(b"Prev"),
        Object::Integer(i64::try_from(doc.base_startxref()).unwrap_or(0)),
    );
    bump_size(&mut trailer, highest);
    // §14.4 / R39: `ID[1]` refreshes exactly when the save changes an object;
    // an identity re-emission is not a change.
    if dirty.changes_content() {
        refresh_changing_identifier(&mut trailer, base_len, appended);
    }
    trailer
}

/// The cross-reference section, in the base file's own form (R33).
fn write_section(
    out: &mut Vec<u8>,
    doc: &Document,
    entries: &mut BTreeMap<u32, XrefEntry>,
    mut trailer: Dict,
    entry_eol: super::XrefEntryEol,
    options: &SaveOptions,
) -> Result<(), WriteError> {
    let section_offset = out.len() as u64;
    match doc.section_shape() {
        SectionShape::Classic { .. } => {
            xref_out::write_classic_table(out, entries, entry_eol)?;
            xref_out::write_classic_tail(out, &trailer, section_offset, options.trailing_eol);
        }
        SectionShape::Stream { id, widths } => {
            // §7.5.8.3: the xref stream's own entry is type 1, pointing at
            // itself. It is never encrypted (§7.5.8.2).
            entries.insert(
                id.num,
                XrefEntry::InUse {
                    offset: section_offset,
                    generation: id.generation,
                },
            );
            bump_size(&mut trailer, entries.keys().copied().max().unwrap_or(0));
            let widths = xref_out::Widths::fit(entries, widths);
            let stream = xref_out::build_xref_stream(id, entries, widths, &trailer)?;
            out.extend_from_slice(&stream.bytes);
            xref_out::write_stream_tail(out, section_offset, options.trailing_eol);
        } // NO WILDCARD ARM: R33 forbids substituting one cross-reference form
          // for another, so a new `SectionShape` must break this match.
    }
    Ok(())
}
