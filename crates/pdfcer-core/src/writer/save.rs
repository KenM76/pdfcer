//! # The two save paths (ISO 32000-1 §7.5.4–§7.5.8)
//!
//! [`save_incremental`] appends a revision (§7.5.6); [`save_full`]
//! rewrites the file with one cross-reference section. Both are pure
//! byte producers — they build a `Vec<u8>` and never touch the
//! filesystem, which is what lets the round-trip harness and the fuzz
//! target compare outputs without I/O.
//!
//! Read [`super`]'s module docs first: they carry the two contracts,
//! the never-normalize rule (R33), the `/ID` reasoning (R39), the
//! redaction-forbids-incremental rule (R35) and the fingerprint rule
//! (R41). This file is the enactment.
//!
//! ## `save_incremental`, step by step (§7.5.6)
//!
//! 1. **Copy the base file. Never open it for writing.** Nothing below
//!    the original EOF may change — that is §7.5.6's *"changes shall be
//!    appended to the end of the file, leaving its original contents
//!    intact"*, and it is simultaneously what makes the signature claim
//!    (§12.8.1 NOTE 1) and the round-trip claim true.
//! 2. **Empty dirty set ⇒ stop.** Zero edits means zero bytes, not
//!    "the input plus an empty revision".
//! 3. **If the base's last byte is not an EOL, write one.** §7.2.3: a
//!    comment runs *"up to but not including the end of the line"* — so
//!    an appended `12 0 obj` token fused onto an unterminated `%%EOF`
//!    line is swallowed by that comment and the file silently loses its
//!    entire update. The spec states no rule here (a recorded NEGATIVE
//!    RESULT); §7.5.1's general line discipline supplies it in
//!    practice, and this handles the files where it did not.
//! 4. **Append each dirty object's definition**, recording its offset.
//! 5. **Append one cross-reference section** in the base file's own
//!    form, carrying entries *only* for the dirty set — plus object 0
//!    (see below).
//! 6. **Append the trailer**: every entry of the previous trailer
//!    *except* `Prev`, then a new `Prev` = the offset the base file's
//!    own `startxref` named. Copying the old `Prev` **and** adding a
//!    new one is a duplicate key, which §7.3.7 prohibits — the trap
//!    §7.5.6 requirement 3 sets for naive implementations.
//! 7. **`startxref` + `%%EOF`.**
//!
//! ### Why object 0 is always in the update section
//!
//! §7.5.6 requirement 1 says an update section *"shall contain entries
//! **only for** objects that have been changed, replaced, or deleted"*
//! — a restriction, not merely permission to omit. Read literally, an
//! unchanged object 0 does not belong there.
//!
//! **The standard's own worked example violates that reading.** Annex
//! H.7 stage 2 emits a `0 1` subsection carrying object 0's free-list
//! head even though it is byte-identical to stage 1's. pdfcer follows
//! the example, not the literal text: the entry costs 20 bytes, keeps
//! the free-list head unambiguous for readers that merge sections
//! rather than probe them, and cannot be wrong for any reader. The
//! spec-vs-own-example tension is recorded in
//! `iso32000__annex__h7.md`; this is the resolution.
//!
//! ## Applying real edits (Pass 3.1)
//!
//! A [`DirtySet`] entry may now carry a **replacement value**. Where the
//! Pass 3.0 loop had one decision (verbatim bytes, or re-serialize a
//! compressed object), it now has three:
//!
//! | dirty entry | base object | what is written | counted as |
//! |---|---|---|---|
//! | re-emission | `Provenance::File` | its source bytes, verbatim | `objects_verbatim` |
//! | re-emission | `Provenance::RecoveredFile` | re-serialized from values | `objects_reserialized` (no promotion) |
//! | re-emission | `Provenance::ObjectStream` | re-serialized from values | `objects_reserialized` + **promotion** |
//! | replacement | `Provenance::File` | the new value, serialized | `objects_reserialized` |
//! | replacement | `Provenance::ObjectStream` | the new value, serialized | `objects_reserialized` + **promotion** |
//! | replacement | *absent from the base* | the new value, serialized | `objects_reserialized` (a **created** object) |
//!
//! ### Promotion, and the stale copy it leaves (R38, decision 007 W3)
//!
//! A compressed object has no contiguous file bytes, so an edit to one
//! cannot be a patch. The two available moves are *rewrite the whole
//! container* — which perturbs every **other** object inside it, a
//! minimal-diff violation by proxy — or *promote the edited object out*,
//! writing it as an ordinary file-level object whose new type-1
//! cross-reference entry supersedes the type-2 one. R38 chooses
//! promotion, and the supersession is sound in both section forms:
//! §7.5.6 makes the most recent copy win for a classic or stream update
//! section, and §7.5.8.4's search order consults the newest classic
//! section *before* a forwarded `/XRefStm`, so a hybrid file behaves the
//! same way.
//!
//! ⚠️ **Promotion leaves the object's previous value inside its old
//! container, and a full rewrite does not change that** — the container
//! is itself a `Provenance::File` stream copied through verbatim. For an
//! ordinary edit this is exactly the same "superseded content survives"
//! property that §7.5.6 gives incremental save by construction, and it is
//! harmless. **It is not harmless for redaction.** `ARCHITECTURE.md` §5.2
//! (from decision 007 W3) records that R35's full-rewrite requirement
//! "closes the stale-copy path"; with object streams carried through
//! intact, it does not — the Redaction Pass must additionally rewrite
//! (or decompose) any container holding a redacted object, and must have
//! a test that greps the saved bytes. Recorded here, at the code that
//! creates the condition, so that Pass cannot inherit the gap silently.
//!
//! Every promotion is pushed onto [`SaveReport::promoted`] — counted
//! *and named*, because it is a byte-level change to an object the
//! operator did not edit the *representation* of.
//!
//! ### Created objects and the trailer patch
//!
//! A replacement whose id the base document does not define is a created
//! object: it is appended, gets a fresh type-1 entry, and raises `/Size`
//! through the same [`bump_size`] path as any other entry. Its only Pass
//! 3.1 use is an operator metadata edit on a file with no `/Info`
//! dictionary, which also needs the trailer to gain `/Info N 0 R` — hence
//! [`DirtySet::patch_trailer`], applied over the §7.5.6-copied base
//! trailer *before* the writer's own `/Prev` and `/Size` are set, so a
//! patch can never displace those.
//!
//! ## `save_full`, and the one thing it must not do
//!
//! A full rewrite re-emits every `Provenance::File` object **from its
//! retained source bytes**, byte for byte. It does *not* re-serialize
//! them from values — §5's contract is byte identity for untouched
//! content, and PDF syntax is non-canonical enough that re-serializing
//! would change bytes on every second object (`crate::span`'s module
//! docs work the three cases).
//!
//! The **one** file-level exception is `Provenance::RecoveredFile`, and it
//! is not a weakening of the rule but a precondition of it: those bytes
//! contradict the value pdfcer parsed from them (a stream whose extent
//! recovery re-derived from `endstream`, leaving the old `/Length` in the
//! source text). Copying them verbatim would emit a stream whose declared
//! length does not match its data — a file pdfcer would refuse to reload,
//! i.e. "save then reopen" would lose the operator's document. §5 promises
//! byte identity for content pdfcer did not touch; pdfcer *did* touch this
//! object's length, deliberately and disclosed, which is exactly the
//! condition under which §5 does not bind.
//!
//! ### Object streams survive a full rewrite intact
//!
//! A compressed object (`Provenance::ObjectStream`) has no file-level
//! bytes of its own, so a naive full rewrite would have to promote it
//! out of its container — perturbing every *other* object in that
//! container, a minimal-diff violation by proxy (decision 007 W3).
//!
//! pdfcer does not do that, and the reason is worth stating because it
//! is not obvious: a type-2 cross-reference entry's fields are the
//! **container's object number and the index within it** — neither is a
//! byte offset. So re-emitting the container verbatim (it is itself an
//! ordinary `Provenance::File` stream object) leaves every type-2 entry
//! *still correct*. Object streams are carried through untouched, with
//! zero promotions, and the only thing that changes is the container's
//! own offset in its type-1 entry.
//!
//! ### Hybrid-reference files are refused, by name
//!
//! See [`WriteError::HybridFullRewrite`]. Incremental save of a hybrid
//! file works and is the supported path.

use std::collections::{BTreeMap, BTreeSet};

use crate::document::Document;
use crate::object::{Dict, IndirectObject, Name, ObjId, Object, Provenance};
use crate::xref::{SectionShape, XrefEntry};

use super::encoder::IdentityEncoder;
use super::{DirtySet, ProducerPolicy, SaveOptions, WriteError, serialize, xref_out};

/// Largest object number [`save_full`] will build a cross-reference
/// table up to before refusing (ISO 32000-1 Annex C, Table C.1).
///
/// # Why a bound is structurally necessary here
///
/// §7.5.4's completeness requirement makes a single-section full rewrite
/// emit one entry per object number from 0 to the highest one **defined
/// in the file** — "even if one or more of the object numbers in this
/// range do not actually occur". The cost is therefore driven by the
/// largest object NUMBER, not by the object COUNT, and the largest
/// number is chosen by whoever wrote the input.
///
/// # Where the value comes from — sourced, not guessed
///
/// Annex C Table C.1 gives **8,388,607 (2²³ − 1) maximum indirect
/// objects**, a legacy Acrobat architectural limit. A file whose highest
/// object number exceeds that cannot have a conforming object *count*
/// either, so the bound refuses nothing a conforming producer can make.
///
/// The same table caps an integer at 2,147,483,647 (2³¹ − 1), which is
/// worth knowing for the file that prompted this: pdfium's
/// `bug_455199.pdf` names `2147483648 0 obj` — one MORE than the largest
/// integer the spec permits, so the object number is not merely
/// implausible, it is unrepresentable as a conforming PDF integer.
///
/// Deliberately **not** clamped to the object count: a sparse-but-small
/// file with one high number is exactly the adversarial shape this
/// guards, and a count-based bound would let it through.
///
/// Sized in the spirit of [`crate::forms::MAX_FORM_FIELDS`] and
/// [`crate::annot::MAX_ANNOTS_PER_PAGE`] — far above any conformant
/// corpus, so the veraPDF §6.1.12 implementation-limits suite keeps
/// comfortable headroom.
pub const MAX_REWRITE_OBJECT_NUMBER: u32 = 8_388_607;

/// What a save actually did — the honest report the CLI, the GUI and
/// the corpus harness all print.
///
/// Counters exist for the things that would otherwise be invisible: a
/// save that quietly promoted objects out of an object stream, or that
/// spent a document's Fast Web View property, has changed something the
/// operator cares about and must say so ("fuzzy, never sneaky").
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SaveReport {
    /// Total bytes in the produced file.
    pub bytes_written: usize,
    /// Bytes appended past the base file's original length. Exactly
    /// `0` for an empty-dirty-set incremental save, and equal to
    /// [`SaveReport::bytes_written`] for a full rewrite.
    pub bytes_appended: usize,
    /// Number of object definitions emitted in this save.
    pub objects_written: usize,
    /// Objects re-emitted **verbatim** from their retained source
    /// bytes — the §5 invariant's numerator.
    pub objects_verbatim: usize,
    /// Objects that had to be re-serialized from values because they
    /// had no file-level bytes (compressed in an object stream) or
    /// because a policy deliberately rewrote them (`/Producer`).
    ///
    /// Every one of these is a byte-level divergence from the input,
    /// and each is counted rather than rounded away (R20-style).
    pub objects_reserialized: usize,
    /// Whether the output is byte-identical to the input. Only ever
    /// true for an empty-dirty-set [`save_incremental`].
    pub byte_identical: bool,
    /// Whether this save spent the input's live Fast Web View property
    /// (Annex F.1; R36). Reported, never repaired.
    pub delinearized: bool,
    /// Objects that were **promoted out of an object stream** (R38) —
    /// counted *and named*, per decision 007 W3's mitigation.
    ///
    /// A compressed object cannot be written in place, so touching one
    /// moves it to file level. That is a representation change to an
    /// object whose *value* the operator may not have edited at all
    /// (an identity re-emission promotes too), and it leaves the old
    /// value behind inside the untouched container — see this module's
    /// "Promotion, and the stale copy it leaves". Both facts are things
    /// an operator can act on, so neither is rounded away.
    ///
    /// `promoted.len()` is the count; there is deliberately no separate
    /// counter field to fall out of step with the list.
    pub promoted: Vec<ObjId>,
    /// Objects given a type-0 (free) cross-reference entry by this save
    /// — the Pass 3.2 deletion path (§7.5.4, decision 007 W9).
    ///
    /// Counted rather than named, unlike [`SaveReport::promoted`],
    /// because a deletion is something the **operator asked for** and
    /// already knows about, whereas a promotion is a representation
    /// change pdfcer made on its own initiative. The honesty obligation
    /// differs accordingly.
    pub objects_deleted: usize,
}

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
/// use pdfcer_core::document::Document;
/// use pdfcer_core::writer::{DirtySet, SaveOptions, save_incremental};
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
    // Was `_options` until R169 gave `SaveOptions` two knobs an
    // incremental save DOES honour: the §7.5.4 entry terminator and the
    // §7.5.5 trailing EOL both describe bytes this path writes into the
    // appended revision. `producer` is still ignored here — that one is
    // about `/Info`, which an append must never touch.
    options: &SaveOptions,
) -> Result<(Vec<u8>, SaveReport), WriteError> {
    // Decision 013 (the recovered-base rule): a document loaded via
    // cross-reference recovery had an INVALID base xref, so an incremental
    // append onto it would write a section whose `/Prev` points at a
    // cross-reference section that does not correctly exist. Refuse by name
    // — even an empty dirty set — and force the caller onto `save_full`,
    // which emits a fresh valid classic cross-reference. Sibling of R35 /
    // R58. Checked FIRST so no later path can append to a broken base.
    if doc.loaded_via_recovery() {
        return Err(WriteError::RecoveredBaseForbidsIncremental);
    }

    // 7.6: a decrypted document's buffer and parsed objects deliberately
    // disagree (streams plaintext in both, strings plaintext only in the
    // objects), so re-emitting either verbatim produces a file that claims
    // encryption it does not have. See `WriteError::EncryptedSaveUnsupported`
    // for why this is a refusal rather than a best effort.
    if doc.encryption().is_some() {
        return Err(WriteError::EncryptedSaveUnsupported);
    }

    let base = doc.bytes();
    // Resolve `EOL-A1` against the FILE BEING SAVED, once, here.
    //
    // This is the only layer that has both the operator's setting and the
    // base file's bytes, which is exactly why the default can now be
    // "match the source" at all. Resolving once rather than at each
    // `write_classic_table` call keeps an incremental save and a full
    // rewrite of the same document from ever disagreeing about its form.
    let entry_eol = options.xref_entry_eol.resolve(base);

    let mut out = base.to_vec();
    // R45: replacement values may carry authored appearance streams whose
    // spans point past the base file into the session's staging buffer.
    // `combined` is `base` alone when nothing was authored (zero-copy, the
    // unchanged pre-6.1 path) and `base ++ staging` otherwise, so a base
    // span resolves in the prefix and an authored span in the suffix. The
    // verbatim path below still reads `base` (its spans are always
    // file-level, i.e. in the prefix).
    let combined = dirty.combined_source(base);

    // Step 2. Zero edits means zero bytes. This is the whole contract,
    // and it is deliberately checked before anything else so that no
    // later code path can accidentally append to an unchanged document.
    if dirty.is_empty() {
        return Ok((
            out,
            SaveReport {
                bytes_written: base.len(),
                bytes_appended: 0,
                objects_written: 0,
                objects_verbatim: 0,
                objects_reserialized: 0,
                byte_identical: true,
                // A save that wrote nothing cannot have de-linearized
                // anything.
                delinearized: false,
                promoted: Vec::new(),
                objects_deleted: 0,
            },
        ));
    }

    // Step 3. Separate the appended region from an unterminated final
    // line (module docs — §7.2.3's comment-to-end-of-line rule).
    if !matches!(out.last(), Some(b'\n' | b'\r')) {
        out.push(b'\n');
    }

    // Step 4. Object definitions, in ascending order.
    //
    // `body_start` is remembered because §14.4's changing identifier is
    // digested over exactly this region — the appended object
    // definitions, and nothing after them. Digesting the finished file
    // would be circular: `/ID` lives in the trailer, which is part of
    // the file (see `super::fileid`).
    let body_start = out.len();
    let mut entries: BTreeMap<u32, XrefEntry> = BTreeMap::new();
    let mut verbatim = 0usize;
    let mut reserialized = 0usize;
    let mut promoted: Vec<ObjId> = Vec::new();
    for id in dirty.iter() {
        // A deletion writes no body at all — its whole expression is a
        // type-0 entry, and those are built together after the loop so
        // the linked list can be chained in one place (see
        // `apply_free_list`).
        if dirty.is_deleted(id) {
            continue;
        }
        let offset = out.len() as u64;
        match dirty.replacement(id) {
            // A real edit: serialize the new value. There are no
            // verbatim bytes to preserve for an object whose value
            // changed — §5 promises byte identity only for what was NOT
            // touched.
            Some(value) => {
                if doc
                    .get(id)
                    .is_some_and(|io| io.provenance.container().is_some())
                {
                    promoted.push(id);
                }
                serialize::write_indirect(&mut out, id, value, &combined, &IdentityEncoder);
                reserialized += 1;
            }
            // An identity re-emission. `UnknownDirtyObject` stays a
            // named refusal here (and only here): a *replacement* for an
            // unknown id is a legitimate created object, but a request
            // to re-emit an object that does not exist has no value to
            // write and cannot be guessed at.
            None => {
                let io = doc.get(id).ok_or(WriteError::UnknownDirtyObject { id })?;
                match emit_object(&mut out, io, base)? {
                    Emission::Verbatim => verbatim += 1,
                    // Re-serialized because its recovered extent
                    // contradicts its source bytes. NOT a promotion —
                    // the object was and stays file-level.
                    Emission::RecoveredReserialized => reserialized += 1,
                    Emission::Promoted => {
                        reserialized += 1;
                        promoted.push(id);
                    }
                }
            }
        }
        entries.insert(
            id.num,
            XrefEntry::InUse {
                offset,
                generation: id.generation,
            },
        );
    }
    let body_end = out.len();

    // The object-0 free-list head, per Annex H.7's own convention
    // (module docs). Re-use whatever the base file recorded so the
    // free list is carried forward unchanged; fall back to the §7.5.4
    // canonical head when the base had no entry for 0 at all.
    entries.entry(0).or_insert_with(|| {
        doc.xref().get(0).unwrap_or(XrefEntry::Free {
            next_free: 0,
            generation: 65_535,
        })
    });
    // Deletions (Pass 3.2): type-0 entries, generation incremented, and
    // spliced onto the head of the base file's free list.
    let deleted = apply_free_list(&mut entries, doc, dirty);

    // Step 6 (prepared before step 5, because an xref stream carries
    // the trailer keys inside its own dictionary).
    //
    // §7.5.6 requirement 3: "all the entries except the Prev entry
    // (if present) from the previous trailer, whether modified or
    // not" — and then a NEW Prev. Copying the old one as well would be
    // a duplicate key (§7.3.7 prohibits those).
    //
    // NOTE: a hybrid file's `/XRefStm` IS such an entry, and is
    // therefore carried forward automatically here. That is §7.5.8.4
    // "form A", the only appended shape that satisfies requirement 3
    // as written — see `iso32000__s__7.5.8.md`'s hybrid write-direction
    // analysis.
    let highest = entries.keys().copied().max().unwrap_or(0);
    let mut trailer = copy_trailer_without_prev(doc.trailer());
    // The operator's trailer changes go on FIRST, so the writer's own
    // `/Prev` and `/Size` below can never be displaced by a patch — a
    // patched `/Prev` would silently drop a whole revision, and a
    // patched `/Size` would make objects vanish from every reader's view
    // (§7.5.5). Those two keys belong to the writer, not to the edit.
    for (key, value) in dirty.trailer_patch().iter() {
        trailer.insert(key.clone(), value.clone());
    }
    trailer.insert(
        Name::from(b"Prev"),
        Object::Integer(i64::try_from(doc.base_startxref()).unwrap_or(0)),
    );
    bump_size(&mut trailer, highest);
    // §14.4 / R39: `ID[1]` refreshes exactly when the save writes a
    // changed object. An identity re-emission is not a change, so this
    // is precisely the line that keeps the Pass 3.0 `append-identity`
    // corpus mode byte-stable while a real edit updates the identifier.
    if dirty.changes_content() {
        refresh_changing_identifier(
            &mut trailer,
            base.len(),
            out.get(body_start..body_end).unwrap_or(&[]),
        );
    }

    // Step 5 + 7. The section, in the base file's own form (R33).
    let section_offset = out.len() as u64;
    match doc.section_shape() {
        SectionShape::Classic { .. } => {
            xref_out::write_classic_table(&mut out, &entries, entry_eol)?;
            xref_out::write_classic_tail(&mut out, &trailer, section_offset, options.trailing_eol);
        }
        SectionShape::Stream { id, widths } => {
            // §7.5.8.3: the xref stream is a top-level indirect object
            // and its own entry is type 1, pointing at itself.
            entries.insert(
                id.num,
                XrefEntry::InUse {
                    offset: section_offset,
                    generation: id.generation,
                },
            );
            bump_size(&mut trailer, entries.keys().copied().max().unwrap_or(0));
            let widths = xref_out::Widths::fit(&entries, widths);
            let stream = xref_out::build_xref_stream(id, &entries, widths, &trailer)?;
            out.extend_from_slice(&stream.bytes);
            xref_out::write_stream_tail(&mut out, section_offset, options.trailing_eol);
        } // NO WILDCARD ARM. A third cross-reference form would have to
          // be emitted, not guessed at — R33 forbids substituting one
          // form for another — so a new `SectionShape` variant must break
          // this match.
    }

    let report = SaveReport {
        bytes_written: out.len(),
        bytes_appended: out.len().saturating_sub(base.len()),
        objects_written: verbatim + reserialized,
        objects_verbatim: verbatim,
        objects_reserialized: reserialized,
        byte_identical: false,
        delinearized: doc.linearization().save_invalidates_fast_web_view(),
        promoted,
        objects_deleted: deleted,
    };
    Ok((out, report))
}

/// Rewrite `doc` as a single-revision file, applying `dirty`, and return
/// the bytes.
///
/// Every `Provenance::File` object the dirty set does **not** name is
/// re-emitted from its retained source bytes verbatim; only the header
/// prefix, object offsets, the cross-reference section and the trailer
/// are newly generated. Byte identity is therefore asserted **per object
/// definition**, never per file (R32 — see [`super`]'s contract table).
///
/// Pass `&DirtySet::empty()` for a pure identity rewrite; that is what
/// the corpus round-trip gate and the fuzz target do, and it makes the
/// no-edit path a strict subset of the edit path rather than a separate
/// one that could drift.
///
/// ⚠️ **A full rewrite destroys every existing digital signature**
/// (§12.8.1) and, unlike incremental save, does **not** by itself remove
/// the superseded value of a promoted compressed object — see this
/// module's "Promotion, and the stale copy it leaves", which the
/// Redaction Pass must read.
///
/// # Errors
///
/// [`WriteError`] — notably [`WriteError::HybridFullRewrite`] for a
/// §7.5.8.4 hybrid-reference input, which is refused by name rather
/// than normalized away.
pub fn save_full(
    doc: &Document,
    dirty: &DirtySet,
    options: &SaveOptions,
) -> Result<(Vec<u8>, SaveReport), WriteError> {
    let base = doc.bytes();
    // Resolve `EOL-A1` against the FILE BEING SAVED, once, here.
    //
    // This is the only layer that has both the operator's setting and the
    // base file's bytes, which is exactly why the default can now be
    // "match the source" at all. Resolving once rather than at each
    // `write_classic_table` call keeps an incremental save and a full
    // rewrite of the same document from ever disagreeing about its form.
    let entry_eol = options.xref_entry_eol.resolve(base);

    // R45: authored appearance streams live in the staging buffer past the
    // base file; replacement/created objects serialize against `base ++
    // staging`. `base` alone (zero-copy) when nothing was authored.
    let combined = dirty.combined_source(base);

    // A hybrid file is a three-part unit §7.5.8.4 says a writer creates
    // "at the same time"; rebuilding it from a merged view is Pass 3.2
    // work, and flattening it would destroy the pre-1.5 view (R33).
    if matches!(
        doc.section_shape(),
        SectionShape::Classic { xref_stm: Some(_) }
    ) {
        return Err(WriteError::HybridFullRewrite);
    }

    // 7.6: same refusal as the incremental path, and for the same reason --
    // a full rewrite re-emits untouched objects from their source span too.
    if doc.encryption().is_some() {
        return Err(WriteError::EncryptedSaveUnsupported);
    }

    // The header region.
    //
    // Normal path: copied verbatim FROM THE `%PDF-` MARKER, so the
    // `%PDF-M.N` line is preserved exactly (never re-derived — re-deriving
    // would normalize a `%PDF-1.4 ` with trailing space) along with the
    // §7.5.2 binary-comment line. Any bytes BEFORE the marker are dropped;
    // see `header_span` for the measurement that forced that, and for why
    // only a full rewrite is allowed to do it.
    //
    // Recovered path (decision 013): a recovered file's base cross-reference
    // was invalid, so §5.6's "never normalize" does not bind it — and its
    // header may be absent or buried behind >1 KiB of leading junk (the
    // offset-start case, where `header_span` finds no marker in its
    // window and would copy nothing, yielding a headerless rewrite that
    // re-triggers recovery on reload). Emit a fresh, clean `%PDF-<version>`
    // header + binary marker so the rewrite loads via the STRICT path.
    let mut out = if doc.loaded_via_recovery() {
        let mut h = format!("%PDF-{}\n", doc.version()).into_bytes();
        h.extend_from_slice(b"%\xE2\xE3\xCF\xD3\n"); // §7.5.2 binary marker
        h
    } else {
        let mut h = base
            .get(header_span(base))
            .unwrap_or(b"%PDF-1.7\n")
            .to_vec();
        if !matches!(h.last(), Some(b'\n' | b'\r')) {
            h.push(b'\n');
        }
        h
    };

    // The object number the new cross-reference stream will occupy, if
    // the base file uses that form. Its old definition is NOT re-emitted
    // as a body object — it *is* the section.
    let xref_stream_id = match doc.section_shape() {
        SectionShape::Stream { id, .. } => Some(id),
        _ => None,
    };
    let base_widths = match doc.section_shape() {
        SectionShape::Stream { widths, .. } => widths,
        _ => [1, 4, 2],
    };

    // The `/Info` object, when a policy is going to rewrite it.
    let info_id = match options.producer {
        ProducerPolicy::Set => doc.trailer().get(b"Info").and_then(Object::as_reference),
        ProducerPolicy::Preserve => None,
    };

    let body_start = out.len();
    let mut entries: BTreeMap<u32, XrefEntry> = BTreeMap::new();
    let mut verbatim = 0usize;
    let mut reserialized = 0usize;
    let mut promoted: Vec<ObjId> = Vec::new();

    // Ascending object-number order. Not required by §7.5.4 (which lets
    // subsections appear in any order) but it is what makes the emitted
    // table a single contiguous subsection, and it keeps the output
    // deterministic across runs — a precondition for byte-comparison
    // testing at all.
    //
    // The dirty set's own ids are folded in so a **created** object (one
    // the base document does not define) is emitted too. `BTreeSet`
    // rather than sort+dedup because the two sources overlap for every
    // ordinary edit.
    let mut numbers: BTreeSet<u32> = doc.xref().iter().map(|(num, _)| num).collect();
    numbers.extend(dirty.iter().map(|id| id.num));

    for num in numbers {
        // A replacement for this object number, if the dirty set has
        // one. Looked up by number rather than by full id because the
        // base file's generation is authoritative here — an edit never
        // changes a generation in this Pass (deletion, which is what
        // increments one, is Pass 3.2).
        let entry = doc.xref().get(num);
        let generation = match entry {
            Some(XrefEntry::InUse { generation, .. }) => generation,
            // A compressed object's generation is always 0 (§7.5.7), and
            // a created object is allocated at generation 0.
            _ => 0,
        };
        let id = ObjId::new(num, generation);

        // A deleted object contributes no body. Its type-0 entry is
        // written by `apply_free_list` below, together with the rest of
        // the chain — `continue` here rather than inserting a
        // placeholder, because a placeholder is exactly the kind of
        // half-written free entry decision 007 W9 warns about.
        if dirty.is_deleted(id) {
            continue;
        }
        let replacement = dirty.replacement(id);

        match entry {
            // A free entry carries no bytes; it is copied through
            // exactly as the base recorded it, which preserves the
            // base file's free list without pdfcer inventing one. A
            // replacement for a free number would be a resurrection,
            // which needs the generation rules deletion brings — Pass
            // 3.2 — so it is left alone here.
            Some(free @ XrefEntry::Free { .. }) => {
                entries.insert(num, free);
            }
            // A compressed object needs no body emission at all *unless
            // it was edited*: its type-2 entry names a container and an
            // index, neither of which is a byte offset, and the
            // container is re-emitted verbatim below (module docs). An
            // edited one must be promoted out (R38), because the
            // container it lives in is about to be copied through with
            // its OLD contents.
            Some(XrefEntry::InStream { .. }) => match replacement {
                None => {
                    if xref_stream_id.is_none() {
                        // The output section is a CLASSIC table, which
                        // cannot express a type-2 entry. This is only
                        // reachable for a **recovered** document (decision
                        // 013): a normal xref-stream file has a `Stream`
                        // section shape (`xref_stream_id.is_some()`) and
                        // carries type-2 entries through untouched; a
                        // recovered document is fixed to
                        // `Classic { xref_stm: None }`, so its compressed
                        // objects must be PROMOTED out to file level (R38).
                        // The old container is re-emitted verbatim too, so
                        // this is a superseding file-level definition, not a
                        // move — but a classic table can only name the
                        // file-level copy.
                        let io = doc.get(id).ok_or(WriteError::MissingObject { num })?;
                        let offset = out.len() as u64;
                        serialize::write_indirect(
                            &mut out,
                            id,
                            &io.value,
                            &combined,
                            &IdentityEncoder,
                        );
                        reserialized += 1;
                        promoted.push(id);
                        entries.insert(num, XrefEntry::InUse { offset, generation });
                    } else if let Some(e) = entry {
                        entries.insert(num, e);
                    }
                }
                Some(value) => {
                    let offset = out.len() as u64;
                    serialize::write_indirect(&mut out, id, value, &combined, &IdentityEncoder);
                    reserialized += 1;
                    promoted.push(id);
                    entries.insert(num, XrefEntry::InUse { offset, generation });
                }
            },
            Some(XrefEntry::InUse { .. }) => {
                if xref_stream_id.is_some_and(|sid| sid.num == num) {
                    // Placeholder; the real offset is filled in once
                    // the body is complete.
                    continue;
                }
                let offset = out.len() as u64;
                match (replacement, Some(id) == info_id) {
                    // An operator metadata edit AND a `/Producer`
                    // policy on the same object. Both apply: the policy
                    // is layered over the edited value, not instead of
                    // it. Doing otherwise would make an unrelated
                    // authorship setting silently discard the
                    // operator's own edit.
                    (Some(value), true) => {
                        write_with_producer(&mut out, id, value, &combined);
                        reserialized += 1;
                    }
                    (Some(value), false) => {
                        serialize::write_indirect(&mut out, id, value, &combined, &IdentityEncoder);
                        reserialized += 1;
                    }
                    (None, true) => {
                        let io = doc.get(id).ok_or(WriteError::MissingObject { num })?;
                        write_with_producer(&mut out, id, &io.value, base);
                        reserialized += 1;
                    }
                    (None, false) => {
                        let io = doc.get(id).ok_or(WriteError::MissingObject { num })?;
                        match emit_object(&mut out, io, base)? {
                            Emission::Verbatim => verbatim += 1,
                            // See the matching arm in `save_incremental`.
                            Emission::RecoveredReserialized => reserialized += 1,
                            Emission::Promoted => {
                                reserialized += 1;
                                promoted.push(id);
                            }
                        }
                    }
                }
                entries.insert(num, XrefEntry::InUse { offset, generation });
            }
            // The base document does not define this number at all: it
            // is a created object, and only a replacement can supply a
            // value for it. A bare re-emission request for an unknown
            // id is the same named refusal as on the incremental path.
            None => {
                let Some(value) = replacement else {
                    return Err(WriteError::UnknownDirtyObject { id });
                };
                let offset = out.len() as u64;
                serialize::write_indirect(&mut out, id, value, &combined, &IdentityEncoder);
                reserialized += 1;
                entries.insert(num, XrefEntry::InUse { offset, generation });
            }
        }
    }
    let body_end = out.len();

    // §7.5.4: "The cross-reference table … shall contain one entry for
    // each object number from 0 to the maximum object number defined in
    // the file." A full rewrite is ONE section, so that obligation
    // lands entirely on it: holes must be filled with free entries.
    //
    // The fill value is §7.5.4's second, "detached" free form — links
    // back to object 0, generation 65535 — which the clause explicitly
    // permits ("the table may contain other free entries that link back
    // to object number 0 and have a generation number of 65,535, even
    // though these entries are not in the linked list itself"). Using
    // it avoids re-deriving a free-list chain pdfcer has no business
    // inventing in a Pass with no deletion.
    let highest = entries
        .keys()
        .copied()
        .max()
        .max(xref_stream_id.map(|id| id.num))
        .unwrap_or(0);
    // THE HOLE-FILLING LOOP BELOW IS O(highest), AND `highest` IS CHOSEN
    // BY THE INPUT FILE. Guard it before it runs.
    //
    // The completeness obligation above is real and is why the loop
    // exists — but it makes the writer's cost a function of the largest
    // object NUMBER in the file, not of how many objects the file
    // actually contains. A 1.2 KB document naming one object
    // `2147483648 0 obj` therefore asks pdfcer to emit 2,147,483,649
    // cross-reference entries: measured at ~27 MB/s of steady allocation
    // with the CPU pinned, which is roughly an hour of grinding before
    // the allocator gives up. Not an infinite loop — worse in one
    // respect, because it looks like progress.
    //
    // That is a real corpus file (pdfium's `bug_455199.pdf`), and the
    // consequence in the GUI is an unrecoverable freeze: no error, no
    // cancel, no save.
    //
    // So this refuses by name (R27) rather than grinding. Refusal is the
    // honest outcome: complying literally would emit a ~40 GB
    // cross-reference table for a 1.2 KB input, and quietly emitting a
    // SPARSE table instead would violate §7.5.4's completeness
    // requirement for a single-section full rewrite — trading a hang for
    // a malformed file.
    if highest > MAX_REWRITE_OBJECT_NUMBER {
        return Err(WriteError::ObjectNumberTooLarge {
            num: highest,
            max: MAX_REWRITE_OBJECT_NUMBER,
        });
    }
    entries.entry(0).or_insert(XrefEntry::Free {
        next_free: 0,
        generation: 65_535,
    });
    for num in 0..=highest {
        entries.entry(num).or_insert(XrefEntry::Free {
            next_free: 0,
            generation: 65_535,
        });
    }
    // Deletions get their real type-0 entries last, so the hole-filling
    // loop above cannot leave a deleted object wearing a detached free
    // entry with the wrong generation.
    let deleted = apply_free_list(&mut entries, doc, dirty);

    let mut trailer = copy_trailer_without_prev(doc.trailer());
    // Operator trailer changes first; the writer's own keys below win.
    for (key, value) in dirty.trailer_patch().iter() {
        trailer.insert(key.clone(), value.clone());
    }
    // A single section has no predecessor and no hybrid companion.
    trailer.0.retain(|(k, _)| k.as_bytes() != b"XRefStm");
    bump_size(&mut trailer, highest);
    if dirty.changes_content() {
        refresh_changing_identifier(
            &mut trailer,
            base.len(),
            out.get(body_start..body_end).unwrap_or(&[]),
        );
    }

    let section_offset = out.len() as u64;
    match xref_stream_id {
        None => {
            xref_out::write_classic_table(&mut out, &entries, entry_eol)?;
            xref_out::write_classic_tail(&mut out, &trailer, section_offset, options.trailing_eol);
        }
        Some(id) => {
            entries.insert(
                id.num,
                XrefEntry::InUse {
                    offset: section_offset,
                    generation: id.generation,
                },
            );
            let widths = xref_out::Widths::fit(&entries, base_widths);
            let stream = xref_out::build_xref_stream(id, &entries, widths, &trailer)?;
            out.extend_from_slice(&stream.bytes);
            xref_out::write_stream_tail(&mut out, section_offset, options.trailing_eol);
        }
    }

    let report = SaveReport {
        bytes_written: out.len(),
        bytes_appended: out.len(),
        objects_written: verbatim + reserialized,
        objects_verbatim: verbatim,
        objects_reserialized: reserialized,
        byte_identical: false,
        delinearized: doc.linearization().save_invalidates_fast_web_view(),
        promoted,
        objects_deleted: deleted,
    };
    Ok((out, report))
}

/// The values a full **encrypting** rewrite needs beyond the ordinary
/// [`save_full`] inputs: the file key, the object the `/Encrypt` dictionary
/// occupies, that dictionary's value, and the two-element `/ID`.
///
/// At `/V` 5 `/ID` plays **no** part in key derivation (unlike `/R` 4), so a
/// freshly generated pair is written verbatim; it exists because §7.5.5 makes
/// `/ID` mandatory whenever `/Encrypt` is present.
#[derive(Debug, Clone)]
pub struct EncryptParams {
    /// The 32-byte file encryption key (never serialised; drives the encoder).
    pub file_key: [u8; 32],
    /// The object number and generation the `/Encrypt` dictionary is written
    /// at — its own strings/streams are exempt from encryption (§7.6).
    pub encrypt_dict: ObjId,
    /// The `/Encrypt` dictionary as a value ready to serialise (`/V 5 /R 6
    /// /CFM AESV3`, with the byte-string `/O`/`/U`/`/OE`/`/UE`/`/Perms`).
    pub encrypt_dict_value: Object,
    /// The `/ID` array's two byte strings.
    pub file_id: [Vec<u8>; 2],
    /// `/EncryptMetadata` — when false, `/Metadata` streams are left in clear.
    pub encrypt_metadata: bool,
}

/// Rewrite the whole document as one **encrypted** revision (`Pass 5.4`,
/// ISO 32000-2:2020 §7.6 + Algorithm 1.A).
///
/// # Why this is a separate function, not a flag on [`save_full`]
///
/// Encryption is decision 007 **W8**'s canonical "touches EVERY string and
/// EVERY stream" transformation, so the minimal-diff invariant
/// (`ARCHITECTURE.md` §5) is *deliberately* waived here: no object is emitted
/// verbatim from its source span, because a verbatim copy would leave the
/// object's strings and streams in clear. Every object is re-serialised
/// through the [`EncryptingEncoder`], which is the ONLY difference from a
/// plaintext rewrite — the serializer recomputes `/Length` from the encrypted
/// bytes (§7.3.8.2, IV + ciphertext + PKCS#7 pad), so nothing else needs to
/// change.
///
/// # Structural normalisation this DOES perform, and why it is sanctioned
///
/// The output is always a **classic cross-reference table** with every
/// compressed object promoted to file level and every `/Type /ObjStm`
/// container dropped. This is the same normalisation a recovered document
/// gets (decision 013), sanctioned here for the same reason it is sanctioned
/// there: the operation is a whole-file rewrite by nature, so §5's "never
/// normalise gratuitously" does not bind it — and a classic table sidesteps
/// the special-case exemptions an xref STREAM would need (its own bytes must
/// stay in clear, §7.6.2). An object stream's exemption is moot once it is
/// gone.
///
/// # What is left in clear
///
/// The `/Encrypt` dictionary itself (it is what a reader needs *before* it can
/// decrypt anything), and `/Metadata` streams when `encrypt_metadata` is
/// false (§7.6.2). A signed document is refused UPSTREAM by the caller
/// ([`crate::EditSession`]) — the write path never sees one — so the
/// per-string `/Contents` exemption (N13) is not expressed here.
///
/// # Errors
///
/// - [`WriteError::HybridFullRewrite`] for a hybrid-reference input (R33).
/// - [`WriteError::EncryptedSaveUnsupported`] if the document is ALREADY
///   encrypted — re-encryption decrypts first, which is the caller's job.
/// - [`WriteError::ObjectNumberTooLarge`] (R27) as [`save_full`].
/// - [`WriteError::MissingObject`] / [`WriteError::UnknownDirtyObject`] for a
///   dangling reference, as [`save_full`].
pub fn save_full_encrypted(
    doc: &Document,
    dirty: &DirtySet,
    options: &SaveOptions,
    enc: &EncryptParams,
) -> Result<(Vec<u8>, SaveReport), WriteError> {
    full_reencode(doc, dirty, options, Some(enc))
}

/// Rewrite the whole document as one revision, re-serialising EVERY string and
/// stream through an identity encoder — the plaintext sibling of
/// [`save_full_encrypted`] (`Pass 5.4`, `EditSession::remove_encryption`).
///
/// # Why this is distinct from [`save_full`]
///
/// [`save_full`] copies an untouched object's DEFINITION BYTES verbatim from
/// the source. That is wrong for a document that was **decrypted in place**:
/// decryption shortened each stream's `data_span` (AES strips the 16-byte IV
/// and padding) but left the dictionary's `/Length` and the trailing bytes in
/// the source buffer, so a verbatim copy would emit a stale `/Length` over
/// plaintext-plus-leftover-ciphertext. Re-serialising every object recomputes
/// `/Length` from the shortened span, which is exactly what removing
/// encryption needs — and is why saving a decrypted document through the
/// verbatim path is refused (`WriteError::EncryptedSaveUnsupported`).
///
/// The caller must have already dropped the `/Encrypt` state
/// ([`crate::Document::clear_encryption`]); this function does not write
/// `/Encrypt` or force `/ID`.
///
/// # Errors
///
/// As [`save_full_encrypted`].
pub fn save_full_decrypted(
    doc: &Document,
    dirty: &DirtySet,
    options: &SaveOptions,
) -> Result<(Vec<u8>, SaveReport), WriteError> {
    full_reencode(doc, dirty, options, None)
}

/// The shared body of [`save_full_encrypted`] (`enc = Some`) and
/// [`save_full_decrypted`] (`enc = None`): a full rewrite that re-serialises
/// every object through an [`ObjectEncoder`], never verbatim. See both public
/// wrappers for the contract.
fn full_reencode(
    doc: &Document,
    dirty: &DirtySet,
    options: &SaveOptions,
    enc: Option<&EncryptParams>,
) -> Result<(Vec<u8>, SaveReport), WriteError> {
    use super::encoder::{EncryptingEncoder, IdentityEncoder, ObjectEncoder};

    let base = doc.bytes();
    let entry_eol = options.xref_entry_eol.resolve(base);
    let combined = dirty.combined_source(base);

    if matches!(
        doc.section_shape(),
        SectionShape::Classic { xref_stm: Some(_) }
    ) {
        return Err(WriteError::HybridFullRewrite);
    }
    if doc.encryption().is_some() {
        // Encrypting requires plaintext; re-encryption decrypts first (the
        // caller clears `/Encrypt` after decrypt-in-place). A document still
        // reporting encryption here has not been through that, and its source
        // bytes are ciphertext.
        return Err(WriteError::EncryptedSaveUnsupported);
    }

    // The set of stream objects whose DATA is exempt from encryption
    // (§7.6.2). Empty when not encrypting. With a classic output table there
    // is no xref stream to exempt; the remaining categories are clear
    // `/Metadata` and external (`/F`) streams. Resolved into object numbers so
    // the encoder — which sees only bytes and an owner id — can consult it.
    let mut clear_streams: std::collections::HashSet<u32> = std::collections::HashSet::new();
    if let Some(enc) = enc {
        for (num, _) in doc.xref().iter() {
            let id = ObjId::new(num, 0);
            if let Some(io) = doc.get(id)
                && let Object::Stream(st) = &io.value
            {
                let ty = st.dict.get(b"Type").and_then(Object::as_name);
                let is_metadata = ty.is_some_and(|n| n.0 == b"Metadata");
                let external = st.dict.get(b"F").is_some();
                if (is_metadata && !enc.encrypt_metadata) || external {
                    clear_streams.insert(num);
                }
            }
        }
    }
    let encrypting;
    let identity = IdentityEncoder;
    let encoder: &dyn ObjectEncoder = match enc {
        Some(enc) => {
            encrypting = EncryptingEncoder::new(enc.file_key, enc.encrypt_dict, clear_streams);
            &encrypting
        }
        None => &identity,
    };

    // A clean header copied from the source marker so an encrypted rewrite
    // always loads via the strict path.
    let mut out = {
        let mut h = base
            .get(header_span(base))
            .unwrap_or(b"%PDF-1.7\n")
            .to_vec();
        if !matches!(h.last(), Some(b'\n' | b'\r')) {
            h.push(b'\n');
        }
        h
    };

    // The base file's old xref stream, if any, is NOT re-emitted as a body
    // object — the output is a classic table, so the container disappears.
    let old_xref_stream = match doc.section_shape() {
        SectionShape::Stream { id, .. } => Some(id.num),
        _ => None,
    };

    let body_start = out.len();
    let _ = body_start;
    let mut entries: BTreeMap<u32, XrefEntry> = BTreeMap::new();
    let mut reserialized = 0usize;
    let mut promoted: Vec<ObjId> = Vec::new();

    let mut numbers: BTreeSet<u32> = doc.xref().iter().map(|(num, _)| num).collect();
    numbers.extend(dirty.iter().map(|id| id.num));
    if let Some(enc) = enc {
        numbers.insert(enc.encrypt_dict.num);
    }

    for num in numbers {
        if old_xref_stream == Some(num) {
            continue;
        }
        let entry = doc.xref().get(num);
        let generation = match entry {
            Some(XrefEntry::InUse { generation, .. }) => generation,
            _ => 0,
        };
        let id = ObjId::new(num, generation);

        if dirty.is_deleted(id) {
            continue;
        }

        // The value to serialise: the /Encrypt dict for its number, else a
        // dirty replacement, else the base object. A free base entry carries
        // no body.
        let value_owned;
        let value: &Object = if let Some(e) = enc.filter(|e| num == e.encrypt_dict.num) {
            &e.encrypt_dict_value
        } else if let Some(v) = dirty.replacement(id) {
            v
        } else {
            match entry {
                Some(XrefEntry::Free { .. }) => {
                    entries.insert(
                        num,
                        entry.unwrap_or(XrefEntry::Free {
                            next_free: 0,
                            generation: 65_535,
                        }),
                    );
                    continue;
                }
                _ => {
                    let io = doc.get(id).ok_or(WriteError::MissingObject { num })?;
                    value_owned = io.value.clone();
                    &value_owned
                }
            }
        };

        // A dropped object stream leaves no body; its members are emitted at
        // file level by their own numbers.
        if let Object::Stream(st) = value
            && st
                .dict
                .get(b"Type")
                .and_then(Object::as_name)
                .is_some_and(|n| n.0 == b"ObjStm")
        {
            continue;
        }

        let offset = out.len() as u64;
        serialize::write_indirect(&mut out, id, value, &combined, encoder);
        reserialized += 1;
        if matches!(entry, Some(XrefEntry::InStream { .. })) {
            promoted.push(id);
        }
        entries.insert(num, XrefEntry::InUse { offset, generation });
    }

    let highest = entries.keys().copied().max().unwrap_or(0);
    if highest > MAX_REWRITE_OBJECT_NUMBER {
        return Err(WriteError::ObjectNumberTooLarge {
            num: highest,
            max: MAX_REWRITE_OBJECT_NUMBER,
        });
    }
    entries.entry(0).or_insert(XrefEntry::Free {
        next_free: 0,
        generation: 65_535,
    });
    for num in 0..=highest {
        entries.entry(num).or_insert(XrefEntry::Free {
            next_free: 0,
            generation: 65_535,
        });
    }
    let deleted = apply_free_list(&mut entries, doc, dirty);

    let mut trailer = copy_trailer_without_prev(doc.trailer());
    for (key, value) in dirty.trailer_patch().iter() {
        trailer.insert(key.clone(), value.clone());
    }
    trailer.0.retain(|(k, _)| k.as_bytes() != b"XRefStm");
    if let Some(enc) = enc {
        // §7.5.5: /Encrypt names the security handler dictionary; /ID is
        // mandatory when /Encrypt is present. Both are set explicitly here —
        // never derived — because encryption owns them.
        trailer.insert(Name::from(b"Encrypt"), Object::Reference(enc.encrypt_dict));
        trailer.insert(
            Name::from(b"ID"),
            Object::Array(vec![
                Object::String(enc.file_id[0].clone()),
                Object::String(enc.file_id[1].clone()),
            ]),
        );
    }
    bump_size(&mut trailer, highest);

    let section_offset = out.len() as u64;
    xref_out::write_classic_table(&mut out, &entries, entry_eol)?;
    xref_out::write_classic_tail(&mut out, &trailer, section_offset, options.trailing_eol);

    let report = SaveReport {
        bytes_written: out.len(),
        bytes_appended: out.len(),
        objects_written: reserialized,
        objects_verbatim: 0,
        objects_reserialized: reserialized,
        byte_identical: false,
        delinearized: doc.linearization().save_invalidates_fast_web_view(),
        promoted,
        objects_deleted: deleted,
    };
    Ok((out, report))
}

/// Give every deleted object a conforming type-0 entry and splice them
/// onto the head of the file's free list. Returns how many were freed.
///
/// This function is decision 007 **W9** in executable form:
///
/// > A malformed type-0 free chain produces files Acrobat tolerates and
/// > stricter readers reject — the worst failure shape, because the
/// > obvious test passes.
///
/// Three rules, each from §7.5.4, each easy to get wrong:
///
/// 1. **The 10-digit field of a free entry is an object number, not an
///    offset** — *"the object number of the next free object"*. The
///    entries therefore form a **linked list**, and one that terminates
///    at 0.
/// 2. **A free entry's generation is the generation the number would get
///    if reused** — *"the generation number to be used if the object is
///    ever reused"* — i.e. one more than the object being freed. It
///    saturates at 65,535, which §7.5.4 defines as the marker for a
///    number that *"shall not be reused"*: an object already at the
///    maximum generation cannot be recycled, and pretending otherwise
///    would hand out an identity a stale reference could match.
/// 3. **Object 0 is the list head**, always free, always generation
///    65,535 — *"the head of the linked list of free objects"*.
///
/// ## Splicing rather than rebuilding, and why
///
/// The new entries are pushed onto the **front** of the existing list:
/// `0 → d₁ → d₂ → … → dₙ → (whatever object 0 used to point at)`. The
/// alternative — walking the base file's chain and appending to its tail
/// — would need to read entries this update section does not carry and
/// would rewrite free entries pdfcer was not asked to touch (§5). Front
/// insertion touches exactly the head plus the new entries, and §7.5.4
/// imposes no ordering on the list, so it is fully conforming.
///
/// Note what is deliberately **not** done: pre-existing "detached" free
/// entries (`next_free 0`, generation 65,535 — a form §7.5.4 explicitly
/// permits) are left exactly as the base file wrote them. Re-deriving a
/// tidy chain across them would be pdfcer inventing structure nobody
/// asked for, which R33 forbids for the same reason it forbids
/// normalizing anything else.
fn apply_free_list(
    entries: &mut BTreeMap<u32, XrefEntry>,
    doc: &Document,
    dirty: &DirtySet,
) -> usize {
    let freed: Vec<ObjId> = dirty.deletions().collect();
    if freed.is_empty() {
        return 0;
    }

    // Where the current head points, so the tail of the new run can be
    // spliced in front of it without losing the old list.
    let old_next_free = match entries.get(&0) {
        Some(XrefEntry::Free { next_free, .. }) => *next_free,
        // Object 0 is not free (a malformed file — §7.5.4 makes it
        // "always free"). Terminating the new list at 0 is the only
        // safe reading; chaining into a live object would be worse.
        _ => 0,
    };

    for (position, id) in freed.iter().enumerate() {
        // The next link: the following deleted object, or the old head's
        // target once the run is exhausted.
        let next_free = freed
            .get(position + 1)
            .map_or(old_next_free, |next| next.num);
        // Rule 2. The generation to record is the *deleted object's*
        // generation plus one — read from the cross-reference table
        // rather than from the id, because a caller could name a stale
        // generation and the table is what the file actually asserts.
        let current = match doc.xref().get(id.num) {
            Some(XrefEntry::InUse { generation, .. }) => generation,
            // A compressed object has no generation field at all
            // (§7.5.7 fixes it at 0), and an object the base does not
            // define starts at 0 too.
            _ => id.generation,
        };
        entries.insert(
            id.num,
            XrefEntry::Free {
                next_free,
                generation: current.saturating_add(1),
            },
        );
    }

    // Rule 3, applied last so it cannot be overwritten by the loop above
    // (an operator cannot delete object 0 — nothing hands out that id —
    // but the writer should not depend on that being true).
    let head = freed.first().map_or(old_next_free, |first| first.num);
    entries.insert(
        0,
        XrefEntry::Free {
            next_free: head,
            generation: 65_535,
        },
    );
    freed.len()
}

/// How an object's definition reached the output — the distinction the
/// §5 invariant is measured on.
enum Emission {
    /// Copied byte-for-byte from the retained source buffer.
    Verbatim,
    /// Rebuilt from the value tree because there were no file-level
    /// bytes to copy — which, for [`emit_object`], can only mean the
    /// object lived inside an object stream and has now been **promoted**
    /// out of it (R38). The variant is named for the consequence rather
    /// than the mechanism because the consequence is what gets reported.
    Promoted,
    /// Rebuilt from the value tree because the object's file-level bytes,
    /// though present, contradict its parsed value
    /// ([`Provenance::RecoveredFile`] — a stream whose extent recovery
    /// re-derived from `endstream`). Distinct from [`Emission::Promoted`]
    /// because nothing was promoted out of an object stream: the object
    /// was and remains file-level, so it must not appear in
    /// [`SaveReport::promoted`]. Counted as re-serialized.
    RecoveredReserialized,
}

/// Replace `ID[1]` with a fresh changing identifier (§14.4).
///
/// Called **only** when the save writes at least one changed object —
/// that condition lives at the call sites, not here, so this function
/// cannot accidentally become "regenerate on every save".
///
/// Three no-op cases, each a deliberate refusal to invent document
/// identity (see [`super`]'s "`/ID` on save"):
///
/// - **no `/ID` in the base trailer** — nothing is synthesized;
/// - **`/ID` is an indirect reference** — legal in an unencrypted file,
///   but rewriting it would mean editing an object the operator did not
///   touch and adding it to the dirty set behind their back;
/// - **`/ID` is malformed** (not an array of exactly two strings) —
///   §14.4 states no recovery rule, so the operator's value is passed
///   through exactly as found.
///
/// `ID[0]` is always preserved: §14.4 says it *"shall not change when the
/// file is incrementally updated"*, and §7.6.3.3 Algorithm 2 step (e)
/// feeds it into the encryption key, so a change here would surface in
/// Pass 5 as a decryption failure that looks like a crypto bug.
fn refresh_changing_identifier(trailer: &mut Dict, base_len: usize, appended: &[u8]) {
    let Some(Object::Array(items)) = trailer.get(b"ID") else {
        return;
    };
    let (Some(Object::String(permanent)), Some(Object::String(changing)), 2) =
        (items.first(), items.get(1), items.len())
    else {
        return;
    };
    let fresh = super::fileid::changing_identifier(changing, base_len, appended);
    let replacement = Object::Array(vec![
        Object::String(permanent.clone()),
        Object::String(fresh.to_vec()),
    ]);
    trailer.insert(Name::from(b"ID"), replacement);
}

/// Emit one object definition, preferring its verbatim source bytes.
///
/// The `Provenance` enum makes this a total function with no sentinel:
/// `File` has bytes, `ObjectStream` does not, and there is no third
/// "might have bytes" state to guess about (see
/// `crate::object::Provenance`'s type docs).
fn emit_object(
    out: &mut Vec<u8>,
    io: &IndirectObject,
    source: &[u8],
) -> Result<Emission, WriteError> {
    match io.provenance {
        Provenance::File(span) => {
            let bytes = span
                .slice(source)
                .ok_or(WriteError::SpanOutOfRange { id: io.id, span })?;
            out.extend_from_slice(bytes);
            // §7.5.1 line discipline: terminate the `endobj` line so
            // the next definition starts cleanly. The span ends at the
            // last byte of `endobj`, by the Provenance::File contract.
            out.push(b'\n');
            Ok(Emission::Verbatim)
        }
        // The bytes exist but contradict the value (a recovered stream
        // extent), so copying them would emit a file whose `/Length`
        // under- or over-runs its own data — one pdfcer would then refuse
        // to reload. Re-serializing regenerates `/Length` from the actual
        // data (`serialize`'s "always recomputed, never trusted"), which
        // is the only way a recovered document's full rewrite can be a
        // valid PDF.
        Provenance::RecoveredFile(_) => {
            serialize::write_indirect(out, io.id, &io.value, source, &IdentityEncoder);
            Ok(Emission::RecoveredReserialized)
        }
        // R38: promote-to-uncompressed. Reached in Pass 3.0 only via
        // an explicit identity re-emission of a compressed object; a
        // full rewrite never takes this branch, because it carries
        // object streams through intact (module docs).
        Provenance::ObjectStream { .. } => {
            serialize::write_indirect(out, io.id, &io.value, source, &IdentityEncoder);
            Ok(Emission::Promoted)
        }
    }
}

/// Re-serialize the document information dictionary with pdfcer's
/// `/Producer` (§14.3.3 Table 317), under [`ProducerPolicy::Set`].
///
/// This is the **only** object a full rewrite deliberately makes
/// non-verbatim, it happens only when the policy asks for it, and it is
/// counted in [`SaveReport::objects_reserialized`] so it can never be
/// mistaken for a passthrough. If the value is not a dictionary the
/// object is emitted verbatim instead — a `/Info` pointing at a
/// non-dictionary is a malformed file, and mangling it further is not
/// an improvement.
fn write_with_producer(out: &mut Vec<u8>, id: ObjId, value: &Object, source: &[u8]) {
    let Object::Dict(dict) = value else {
        // A `/Info` pointing at a non-dictionary is a malformed file,
        // and mangling it further is not an improvement — emit the
        // value unchanged.
        serialize::write_indirect(out, id, value, source, &IdentityEncoder);
        return;
    };
    let mut updated = dict.clone();
    updated.insert(
        Name::from(b"Producer"),
        Object::String(super::producer_string().into_bytes()),
    );
    serialize::write_indirect(out, id, &Object::Dict(updated), source, &IdentityEncoder);
}

/// Copy a trailer dictionary, dropping `Prev`.
///
/// §7.5.6 requirement 3 in one function: *"The added trailer shall
/// contain all the entries **except** the `Prev` entry (if present)
/// from the previous trailer, whether modified or not."* Keys that
/// describe the *physical* shape of the previous cross-reference stream
/// (`/Type`, `/W`, `/Index`, `/Filter`, `/DecodeParms`, `/Length`) are
/// dropped too: when the base file's newest section was a stream, its
/// dictionary IS the trailer, and forwarding those keys would emit a
/// dictionary that contradicts the bytes beneath it.
/// [`xref_out::build_xref_stream`] also drops them, belt and braces.
fn copy_trailer_without_prev(trailer: &Dict) -> Dict {
    let mut out = Dict::new();
    for (key, value) in trailer.iter() {
        if matches!(
            key.as_bytes(),
            b"Prev" | b"Type" | b"W" | b"Index" | b"Filter" | b"DecodeParms" | b"Length"
        ) {
            continue;
        }
        out.insert(key.clone(), value.clone());
    }
    out
}

/// Raise `/Size` to cover `highest`, never lowering it.
///
/// Table 15: `/Size` is *"1 greater than the highest object number
/// defined in the file"*, across the **whole `/Prev` chain** — not a
/// count of this section's entries. It is also a hard reader-side
/// filter (§7.5.5: objects numbered at or above it *"shall be ignored
/// and defined to be missing"*), so under-reporting it silently deletes
/// objects from every reader's view. Never lowering it is what keeps a
/// small update section on a large file correct.
fn bump_size(trailer: &mut Dict, highest: u32) {
    let needed = i64::from(highest) + 1;
    let current = trailer.get(b"Size").and_then(Object::as_int).unwrap_or(0);
    trailer.insert(Name::from(b"Size"), Object::Integer(current.max(needed)));
}

/// Byte range of the header region to copy verbatim into a full rewrite:
/// from the `%PDF-M.N` marker through that line, plus the §7.5.2
/// binary-comment line if one follows.
///
/// §7.5.2: *"If a PDF file contains binary data, as most do, the header
/// line shall be immediately followed by a comment line containing at
/// least four binary characters — that is, characters whose codes are
/// 128 or greater."* Dropping that line would make a binary file look
/// like a text file to transfer tools that inspect it, which is exactly
/// what it exists to prevent.
///
/// # Bytes BEFORE the marker are deliberately DROPPED (changed 2026-08-07)
///
/// This used to return a length from byte 0, carrying any leading junk
/// through, on the reasoning that pdfcer's probe tolerates a preamble and
/// §5 says not to normalize what the operator did not ask about. That was
/// wrong, and the measurement that settled it is worth keeping:
///
/// **veraPDF cannot open ANY file that has a preamble and spec-literal
/// offsets** — not merely one whose producer wrote header-relative
/// offsets. A minimal 3-object file, offsets absolute from byte 0 exactly
/// as §7.5.4/§7.5.5 require, with 19 bytes of junk ahead of `%PDF-`:
/// *"can not locate xref table"*. The identical file with the junk removed
/// parses clean. So an independent, conformance-focused reader treats
/// offsets as **header-relative** whenever a preamble exists.
///
/// That makes preamble preservation a defect generator rather than a
/// courtesy. `iso32000__s__7.5.md` records the ambiguity as real and
/// **unresolved by ISO 32000-1** — the spec position is byte 0, but the
/// spec gives no guidance for readers that disagree, and a file pdfcer
/// writes has to be readable by the readers that exist.
///
/// Dropping the preamble makes the two interpretations **coincide**:
/// with the header at byte 0, absolute and header-relative are the same
/// number, and every reader agrees. It also stops re-emitting a §7.5.2
/// violation ("The first line of a PDF file shall be a header") that the
/// operator never asked pdfcer to preserve.
///
/// # Why only a FULL rewrite may do this
///
/// `save_full` promises per-object-definition byte identity, a reloadable
/// file and an identical raster — explicitly **not** whole-file identity,
/// because offsets legitimately move. Removing a preamble is inside that
/// contract. Incremental and identity-append saves promise byte identity
/// or byte-prefix behaviour and **must** carry the preamble through; they
/// do not call this function.
fn header_span(buf: &[u8]) -> core::ops::Range<usize> {
    let window = buf
        .get(..buf.len().min(crate::HEADER_SCAN_WINDOW))
        .unwrap_or(buf);
    let Some(marker) = window.windows(5).position(|w| w == b"%PDF-") else {
        return 0..0;
    };
    let mut pos = marker;
    // End of the header line.
    while !matches!(buf.get(pos), None | Some(b'\r' | b'\n')) {
        pos += 1;
    }
    pos = skip_eol(buf, pos);
    // An immediately-following comment line, if any (§7.5.2).
    if matches!(buf.get(pos), Some(b'%')) {
        while !matches!(buf.get(pos), None | Some(b'\r' | b'\n')) {
            pos += 1;
        }
        pos = skip_eol(buf, pos);
    }
    marker..pos
}

/// Advance past one EOL marker (CR, LF, or CRLF — §7.2.2's rule that
/// CRLF is *one* marker, not two).
fn skip_eol(buf: &[u8], pos: usize) -> usize {
    match buf.get(pos) {
        Some(b'\r') => {
            if matches!(buf.get(pos + 1), Some(b'\n')) {
                pos + 2
            } else {
                pos + 1
            }
        }
        Some(b'\n') => pos + 1,
        _ => pos,
    }
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
    use crate::writer::SaveOptions;

    /// A small, offset-consistent classic PDF.
    fn classic_pdf() -> Vec<u8> {
        let mut buf = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
        let bodies = [
            "<< /Type /Catalog /Pages 2 0 R >>",
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Resources << >> >>",
            "<< /Producer (Somebody Else) /Title (Original) >>",
        ];
        let mut offsets = Vec::new();
        for (i, body) in bodies.iter().enumerate() {
            offsets.push(buf.len());
            buf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
        }
        let xref_at = buf.len();
        buf.extend_from_slice(b"xref\n0 5\n0000000000 65535 f \n");
        for off in &offsets {
            buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        buf.extend_from_slice(
            format!(
                "trailer\n<< /Size 5 /Root 1 0 R /Info 4 0 R >>\nstartxref\n{xref_at}\n%%EOF\n"
            )
            .as_bytes(),
        );
        buf
    }

    fn doc() -> Document {
        Document::from_bytes(classic_pdf()).unwrap()
    }

    #[test]
    fn empty_dirty_set_incremental_save_is_byte_identical() {
        // THE headline contract of Pass 3.0. Zero edits, zero bytes.
        let bytes = classic_pdf();
        let d = Document::from_bytes(bytes.clone()).unwrap();
        let (out, report) =
            save_incremental(&d, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
        assert_eq!(out, bytes);
        assert!(report.byte_identical);
        assert_eq!(report.bytes_appended, 0);
        assert_eq!(report.objects_written, 0);
    }

    #[test]
    fn incremental_save_never_rewrites_info_r41() {
        // R41 / decision 001 §6.1 obligation 6, at its enforcement
        // point. The /Info object's bytes must be findable, unchanged,
        // and must appear exactly once — no appended replacement.
        let bytes = classic_pdf();
        let d = Document::from_bytes(bytes.clone()).unwrap();
        let (out, _) = save_incremental(&d, &DirtySet::empty(), &SaveOptions::default()).unwrap();
        let needle = b"/Producer (Somebody Else)";
        assert_eq!(count_occurrences(&out, needle), 1);
        assert_eq!(count_occurrences(&out, b"pdfcer "), 0);
        // And with a real append that does not touch /Info.
        let (out, _) = save_incremental(
            &d,
            &DirtySet::identity_reemission([ObjId::new(3, 0)]),
            &SaveOptions::default(),
        )
        .unwrap();
        assert_eq!(count_occurrences(&out, needle), 1);
        assert_eq!(count_occurrences(&out, b"pdfcer "), 0);
    }

    #[test]
    fn incremental_append_preserves_every_prior_byte() {
        // §7.5.6: "changes shall be appended to the end of the file,
        // leaving its original contents intact". This is also the
        // signature-safety claim (§12.8.1 NOTE 1) in one assertion.
        let bytes = classic_pdf();
        let d = Document::from_bytes(bytes.clone()).unwrap();
        let (out, report) = save_incremental(
            &d,
            &DirtySet::identity_reemission([ObjId::new(3, 0)]),
            &SaveOptions::identity(),
        )
        .unwrap();
        assert!(out.starts_with(&bytes));
        assert!(report.bytes_appended > 0);
        assert_eq!(report.objects_verbatim, 1);
        assert_eq!(report.objects_reserialized, 0);
        assert!(!report.byte_identical);
    }

    #[test]
    fn appended_revision_reloads_to_an_equivalent_document() {
        let d = doc();
        let (out, _) = save_incremental(
            &d,
            &DirtySet::identity_reemission([ObjId::new(1, 0), ObjId::new(3, 0)]),
            &SaveOptions::identity(),
        )
        .unwrap();
        let back = Document::from_bytes(out).unwrap();
        assert_eq!(back.object_count(), d.object_count());
        for io in d.objects() {
            assert_eq!(
                back.get(io.id).map(|b| &b.value),
                Some(&io.value),
                "object {} differs after round trip",
                io.id
            );
        }
    }

    #[test]
    fn appended_trailer_has_prev_and_exactly_one_of_it() {
        // §7.5.6 requirement 3's trap: copying the old Prev AND adding
        // a new one is a duplicate key, which §7.3.7 prohibits and
        // pdfcer's own parser rejects at parse time.
        let d = doc();
        let (out, _) = save_incremental(
            &d,
            &DirtySet::identity_reemission([ObjId::new(3, 0)]),
            &SaveOptions::identity(),
        )
        .unwrap();
        let back = Document::from_bytes(out.clone()).unwrap();
        // Prev must name the base file's own startxref, not its /Prev.
        assert_eq!(
            back.trailer().get(b"Prev").and_then(Object::as_int),
            Some(i64::try_from(d.base_startxref()).unwrap())
        );
        // Two revisions, two %%EOF markers (§7.5.6 requirement 4).
        assert_eq!(count_occurrences(&out, b"%%EOF"), 2);
    }

    #[test]
    fn a_second_append_chains_prev_correctly() {
        // Chain shape after k updates: startxref -> Sk, Sk.Prev -> Sk-1.
        // Getting this wrong loses a whole revision silently.
        let d = doc();
        let (once, _) = save_incremental(
            &d,
            &DirtySet::identity_reemission([ObjId::new(3, 0)]),
            &SaveOptions::identity(),
        )
        .unwrap();
        let d2 = Document::from_bytes(once).unwrap();
        let (twice, _) = save_incremental(
            &d2,
            &DirtySet::identity_reemission([ObjId::new(1, 0)]),
            &SaveOptions::identity(),
        )
        .unwrap();
        assert_eq!(count_occurrences(&twice, b"%%EOF"), 3);
        let d3 = Document::from_bytes(twice).unwrap();
        assert_eq!(d3.object_count(), d.object_count());
        assert_eq!(
            d3.trailer().get(b"Prev").and_then(Object::as_int),
            Some(i64::try_from(d2.base_startxref()).unwrap())
        );
    }

    #[test]
    fn update_section_carries_object_zero_and_only_dirty_objects() {
        // §7.5.6 requirement 1 ("only for") plus Annex H.7's own
        // object-0 convention. The update table must have exactly two
        // entries: object 0 and object 3.
        let d = doc();
        let (out, _) = save_incremental(
            &d,
            &DirtySet::identity_reemission([ObjId::new(3, 0)]),
            &SaveOptions::identity(),
        )
        .unwrap();
        let tail = &out[classic_pdf().len()..];
        let text = String::from_utf8_lossy(tail).into_owned();
        assert!(text.contains("xref\n0 1\n"), "{text}");
        assert!(text.contains("\n3 1\n"), "{text}");
        assert!(!text.contains("\n0 5\n"), "full table re-emitted: {text}");
    }

    #[test]
    fn missing_final_eol_gets_one_before_the_append() {
        // §7.2.3: a comment runs to end of line, so an appended token
        // fused onto an unterminated %%EOF is swallowed entirely.
        let mut bytes = classic_pdf();
        while matches!(bytes.last(), Some(b'\n' | b'\r')) {
            bytes.pop();
        }
        let d = Document::from_bytes(bytes.clone()).unwrap();
        let (out, _) = save_incremental(
            &d,
            &DirtySet::identity_reemission([ObjId::new(3, 0)]),
            &SaveOptions::identity(),
        )
        .unwrap();
        assert!(out.starts_with(&bytes));
        assert_eq!(out[bytes.len()], b'\n');
        // And the result must actually load.
        assert!(Document::from_bytes(out).is_ok());
    }

    #[test]
    fn full_rewrite_reemits_every_object_definition_verbatim() {
        // R32's per-OBJECT assertion — never per file.
        let d = doc();
        let (out, report) = save_full(&d, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
        assert!(!report.byte_identical);
        assert_eq!(report.objects_reserialized, 0);
        assert_eq!(report.objects_verbatim, 4);
        for io in d.objects() {
            let want = io.file_span().unwrap().slice(d.bytes()).unwrap();
            assert!(
                out.windows(want.len()).any(|w| w == want),
                "object {} definition bytes missing from the rewrite",
                io.id
            );
        }
    }

    #[test]
    fn full_rewrite_reloads_to_an_equivalent_document() {
        let d = doc();
        let (out, _) = save_full(&d, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
        let back = Document::from_bytes(out).unwrap();
        assert_eq!(back.object_count(), d.object_count());
        for io in d.objects() {
            assert_eq!(back.get(io.id).map(|b| &b.value), Some(&io.value));
        }
        assert_eq!(back.version().to_string(), d.version().to_string());
    }

    #[test]
    fn full_rewrite_preserves_the_binary_comment_line() {
        // §7.5.2: dropping it makes a binary file look textual to
        // transfer tools — the exact thing the line exists to prevent.
        let d = doc();
        let (out, _) = save_full(&d, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
        assert!(out.starts_with(b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n"));
    }

    #[test]
    fn producer_policy_is_suppressible_and_effective() {
        // The R41 acceptance criterion, from the pdfcer-core side.
        let d = doc();
        let (preserved, rep) = save_full(&d, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
        assert_eq!(count_occurrences(&preserved, b"pdfcer "), 0);
        assert_eq!(count_occurrences(&preserved, b"Somebody Else"), 1);
        assert_eq!(rep.objects_reserialized, 0);

        let (stamped, rep) = save_full(&d, &DirtySet::empty(), &SaveOptions::default()).unwrap();
        assert_eq!(count_occurrences(&stamped, b"Somebody Else"), 0);
        assert_eq!(rep.objects_reserialized, 1);
        let back = Document::from_bytes(stamped).unwrap();
        let info = back
            .resolve(back.trailer().get(b"Info").unwrap())
            .as_dict()
            .unwrap()
            .clone();
        let Object::String(p) = info.get(b"Producer").unwrap() else {
            panic!("producer is not a string");
        };
        assert_eq!(p, super::super::producer_string().as_bytes());
        // Other /Info keys survive.
        assert!(info.contains_key(b"Title"));
    }

    #[test]
    fn producer_set_creates_nothing_when_info_is_absent() {
        // The deliberate narrowing recorded on ProducerPolicy::Set:
        // manufacturing an /Info on a file that had none is the
        // fingerprinting behavior R41 exists to prevent.
        let mut bytes = classic_pdf();
        let at = bytes
            .windows(11)
            .position(|w| w == b" /Info 4 0 R")
            .or_else(|| bytes.windows(12).position(|w| w == b" /Info 4 0 R"));
        if let Some(pos) = at {
            bytes.splice(pos..pos + 12, std::iter::repeat_n(b' ', 12));
        }
        let d = Document::from_bytes(bytes).unwrap();
        assert!(d.trailer().get(b"Info").is_none());
        let (out, rep) = save_full(&d, &DirtySet::empty(), &SaveOptions::default()).unwrap();
        assert_eq!(count_occurrences(&out, b"pdfcer "), 0);
        assert_eq!(rep.objects_reserialized, 0);
    }

    #[test]
    fn full_rewrite_fills_object_number_holes_with_free_entries() {
        // §7.5.4: a single section must cover 0..max with no holes.
        let mut buf = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (num, body) in [
            (1u32, "<< /Type /Catalog /Pages 2 0 R >>"),
            (2, "<< /Type /Pages /Kids [] /Count 0 >>"),
            // Deliberate hole at 3-4.
            (5, "<< /Note (sparse) >>"),
        ] {
            offsets.push((num, buf.len()));
            buf.extend_from_slice(format!("{num} 0 obj\n{body}\nendobj\n").as_bytes());
        }
        let xref_at = buf.len();
        buf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
        for num in 1..=5u32 {
            match offsets.iter().find(|(n, _)| *n == num) {
                Some((_, off)) => {
                    buf.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
                }
                None => buf.extend_from_slice(b"0000000000 65535 f \n"),
            }
        }
        buf.extend_from_slice(
            format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n").as_bytes(),
        );
        let d = Document::from_bytes(buf).unwrap();
        let (out, _) = save_full(&d, &DirtySet::empty(), &SaveOptions::identity()).unwrap();
        let back = Document::from_bytes(out.clone()).unwrap();
        assert_eq!(back.object_count(), 3);
        // One contiguous subsection covering 0..=5.
        let text = String::from_utf8_lossy(&out).into_owned();
        assert!(text.contains("xref\n0 6\n"), "{text}");
    }

    #[test]
    fn size_never_shrinks_on_an_update() {
        // Table 15: /Size is a high-water mark across the whole chain,
        // not a count of this section's entries. Under-reporting it
        // silently deletes objects from every reader's view.
        let d = doc();
        let (out, _) = save_incremental(
            &d,
            &DirtySet::identity_reemission([ObjId::new(1, 0)]),
            &SaveOptions::identity(),
        )
        .unwrap();
        let back = Document::from_bytes(out).unwrap();
        assert_eq!(
            back.trailer().get(b"Size").and_then(Object::as_int),
            Some(5)
        );
    }

    #[test]
    fn header_span_handles_all_three_shapes() {
        assert_eq!(header_span(b"%PDF-1.7\nrest"), 0..9);
        assert_eq!(header_span(b"%PDF-1.7\r\n%\x80\x81\x82\x83\r\nrest"), 0..17);
        // No header at all: copy nothing rather than guessing.
        assert_eq!(header_span(b"not a pdf"), 0..0);
        // A non-comment second line is not swallowed.
        assert_eq!(header_span(b"%PDF-1.4\n1 0 obj\n"), 0..9);
    }

    /// The span STARTS at the marker, so a preamble is excluded.
    ///
    /// This is the unit-level statement of the 2026-08-07 change. Without
    /// it the only coverage would be the end-to-end rewrite test, and a
    /// regression that reintroduced `..end` would still satisfy every
    /// assertion above — all four of those cases have the marker at byte
    /// 0, where `0..end` and `..end` are indistinguishable.
    #[test]
    fn header_span_excludes_bytes_before_the_marker() {
        assert_eq!(header_span(b" %PDF-1.4\nrest"), 1..10);
        assert_eq!(header_span(b"JUNK\n%PDF-1.4\nrest"), 5..14);
        // With the binary-comment line, still measured from the marker.
        // 3-byte BOM, then `%PDF-1.7\n` (9) and `%\x80\x81\x82\x83\n` (6).
        assert_eq!(
            header_span(b"\xEF\xBB\xBF%PDF-1.7\n%\x80\x81\x82\x83\n"),
            3..18
        );
    }

    #[test]
    fn unknown_dirty_object_is_a_named_refusal() {
        let d = doc();
        let err = save_incremental(
            &d,
            &DirtySet::identity_reemission([ObjId::new(99, 0)]),
            &SaveOptions::identity(),
        )
        .unwrap_err();
        assert!(matches!(err, WriteError::UnknownDirtyObject { .. }));
    }

    fn count_occurrences(haystack: &[u8], needle: &[u8]) -> usize {
        if needle.is_empty() || haystack.len() < needle.len() {
            return 0;
        }
        haystack
            .windows(needle.len())
            .filter(|w| *w == needle)
            .count()
    }
}
