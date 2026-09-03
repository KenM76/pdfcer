//! # Cross-reference recovery — rebuild-by-scan (decision 013 Pass B)
//!
//! When a file's stored cross-reference machinery cannot be parsed
//! ([`crate::xref::load_xref_chain`] returns `Err`, or the `%PDF-` header
//! probe fails), this module reconstructs a usable cross-reference table
//! and trailer by **scanning the whole buffer for `N G obj` headers** and
//! walking any recovered object streams. It is the single highest-leverage
//! real-world robustness fix in pdfcer: a swept 1,109-file real-world
//! corpus showed 605 of 712 load failures (85%) were this one missing
//! capability, and every mature reader (pdfium, qpdf, poppler, mupdf,
//! pdf.js) closes the gap the same way. See
//! `docs/decisions/013-xref-recovery.md`.
//!
//! ## Recovery is a FALLBACK, never the default (the load-bearing rule)
//!
//! Recovery runs **only after the strict path has already failed**. A file
//! that returns `Ok(LoadedXref)` from `load_xref_chain` takes the normal
//! path unchanged, with zero recovery code in its control flow — so the
//! round-trip/minimal-diff invariant (`ARCHITECTURE.md` §5) for cleanly
//! loading files is preserved **by construction**, not by policy. The
//! trigger lives in [`crate::document::Document::from_bytes`], gated by
//! [`recovery_reason_for`]; this module is only ever entered on the error
//! path.
//!
//! ## What is reconstructed vs. what is spec
//!
//! §7.5.4 (classic table), §7.5.5 (the load algorithm, `startxref`,
//! `%%EOF`), §7.5.7 (object streams), §7.5.8 (cross-reference streams) and
//! Annex H.7 define the **well-formed structure** this module rebuilds a
//! valid instance of. **No ISO clause defines a recovery *procedure*** —
//! rebuild-by-scan is a deliberate reader-robustness **policy** grounded
//! in universal reader behaviour (the same outcome-over-method pattern the
//! xref layer already uses, e.g. tolerating a missing `/Type` on an xref
//! stream). Where recovery cannot succeed it fails clean (R27); it never
//! returns a partial or guessed [`crate::document::Document`].
//!
//! ## The algorithm (two phases + trailer, mirroring the normal loader)
//!
//! **Phase 1 — file-level scan.** A single linear pass locates every
//! `N G obj` header. Each candidate is then **confirmed** by actually
//! parsing it with the ordinary [`Parser::parse_indirect_object`] (rule
//! 13: no new tokenizer, no new dependency) — a candidate that does not
//! parse, or whose parsed identity disagrees with its scanned header, is a
//! binary-data false positive and is dropped. Confirmed objects are keyed
//! by number **last-valid-wins by file order** (the reader convention:
//! incremental updates append, so a later definition supersedes an
//! earlier one). Each becomes a synthetic [`XrefEntry::InUse`].
//!
//! Confirmation runs under [`StreamLengthPolicy::RecoverFromEndstream`],
//! **not** under the parser's default strictness. That one word is what
//! makes the scan's object count match the file's real object count on
//! damaged input. A `/Length` that does not land on `endstream` is
//! §7.3.8.2 "an error" on the clean path, but here it is the single most
//! common surviving symptom of the very damage that broke the
//! cross-reference table in the first place: the dominant real-world shape
//! is a file whose `/Length` values were computed with LF line endings and
//! which was later converted to CRLF, growing every stream by one byte per
//! line and shifting every stored offset (which is *why* `startxref` no
//! longer points at `xref`). Confirming strictly would silently drop
//! exactly the content streams the page tree then demands, turning a
//! recoverable file into an unopenable one. Repairs are counted into
//! [`RecoveryReport::stream_lengths_recovered`] and disclosed.
//!
//! The same policy must be used by [`crate::document`]'s re-parse of the
//! recovered table — an object accepted here and rejected there would fail
//! the whole load — which is why the policy is a parameter of
//! [`parse_object_at`] rather than a local choice.
//!
//! **Phase 2 — object streams.** *After* phase 1 (the same forced order as
//! [`crate::document`]'s normal two-phase load), every confirmed
//! `/Type /ObjStm` container is decoded and its `/N` pair table read; each
//! compressed member becomes a synthetic [`XrefEntry::InStream`] (type-2).
//! Compressed members are **not** at file-level `N G obj` offsets, so the
//! scan cannot find them directly — the container's pair table is the
//! authority, exactly as a type-2 xref entry normally is. A file-level
//! (scanned) definition **wins** over an object-stream member of the same
//! number (a documented limitation: true newest-wins needs the revision
//! ordering the scan discards). For a normal xref-stream file most objects
//! arrive via this phase — the design handles "few file-level objects,
//! bulk from ObjStm".
//!
//! **Trailer.** In priority order: (1) the **last `trailer` keyword**'s
//! dictionary (classic files — the dominant offset-shift case); (2) a
//! recovered cross-reference **stream** object's own Table 17 dictionary
//! (§7.5.8.1: it "carries what a trailer would carry" — its *data* was
//! unreadable, which is *why* strict failed, but its dictionary with
//! `/Root`/`/Size`/`/Info`/`/Encrypt`/`/ID` parsed fine as an ordinary
//! file-level object); (3) **synthesize** from a scanned `/Type /Catalog`
//! object. The trailer is normalized to just the semantic keys — §5.6's
//! "never normalize" governs *clean passthrough*, and a recovered file's
//! base was invalid, so emitting a fresh classic form is correct, not a
//! violation (decision 013 §3.3.4). If `/Encrypt` is present recovery
//! refuses ([`RecoverError::Encrypted`]) — the same §7.6 capability gap
//! the clean path enforces, re-checked here so a broken-xref-**and**-
//! encrypted file still fails for the right reason. If no `/Catalog` can
//! be found by any route, recovery fails clean ([`RecoverError::NoCatalog`]).
//!
//! ## Absolute offsets subsume the offset-start case
//!
//! The scan records `N G obj` at **absolute** byte positions and never
//! trusts a stored offset, so it is header-independent: it also handles a
//! file with leading bytes before `%PDF-` (the "offset start" case,
//! decision 007 §10 item 6) with no rebasing — spans are absolute from
//! byte 0 (`crate::span`). The [`RecoveryReport::offset_start`] flag simply
//! records that the marker was not at byte 0.
//!
//! ## Resource guards (R25, `ARCHITECTURE.md` §10)
//!
//! The scan is O(n) single-pass. Total synthesized entries are capped by
//! [`crate::xref::MAX_XREF_ENTRIES`] — an `obj`-token-dense adversarial
//! file stops at the cap and fails clean rather than allocating without
//! bound. Object-stream decoding in phase 2 runs through the same
//! `crate::filters` / [`ObjectStream`] machinery as the normal path, so
//! the §10.1 decompression-bomb ceilings apply. There is no recursion
//! (§7.5.7's two-level guarantee holds for recovered containers too), and
//! the "no catalog → refuse" terminal together with the entry cap bound
//! worst-case work: recovery always terminates and never hangs.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use crate::PdfVersion;
use crate::document::parse_object_at;
use crate::lexer::{is_delimiter, is_regular, is_whitespace};
use crate::object::{Dict, Name, ObjId, Object};
use crate::objstm::ObjectStream;
use crate::parser::{Parser, StreamLengthPolicy, TerminatorPolicy};
use crate::xref::{MAX_XREF_ENTRIES, XrefEntry, XrefErrorKind, XrefTable};

/// Why the strict load failed, carried into the [`RecoveryReport`] so the
/// disclosure (R20) can name the originating cause.
///
/// This is a self-contained enum rather than a borrow of
/// [`XrefErrorKind`] so a recovered [`crate::document::Document`] does not
/// pin the xref error's payloads for its whole lifetime. The mapping from
/// an `XrefErrorKind` (and the header-probe failure) lives in
/// [`recovery_reason_for`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RecoveryReason {
    /// No `startxref` keyword in the trailing scan window.
    StartxrefNotFound,
    /// `startxref` present but its offset was unusable / out of range.
    BadStartxrefOffset,
    /// The `startxref` target was neither an `xref` keyword nor an
    /// `N G obj` stream header — the classic offset-shift signature.
    NotAnXrefSection,
    /// A classic 20-byte entry deviated from §7.5.4.
    BadEntry,
    /// A subsection header line was not `first count`.
    BadSubsectionHeader,
    /// The `trailer` dictionary was missing or malformed, or a `/Prev`
    /// offset was out of range.
    BadTrailer,
    /// The `/Prev` chain was cyclic or exceeded the section cap.
    PrevChainCycle,
    /// The strict path hit the entry cap; recovery re-attempts under it.
    TooManyEntries,
    /// A cross-reference stream's dictionary/`W`/`Index`/data was
    /// malformed — unrecoverable in place, so the file-level objects are
    /// scanned instead.
    BadXrefStream,
    /// A cross-reference stream's filter chain failed to decode.
    XrefStreamDecode,
    /// A structural parse error inside the trailer dictionary.
    TrailerParse,
    /// The `%PDF-` header probe failed (offset-start / leading-junk /
    /// headerless); recovery is header-independent and attempts a rebuild
    /// anyway, succeeding only if it finds objects **and** a `/Catalog`.
    MissingHeader,
}

/// Where the recovered trailer's keys came from — disclosed and counted
/// (R20), because the operator should be able to tell a parsed trailer
/// from a synthesized one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum TrailerSource {
    /// Parsed from the last `trailer` keyword's dictionary (§7.5.5) — the
    /// classic-table case, which is the dominant offset-shift shape.
    TrailerKeyword,
    /// Lifted from a recovered cross-reference **stream** object's own
    /// Table 17 dictionary (§7.5.8.1). The stream's *data* was unreadable
    /// (that is why strict failed), but its dictionary — carrying `/Root`,
    /// `/Size`, `/Info`, `/Encrypt`, `/ID` — parsed as an ordinary
    /// file-level object.
    XrefStreamDict,
    /// Synthesized: `/Root` set to a scanned `/Type /Catalog` object,
    /// `/Size` from the highest recovered number, `/ID`/`/Encrypt`/`/Info`
    /// lifted from any recovered cross-reference-stream dictionary found.
    SynthesizedFromCatalog,
}

/// The counted, disclosed record of a recovery (fuzzy-never-sneaky, R20).
///
/// Stored on the recovered [`crate::document::Document`]; surfaced by the
/// CLI (a diagnostic line + a distinct exit status) and the GUI (a
/// non-blocking banner). Every field is a fact an operator can act on, so
/// none is rounded away.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct RecoveryReport {
    /// Why the strict load failed (the originating cause).
    pub reason: RecoveryReason,
    /// Objects recovered by the file-level `N G obj` scan (type-1).
    pub file_level_objects: usize,
    /// Objects recovered from `/Type /ObjStm` containers (type-2).
    pub objstm_objects: usize,
    /// Duplicate object numbers resolved last-valid-wins among file-level
    /// definitions (how many times a number had more than one valid
    /// definition).
    pub last_wins_collisions: usize,
    /// Recovered objects whose stream extent had to be re-derived from the
    /// `endstream` keyword because the stored `/Length` was unusable
    /// (absent, unresolvable, or simply not landing on `endstream` —
    /// §7.3.8.2 defines `/Length` in terms of that keyword, so the two are
    /// halves of one statement and this counts how often they disagreed).
    ///
    /// Counted over the objects actually **kept**, not over every candidate
    /// examined, so the number matches what the loaded document contains.
    /// A non-zero value means some stream's byte extent is pdfcer's reading
    /// of the file rather than the file's own claim — the operator is told
    /// (R20) because the two can differ, most visibly for a stream whose
    /// binary data happens to contain the bytes `endstream`.
    pub stream_lengths_recovered: usize,
    /// How many definitions were accepted with **no `endobj` keyword**
    /// (§7.3.10 requires one).
    ///
    /// Non-zero means the file omitted a required terminator and pdfcer
    /// bounded the object at the next `N G obj` header instead of dropping
    /// it. Dropping is what pdfcer used to do, and it is how a document
    /// whose catalog named an absent `/Pages` object got written — see
    /// [`crate::parser::TerminatorPolicy`]. Disclosed, never silent (R20).
    pub missing_endobj_recovered: usize,
    /// Where the recovered trailer's keys came from.
    pub trailer_source: TrailerSource,
    /// Whether the `%PDF-` marker was not at byte 0 (offset-start /
    /// leading-junk / headerless) — recovery handled it via absolute
    /// offsets with no rebasing.
    pub offset_start: bool,
}

/// A reconstructed cross-reference table + trailer, ready for the document
/// layer to eager-parse exactly as if [`crate::xref::load_xref_chain`] had
/// produced it.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct RecoveredXref {
    /// The synthesized merged table (type-1 file-level + type-2 ObjStm).
    pub table: XrefTable,
    /// The recovered trailer, normalized to `/Root`/`/Size`/`/Info`/
    /// `/Encrypt`/`/ID`.
    pub trailer: Dict,
    /// Highest recovered object number (the number a writer allocates
    /// above); also the basis for the synthesized `/Size`.
    pub highest_object_number: u32,
    /// PDF version detected by scanning for `%PDF-` (whole-buffer, so it
    /// survives the offset-start case); a conservative default when the
    /// file carries no marker at all.
    pub version: PdfVersion,
    /// The counted disclosure record.
    pub report: RecoveryReport,
}

/// Why a recovery attempt could not produce a document — every case a
/// **named, fail-clean refusal** (R27), never a partial/garbage document.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum RecoverError {
    /// The scan found no confirmable `N G obj` objects at all — this is
    /// not a recoverable PDF (the "not a PDF" outcome on the header path).
    #[error("cross-reference recovery found no parseable objects")]
    NoObjects,
    /// Objects were recovered but no `/Type /Catalog` could be found by
    /// any route — a document root is required (§7.7.2), so recovery
    /// refuses rather than return a rootless document.
    #[error("cross-reference recovery found no document catalog (/Type /Catalog)")]
    NoCatalog,
    /// The recovered trailer carries `/Encrypt` (§7.6). pdfcer has no
    /// security handler, so a broken-xref-**and**-encrypted file is
    /// refused post-rebuild for the same reason the clean path refuses it.
    #[error("recovered document is encrypted (\u{a7}7.6); recovery cannot proceed")]
    Encrypted,
    /// The synthesized entries exceeded [`MAX_XREF_ENTRIES`] (R25 guard) —
    /// an `obj`-token-dense adversarial file stops here rather than
    /// allocating without bound.
    #[error("recovered cross-reference entries exceed MAX_XREF_ENTRIES")]
    TooManyEntries,
}

/// Map a strict-load [`XrefErrorKind`] to the recovery it should trigger,
/// or `None` when recovery **must not** fire.
///
/// This one function is the load-bearing trigger gate (decision 013
/// §3.3.1): it simultaneously decides *whether* to recover and *why*.
///
/// - `EncryptionUnsupported` returns `None`: encryption is a deliberate
///   **named capability gap**, not damage. Scanning would still surface
///   ciphertext, and recovery re-checks `/Encrypt` after rebuilding the
///   trailer anyway — so refusing up front here is both correct and
///   cheaper.
/// - `XrefErrorKind` is `#[non_exhaustive]`, but this match lives in the
///   same crate, so it is **exhaustive without a wildcard**: adding a
///   cross-reference error kind in a future Pass forces the author to
///   classify it here (recover or refuse) rather than silently defaulting.
///
/// Object-level failures *after* a clean xref (`DocError::BadObject`,
/// `ObjectIdMismatch`, `ObjectStream*`) are **not** reachable here — they
/// arise in the document layer after `load_xref_chain` already returned
/// `Ok`, so recovery never sees them. That scoping (xref-parse failure
/// only) is a documented limitation, not an oversight.
#[must_use]
pub(crate) fn recovery_reason_for(kind: &XrefErrorKind) -> Option<RecoveryReason> {
    match kind {
        XrefErrorKind::StartxrefNotFound => Some(RecoveryReason::StartxrefNotFound),
        XrefErrorKind::BadStartxrefOffset => Some(RecoveryReason::BadStartxrefOffset),
        XrefErrorKind::NotAnXrefSection => Some(RecoveryReason::NotAnXrefSection),
        XrefErrorKind::BadEntry => Some(RecoveryReason::BadEntry),
        XrefErrorKind::BadSubsectionHeader => Some(RecoveryReason::BadSubsectionHeader),
        XrefErrorKind::BadTrailer(_) => Some(RecoveryReason::BadTrailer),
        XrefErrorKind::PrevChainCycle => Some(RecoveryReason::PrevChainCycle),
        XrefErrorKind::TooManyEntries => Some(RecoveryReason::TooManyEntries),
        XrefErrorKind::BadXrefStream(_) => Some(RecoveryReason::BadXrefStream),
        XrefErrorKind::XrefStreamDecode(_) => Some(RecoveryReason::XrefStreamDecode),
        XrefErrorKind::Parse(_) => Some(RecoveryReason::TrailerParse),
        // §7.6: a NAMED capability gap, not damage — do not recover.
        XrefErrorKind::EncryptionUnsupported => None,
    }
}

/// One confirmed file-level object: its identity, the offset of its
/// `N G obj` header, and its parsed value (kept so phase 2 and the
/// catalog/trailer search need not re-parse it).
struct Confirmed {
    id: ObjId,
    offset: usize,
    value: Object,
    /// Stream extents re-derived from `endstream` while parsing THIS
    /// object (0 or 1 in practice — one object holds at most one stream).
    /// Carried per-object so the report can sum over the objects actually
    /// kept, rather than over superseded duplicates the document never sees.
    lengths_recovered: usize,
    /// Whether THIS object was accepted with no `endobj` keyword (0 or 1).
    /// Same per-object carriage, same reason.
    missing_endobj: usize,
}

/// Reconstruct a cross-reference table + trailer from a full-buffer scan.
///
/// See the module docs for the algorithm. This is the recovery entry point
/// [`crate::document::Document::from_bytes`] calls on the strict-load error
/// path.
///
/// # Errors
///
/// [`RecoverError`] — every case a named fail-clean refusal (no objects, no
/// catalog, encrypted, or the entry cap). A recovered document is returned
/// only when a `/Catalog` is reachable and the file is not encrypted.
pub fn recover(buf: &[u8], reason: RecoveryReason) -> Result<RecoveredXref, RecoverError> {
    // Phase 1: locate + confirm every file-level `N G obj` object.
    let candidates = scan_object_headers(buf);
    let (confirmed, last_wins_collisions) = confirm_candidates(buf, &candidates)?;
    if confirmed.is_empty() {
        return Err(RecoverError::NoObjects);
    }

    // Build the type-1 (file-level) entries.
    let mut entries: HashMap<u32, XrefEntry> = HashMap::with_capacity(confirmed.len());
    for c in confirmed.values() {
        entries.insert(
            c.id.num,
            XrefEntry::InUse {
                offset: c.offset as u64,
                generation: c.id.generation,
            },
        );
    }

    // Phase 2: walk recovered `/Type /ObjStm` containers for compressed
    // members (type-2 entries).
    let objstm_objects = recover_object_streams(buf, &confirmed, &mut entries)?;

    let highest_object_number = entries.keys().copied().max().unwrap_or(0);

    // Trailer recovery + the §7.6 encryption re-check.
    let (mut trailer, trailer_source) = recover_trailer(buf, &confirmed)?;
    if trailer.contains_key(b"Encrypt") {
        return Err(RecoverError::Encrypted);
    }
    // Ensure `/Size` covers every recovered object. The document layer does
    // not re-run the §7.5.5 `/Size` filter on a synthesized table, but a
    // stale-low `/Size` would still mislead `next_object_number`/the
    // writer; setting it correctly is right for a rebuilt file (its base
    // was invalid, so §5.6 does not bind).
    set_size_at_least(&mut trailer, highest_object_number);

    let version = detect_version(buf);
    let offset_start = find_bytes(buf, 0, b"%PDF-") != Some(0);
    // Sum over the objects actually KEPT (superseded duplicates are not in
    // the loaded document, so counting them would overstate the repair).
    let stream_lengths_recovered = confirmed.values().map(|c| c.lengths_recovered).sum();
    let missing_endobj_recovered = confirmed.values().map(|c| c.missing_endobj).sum();

    Ok(RecoveredXref {
        table: XrefTable::from_entries(entries),
        trailer,
        highest_object_number,
        version,
        report: RecoveryReport {
            reason,
            file_level_objects: confirmed.len(),
            objstm_objects,
            last_wins_collisions,
            trailer_source,
            offset_start,
            stream_lengths_recovered,
            missing_endobj_recovered,
        },
    })
}

/// A scanned `N G obj` candidate: its identity and the byte offset of the
/// object number (the start of the full `N G obj … endobj` definition).
struct Candidate {
    id: ObjId,
    offset: usize,
}

/// Phase 1 scan: one linear forward pass locating every `N G obj` header.
///
/// Reader-robust by construction: it searches for the literal `obj`
/// keyword (byte-level), then walks **backward** over the whitespace and
/// two integer runs that must precede it. It does not fully tokenize the
/// buffer, so binary stream data cannot abort the scan the way a strict
/// lex would. Candidates are validated (parsed) separately in
/// [`confirm_candidates`]; this pass only proposes.
fn scan_object_headers(buf: &[u8]) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut from = 0usize;
    while let Some(obj_start) = find_bytes(buf, from, b"obj") {
        let obj_end = obj_start + 3;
        // Always advance so the scan is strictly forward (O(n) total).
        from = obj_end;

        // Token boundary AFTER `obj`: EOF or a non-regular byte, so `object`
        // / `objx` do not match.
        if buf.get(obj_end).is_some_and(|&b| is_regular(b)) {
            continue;
        }
        if let Some(c) = parse_header_backward(buf, obj_start) {
            out.push(c);
        }
    }
    out
}

/// Given the offset of the `o` in an `obj` keyword, recover the preceding
/// `N G` header by walking backward, or `None` if the bytes before it are
/// not a well-formed `N SP+ G` (e.g. `endobj`, where `d` precedes `obj`).
fn parse_header_backward(buf: &[u8], obj_start: usize) -> Option<Candidate> {
    let mut p = obj_start;
    // At least one whitespace byte between the generation and `obj`. This
    // is also what rejects `endobj` (`...d obj`? no — `endobj` has `d`
    // immediately before `obj`, no whitespace, so this fails cleanly).
    if !skip_ws_back(buf, &mut p) {
        return None;
    }
    let gen_end = p;
    skip_digits_back(buf, &mut p);
    let gen_start = p;
    if gen_start == gen_end {
        return None; // no generation digits
    }
    // At least one whitespace byte between the object number and generation.
    if !skip_ws_back(buf, &mut p) {
        return None;
    }
    let num_end = p;
    skip_digits_back(buf, &mut p);
    let num_start = p;
    if num_start == num_end {
        return None; // no object-number digits
    }
    // Boundary before the object number: start-of-buffer or a non-regular
    // byte, so a number is never sliced out of the middle of a longer run
    // (binary junk like `x12 0 obj`).
    if num_start > 0 && buf.get(num_start - 1).is_some_and(|&b| is_regular(b)) {
        return None;
    }

    let num = u32::try_from(parse_uint(buf.get(num_start..num_end)?)?).ok()?;
    let generation = u16::try_from(parse_uint(buf.get(gen_start..gen_end)?)?).ok()?;
    // Object 0 is permanently the free-list head (§7.5.4); it never names a
    // real object.
    if num == 0 {
        return None;
    }
    Some(Candidate {
        id: ObjId::new(num, generation),
        offset: num_start,
    })
}

/// Confirm each scanned candidate by actually parsing it, keeping the
/// **last valid** definition per object number (file order).
///
/// Confirmation is what makes the scan safe to hand to the strict document
/// loader: a candidate whose offset does not parse as a well-formed
/// `N G obj … endobj`, or whose parsed identity disagrees with the scanned
/// header, is a binary-data false positive and is dropped rather than
/// poisoning the table. Indirect stream `/Length` values are resolved
/// against a provisional table built from all candidate offsets (the same
/// §7.3.10 EXAMPLE 3 pattern the normal loader uses).
///
/// Returns the confirmed objects keyed by number and the count of
/// last-valid-wins collisions.
fn confirm_candidates(
    buf: &[u8],
    candidates: &[Candidate],
) -> Result<(HashMap<u32, Confirmed>, usize), RecoverError> {
    // Provisional last-wins offset table for indirect `/Length` resolution.
    // Candidates are in ascending file order, so a later insert supersedes.
    // Built ONCE (not per candidate — that would be O(n^2)).
    let mut provisional: HashMap<u32, XrefEntry> = HashMap::with_capacity(candidates.len());
    for c in candidates {
        provisional.insert(
            c.id.num,
            XrefEntry::InUse {
                offset: c.offset as u64,
                generation: c.id.generation,
            },
        );
    }
    let length_table = XrefTable::from_entries(provisional);

    let mut confirmed: HashMap<u32, Confirmed> = HashMap::new();
    let mut collisions = 0usize;
    for c in candidates {
        // RecoverFromEndstream, and ONLY here (plus the matching re-parse in
        // `document::assemble`): this is the single highest-yield leniency
        // in the whole recovery path. A corpus census over 4,012 real PDFs
        // found 341 files unopenable with "page /Contents is neither a
        // stream nor an array of streams"; in 337 of them the missing
        // content stream's `N G obj` header was physically present and had
        // been dropped **here**, overwhelmingly because the file's
        // `/Length` values were computed for LF line endings and the file
        // was later converted to CRLF, so every stream is one byte per line
        // longer than it claims. Confirming such an object under the strict
        // policy discards a perfectly readable content stream.
        let (io, repairs) = match parse_object_at(
            buf,
            &length_table,
            c.offset as u64,
            StreamLengthPolicy::RecoverFromEndstream,
            // A definition whose `endobj` is missing but whose body parsed
            // cleanly is kept. Dropping it costs the whole object, and when
            // that object is the `/Pages` node the catalog is left naming
            // nothing — the defect the veraPDF gate found on qpdf's
            // `bad6.pdf`. See `TerminatorPolicy`.
            TerminatorPolicy::RecoverAtNextHeader,
        ) {
            Ok(pair) => pair,
            Err(_) => continue, // false positive / unparseable — drop
        };
        if io.id != c.id {
            continue; // scan/parse disagreement — drop
        }
        // Last-valid-wins: a later valid definition supersedes an earlier
        // one (reader convention for appended incremental updates).
        if confirmed
            .insert(
                c.id.num,
                Confirmed {
                    id: c.id,
                    offset: c.offset,
                    value: io.value,
                    lengths_recovered: repairs.stream_lengths,
                    missing_endobj: repairs.missing_endobj,
                },
            )
            .is_some()
        {
            collisions += 1;
        }
        if confirmed.len() > MAX_XREF_ENTRIES {
            return Err(RecoverError::TooManyEntries);
        }
    }
    Ok((confirmed, collisions))
}

/// Phase 2: for every confirmed `/Type /ObjStm` container, synthesize a
/// type-2 entry for each compressed member the container's pair table
/// names, and return the count added.
///
/// A file-level (scanned) definition wins over an object-stream member of
/// the same number (documented limitation B-W4); among object streams the
/// first by container number wins (deterministic; true newest-wins needs
/// revision ordering the scan discards).
fn recover_object_streams(
    buf: &[u8],
    confirmed: &HashMap<u32, Confirmed>,
    entries: &mut HashMap<u32, XrefEntry>,
) -> Result<usize, RecoverError> {
    // Deterministic order: container object number ascending.
    let mut containers: Vec<&Confirmed> = confirmed
        .values()
        .filter(|c| is_object_stream(&c.value))
        .collect();
    containers.sort_by_key(|c| c.id.num);

    let mut added = 0usize;
    for container in containers {
        let Object::Stream(stream) = &container.value else {
            continue;
        };
        let Some(raw) = stream.data_span.slice(buf) else {
            continue;
        };
        // Reuse the ordinary object-stream machinery (its own §10.1
        // decompression-bomb + `/N` guards apply). A container that fails
        // to decode is skipped, not fatal — recovery is best-effort here.
        let Ok(objstm) = ObjectStream::parse(&stream.dict, raw) else {
            continue;
        };
        for (index, member_num) in objstm.member_numbers().enumerate() {
            // A file-level definition of this number wins.
            if confirmed.contains_key(&member_num) {
                continue;
            }
            // First object stream to claim the number wins.
            if let Entry::Vacant(slot) = entries.entry(member_num) {
                slot.insert(XrefEntry::InStream {
                    stream_num: container.id.num,
                    index: u32::try_from(index).unwrap_or(u32::MAX),
                });
                added += 1;
                if entries.len() > MAX_XREF_ENTRIES {
                    return Err(RecoverError::TooManyEntries);
                }
            }
        }
    }
    Ok(added)
}

/// Recover the trailer dictionary and record where it came from.
///
/// Priority: the last `trailer` keyword → a recovered cross-reference
/// stream's own dictionary → synthesis from a scanned `/Type /Catalog`.
/// The result is normalized to the trailer-semantic keys only.
///
/// # Errors
///
/// [`RecoverError::NoCatalog`] if no `/Root`/`/Catalog` is reachable by any
/// route.
fn recover_trailer(
    buf: &[u8],
    confirmed: &HashMap<u32, Confirmed>,
) -> Result<(Dict, TrailerSource), RecoverError> {
    // 1. The last `trailer` keyword's dictionary (classic files).
    if let Some(dict) = last_trailer_dict(buf)
        && dict.get(b"Root").and_then(Object::as_reference).is_some()
    {
        return Ok((normalize_trailer(&dict), TrailerSource::TrailerKeyword));
    }

    // 2. A recovered cross-reference stream's own dictionary carries the
    //    trailer keys (§7.5.8.1). Prefer one that actually has `/Root`.
    if let Some(dict) = xref_stream_trailer_dict(confirmed) {
        return Ok((normalize_trailer(dict), TrailerSource::XrefStreamDict));
    }

    // 3. Synthesize from a scanned catalog (file-level or compressed).
    if let Some(root) = find_catalog_ref(buf, confirmed) {
        let mut trailer = Dict::new();
        trailer.insert(Name::from(b"Root"), Object::Reference(root));
        // Preserve `/ID` and (for the refusal) `/Encrypt`, plus `/Info`,
        // from any recovered xref-stream dictionary — needed for §7.6 key
        // derivation continuity (R39) and honest metadata.
        if let Some(src) = any_xref_or_trailer_dict(buf, confirmed) {
            for key in [&b"ID"[..], b"Info", b"Encrypt"] {
                if let Some(v) = src.get(key) {
                    trailer.insert(Name::from(key), v.clone());
                }
            }
        }
        return Ok((trailer, TrailerSource::SynthesizedFromCatalog));
    }

    Err(RecoverError::NoCatalog)
}

/// Copy a source dictionary down to just the trailer-semantic keys
/// (`/Root`, `/Size`, `/Info`, `/Encrypt`, `/ID`), dropping the physical
/// cross-reference-stream keys (`/Type`, `/W`, `/Index`, `/Filter`,
/// `/DecodeParms`, `/Length`, `/Prev`, `/XRefStm`).
///
/// A recovered file's base cross-reference was invalid, so §5.6's
/// "never normalize" does not bind it — emitting a clean trailer is the
/// correct, honest output (decision 013 §3.3.4).
fn normalize_trailer(src: &Dict) -> Dict {
    let mut out = Dict::new();
    for key in [&b"Root"[..], b"Size", b"Info", b"Encrypt", b"ID"] {
        if let Some(v) = src.get(key) {
            out.insert(Name::from(key), v.clone());
        }
    }
    out
}

/// Find the last `trailer` keyword at a token boundary and parse the
/// dictionary that follows it.
fn last_trailer_dict(buf: &[u8]) -> Option<Dict> {
    const KW: &[u8] = b"trailer";
    let mut found = None;
    let mut from = 0usize;
    while let Some(p) = find_bytes(buf, from, KW) {
        from = p + KW.len();
        let before_ok = p == 0
            || buf
                .get(p - 1)
                .is_some_and(|&b| is_whitespace(b) || is_delimiter(b));
        let after_ok = !buf.get(p + KW.len()).is_some_and(|&b| is_regular(b));
        if before_ok && after_ok {
            found = Some(p + KW.len());
        }
    }
    match Parser::at(buf, found?).parse_object() {
        Ok(Object::Dict(d)) => Some(d),
        _ => None,
    }
}

/// A recovered cross-reference **stream** object's own dictionary, if one
/// carries `/Root` — the trailer for a pure xref-stream file whose stream
/// data was unreadable but whose Table 17 dictionary parsed.
fn xref_stream_trailer_dict(confirmed: &HashMap<u32, Confirmed>) -> Option<&Dict> {
    // Deterministic: highest object number wins (the newest xref stream).
    let mut candidates: Vec<&Confirmed> = confirmed
        .values()
        .filter(|c| {
            dict_of(&c.value).is_some_and(|d| {
                d.get(b"Type").and_then(Object::as_name).map(Name::as_bytes) == Some(b"XRef")
                    && d.get(b"Root").and_then(Object::as_reference).is_some()
            })
        })
        .collect();
    candidates.sort_by_key(|c| c.id.num);
    candidates.last().and_then(|c| dict_of(&c.value))
}

/// Any recovered cross-reference-stream (or the parsed trailer) dictionary,
/// as a donor for `/ID`/`/Info`/`/Encrypt` during synthesis.
fn any_xref_or_trailer_dict<'a>(
    buf: &'a [u8],
    confirmed: &'a HashMap<u32, Confirmed>,
) -> Option<Dict> {
    if let Some(d) = confirmed.values().find_map(|c| {
        dict_of(&c.value).filter(|d| {
            d.get(b"Type").and_then(Object::as_name).map(Name::as_bytes) == Some(b"XRef")
        })
    }) {
        return Some(d.clone());
    }
    last_trailer_dict(buf)
}

/// Find a document catalog by any route: a scanned file-level object, else
/// a compressed member of a recovered object stream.
fn find_catalog_ref(buf: &[u8], confirmed: &HashMap<u32, Confirmed>) -> Option<ObjId> {
    // File-level: highest object number wins (deterministic).
    let mut file_level: Vec<&Confirmed> = confirmed
        .values()
        .filter(|c| is_catalog(&c.value))
        .collect();
    file_level.sort_by_key(|c| c.id.num);
    if let Some(c) = file_level.last() {
        return Some(c.id);
    }

    // Compressed: scan object-stream members (§7.5.7 permits the catalog in
    // an object stream except in linearized files).
    let mut containers: Vec<&Confirmed> = confirmed
        .values()
        .filter(|c| is_object_stream(&c.value))
        .collect();
    containers.sort_by_key(|c| c.id.num);
    for container in containers {
        let Object::Stream(stream) = &container.value else {
            continue;
        };
        let Some(raw) = stream.data_span.slice(buf) else {
            continue;
        };
        let Ok(objstm) = ObjectStream::parse(&stream.dict, raw) else {
            continue;
        };
        for index in 0..objstm.len() {
            if let Ok((num, value)) = objstm.object_at(index)
                && is_catalog(&value)
            {
                // Compressed objects are generation 0 (§7.5.7).
                return Some(ObjId::new(num, 0));
            }
        }
    }
    None
}

/// Raise `/Size` to at least `highest + 1`, never lowering it (Table 15:
/// `/Size` is one greater than the highest object number defined).
fn set_size_at_least(trailer: &mut Dict, highest: u32) {
    let needed = i64::from(highest) + 1;
    let current = trailer.get(b"Size").and_then(Object::as_int).unwrap_or(0);
    trailer.insert(Name::from(b"Size"), Object::Integer(current.max(needed)));
}

/// Detect the PDF version by scanning the whole buffer for `%PDF-M.N`
/// (whole-buffer so it survives the offset-start case); a conservative
/// `1.7` default when the file carries no marker at all.
fn detect_version(buf: &[u8]) -> PdfVersion {
    const DEFAULT: PdfVersion = PdfVersion { major: 1, minor: 7 };
    let Some(pos) = find_bytes(buf, 0, b"%PDF-") else {
        return DEFAULT;
    };
    let after = pos + 5;
    let (Some(major), maj_len) = take_u8(buf.get(after..).unwrap_or(&[])) else {
        return DEFAULT;
    };
    if buf.get(after + maj_len) != Some(&b'.') {
        return DEFAULT;
    }
    let (Some(minor), _) = take_u8(buf.get(after + maj_len + 1..).unwrap_or(&[])) else {
        return DEFAULT;
    };
    PdfVersion { major, minor }
}

// --- small byte helpers (checked-slicing per the crate panic-free policy) --

/// Index of the first occurrence of `needle` in `buf` at or after `from`,
/// or `None`. Cumulatively O(n) across a forward scan because callers
/// advance `from` past each hit.
fn find_bytes(buf: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    let hay = buf.get(from..)?;
    if needle.is_empty() || needle.len() > hay.len() {
        return None;
    }
    hay.windows(needle.len())
        .position(|w| w == needle)
        .map(|p| from + p)
}

/// Walk `*p` backward over whitespace bytes; return whether at least one
/// was consumed.
fn skip_ws_back(buf: &[u8], p: &mut usize) -> bool {
    let start = *p;
    while *p > 0 && buf.get(*p - 1).is_some_and(|&b| is_whitespace(b)) {
        *p -= 1;
    }
    *p < start
}

/// Walk `*p` backward over ASCII digits.
fn skip_digits_back(buf: &[u8], p: &mut usize) {
    while *p > 0 && buf.get(*p - 1).is_some_and(|&b| b.is_ascii_digit()) {
        *p -= 1;
    }
}

/// Parse a run of ASCII digits into `u64`, `None` on a non-digit or
/// overflow.
fn parse_uint(bytes: &[u8]) -> Option<u64> {
    if bytes.is_empty() {
        return None;
    }
    let mut v: u64 = 0;
    for &b in bytes {
        let d = b.checked_sub(b'0')?;
        if d > 9 {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u64::from(d))?;
    }
    Some(v)
}

/// Consume a leading run of ASCII digits as a `u8`, returning the value and
/// bytes consumed (mirrors the header probe's `take_u8`; a version
/// component never exceeds a single digit legitimately).
fn take_u8(bytes: &[u8]) -> (Option<u8>, usize) {
    let mut value: u32 = 0;
    let mut consumed = 0usize;
    for &b in bytes {
        if b.is_ascii_digit() {
            value = value.saturating_mul(10).saturating_add(u32::from(b - b'0'));
            consumed += 1;
        } else {
            break;
        }
    }
    if consumed == 0 {
        (None, 0)
    } else {
        (u8::try_from(value).ok(), consumed)
    }
}

/// A dictionary view of an object (plain dict or a stream's dict).
fn dict_of(value: &Object) -> Option<&Dict> {
    value.as_dict()
}

/// Whether an object is a `/Type /Catalog` dictionary (§7.7.2).
fn is_catalog(value: &Object) -> bool {
    dict_of(value)
        .and_then(|d| d.get(b"Type"))
        .and_then(Object::as_name)
        .map(Name::as_bytes)
        == Some(b"Catalog")
}

/// Whether an object is a `/Type /ObjStm` stream (§7.5.7). A container
/// without an explicit `/Type` is tolerated by [`ObjectStream::parse`] but
/// the scan needs a positive signal to enumerate it, so recovery requires
/// the explicit tag here (a scanned object with `/N`+`/First` but no
/// `/Type` is not treated as a container — a conservative choice that
/// avoids mis-enumerating an ordinary stream).
fn is_object_stream(value: &Object) -> bool {
    matches!(value, Object::Stream(_))
        && dict_of(value)
            .and_then(|d| d.get(b"Type"))
            .and_then(Object::as_name)
            .map(Name::as_bytes)
            == Some(b"ObjStm")
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
    use crate::graph::ObjectGraph as _;

    /// The trigger gate: encryption is a named capability gap (no recovery);
    /// every unparseable-xref kind recovers.
    #[test]
    fn recovery_reason_for_gates_encryption_out() {
        assert_eq!(
            recovery_reason_for(&XrefErrorKind::EncryptionUnsupported),
            None
        );
        assert_eq!(
            recovery_reason_for(&XrefErrorKind::NotAnXrefSection),
            Some(RecoveryReason::NotAnXrefSection)
        );
        assert_eq!(
            recovery_reason_for(&XrefErrorKind::StartxrefNotFound),
            Some(RecoveryReason::StartxrefNotFound)
        );
        assert_eq!(
            recovery_reason_for(&XrefErrorKind::BadXrefStream("x")),
            Some(RecoveryReason::BadXrefStream)
        );
    }

    /// The backward header walk locates `N G obj` and rejects `endobj`
    /// (whose `obj` has no preceding whitespace-then-digits).
    #[test]
    fn scan_finds_headers_and_rejects_endobj() {
        let buf = b"%PDF-1.7\n12 0 obj\n<< >>\nendobj\n34 5 obj null endobj\n";
        let headers = scan_object_headers(buf);
        let ids: Vec<(u32, u16)> = headers
            .iter()
            .map(|c| (c.id.num, c.id.generation))
            .collect();
        // Only the two real headers; the `obj` inside `endobj` is rejected.
        assert_eq!(ids, vec![(12, 0), (34, 5)]);
    }

    /// A false-positive `N G obj` sitting inside a byte run (no boundary
    /// before the number) is not proposed.
    #[test]
    fn scan_rejects_number_without_leading_boundary() {
        // `x12 0 obj` — the `12` is glued to a regular byte, so it is not a
        // valid object-number token start.
        let buf = b"x12 0 obj\n";
        assert!(scan_object_headers(buf).is_empty());
    }

    /// Object number 0 (the free-list head) is never proposed as an object.
    #[test]
    fn scan_rejects_object_zero() {
        let buf = b" 0 0 obj\n";
        assert!(scan_object_headers(buf).is_empty());
    }

    /// Version detection scans the whole buffer, so it survives leading junk
    /// (the offset-start case); a headerless buffer defaults conservatively.
    #[test]
    fn detect_version_scans_whole_buffer() {
        assert_eq!(
            detect_version(b"junk...%PDF-2.0\nrest"),
            PdfVersion { major: 2, minor: 0 }
        );
        assert_eq!(
            detect_version(b"no header here"),
            PdfVersion { major: 1, minor: 7 }
        );
    }

    /// A buffer with no confirmable objects fails clean with `NoObjects`
    /// (the header-path "not a PDF" outcome).
    #[test]
    fn recover_on_empty_input_is_no_objects() {
        let err = recover(b"not a pdf at all", RecoveryReason::MissingHeader).unwrap_err();
        assert_eq!(err, RecoverError::NoObjects);
    }

    /// Objects present but no `/Catalog` fails clean with `NoCatalog`, never
    /// a garbage document.
    #[test]
    fn recover_without_catalog_is_no_catalog() {
        let buf = b"%PDF-1.7\n1 0 obj\n<< /Type /Pages /Count 0 /Kids [] >>\nendobj\n";
        let err = recover(buf, RecoveryReason::StartxrefNotFound).unwrap_err();
        assert_eq!(err, RecoverError::NoCatalog);
    }

    /// The corpus's dominant damage shape, in miniature: a classic file
    /// whose `/Length` values were computed for LF line endings and which
    /// was then converted to CRLF. Every stream is now one byte per line
    /// longer than it claims, and every stored offset is stale — which is
    /// why `startxref` no longer lands on `xref` and recovery fires.
    ///
    /// Before the `StreamLengthPolicy::RecoverFromEndstream` confirmation,
    /// object 4 failed to parse and was silently dropped, so the scan
    /// reported one object fewer than the file contains and the page's
    /// `/Contents` resolved to null. This asserts the object is now KEPT,
    /// with the right bytes, and that the repair is disclosed.
    #[test]
    fn recover_keeps_streams_whose_length_predates_crlf_conversion() {
        // Written LF-first, then converted — exactly how the real files
        // were damaged — so the /Length values are honestly stale rather
        // than hand-picked to make the test pass.
        let lf = "%PDF-1.4\n\
                  1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
                  2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 \
                  /MediaBox [0 0 10 10] /Resources << >> >>\nendobj\n\
                  3 0 obj\n<< /Type /Page /Parent 2 0 R /Contents 4 0 R >>\nendobj\n\
                  4 0 obj\n<< /Length 14 >>\nstream\nBT\n(hi) Tj\nET\nendstream\nendobj\n\
                  trailer\n<< /Root 1 0 R /Size 5 >>\nstartxref\n9\n%%EOF\n";
        // `/Length 14` is what the LF form measures: "BT\n(hi) Tj\nET\n".
        // (13 would also be legal — §7.3.8.1's trailing EOL is optional in
        // the count — and either value parses strictly BEFORE conversion,
        // which is the point: the file was VALID until the line endings
        // changed underneath it.)
        assert_eq!("BT\n(hi) Tj\nET\n".len(), 14);
        let buf = lf.replace('\n', "\r\n").into_bytes();

        let rec = recover(&buf, RecoveryReason::NotAnXrefSection).expect("recovery");
        assert_eq!(
            rec.report.file_level_objects, 4,
            "all four objects survive confirmation — the stream is no longer dropped"
        );
        assert_eq!(
            rec.report.stream_lengths_recovered, 1,
            "the one repaired extent is counted for disclosure"
        );

        // And the recovered stream holds the real content, not a truncation.
        let doc = crate::document::Document::from_bytes(buf).expect("load");
        let pages = crate::page_tree::pages(&doc).expect("page tree");
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].contents.len(), 1, "the content stream is present");
        assert_eq!(
            pages[0].contents_unresolved, 0,
            "nothing had to be degraded — the object was actually recovered"
        );
        let Some(Object::Stream(s)) = doc.value(pages[0].contents[0]) else {
            panic!("content stream missing");
        };
        assert_eq!(
            s.data_span.slice(doc.bytes()).unwrap(),
            b"BT\r\n(hi) Tj\r\nET",
            "the extent is re-derived to the real CRLF payload, EOL backed off"
        );
    }

    /// A recovered `/Encrypt` trailer refuses post-rebuild (the §7.6
    /// capability gap re-checked after recovery).
    #[test]
    fn recover_encrypted_refuses() {
        let mut buf: Vec<u8> = b"%PDF-1.7\n".to_vec();
        buf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        buf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n");
        // A `trailer` keyword carrying /Root AND /Encrypt.
        buf.extend_from_slice(b"trailer\n<< /Root 1 0 R /Encrypt 9 0 R /Size 3 >>\n");
        let err = recover(&buf, RecoveryReason::StartxrefNotFound).unwrap_err();
        assert_eq!(err, RecoverError::Encrypted);
    }
}
