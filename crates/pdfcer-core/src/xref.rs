//! # Cross-reference machinery (ISO 32000-1 §7.5.4–§7.5.8)
//!
//! Locates and parses every kind of cross-reference section a PDF can
//! have — classic `xref` tables (§7.5.4), PDF 1.5 cross-reference
//! **streams** (§7.5.8), and the hybrid-reference combination of both
//! (§7.5.8.4) — producing the merged object-number → entry map the
//! document layer resolves through, plus the document's trailer.
//!
//! Spec sources in the PDF-spec RAG: `iso32000__s__7.5.4.md` (the
//! 20-byte entry format, subsections, free-list), `iso32000__s__7.5.5.md`
//! (trailer, `startxref`, `%%EOF`, the nine-step load algorithm),
//! `iso32000__s__7.5.6.md` (incremental updates / most-recent-copy
//! rule), `iso32000__s__7.5.8.md` (Tables 17/18/19 — xref-stream
//! dictionary, entry types, `XRefStm`), `iso32000__s__7.5.7.md` (object
//! streams, the targets of type-2 entries). Clause numbers are
//! ISO 32000-1:2008.
//!
//! ## The chain walk (§7.5.5's load algorithm, steps 1–9)
//!
//! 1. Scan **backward** from EOF for the last `startxref` keyword
//!    (§7.5.5: readers "should read a PDF file from its end"; the
//!    marker may be followed by trailing bytes — the scan window
//!    [`STARTXREF_SCAN_WINDOW`] is pdfcer policy, not spec).
//! 2. Seek to the offset and classify what is there by its first token:
//!    the `xref` keyword → a classic section (§7.5.4); an integer (the
//!    `N` of an `N G obj` header) → a cross-reference **stream**
//!    (§7.5.8.1 amends `startxref` to point at the stream object
//!    itself). Anything else is malformed.
//! 3. Parse the section. Its "trailer" is either the literal `trailer`
//!    dictionary (classic) or the xref stream's own dictionary
//!    (§7.5.8.1: the stream dictionary "carries what a trailer would
//!    carry"). **Both forms may appear at different links of the same
//!    chain** — a file may be updated from classic to stream form or
//!    vice versa, and each link is classified independently.
//! 4. Merge entries **first-wins**: the newest section is parsed first,
//!    and §7.5.6's most-recent-copy rule is exactly "an entry already
//!    established by a newer section is never overwritten."
//! 5. If the section's trailer carries `/XRefStm` this is a
//!    **hybrid-reference file** (§7.5.8.4): parse the cross-reference
//!    stream at that offset and merge it **before** following `/Prev` —
//!    see the dedicated section below.
//! 6. Follow `/Prev` (which despite Table 15's wording MUST be direct —
//!    a flagged ISO 32000-1 defect, see the RAG) and repeat, with a
//!    cycle guard ([`MAX_XREF_SECTIONS`], pdfcer policy).
//! 7. The **newest** trailer is the document's trailer (its `/Root`,
//!    `/Size`, …).
//!
//! `/Size` is then a hard filter (Table 15): entries with object
//! number ≥ `Size` are "ignored and defined to be missing."
//!
//! ## Cross-reference streams (§7.5.8) — what makes bootstrap possible
//!
//! An xref stream is an ordinary indirect stream object whose data,
//! once run through its `/Filter` chain, is a packed array of
//! fixed-width big-endian rows. Decoding it therefore needs `/Filter`,
//! `/DecodeParms` and `/W` **before any cross-reference exists** —
//! which is exactly why §7.5.8.2 requires every Table 17 entry, every
//! element of the `Index`/`W` arrays, and `Filter`/`DecodeParms` to be
//! **direct** objects. pdfcer relies on that: the stream is parsed with
//! a null `/Length` resolver, so an (illegal, unbootstrappable)
//! indirect `/Length` surfaces as a clean
//! [`XrefErrorKind::BadXrefStream`] rather than a wrong parse.
//!
//! Row decoding follows Table 18 exactly:
//!
//! | field 1 (type) | field 2 | field 3 |
//! |---|---|---|
//! | 0 — free | next free object number | generation for reuse |
//! | 1 — in use, uncompressed | byte offset in file | generation (default 0) |
//! | 2 — **compressed** | object stream's object number | index within it |
//!
//! `/W` gives each field's byte width; **a width of 0 means the field
//! is absent and its Table 18 default applies** — and if `W[0]` is 0
//! the type itself defaults to 1. Multi-byte fields are big-endian
//! ("high-order byte first"). Any type value other than 0/1/2 "shall be
//! interpreted as a reference to the null object" — pdfcer records those
//! rows as free entries, which is precisely that semantics (§7.5.4
//! free ⇒ §7.3.10 null) while still letting the row shadow an older
//! section's entry, as a newest-wins entry must.
//!
//! ## Hybrid-reference files (§7.5.8.4) — the search-order rule
//!
//! A hybrid file has a classic table (so a pre-1.5 reader can open it)
//! *plus* an xref stream, named by the trailer's `/XRefStm`, that
//! defines additional objects the classic table deliberately marks
//! **free**. The operative reader rule is:
//!
//! > if an entry is not found in any given standard cross-reference
//! > section, the search shall proceed to a cross-reference stream
//! > specified by the `XRefStm` entry **before** looking in the
//! > previous cross-reference section (the `Prev` entry).
//!
//! Implemented as a merge, that is: for each link of the chain, merge
//! `classic entries`, then `XRefStm entries`, then move to `/Prev`.
//! First-wins insertion then reproduces the search order exactly — the
//! `XRefStm` stream cannot override its own section's classic table,
//! but it does override everything older, which is what makes hidden
//! objects visible to a 1.5+ reader and invisible (free ⇒ null) to a
//! 1.4 reader.
//!
//! Two deliberate refinements, both spec-sourced:
//!
//! - The `XRefStm` stream's own `/Prev` is **not** followed. Table 17
//!   states `Prev` is "not meaningful in hybrid-reference files"; chain
//!   continuation belongs to the *trailer's* `/Prev`, and the
//!   `XRefStm` target is a leaf lookup, not a chain link.
//! - A **broken `XRefStm` is not fatal.** §7.5.8.4 guarantees that
//!   everything reachable from the root is visible in the classic
//!   tables (the catalog, page tree, fonts and anything required "shall
//!   not be hidden"), so ignoring an unparseable `XRefStm` degrades
//!   exactly to what a conforming pre-1.5 reader sees — a documented,
//!   safe fallback rather than a guess. A broken *primary* section is
//!   still fatal.
//!
//! ## Encryption is detected here and handled one layer up (§7.6)
//!
//! If the newest trailer has `/Encrypt`, strings and stream data in the
//! file are encrypted and every downstream layer would silently decode
//! garbage.
//!
//! **This module used to refuse such a document outright.** It no longer
//! does: [`crate::crypto`] implements the `/Standard` handler for RC4, so
//! the decision is no longer "encrypted or not" but "which configuration,
//! and does a password open it" — and neither question can be answered
//! from the cross-reference layer, which has no objects yet and so cannot
//! resolve an indirect `/O`, `/U` or `/CF`.
//!
//! So this layer *detects* and reports; [`crate::document::Document`]
//! decides. [`XrefErrorKind::EncryptionUnsupported`] survives for the one
//! case that genuinely belongs here — a document whose cross-reference
//! machinery is broken **and** which is encrypted, where rebuild-by-scan
//! would have to parse ciphertext to find objects (see
//! [`crate::recover`]).
//!
//! ## Entry format enforced strictly (§7.5.4)
//!
//! Classic entries are exactly 20 bytes: 10-digit offset, SP, 5-digit
//! generation, SP, `n`/`f`, and a 2-byte EOL that is one of `SP CR`,
//! `SP LF`, `CR LF`. Real-world 19/21-byte deviants exist; pdfcer
//! refuses them (fail-clean) — tolerance is a later, corpus-evidenced
//! addition recorded in `C:\personal_rag\pdf\` first.

use std::collections::HashMap;

use crate::filters::{self, FilterError};
use crate::object::{Dict, ObjId, Object};
use crate::parser::{ParseError, Parser};
use crate::span::ByteSpan;

/// How many trailing bytes are scanned backward for the last
/// `startxref` keyword.
///
/// pdfcer policy, not spec: §7.5.5 places `startxref` on the
/// third-to-last line but says nothing about trailing bytes after
/// `%%EOF`, and real files accumulate them. 4 KiB is far beyond any
/// legitimate trailer tail while keeping the scan O(1) in file size.
pub const STARTXREF_SCAN_WINDOW: usize = 4096;

/// Maximum number of cross-reference sections followed through `/Prev`
/// before the chain is declared cyclic/hostile.
///
/// pdfcer policy (ARCHITECTURE.md §10): each legitimate section is one
/// incremental update; a document with thousands is implausible, and an
/// unguarded `Prev` cycle is an infinite loop.
pub const MAX_XREF_SECTIONS: usize = 1024;

/// Maximum total xref entries accepted across all sections.
///
/// pdfcer policy (ARCHITECTURE.md §10.1): bounds allocation against a
/// hostile subsection header claiming billions of entries. 10 million
/// objects is far beyond any legitimate document.
pub const MAX_XREF_ENTRIES: usize = 10_000_000;

/// Maximum byte width accepted for a single `/W` field of a
/// cross-reference stream (§7.5.8.2).
///
/// pdfcer policy: the spec puts no ceiling on `W`'s elements, but a
/// field wider than 8 bytes cannot be represented in the `u64` the
/// decoder reads into, and no legitimate producer emits one (real files
/// use 1–4). A wider field is malformed, not merely large.
pub const MAX_W_FIELD_WIDTH: usize = 8;

/// Maximum total row width (the sum of `/W`) accepted for a
/// cross-reference stream.
///
/// pdfcer policy: bounds the per-row read for a hostile `W` array such
/// as `[8 8 8 8 8 …]`. PDF 1.5's `W` "always contains three integers"
/// and 32 bytes is four maximal fields — far past anything real.
pub const MAX_XREF_STREAM_ROW: usize = 32;

/// One cross-reference entry: §7.5.4's classic `n`/`f` types plus
/// §7.5.8.3 Table 18's type-2 compressed-object entry.
///
/// `#[non_exhaustive]`: §7.5.8.3 explicitly reserves further entry
/// types ("any other value shall be interpreted as a reference to the
/// null object, thus permitting new entry types to be defined in the
/// future"), so downstream matches must stay open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum XrefEntry {
    /// Classic type `n` / stream type **1**: the object lives at
    /// `offset` (from the beginning of the file) with generation
    /// `generation`.
    InUse {
        /// Byte offset of the object's `N G obj` header.
        offset: u64,
        /// Generation that a reference must match to resolve.
        generation: u16,
    },
    /// Classic type `f` / stream type **0**: the object number is free.
    /// Kept (not dropped!) — free-with-generation-65535 entries are how
    /// hybrid files hide objects, and a free entry shadows an older
    /// in-use entry during the first-wins merge (deletion via
    /// incremental update).
    Free {
        /// Object number of the next free object (the linked list).
        next_free: u32,
        /// Generation to assign when this number is next reused.
        generation: u16,
    },
    /// Stream type **2** (§7.5.8.3 Table 18): the object is
    /// **compressed** inside an object stream (§7.5.7).
    ///
    /// Note what this variant deliberately does *not* carry: a
    /// generation. A type-2 entry has no generation field at all — its
    /// field 3 is the index within the container — because §7.3.10/
    /// §7.5.7 fix the generation of every compressed object, and of the
    /// container itself, at **0**. Storing a generation here would
    /// invent information the file does not contain.
    InStream {
        /// Object number of the object stream holding this object. Its
        /// generation is implicitly 0.
        stream_num: u32,
        /// 0-based index of this object within that object stream's
        /// pair table.
        index: u32,
    },
}

/// The merged cross-reference map for a document: object number →
/// newest entry, plus the newest trailer.
#[derive(Debug, Clone, Default)]
pub struct XrefTable {
    /// Object number → the entry from the NEWEST section that defines
    /// it (§7.5.6 most-recent-copy, enforced by first-wins insertion
    /// during the newest-to-oldest walk).
    entries: HashMap<u32, XrefEntry>,
}

impl XrefTable {
    /// Build a table directly from a synthesized object-number → entry
    /// map — the cross-reference **recovery** path (`crate::recover`).
    ///
    /// The normal load path never uses this: it fills `entries` by walking
    /// real cross-reference sections ([`load_xref_chain`]). Recovery, by
    /// contrast, *reconstructs* the map from a full-file object scan when
    /// no trustworthy section exists (decision 013 §3.3), then hands the
    /// document layer a table shaped exactly as a normal load would have
    /// produced — so every downstream layer (object parse, §7.5.7
    /// resolution, the writer) is unchanged. `pub(crate)` because building
    /// an arbitrary table outside the crate would let a caller assert
    /// cross-references the bytes do not support.
    #[must_use]
    pub(crate) fn from_entries(entries: HashMap<u32, XrefEntry>) -> Self {
        Self { entries }
    }

    /// The newest entry for `num`, if any section defined one (and it
    /// survived the `/Size` filter).
    #[must_use]
    pub fn get(&self, num: u32) -> Option<XrefEntry> {
        self.entries.get(&num).copied()
    }

    /// Iterate `(object number, entry)` pairs (unordered).
    pub fn iter(&self) -> impl Iterator<Item = (u32, XrefEntry)> + '_ {
        self.entries.iter().map(|(&n, &e)| (n, e))
    }

    /// Number of merged entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the table is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A cross-reference-level structural error.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("xref error at byte {offset}: {kind}")]
pub struct XrefError {
    /// Byte offset where the problem was detected.
    pub offset: usize,
    /// What was wrong.
    pub kind: XrefErrorKind,
}

/// Classification of cross-reference errors.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum XrefErrorKind {
    /// No `startxref` keyword found in the trailing scan window.
    #[error("no startxref found in the last {STARTXREF_SCAN_WINDOW} bytes")]
    StartxrefNotFound,
    /// `startxref` found but not followed by a usable integer offset,
    /// or the offset points outside the file.
    #[error("startxref offset missing or out of range")]
    BadStartxrefOffset,
    /// Neither an `xref` keyword nor an `N G obj` header at the target
    /// offset — the section cannot be classified at all (§7.5.5 step 3
    /// / §7.5.8.1).
    #[error("expected an xref keyword or a cross-reference stream object at this offset")]
    NotAnXrefSection,
    /// A cross-reference stream (§7.5.8) was located but its
    /// dictionary, `/W`, `/Index` or packed data violated §7.5.8.2/
    /// §7.5.8.3. The payload is a static description of which rule.
    #[error("malformed cross-reference stream: {0}")]
    BadXrefStream(&'static str),
    /// The cross-reference stream's `/Filter` chain failed to decode
    /// (§7.5.8.2 forbids a `Crypt` filter here, so this is genuinely a
    /// damaged or unsupported-filter stream).
    #[error("cross-reference stream could not be decoded: {0}")]
    XrefStreamDecode(#[from] FilterError),
    /// The document is encrypted (§7.6) **and** its cross-reference
    /// machinery could not be read, so rebuild-by-scan would have to find
    /// objects in ciphertext.
    ///
    /// **No longer raised for a merely encrypted document.** Since
    /// [`crate::crypto`] implements the `/Standard` handler, an encrypted
    /// file with an intact cross-reference table is loaded and decrypted
    /// (or refused with a *specific* reason — a named cipher, a named
    /// handler, a password prompt). This variant now means exactly what its
    /// name says in the recovery context: encryption that blocks
    /// *recovery*, raised by [`crate::document`] when
    /// [`crate::recover::RecoverError::Encrypted`] comes back.
    #[error(
        "encrypted documents (\u{a7}7.6) cannot be recovered by scanning: object bodies are ciphertext"
    )]
    EncryptionUnsupported,
    /// A subsection header line wasn't `first count` (two
    /// non-negative integers).
    #[error("malformed xref subsection header")]
    BadSubsectionHeader,
    /// An entry violated the exact 20-byte §7.5.4 format.
    #[error("malformed 20-byte xref entry")]
    BadEntry,
    /// Total entries exceeded [`MAX_XREF_ENTRIES`] (pdfcer guard).
    #[error("xref entry count exceeds MAX_XREF_ENTRIES")]
    TooManyEntries,
    /// `trailer` keyword or its dictionary missing/malformed after a
    /// section's subsections.
    #[error("missing or malformed trailer: {0}")]
    BadTrailer(&'static str),
    /// The `/Prev` chain revisited an offset or exceeded
    /// [`MAX_XREF_SECTIONS`] (cycle guard, pdfcer policy).
    #[error("xref /Prev chain is cyclic or too long")]
    PrevChainCycle,
    /// Structural parse failure inside the trailer dictionary.
    #[error(transparent)]
    Parse(#[from] ParseError),
}

impl XrefError {
    const fn new(offset: usize, kind: XrefErrorKind) -> Self {
        Self { offset, kind }
    }
}

/// The physical form of one cross-reference section — the fact a
/// minimal-diff writer needs in order to **match** rather than
/// normalize (`ARCHITECTURE.md` §5; decision 007 R33).
///
/// ## Why this is load-bearing and not diagnostic trivia
///
/// §7.5.6 nowhere requires an appended update section to use the same
/// form as the section it supersedes (recorded as a NEGATIVE RESULT in
/// `iso32000__s__7.5.6.md`). A writer is therefore *free* to answer a
/// classic `xref` table with an appended cross-reference stream — and
/// doing so is precisely the "silent normalization" failure decision
/// 007 W4 names: the result is a plausible, working, WRONG file that
/// silently raises a PDF 1.4 document's effective version to 1.5.
///
/// pdfcer's rule is to emit whatever the base file's **newest** section
/// already used, which means the load path has to remember it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SectionShape {
    /// A classic §7.5.4 `xref` table with a §7.5.5 `trailer` dictionary.
    Classic {
        /// The section's `/XRefStm` offset (§7.5.8.4 Table 19) when the
        /// file is **hybrid-reference**. `Some` here is what makes an
        /// append a form-A hybrid append: §7.5.6 requirement 3 obliges
        /// the new trailer to carry every previous-trailer entry except
        /// `Prev`, and `/XRefStm` is such an entry.
        xref_stm: Option<u64>,
    },
    /// A §7.5.8 cross-reference **stream**: an ordinary indirect stream
    /// object that also carries the trailer's keys.
    Stream {
        /// The stream object's own identifier. A new section written in
        /// this form re-uses this object number, because the base file
        /// already spends it on exactly this role and allocating a fresh
        /// one would grow `/Size` for no reason.
        id: ObjId,
        /// The base stream's `/W` field widths. Re-used verbatim when
        /// the new section's values still fit (they usually do); widened
        /// only when an offset no longer fits, which is a value change
        /// forced by the data, not a normalization.
        widths: [usize; 3],
    },
}

/// Result of a successful chain load: the merged table, the newest
/// trailer dictionary, and the physical facts a minimal-diff writer
/// needs about the newest section.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct LoadedXref {
    /// Merged object-number → entry map (newest wins, `/Size`-filtered).
    pub table: XrefTable,
    /// The trailer dictionary of the NEWEST section — the one whose
    /// `/Root`/`/Info`/`/Encrypt` govern the document (§7.5.5 load
    /// algorithm step 9).
    pub trailer: Dict,
    /// The byte offset the file's own `startxref` names (§7.5.5).
    ///
    /// This is the **exact value an appended update's `/Prev` must
    /// carry** — §7.5.6 Q2: "the location of the previous
    /// cross-reference section", i.e. the section this update
    /// supersedes, which is the one `startxref` currently points at.
    /// Deriving it from the base trailer's own `/Prev` instead is the
    /// classic off-by-one-revision bug.
    pub startxref: u64,
    /// The physical form of the newest section — see [`SectionShape`].
    pub newest_shape: SectionShape,
    /// The highest object number the chain mentions **before** the
    /// `/Size` filter runs.
    ///
    /// ## Why the unfiltered number has to be kept
    ///
    /// `/Size` is a hard reader-side filter (Table 15, §7.5.5), so a
    /// file whose `/Size` under-reports loads with those entries
    /// *invisible* — and a writer that allocates a "new" object number
    /// from the filtered view will happily pick a number the file
    /// already defines. The result is an update section whose entry
    /// collides with a live one, which is the worst failure shape
    /// available: a file that looks fine and resolves an object to the
    /// wrong bytes.
    ///
    /// Found by the `writer_roundtrip` fuzz target on a corpus file
    /// carrying `/Size 3` over six real entries.
    pub highest_object_number: u32,
    /// How many entries the `/Size` filter removed.
    ///
    /// Non-zero means the file is **hiding** cross-reference entries
    /// behind an under-reported `/Size`. That matters to the writer
    /// beyond diagnostics: raising `/Size` — which any object creation
    /// does — would *expose* every one of them, resurrecting objects
    /// the operator never touched and which may not even parse. See
    /// [`crate::document::Document::suppressed_object_count`].
    pub suppressed_by_size: usize,
}

/// Locate `startxref` (§7.5.5) and load the full cross-reference chain
/// from `buf` — classic tables, cross-reference streams, and
/// hybrid-reference `XRefStm` lookups, in the spec's search order.
///
/// See the module docs for the step-by-step mapping onto §7.5.5's load
/// algorithm, the §7.5.8.4 hybrid rule, and the deliberate
/// encryption refusal.
///
/// # Errors
///
/// [`XrefError`] — see [`XrefErrorKind`] for every case.
pub fn load_xref_chain(buf: &[u8]) -> Result<LoadedXref, XrefError> {
    let first_offset = find_startxref(buf)?;

    let mut table = XrefTable::default();
    let mut newest_trailer: Option<Dict> = None;
    let mut newest_shape: Option<SectionShape> = None;
    let mut visited: Vec<usize> = Vec::new();
    let mut next_offset = Some(first_offset);

    while let Some(offset) = next_offset {
        if visited.contains(&offset) || visited.len() >= MAX_XREF_SECTIONS {
            return Err(XrefError::new(offset, XrefErrorKind::PrevChainCycle));
        }
        visited.push(offset);

        let section = parse_section_at(buf, offset, table.entries.len())?;
        merge_first_wins(&mut table, section.entries);

        // §7.5.8.4 hybrid-reference lookup: the `XRefStm` stream is
        // consulted BEFORE `/Prev`, and merged first-wins so it can
        // override older sections but not this section's own classic
        // table. See the module docs for why a failure here is
        // non-fatal and why its own `/Prev` is not followed.
        if let Some(Object::Integer(v)) = section.trailer.get(b"XRefStm")
            && let Ok(stm_offset) = usize::try_from(*v)
            && stm_offset < buf.len()
            && !visited.contains(&stm_offset)
            && visited.len() < MAX_XREF_SECTIONS
        {
            visited.push(stm_offset);
            if let Ok(hidden) = parse_xref_stream_section(buf, stm_offset, table.entries.len()) {
                merge_first_wins(&mut table, hidden.entries);
            }
        }

        // `/Prev` — required-direct (Table 15 defect noted in module
        // docs); a non-integer is malformed.
        next_offset = match section.trailer.get(b"Prev") {
            None => None,
            Some(Object::Integer(v)) => match usize::try_from(*v) {
                Ok(o) if o < buf.len() => Some(o),
                _ => {
                    return Err(XrefError::new(
                        offset,
                        XrefErrorKind::BadTrailer("Prev offset out of range"),
                    ));
                }
            },
            Some(_) => {
                return Err(XrefError::new(
                    offset,
                    XrefErrorKind::BadTrailer("Prev is not a direct integer"),
                ));
            }
        };

        if newest_trailer.is_none() {
            newest_shape = Some(section.shape);
            newest_trailer = Some(section.trailer);
        }
    }

    let trailer = newest_trailer.ok_or(XrefError::new(
        first_offset,
        XrefErrorKind::NotAnXrefSection,
    ))?;
    let newest_shape = newest_shape.ok_or(XrefError::new(
        first_offset,
        XrefErrorKind::NotAnXrefSection,
    ))?;

    // §7.6: encryption is NOT refused here any more. The trailer is where
    // the fact is discovered, but not where it can be acted on — resolving
    // an `/O`, `/U` or `/CF` entry that is an indirect reference needs
    // objects, and there are none yet. `Document::assemble` authenticates
    // and decrypts; see `crate::crypto`.

    // Captured BEFORE the filter: the writer needs to know what the
    // file physically mentions, not what a reader is allowed to see.
    let highest_object_number = table.entries.keys().copied().max().unwrap_or(0);
    let before_filter = table.entries.len();

    // `/Size` hard filter (Table 15): entries numbered >= Size are
    // "ignored and defined to be missing."
    if let Some(Object::Integer(size)) = trailer.get(b"Size")
        && let Ok(size) = u32::try_from(*size)
    {
        table.entries.retain(|&num, _| num < size);
    }
    let suppressed_by_size = before_filter.saturating_sub(table.entries.len());

    Ok(LoadedXref {
        table,
        trailer,
        highest_object_number,
        suppressed_by_size,
        startxref: first_offset as u64,
        newest_shape,
    })
}

/// Merge one section's entries into `table` **first-wins**: an entry
/// already established by a newer section (or by this section's own
/// classic table, when merging a hybrid `XRefStm`) is never
/// overwritten. That single rule implements both §7.5.6's
/// most-recent-copy semantics and §7.5.8.4's search order — see the
/// module docs.
fn merge_first_wins(table: &mut XrefTable, entries: Vec<(u32, XrefEntry)>) {
    for (num, entry) in entries {
        table.entries.entry(num).or_insert(entry);
    }
}

/// Scan the trailing [`STARTXREF_SCAN_WINDOW`] bytes for the LAST
/// `startxref` keyword and parse the offset on the following line.
///
/// "Last" matters: each incremental update appends its own
/// `startxref`/`%%EOF` (§7.5.6), and the newest one wins (§7.5.5).
fn find_startxref(buf: &[u8]) -> Result<usize, XrefError> {
    const KEYWORD: &[u8] = b"startxref";
    let window_start = buf.len().saturating_sub(STARTXREF_SCAN_WINDOW);
    let window = buf.get(window_start..).unwrap_or(buf);

    let pos_in_window = window
        .windows(KEYWORD.len())
        .rposition(|w| w == KEYWORD)
        .ok_or(XrefError::new(buf.len(), XrefErrorKind::StartxrefNotFound))?;
    let after_kw = window_start + pos_in_window + KEYWORD.len();

    // The offset is the next token; lexing is exactly right here (it
    // skips the EOL and reads the integer).
    let mut lx = crate::lexer::Lexer::at(buf, after_kw);
    let offset = match lx.next_token() {
        Ok(Some(t)) => match t.kind {
            crate::lexer::TokenKind::Integer(v) => usize::try_from(v).ok(),
            _ => None,
        },
        _ => None,
    };
    match offset {
        Some(o) if o < buf.len() => Ok(o),
        _ => Err(XrefError::new(after_kw, XrefErrorKind::BadStartxrefOffset)),
    }
}

/// One parsed cross-reference section: its entries and the dictionary
/// that plays the trailer role for it — the literal `trailer` dict for
/// a classic section, or the stream dictionary for a cross-reference
/// stream (§7.5.8.1).
struct Section {
    entries: Vec<(u32, XrefEntry)>,
    trailer: Dict,
    /// The section's physical form — carried up to [`LoadedXref`] for
    /// the newest section only, where the writer needs it (R33).
    shape: SectionShape,
}

/// Classify and parse one cross-reference section at `offset`
/// (§7.5.5 load-algorithm step 3).
///
/// Classification is by the first token, per §7.5.5/§7.5.8.1: the
/// `xref` keyword means a classic section; an integer means the start
/// of an `N G obj` header, i.e. a cross-reference stream. Each link of
/// a `/Prev` chain is classified independently, because a file may
/// legitimately mix the two forms across incremental updates.
///
/// `already_merged` feeds the [`MAX_XREF_ENTRIES`] guard across the
/// whole chain, not per-section.
fn parse_section_at(
    buf: &[u8],
    offset: usize,
    already_merged: usize,
) -> Result<Section, XrefError> {
    let mut lx = crate::lexer::Lexer::at(buf, offset);
    let first = match lx.next_token() {
        Ok(Some(t)) => t,
        _ => return Err(XrefError::new(offset, XrefErrorKind::NotAnXrefSection)),
    };
    match first.kind {
        crate::lexer::TokenKind::Keyword if first.span.slice(buf) == Some(b"xref") => {}
        crate::lexer::TokenKind::Integer(_) => {
            return parse_xref_stream_section(buf, offset, already_merged);
        }
        _ => return Err(XrefError::new(offset, XrefErrorKind::NotAnXrefSection)),
    }

    // After `xref`: an EOL, then subsections. Entry parsing is BYTE
    // EXACT (20-byte records), so from here the code works on raw
    // offsets, using the lexer only for the variable-width subsection
    // headers and the trailer.
    let mut cursor = first.span.end();
    let mut entries: Vec<(u32, XrefEntry)> = Vec::new();

    loop {
        // Peek the next token to decide: subsection header (integer),
        // or `trailer`.
        let mut peek_lx = crate::lexer::Lexer::at(buf, cursor);
        let tok = match peek_lx.next_token() {
            Ok(Some(t)) => t,
            _ => {
                return Err(XrefError::new(
                    cursor,
                    XrefErrorKind::BadTrailer("input ended inside xref section"),
                ));
            }
        };

        match tok.kind {
            crate::lexer::TokenKind::Keyword if tok.span.slice(buf) == Some(b"trailer") => {
                // Parse the trailer dictionary with the object parser.
                let mut parser = Parser::at(buf, tok.span.end());
                let dict = match parser.parse_object() {
                    Ok(Object::Dict(d)) => d,
                    Ok(_) => {
                        return Err(XrefError::new(
                            tok.span.end(),
                            XrefErrorKind::BadTrailer("trailer value is not a dictionary"),
                        ));
                    }
                    Err(e) => return Err(XrefError::new(e.offset, XrefErrorKind::Parse(e))),
                };
                // §7.5.8.4 Table 19: `/XRefStm` marks this a
                // hybrid-reference section. Captured here (rather than
                // re-read from the trailer later) so the writer's
                // form-A append has the offset without re-deriving it.
                let xref_stm = dict
                    .get(b"XRefStm")
                    .and_then(Object::as_int)
                    .and_then(|v| u64::try_from(v).ok());
                return Ok(Section {
                    entries,
                    trailer: dict,
                    shape: SectionShape::Classic { xref_stm },
                });
            }
            crate::lexer::TokenKind::Integer(first_num) => {
                // Subsection header: `first count`.
                let count_tok = match peek_lx.next_token() {
                    Ok(Some(t)) => t,
                    _ => {
                        return Err(XrefError::new(cursor, XrefErrorKind::BadSubsectionHeader));
                    }
                };
                let crate::lexer::TokenKind::Integer(count) = count_tok.kind else {
                    return Err(XrefError::new(
                        count_tok.span.start,
                        XrefErrorKind::BadSubsectionHeader,
                    ));
                };
                let (Ok(first_num), Ok(count)) = (u32::try_from(first_num), usize::try_from(count))
                else {
                    return Err(XrefError::new(
                        tok.span.start,
                        XrefErrorKind::BadSubsectionHeader,
                    ));
                };
                if already_merged + entries.len() + count > MAX_XREF_ENTRIES {
                    return Err(XrefError::new(
                        tok.span.start,
                        XrefErrorKind::TooManyEntries,
                    ));
                }

                // Entries begin after the header line's EOL. §7.5.4's
                // fixed format starts each entry at a known offset;
                // skip the single EOL (CR, LF, or CRLF) after the
                // count token.
                let mut entry_pos = skip_one_eol(buf, count_tok.span.end());
                for k in 0..count {
                    let record = buf
                        .get(entry_pos..entry_pos + 20)
                        .ok_or(XrefError::new(entry_pos, XrefErrorKind::BadEntry))?;
                    let span = ByteSpan::new(entry_pos, 20);
                    let entry = parse_entry(record)
                        .ok_or(XrefError::new(span.start, XrefErrorKind::BadEntry))?;
                    // Object number = subsection first + index; a
                    // number defined twice IN THE SAME SECTION is
                    // spec-invalid but across subsections we just take
                    // the first occurrence (consistent with the
                    // section-level "shall not" being writer-side).
                    let num = first_num.saturating_add(u32::try_from(k).unwrap_or(u32::MAX));
                    if !entries.iter().any(|(n, _)| *n == num) {
                        entries.push((num, entry));
                    }
                    entry_pos += 20;
                }
                cursor = entry_pos;
            }
            _ => {
                return Err(XrefError::new(
                    tok.span.start,
                    XrefErrorKind::BadTrailer("expected subsection header or trailer keyword"),
                ));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Cross-reference streams (§7.5.8)
// ---------------------------------------------------------------------------

/// The resolved `/W` field-width specification of one cross-reference
/// stream (§7.5.8.2).
#[derive(Debug, Clone, Copy)]
struct WidthSpec {
    /// Byte widths of Table 18's fields 1, 2 and 3. A `0` means the
    /// field is **absent from the data** and its Table 18 default
    /// applies; it consumes no bytes.
    fields: [usize; 3],
    /// Total bytes per row — the sum of **every** element of `/W`,
    /// including any beyond the third. Spec: "the sum of the items =
    /// total length of each entry", and §7.5.8.3 says fields are
    /// written in increasing field order, so trailing fields pdfcer does
    /// not interpret still occupy their bytes and must be skipped.
    row: usize,
}

/// Parse one cross-reference stream section (§7.5.8) at `offset`: the
/// indirect stream object, its Table 17 dictionary, and the packed
/// entry rows in its decoded data.
///
/// The returned [`Section::trailer`] is the stream's own dictionary —
/// §7.5.8.1: it "carries what a trailer would carry", so `/Root`,
/// `/Size`, `/Prev`, `/Info`, `/Encrypt` and `/ID` are read from it
/// exactly as from a classic `trailer` dict.
fn parse_xref_stream_section(
    buf: &[u8],
    offset: usize,
    already_merged: usize,
) -> Result<Section, XrefError> {
    // The null `/Length` resolver is correct-by-spec, not a shortcut:
    // §7.5.8.2 makes every entry a reader needs in order to decode the
    // xref direct, precisely because no xref exists yet. A file with an
    // indirect `/Length` on its xref stream is unbootstrappable, and
    // surfaces here as a parse error rather than a wrong guess.
    let io = Parser::at(buf, offset)
        .parse_indirect_object(&mut |_| None)
        .map_err(|e| XrefError::new(e.offset, XrefErrorKind::Parse(e)))?;
    let Object::Stream(stream) = io.value else {
        return Err(XrefError::new(
            offset,
            XrefErrorKind::BadXrefStream("startxref target is not a stream object"),
        ));
    };
    let dict = stream.dict;

    // Table 17: `/Type` shall be `/XRef`. A *wrong* type means the
    // offset led somewhere else entirely and nothing here can be
    // trusted; a *missing* type is tolerated, because `/W` + `/Size`
    // are what actually drive decoding and refusing on the strength of
    // a redundant tag would reject files that decode perfectly. The
    // deviation is deliberate and recorded here.
    if let Some(ty) = dict.get(b"Type")
        && ty.as_name().map(crate::object::Name::as_bytes) != Some(b"XRef")
    {
        return Err(XrefError::new(
            offset,
            XrefErrorKind::BadXrefStream("/Type is present but is not /XRef"),
        ));
    }

    let raw = stream.data_span.slice(buf).ok_or(XrefError::new(
        offset,
        XrefErrorKind::BadXrefStream("stream data span outside the buffer"),
    ))?;
    let data = filters::decode_stream(&dict, raw)
        .map_err(|e| XrefError::new(offset, XrefErrorKind::XrefStreamDecode(e)))?;

    let spec = width_spec(&dict).ok_or(XrefError::new(
        offset,
        XrefErrorKind::BadXrefStream("/W missing, not a direct integer array, or out of range"),
    ))?;

    // Table 15/17 `/Size`: required, direct. It is also the default
    // `/Index` upper bound.
    let size = dict
        .get(b"Size")
        .and_then(Object::as_int)
        .and_then(|v| u32::try_from(v).ok())
        .ok_or(XrefError::new(
            offset,
            XrefErrorKind::BadXrefStream("/Size missing or not a direct non-negative integer"),
        ))?;

    let index = index_pairs(&dict, size).ok_or(XrefError::new(
        offset,
        XrefErrorKind::BadXrefStream("/Index is not an array of direct integer pairs"),
    ))?;

    let mut entries: Vec<(u32, XrefEntry)> = Vec::new();
    let mut pos = 0usize;
    for (first_num, count) in index {
        if already_merged
            .saturating_add(entries.len())
            .saturating_add(count)
            > MAX_XREF_ENTRIES
        {
            return Err(XrefError::new(offset, XrefErrorKind::TooManyEntries));
        }
        for k in 0..count {
            let row = data
                .get(pos..)
                .and_then(|rest| rest.get(..spec.row))
                .ok_or(XrefError::new(
                    offset,
                    XrefErrorKind::BadXrefStream("decoded data is shorter than /Index requires"),
                ))?;
            pos = pos.saturating_add(spec.row);
            let num = first_num.saturating_add(u32::try_from(k).unwrap_or(u32::MAX));
            // A row whose field values fall outside the spec's ranges
            // (a generation above 65,535, an object number above
            // `u32`) describes no representable object; it is dropped
            // rather than failing the load, exactly as §7.5.8.3's
            // unknown-type rule drops forward-incompatible rows.
            if let Some(entry) = decode_row(row, spec)
                && !entries.iter().any(|(n, _)| *n == num)
            {
                entries.push((num, entry));
            }
        }
    }

    // Field widths beyond the third are not pdfcer's to reproduce (PDF
    // 1.5's `/W` "always contains three integers"); the first three are
    // what a re-emitted section needs. A shorter `/W` reads as
    // zero-width trailing fields, which is exactly Table 17's rule.
    let widths = [
        spec.fields.first().copied().unwrap_or(0),
        spec.fields.get(1).copied().unwrap_or(0),
        spec.fields.get(2).copied().unwrap_or(0),
    ];

    Ok(Section {
        entries,
        trailer: dict,
        shape: SectionShape::Stream { id: io.id, widths },
    })
}

/// Read and validate `/W` (§7.5.8.2) into a [`WidthSpec`].
///
/// Returns `None` if `/W` is absent, is not a direct array of direct
/// non-negative integers, has no elements, or violates
/// [`MAX_W_FIELD_WIDTH`] / [`MAX_XREF_STREAM_ROW`]. A row width of 0
/// is also rejected: it would make the row loop consume no data and
/// every entry identical.
fn width_spec(dict: &Dict) -> Option<WidthSpec> {
    let items = dict.get(b"W")?.as_array()?;
    if items.is_empty() {
        return None;
    }
    let mut row = 0usize;
    let mut fields = [0usize; 3];
    for (i, item) in items.iter().enumerate() {
        let w = usize::try_from(item.as_int()?).ok()?;
        if w > MAX_W_FIELD_WIDTH {
            return None;
        }
        row = row.checked_add(w)?;
        if let Some(slot) = fields.get_mut(i) {
            *slot = w;
        }
    }
    if row == 0 || row > MAX_XREF_STREAM_ROW {
        return None;
    }
    Some(WidthSpec { fields, row })
}

/// Read `/Index` (§7.5.8.2) as `(first object number, count)` pairs,
/// defaulting to the spec's `[0 Size]` when absent.
///
/// Returns `None` for a malformed `/Index` (not an array, an odd
/// number of elements, a non-integer or negative element, or a count
/// that does not fit `usize`).
fn index_pairs(dict: &Dict, size: u32) -> Option<Vec<(u32, usize)>> {
    let Some(items) = dict.get(b"Index") else {
        // Table 17 default value: `[0 Size]` — the data then holds
        // exactly `Size` rows covering every object number.
        return Some(vec![(0, usize::try_from(size).unwrap_or(usize::MAX))]);
    };
    let items = items.as_array()?;
    if items.len() % 2 != 0 {
        return None;
    }
    let mut pairs = Vec::with_capacity(items.len() / 2);
    for pair in items.chunks_exact(2) {
        let (Some(first), Some(count)) = (pair.first(), pair.get(1)) else {
            return None;
        };
        let first = u32::try_from(first.as_int()?).ok()?;
        let count = usize::try_from(count.as_int()?).ok()?;
        pairs.push((first, count));
    }
    Some(pairs)
}

/// Decode one packed cross-reference-stream row into an [`XrefEntry`]
/// per §7.5.8.3 Table 18.
///
/// Field semantics, defaults and the big-endian rule are all spelled
/// out in the module docs. `None` means the row does not describe a
/// representable entry (a type-1 row with no offset field, or a field
/// value outside the spec's ranges) and should be skipped.
fn decode_row(row: &[u8], spec: WidthSpec) -> Option<XrefEntry> {
    let mut cursor = 0usize;
    // Reads the next `width` bytes as an unsigned big-endian integer
    // ("fields requiring more than one byte are stored with the
    // high-order byte first"). Width 0 → the field is not present and
    // consumes nothing; the caller substitutes the Table 18 default.
    let mut read = |width: usize| -> Option<u64> {
        if width == 0 {
            return None;
        }
        let bytes = row.get(cursor..cursor.checked_add(width)?)?;
        cursor = cursor.saturating_add(width);
        let mut value: u64 = 0;
        for &b in bytes {
            value = value.checked_mul(256)?.checked_add(u64::from(b))?;
        }
        Some(value)
    };

    // "If the first element is zero, the type field shall not be
    // present, and shall default to type 1."
    let ty = read(spec.fields[0]).unwrap_or(1);
    let field2 = read(spec.fields[1]);
    let field3 = read(spec.fields[2]);

    match ty {
        0 => Some(XrefEntry::Free {
            // Table 18 states no default for a zero-width field here;
            // 0 (the free-list terminator) is the only sensible value
            // and is noted as such in the spec RAG's gotchas.
            next_free: u32::try_from(field2.unwrap_or(0)).ok()?,
            generation: u16::try_from(field3.unwrap_or(0)).ok()?,
        }),
        1 => Some(XrefEntry::InUse {
            // No default exists for a type-1 offset: without it the
            // entry points nowhere, so the row is unusable.
            offset: field2?,
            // Table 18: "Default value: 0."
            generation: u16::try_from(field3.unwrap_or(0)).ok()?,
        }),
        2 => Some(XrefEntry::InStream {
            stream_num: u32::try_from(field2?).ok()?,
            index: u32::try_from(field3.unwrap_or(0)).ok()?,
        }),
        // §7.5.8.3: "Any other value shall be interpreted as a
        // reference to the null object, thus permitting new entry types
        // to be defined in the future." A free entry IS that reading
        // (§7.5.4 free ⇒ §7.3.10 null), and — unlike simply dropping
        // the row — it still shadows older sections, which is what a
        // newest-wins entry saying "this object is null" must do.
        _ => Some(XrefEntry::Free {
            next_free: 0,
            generation: 0,
        }),
    }
}

/// Skip exactly one EOL marker (CR, LF, or CRLF) at `pos`, plus any
/// spaces before it (the subsection-header line may have trailing
/// spaces in the wild — but entry alignment needs us at the next line,
/// and anything other than spaces/EOL here is malformed enough to
/// surface at entry parse instead).
fn skip_one_eol(buf: &[u8], mut pos: usize) -> usize {
    while buf.get(pos) == Some(&b' ') {
        pos += 1;
    }
    match (buf.get(pos), buf.get(pos + 1)) {
        (Some(b'\r'), Some(b'\n')) => pos + 2,
        (Some(b'\r' | b'\n'), _) => pos + 1,
        _ => pos,
    }
}

/// Parse one exact 20-byte entry record (§7.5.4):
/// `nnnnnnnnnn ggggg t ee` where `t` ∈ {`n`,`f`} and `ee` is one of
/// `SP CR`, `SP LF`, `CR LF`. Returns `None` on any deviation.
fn parse_entry(record: &[u8]) -> Option<XrefEntry> {
    if record.len() != 20 {
        return None;
    }
    let field1 = record.get(0..10)?;
    let sp1 = record.get(10)?;
    let field2 = record.get(11..16)?;
    let sp2 = record.get(16)?;
    let ty = record.get(17)?;
    let eol = record.get(18..20)?;

    if *sp1 != b' ' || *sp2 != b' ' {
        return None;
    }
    if !matches!(eol, b" \r" | b" \n" | b"\r\n") {
        return None;
    }
    let field1 = parse_fixed_decimal(field1)?;
    let field2 = parse_fixed_decimal(field2)?;
    let generation = u16::try_from(field2).ok()?;

    match ty {
        b'n' => Some(XrefEntry::InUse {
            offset: field1,
            generation,
        }),
        b'f' => Some(XrefEntry::Free {
            next_free: u32::try_from(field1).ok()?,
            generation,
        }),
        _ => None,
    }
}

/// Parse a fixed-width, zero-padded decimal field (all bytes must be
/// ASCII digits).
fn parse_fixed_decimal(field: &[u8]) -> Option<u64> {
    let mut value: u64 = 0;
    for &b in field {
        if !b.is_ascii_digit() {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(u64::from(b - b'0'))?;
    }
    Some(value)
}

// ---------------------------------------------------------------------------
// Observing the base file's own entry EOL (§7.5.4, spec ambiguity EOL-A1)
// ---------------------------------------------------------------------------

/// Read back the 2-byte entry EOL a file's own classic cross-reference
/// table used (§7.5.4).
///
/// # Why this exists
///
/// §7.5.4 fixes the entry at *"exactly 20 bytes long, including the
/// end-of-line marker"* and then permits **three** spellings of those two
/// bytes — `SP CR`, `SP LF`, `CR LF` — with no rule preferring any of
/// them. That is the ambiguity catalogued as `EOL-A1`, and until this
/// function existed pdfcer answered it by always writing `SP LF`.
///
/// Always writing one form is wrong on pdfcer's **own** invariant. Rule 3
/// says objects pdfcer did not logically touch are re-emitted
/// byte-identical; a full rewrite of a `CR LF` file under a fixed `SP LF`
/// changes two bytes in **every entry of the table** — a 10,000-byte diff
/// on a 5,000-object file nobody edited. Minimal-diff editing exists to
/// prevent exactly that, so the setting's default is now
/// [`XrefEntryEol::MatchSource`] and this is what resolves it.
///
/// It is the same principle `Document::section_shape` already serves at a
/// coarser grain — *the base file's own form* (R33). This is that idea one
/// level finer: not merely "was it a table or a stream", but "which of the
/// three legal spellings did it use".
///
/// # How it decides, and why it reads only the first entry
///
/// Finds the **last** `xref` keyword in the file (the newest section in an
/// incrementally-updated file, which is the form a rewrite should match),
/// steps over the subsection header, and reads bytes 18..20 of the first
/// 20-byte record.
///
/// One entry, not a survey, because §7.5.4's fixed width means a file that
/// mixed forms *within* a table would already be malformed in a way the
/// entry parser rejects — so the first entry either speaks for all of them
/// or the file does not load at all. Sampling more would cost a scan to
/// answer a question the format has already answered.
///
/// # When it returns `None`
///
/// - The file has no classic `xref` table (a cross-reference **stream**
///   file — §7.5.8 has no entry EOL at all, being binary).
/// - The bytes at the expected position are not one of the three legal
///   pairs, i.e. the file is non-conforming here.
/// - There is no file yet: a document pdfcer assembles from nothing has no
///   base form to match.
///
/// In every one of those cases the caller falls back to `SP LF`, which is
/// what pdfcer emitted before this existed — so a file with nothing to
/// match is written exactly as it always was.
#[must_use]
pub fn observed_entry_eol(buf: &[u8]) -> Option<crate::settings::XrefEntryEol> {
    use crate::settings::XrefEntryEol;

    // The LAST `xref` keyword: in an incrementally-updated file the newest
    // section is the one a rewrite is replacing, so it is the one whose
    // form should carry forward.
    let at = last_xref_keyword(buf)?;

    // `xref` then an EOL, then the subsection header `first count`, then
    // its EOL, then the entries.
    let mut pos = skip_one_eol(buf, at + 4);
    // Step over the subsection header line. Deliberately tolerant: it is
    // digits and spaces, and anything else means this is not a table the
    // entry parser would have accepted either.
    let header_start = pos;
    while matches!(buf.get(pos), Some(b'0'..=b'9' | b' ')) {
        pos += 1;
    }
    if pos == header_start {
        return None;
    }
    pos = skip_one_eol(buf, pos);

    let record = buf.get(pos..pos + 20)?;
    // Confirm it really is an entry before trusting its last two bytes —
    // otherwise a file whose table starts unexpectedly would have its
    // trailing bytes read as an EOL and silently set the output's form.
    parse_entry(record)?;
    match (record.get(18), record.get(19)) {
        (Some(b' '), Some(b'\n')) => Some(XrefEntryEol::SpaceLf),
        (Some(b' '), Some(b'\r')) => Some(XrefEntryEol::SpaceCr),
        (Some(b'\r'), Some(b'\n')) => Some(XrefEntryEol::CrLf),
        _ => None,
    }
}

/// Byte offset of the last `xref` keyword that begins a line.
///
/// Anchored to a line start so the `startxref` keyword — which ends in the
/// same four letters — cannot be mistaken for a table header. That is not
/// hypothetical: `startxref` appears in every classic file, *after* the
/// table, so a naive search for `xref` would find it first when scanning
/// backwards and land on the offset digits instead of an entry.
fn last_xref_keyword(buf: &[u8]) -> Option<usize> {
    let mut found = None;
    let mut i = 0usize;
    while let Some(rel) = buf.get(i..).and_then(|tail| find_subslice(tail, b"xref")) {
        let at = i + rel;
        // A line start, and NOT the tail of `startxref` — which ends in the
        // same four letters and appears in every classic file, *after* the
        // table. Without that exclusion this would return the offset digits
        // following it instead of an entry.
        //
        // Deliberately strict about the preceding byte. An earlier version
        // also tolerated leading spaces on the line; it was both panicky (it
        // sliced) and pointless, because a table whose `xref` keyword is
        // indented is not one the entry parser above would have accepted
        // either — so tolerating it here could only ever produce an EOL
        // reading for a file that does not load.
        let at_line_start = at == 0 || matches!(buf.get(at - 1), Some(b'\r' | b'\n'));
        let is_startxref = at >= 5 && buf.get(at - 5..at) == Some(b"start");
        if at_line_start && !is_startxref {
            found = Some(at);
        }
        i = at + 4;
    }
    found
}

/// First index of `needle` in `haystack`.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
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

    /// Build a minimal classic PDF tail: xref section + trailer +
    /// startxref, positioned so offsets are self-consistent. Returns
    /// the full buffer with `body` at the front.
    fn with_xref_tail(body: &[u8], xref_block: &str, trailer: &str) -> Vec<u8> {
        let mut buf = body.to_vec();
        let xref_at = buf.len();
        buf.extend_from_slice(xref_block.as_bytes());
        buf.extend_from_slice(trailer.as_bytes());
        buf.extend_from_slice(format!("startxref\n{xref_at}\n%%EOF\n").as_bytes());
        buf
    }

    #[test]
    fn parses_spec_example_2_section() {
        // §7.5.4 EXAMPLE 2: one subsection, six entries.
        let buf = with_xref_tail(
            b"",
            "xref\n0 6\n\
             0000000003 65535 f\r\n\
             0000000017 00000 n\r\n\
             0000000081 00000 n\r\n\
             0000000000 00007 f\r\n\
             0000000331 00000 n\r\n\
             0000000409 00000 n\r\n",
            "trailer\n<< /Size 6 /Root 1 0 R >>\n",
        );
        let loaded = load_xref_chain(&buf).unwrap();
        assert_eq!(loaded.table.len(), 6);
        assert_eq!(
            loaded.table.get(0),
            Some(XrefEntry::Free {
                next_free: 3,
                generation: 65535
            })
        );
        assert_eq!(
            loaded.table.get(1),
            Some(XrefEntry::InUse {
                offset: 17,
                generation: 0
            })
        );
        assert_eq!(
            loaded.table.get(3),
            Some(XrefEntry::Free {
                next_free: 0,
                generation: 7
            })
        );
    }

    #[test]
    fn parses_spec_example_3_multiple_subsections() {
        // §7.5.4 EXAMPLE 3: four subsections; object 23 at gen 2.
        let buf = with_xref_tail(
            b"",
            "xref\n0 1\n\
             0000000000 65535 f\r\n\
             3 1\n\
             0000025325 00000 n\r\n\
             23 2\n\
             0000025518 00002 n\r\n\
             0000025635 00000 n\r\n\
             30 1\n\
             0000025777 00000 n\r\n",
            "trailer\n<< /Size 31 /Root 1 0 R >>\n",
        );
        let loaded = load_xref_chain(&buf).unwrap();
        assert_eq!(loaded.table.len(), 5);
        assert_eq!(
            loaded.table.get(23),
            Some(XrefEntry::InUse {
                offset: 25518,
                generation: 2
            })
        );
        assert_eq!(
            loaded.table.get(24),
            Some(XrefEntry::InUse {
                offset: 25635,
                generation: 0
            })
        );
        assert!(loaded.table.get(2).is_none());
    }

    #[test]
    fn all_three_legal_entry_eols_accepted() {
        for eol in [" \r", " \n", "\r\n"] {
            let entry = format!("0000000017 00000 n{eol}");
            assert!(
                parse_entry(entry.as_bytes()).is_some(),
                "EOL {eol:?} rejected"
            );
        }
    }

    #[test]
    fn nineteen_byte_entry_rejected() {
        // A 10+1+5+1+1+1 = 19-byte record (single-char EOL) is a
        // real-world deviation Pass 1 refuses (module docs).
        assert!(parse_entry(b"0000000017 00000 n\n").is_none());
        // And a wrong type byte:
        assert!(parse_entry(b"0000000017 00000 x \n").is_none());
    }

    // ---- decision 013 Pass A: strict-§7.5.4 rejections, corpus-evidenced ----
    //
    // Pass A (the xref-recovery MEASUREMENT step) classified the CRLF-
    // correlated real-world load failures in the acquired corpus. The
    // dominant cause was OFFSET-SHIFT (stale stored offsets from LF->CRLF
    // byte growth — a Pass B rebuild-by-scan job, not a parser bug), and
    // ZERO cases were a genuine rejection of a spec-CONFORMANT classic
    // table. The tests below pin the three distinct GENUINELY-MALFORMED
    // shapes that Pass A confirmed pdfcer correctly fail-cleans on, so a
    // future "tolerate deviant tables" change is a deliberate, evidenced
    // decision (module docs: tolerance is corpus-evidenced and recorded in
    // C:\personal_rag\pdf\ first) rather than an accidental regression.

    #[test]
    fn generation_beyond_spec_max_is_rejected() {
        // Corpus finding (17 files: pdfium XFA + pdfbox eu-001 + qpdf
        // numeric-and-string/i3/fax-decode-parms): the free-list head — or
        // an in-use placeholder — is written `0000000000 65536 f` /
        // `... 65536 n`, i.e. a well-formed 20-byte record whose GENERATION
        // is 65536. §7.5.4 fixes the maximum generation at 65,535 (and
        // object 0 "shall have a generation number of 65,535"), so 65536 is
        // spec-NON-conformant DATA in a structurally-valid record. pdfcer
        // rejects it (u16 range) => the whole load fails clean. This is the
        // intended strict behaviour, NOT a class-(b) Pass-A bug: the table
        // is not spec-valid. It is also NOT the CRLF story — 14 of the 17
        // files are LF. A tolerance broadening (clamp/accept 65536) is a
        // separate decision, out of Pass A's scope.
        assert!(parse_entry(b"0000000000 65536 f \n").is_none());
        assert!(parse_entry(b"0000000000 65536 n \n").is_none());
        // The legal boundary value still parses.
        assert_eq!(
            parse_entry(b"0000000000 65535 f \n"),
            Some(XrefEntry::Free {
                next_free: 0,
                generation: 65535,
            })
        );
    }

    #[test]
    fn sp_cr_lf_mangled_entry_reads_as_sp_cr_and_desyncs() {
        // Corpus finding (minimal-linearize-pass1.pdf): an LF->CRLF
        // conversion turns an `SP LF` (20 0A) entry terminator into
        // `SP CR LF` (20 0D 0A), a 21-byte line. Reading exactly 20 bytes,
        // the parser sees bytes 18-19 = `SP CR` — a LEGAL §7.5.4 EOL — so
        // entry 0 is ACCEPTED and the trailing LF becomes a 1-byte
        // desync that pushes later entries off alignment until one fails
        // BadEntry. So a `SP CR LF`-mangled table still fails clean, just
        // not at byte 0. Documented here so the 20-byte fast path's
        // interaction with the 21-byte deviant is a known, pinned fact
        // (recovery of such files is Pass B / a future tolerance, not this).
        let first20 = b"0000000000 00000 n \r"; // first 20 of `... n \r\n`
        assert_eq!(
            parse_entry(first20),
            Some(XrefEntry::InUse {
                offset: 0,
                generation: 0,
            }),
            "SP CR is a legal EOL, so the 21-byte SP CR LF entry's first 20 bytes parse"
        );
        // The full 21-byte record handed in whole is NOT 20 bytes -> None,
        // confirming the exact-length invariant (guard: assert 20-byte
        // length in unit tests, never rely on round-trip reload).
        assert!(parse_entry(b"0000000000 00000 n \r\n").is_none());
    }

    #[test]
    fn prev_chain_merges_newest_wins() {
        // Base section defines objects 0-3; update redefines object 2
        // and frees object 3. Newest must win for both.
        let base_xref = "xref\n0 4\n\
             0000000000 65535 f\r\n\
             0000000010 00000 n\r\n\
             0000000020 00000 n\r\n\
             0000000030 00000 n\r\n";
        let mut buf: Vec<u8> = b"%PDF-1.4\n".to_vec();
        let base_at = buf.len();
        buf.extend_from_slice(base_xref.as_bytes());
        buf.extend_from_slice(b"trailer\n<< /Size 4 /Root 1 0 R >>\n");
        let upd_at = buf.len();
        buf.extend_from_slice(
            "xref\n2 2\n\
             0000000099 00001 n\r\n\
             0000000000 65535 f\r\n"
                .as_bytes(),
        );
        buf.extend_from_slice(
            format!("trailer\n<< /Size 4 /Root 1 0 R /Prev {base_at} >>\n").as_bytes(),
        );
        buf.extend_from_slice(format!("startxref\n{upd_at}\n%%EOF\n").as_bytes());

        let loaded = load_xref_chain(&buf).unwrap();
        // Newest wins: object 2 from the update…
        assert_eq!(
            loaded.table.get(2),
            Some(XrefEntry::InUse {
                offset: 99,
                generation: 1
            })
        );
        // …object 3 now FREE (shadowing the base in-use entry)…
        assert!(matches!(loaded.table.get(3), Some(XrefEntry::Free { .. })));
        // …and untouched objects still come from the base.
        assert_eq!(
            loaded.table.get(1),
            Some(XrefEntry::InUse {
                offset: 10,
                generation: 0
            })
        );
        // Newest trailer governs (it has /Prev; base doesn't).
        assert!(loaded.trailer.contains_key(b"Prev"));
    }

    #[test]
    fn prev_cycle_is_detected() {
        // A section whose /Prev points at itself.
        let mut buf: Vec<u8> = Vec::new();
        let at = buf.len();
        buf.extend_from_slice(b"xref\n0 1\n0000000000 65535 f\r\n");
        buf.extend_from_slice(format!("trailer\n<< /Size 1 /Prev {at} >>\n").as_bytes());
        buf.extend_from_slice(format!("startxref\n{at}\n%%EOF\n").as_bytes());
        let e = load_xref_chain(&buf).unwrap_err();
        assert_eq!(e.kind, XrefErrorKind::PrevChainCycle);
    }

    #[test]
    fn size_filter_drops_out_of_range_entries() {
        // Table 15: entries numbered >= Size are "ignored and defined
        // to be missing."
        let buf = with_xref_tail(
            b"",
            "xref\n0 3\n\
             0000000000 65535 f\r\n\
             0000000010 00000 n\r\n\
             0000000020 00000 n\r\n",
            "trailer\n<< /Size 2 /Root 1 0 R >>\n",
        );
        let loaded = load_xref_chain(&buf).unwrap();
        assert!(loaded.table.get(1).is_some());
        assert!(loaded.table.get(2).is_none());
    }

    #[test]
    fn encryption_is_reported_to_the_document_layer_not_refused_here() {
        // This assertion is the REVERSE of what it was before
        // `crate::crypto` existed, and the reversal is the point: an
        // encrypted document with an intact cross-reference table now loads
        // as far as the trailer, and the decision about ciphers, passwords
        // and refusals belongs to `Document::assemble`, which has objects to
        // resolve indirect `/O`, `/U` and `/CF` entries with.
        //
        // What this layer must still do is carry `/Encrypt` through intact —
        // silently dropping it would hand ciphertext to the content parser.
        let buf = with_xref_tail(
            b"",
            "xref\n0 1\n0000000000 65535 f\r\n",
            "trailer\n<< /Size 1 /Root 1 0 R /Encrypt 9 0 R >>\n",
        );
        let loaded = load_xref_chain(&buf).expect("encryption is no longer refused here");
        assert!(
            loaded.trailer.contains_key(b"Encrypt"),
            "/Encrypt must survive to the document layer"
        );
    }

    // ---- cross-reference stream row/dictionary decoding (§7.5.8.2-3) ----

    /// `W = [1 2 1]`-style spec, built directly (the packed-row layer
    /// is unit-testable without assembling a whole file; whole-file
    /// coverage lives in `tests/pdf15_streams.rs`).
    fn spec_of(w: [usize; 3]) -> WidthSpec {
        WidthSpec {
            fields: w,
            row: w.iter().sum(),
        }
    }

    #[test]
    fn multibyte_row_fields_are_big_endian() {
        // §7.5.8.3: "Fields requiring more than one byte are stored
        // with the high-order byte first."
        let spec = spec_of([1, 4, 2]);
        let row = [1, 0x00, 0x01, 0x02, 0x03, 0x00, 0x07];
        assert_eq!(
            decode_row(&row, spec),
            Some(XrefEntry::InUse {
                offset: 0x0001_0203,
                generation: 7
            })
        );
    }

    #[test]
    fn zero_width_type_field_defaults_to_type_one() {
        // §7.5.8.2: "If the first element is zero, the type field shall
        // not be present, and shall default to type 1." The type byte
        // must also consume NO bytes.
        let spec = spec_of([0, 2, 1]);
        let row = [0x12, 0x34, 0x05];
        assert_eq!(
            decode_row(&row, spec),
            Some(XrefEntry::InUse {
                offset: 0x1234,
                generation: 5
            })
        );
    }

    #[test]
    fn zero_width_generation_field_defaults_to_zero() {
        // Table 18, type 1, field 3: "Default value: 0."
        let spec = spec_of([1, 2, 0]);
        let row = [1, 0x00, 0x99];
        assert_eq!(
            decode_row(&row, spec),
            Some(XrefEntry::InUse {
                offset: 0x99,
                generation: 0
            })
        );
    }

    #[test]
    fn type_two_row_is_container_plus_index_never_a_generation() {
        // Table 18 type 2: field 2 = object stream number, field 3 =
        // index within it. There is no generation field at all.
        let spec = spec_of([1, 2, 2]);
        let row = [2, 0x00, 0x0A, 0x00, 0x03];
        assert_eq!(
            decode_row(&row, spec),
            Some(XrefEntry::InStream {
                stream_num: 10,
                index: 3
            })
        );
    }

    #[test]
    fn type_zero_row_is_a_free_entry() {
        let spec = spec_of([1, 2, 2]);
        let row = [0, 0x00, 0x00, 0xFF, 0xFF];
        assert_eq!(
            decode_row(&row, spec),
            Some(XrefEntry::Free {
                next_free: 0,
                generation: 65535
            })
        );
    }

    #[test]
    fn unknown_row_type_reads_as_the_null_object() {
        // §7.5.8.3: "Any other value shall be interpreted as a
        // reference to the null object" — recorded as a free entry,
        // which IS null (§7.5.4 → §7.3.10) and still shadows older
        // sections as a newest-wins entry must.
        let spec = spec_of([1, 2, 2]);
        let row = [7, 0x12, 0x34, 0x00, 0x01];
        assert!(matches!(
            decode_row(&row, spec),
            Some(XrefEntry::Free { .. })
        ));
    }

    #[test]
    fn type_one_row_without_an_offset_field_is_unusable() {
        // A zero-width field 2 leaves a type-1 entry pointing nowhere,
        // and Table 18 defines no default for it.
        let spec = spec_of([1, 0, 1]);
        assert_eq!(decode_row(&[1, 0], spec), None);
    }

    #[test]
    fn width_spec_validates_w() {
        use crate::object::Name;
        let w = |items: Vec<Object>| {
            let mut d = Dict::new();
            d.insert(Name::from(b"W"), Object::Array(items));
            d
        };
        let ints = |v: &[i64]| v.iter().copied().map(Object::Integer).collect::<Vec<_>>();

        let spec = width_spec(&w(ints(&[1, 2, 1]))).unwrap();
        assert_eq!(spec.fields, [1, 2, 1]);
        assert_eq!(spec.row, 4);

        // Trailing elements beyond field 3 are not interpreted but DO
        // occupy their bytes (§7.5.8.3: fields in increasing order).
        let spec = width_spec(&w(ints(&[1, 2, 1, 3]))).unwrap();
        assert_eq!(spec.fields, [1, 2, 1]);
        assert_eq!(spec.row, 7);

        // Refusals: all-zero row, absurd field width, non-integer, empty.
        assert!(width_spec(&w(ints(&[0, 0, 0]))).is_none());
        assert!(width_spec(&w(ints(&[1, 99, 1]))).is_none());
        assert!(width_spec(&w(vec![Object::Real(1.0)])).is_none());
        assert!(width_spec(&w(vec![])).is_none());
        assert!(width_spec(&Dict::new()).is_none());
    }

    #[test]
    fn index_defaults_to_zero_size_and_parses_pairs() {
        use crate::object::Name;
        // Table 17: "Default value: [0 Size]."
        assert_eq!(index_pairs(&Dict::new(), 42), Some(vec![(0, 42)]));

        let mut d = Dict::new();
        d.insert(
            Name::from(b"Index"),
            Object::Array(vec![
                Object::Integer(0),
                Object::Integer(1),
                Object::Integer(20),
                Object::Integer(3),
            ]),
        );
        assert_eq!(index_pairs(&d, 42), Some(vec![(0, 1), (20, 3)]));

        // An odd element count is malformed (pairs, per Table 17).
        let mut d = Dict::new();
        d.insert(
            Name::from(b"Index"),
            Object::Array(vec![Object::Integer(0)]),
        );
        assert_eq!(index_pairs(&d, 42), None);
    }

    #[test]
    fn missing_startxref_reported() {
        let e = load_xref_chain(b"not a pdf at all").unwrap_err();
        assert_eq!(e.kind, XrefErrorKind::StartxrefNotFound);
    }

    #[test]
    fn last_startxref_wins() {
        // Two updates → two startxref markers; the LAST one is used
        // (§7.5.5 + §7.5.6). The first points at garbage; if the
        // scanner picked it, loading would fail.
        let mut buf: Vec<u8> = Vec::new();
        buf.extend_from_slice(b"startxref\n999999\n%%EOF\n");
        let at = buf.len();
        buf.extend_from_slice(b"xref\n0 1\n0000000000 65535 f\r\n");
        buf.extend_from_slice(b"trailer\n<< /Size 1 /Root 1 0 R >>\n");
        buf.extend_from_slice(format!("startxref\n{at}\n%%EOF\n").as_bytes());
        assert!(load_xref_chain(&buf).is_ok());
    }
}
