//! # Cross-reference section emission (ISO 32000-1 §7.5.4, §7.5.5, §7.5.8)
//!
//! Both physical forms a section can take, plus the trailer/`startxref`/
//! `%%EOF` tail that closes every revision. Spec sources, all in the
//! PDF-spec RAG: `iso32000__s__7.5.4.md` (classic table byte layout),
//! `iso32000__s__7.5.5.md` (trailer and tail), `iso32000__s__7.5.8.md`
//! (cross-reference streams), and the consolidated write-direction
//! checklist `iso32000__ref__writer_emission.md` (rules B1–B14, C1–C11,
//! E1–E14).
//!
//! ## Never normalize: the form is chosen by the input, not by pdfcer
//!
//! Decision 007 R33 and `ARCHITECTURE.md` §5. §7.5.6 does **not**
//! require an appended section to match the form of the one it
//! supersedes — that is a recorded NEGATIVE RESULT, not an oversight —
//! which is exactly why the rule has to be pdfcer's own. Emitting a
//! cross-reference stream where the base file had a classic table
//! silently raises a PDF 1.4 document's effective version to 1.5 and
//! makes it unreadable by conforming pre-1.5 readers. The caller passes
//! [`crate::xref::SectionShape`] captured at load time; this module
//! never picks.
//!
//! ## The 20-byte rule, and why it gets its own constant
//!
//! §7.5.4, verbatim: *"Each entry shall be exactly 20 bytes long,
//! including the end-of-line marker."* Decision 007 W10 names the
//! failure mode precisely: a 19-byte bare-LF variant produces a file
//! *"most readers repair silently and pdfcer's own lenient parser will
//! happily reload — a false green."* So the length is asserted in a
//! unit test against a literal, not inferred from a successful reload.
//!
//! The end-of-line pair is one of exactly three (§7.5.4): `SP CR`
//! (20 0D), `SP LF` (20 0A), `CR LF` (0D 0A). **`SP CR LF` is not one
//! of them** — it is 21 bytes and the single most common way to get
//! this wrong. §7.5.4 states no preference among the three, so under
//! **R169** the choice is the operator's: it arrives as
//! [`crate::settings::XrefEntryEol`] on
//! [`crate::writer::SaveOptions::xref_entry_eol`] and defaults to
//! `SP LF`, the form pdfcer has always emitted. The enum can express only
//! the three legal pairs, so no setting value can produce a
//! non-conforming entry. Ambiguity ID `EOL-A1`.
//!
//! The tail's trailing end-of-line (`EOL-A2`) is the same story one
//! clause over — see [`write_classic_tail`].
//!
//! ## No comments, ever
//!
//! §7.5.8.4's closing bullet, which a reader-path engineer working only
//! from §7.5.4 would never find: *"PDF comments shall not be included
//! in a cross-reference table or in cross-reference streams."* The
//! `% …` annotations in the standard's own EXAMPLEs are editorial
//! markup, not emitted bytes.
//!
//! ## Subsection ordering: the one real divergence between the forms
//!
//! - Classic (§7.5.4): subsections *"may appear in any order"*.
//! - Stream (§7.5.8.2): `/Index` *"shall be sorted in ascending order
//!   by object number"*.
//!
//! A writer that ports its classic emitter to xref streams without
//! adding a sort produces a non-conforming file. Both emitters here
//! take pre-sorted input and [`build_runs`] preserves that order, so
//! the stricter rule is satisfied by construction in both.

use std::collections::BTreeMap;

use crate::object::{Dict, Name, ObjId, Object};
use crate::settings::{TrailingEol, XrefEntryEol};
use crate::xref::XrefEntry;

use super::encoder::IdentityEncoder;
use super::serialize;

/// Length of one classic cross-reference entry, in bytes (§7.5.4:
/// *"exactly 20 bytes long, including the end-of-line marker"*).
pub const CLASSIC_ENTRY_LEN: usize = 20;

/// Largest object number a classic entry's 10-digit offset field can
/// address (§7.5.4: *"a 10-digit number, padded with leading zeros"*).
///
/// Annex C sets the same bound from the other direction — *"the maximum
/// size of a PDF file is 10 GB minus 1 byte"* is a consequence of this
/// field width, not an independent limit. An offset at or above this
/// cannot be written in the classic form at all, so a file that grew
/// past it must fail clean rather than emit a truncated offset.
pub const MAX_CLASSIC_OFFSET: u64 = 9_999_999_999;

/// A contiguous run of object numbers — one classic subsection, or one
/// `/Index` pair of a cross-reference stream.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Run {
    first: u32,
    entries: Vec<XrefEntry>,
}

/// Group a **sorted** object-number → entry map into maximal runs of
/// consecutive numbers.
///
/// Runs are what both forms want: §7.5.4's subsections and §7.5.8.2's
/// `/Index` pairs are the same idea in two syntaxes. Taking a
/// [`BTreeMap`] rather than a slice makes the ascending order a type
/// property instead of a caller obligation — which matters because
/// §7.5.8.2's ascending `/Index` is a `shall`.
fn build_runs(entries: &BTreeMap<u32, XrefEntry>) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    for (&num, &entry) in entries {
        match runs.last_mut() {
            Some(run) if u64::from(run.first) + run.entries.len() as u64 == u64::from(num) => {
                run.entries.push(entry);
            }
            _ => runs.push(Run {
                first: num,
                entries: vec![entry],
            }),
        }
    }
    runs
}

/// Why a cross-reference section could not be emitted.
///
/// Every variant is a **counted, named refusal** rather than a silent
/// degradation — the R27 fail-clean posture applied to the write side
/// (decision 007 W11).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum XrefOutError {
    /// A byte offset exceeded [`MAX_CLASSIC_OFFSET`], so it cannot be
    /// written into a classic entry's 10-digit field (§7.5.4).
    #[error(
        "byte offset {offset} exceeds the 10-digit classic xref field (max {MAX_CLASSIC_OFFSET})"
    )]
    OffsetTooLarge {
        /// The offset that would not fit.
        offset: u64,
    },
    /// A classic table was asked to carry a type-2 (compressed-object)
    /// entry, which §7.5.4's `n`/`f` grammar cannot express.
    ///
    /// This is only reachable for a **hybrid-reference** file
    /// (§7.5.8.4), where compressed objects live in the `/XRefStm`
    /// stream and the classic table carries free entries for them.
    #[error(
        "object {num} is compressed in object stream {container}; a classic cross-reference table cannot express a type-2 entry"
    )]
    CompressedInClassicTable {
        /// The object that has no classic representation.
        num: u32,
        /// Its container object stream.
        container: u32,
    },
    /// The `sum(Index counts) × sum(W) == decoded length` self-check
    /// failed (§7.5.8.2; identity-writer invariant 9 in
    /// `iso32000__ref__writer_emission.md`).
    ///
    /// Unreachable barring a bug in this module — which is exactly why
    /// it is asserted: the check is one multiplication and it converts
    /// a whole class of silent corruption into a clean refusal.
    #[error(
        "cross-reference stream self-check failed: {rows} rows × {row_len} bytes != {actual} bytes emitted"
    )]
    StreamSelfCheck {
        /// Total entry count across all `/Index` pairs.
        rows: usize,
        /// `W[0] + W[1] + W[2]`.
        row_len: usize,
        /// Bytes actually produced.
        actual: usize,
    },
    /// Compressing the cross-reference stream failed.
    #[error("cross-reference stream could not be Flate-encoded: {0}")]
    Compress(String),
}

/// Emit a classic cross-reference table (§7.5.4) into `out`.
///
/// Returns nothing: the section's own offset is `out.len()` **before**
/// the call, which is what `startxref` must name (§7.5.5: *"the byte
/// offset … to the beginning of the `xref` keyword"*).
///
/// `entries` must be ascending, which [`BTreeMap`] guarantees.
///
/// `eol` chooses which of §7.5.4's three permitted two-byte terminators
/// closes each 20-byte entry (spec ambiguity `EOL-A1`, R169). It is a
/// **parameter, never a global**: two saves of the same document must not
/// differ for a reason invisible at the call site.
///
/// # Errors
///
/// [`XrefOutError::OffsetTooLarge`] or
/// [`XrefOutError::CompressedInClassicTable`] — see their docs.
pub fn write_classic_table(
    out: &mut Vec<u8>,
    entries: &BTreeMap<u32, XrefEntry>,
    eol: XrefEntryEol,
) -> Result<(), XrefOutError> {
    // §7.5.4: "Each cross-reference section shall begin with a line
    // containing the keyword `xref`" — the keyword alone on its line.
    out.extend_from_slice(b"xref\n");

    for run in build_runs(entries) {
        // §7.5.4: "a line containing two numbers separated by a SPACE
        // (20h)". Unpadded decimal — the header has no fixed-width or
        // zero-padding rule (a recorded NEGATIVE RESULT); only the
        // 20-byte *entries* do.
        out.extend_from_slice(run.first.to_string().as_bytes());
        out.push(b' ');
        out.extend_from_slice(run.entries.len().to_string().as_bytes());
        out.push(b'\n');
        for entry in &run.entries {
            write_classic_entry(out, entry, run.first, eol)?;
        }
    }
    Ok(())
}

/// Emit one exactly-20-byte classic entry (§7.5.4).
///
/// Layout, byte by byte — the table this mirrors is in
/// `iso32000__s__7.5.4.md`:
///
/// | Offset | Len | Content |
/// |---|---|---|
/// | 0–9 | 10 | `nnnnnnnnnn`, zero-padded left |
/// | 10 | 1 | `SP` |
/// | 11–15 | 5 | `ggggg`, zero-padded left |
/// | 16 | 1 | `SP` |
/// | 17 | 1 | `n` or `f` |
/// | 18–19 | 2 | EOL pair — whichever of §7.5.4's three `eol` names |
///
/// ## Why bytes 18–19 are a setting and not a constant (`EOL-A1`, R169)
///
/// §7.5.4 permits exactly three forms — `SP CR`, `SP LF`, `CR LF` — and
/// states no preference among them. It is a genuine spec ambiguity, so
/// under R169 the operator chooses, with the shipped default being what
/// pdfcer already emitted (`SP LF`). The three legal forms are the *only*
/// ones [`XrefEntryEol`] can express: `LF CR`, a bare `LF`, a bare `CR`,
/// `SP SP` and `SP CR LF` are non-conforming (the last would also make the
/// entry 21 bytes), and a settings file is not a licence to emit them.
fn write_classic_entry(
    out: &mut Vec<u8>,
    entry: &XrefEntry,
    run_first: u32,
    eol: XrefEntryEol,
) -> Result<(), XrefOutError> {
    let (field1, generation, keyword) = match *entry {
        XrefEntry::InUse { offset, generation } => {
            if offset > MAX_CLASSIC_OFFSET {
                return Err(XrefOutError::OffsetTooLarge { offset });
            }
            (offset, generation, b'n')
        }
        // §7.5.4: a free entry's 10-digit field is "the object number
        // of the next free object", not an offset.
        XrefEntry::Free {
            next_free,
            generation,
        } => (u64::from(next_free), generation, b'f'),
        XrefEntry::InStream {
            stream_num,
            index: _,
        } => {
            return Err(XrefOutError::CompressedInClassicTable {
                num: run_first,
                container: stream_num,
            });
        } // NO WILDCARD ARM. §7.5.8.3 reserves further entry types, and
          // `XrefEntry` is #[non_exhaustive] for downstream crates — but
          // inside pdfcer-core a new variant must break this match, so
          // that whoever adds it decides explicitly whether the classic
          // `n`/`f` grammar can express it (usually: no).
    };

    let start = out.len();
    out.extend_from_slice(format!("{field1:010}").as_bytes());
    out.push(b' ');
    out.extend_from_slice(format!("{generation:05}").as_bytes());
    out.push(b' ');
    out.push(keyword);
    // The EOL pair — one of §7.5.4's three permitted forms, chosen by the
    // operator's `EOL-A1` setting. Each is exactly two bytes, which is
    // what keeps the entry at 20 and the `debug_assert_eq!` below honest.
    //
    // `MatchSource` must already have been resolved by the save path,
    // which is the only layer holding the base file's bytes. Its
    // `bytes()` falls back to `SP LF` here rather than panicking: a
    // writer that aborted mid-table because a setting reached it
    // unresolved would turn a configuration slip into a lost save, and
    // the fallback is the form pdfcer emitted for its whole life before
    // this setting existed.
    out.extend_from_slice(&eol.bytes());
    debug_assert_eq!(out.len() - start, CLASSIC_ENTRY_LEN);
    Ok(())
}

/// Emit the §7.5.5 tail that closes a classic revision:
/// `trailer` + dictionary + `startxref` + offset + `%%EOF`.
///
/// §7.5.5, verbatim: *"The last line of the file shall contain only the
/// end-of-file marker, `%%EOF`. The two preceding lines shall contain,
/// one per line and in order, the keyword `startxref` and the byte
/// offset … The `startxref` line shall be preceded by the trailer
/// dictionary."* So `startxref 1234` on one line is non-conforming, and
/// so is anything sharing the `%%EOF` line.
///
/// Whether the file's final byte may be an EOL is a genuine, recorded
/// spec ambiguity (`EOL-A2`: §7.5.1 requires every line to be terminated;
/// §7.5.5 says the last line "contains only" `%%EOF`), so under R169
/// `trailing` is the operator's choice. Terminating — the shipped default
/// — satisfies §7.5.1's `shall`, and every reader's backward `%%EOF` scan
/// finds the marker either way.
///
/// **Not optional on the append path.** §7.2.3 requires an EOL between
/// `%%EOF` and a following `N G obj`, so an incremental save that appends
/// onto this revision must emit its own separator regardless of what this
/// setting said when the previous revision was written. `save.rs` owns
/// that; see its `SEPARATOR` handling.
pub fn write_classic_tail(
    out: &mut Vec<u8>,
    trailer: &Dict,
    section_offset: u64,
    trailing: TrailingEol,
) {
    out.extend_from_slice(b"trailer\n");
    // The trailer is not an indirect object (§7.5.5) — no `obj`, no
    // object number — so it is serialized as a bare dictionary. `ObjId`
    // 0 0 is passed to the encoder seam only as a placeholder: the
    // trailer is never encrypted (its `/ID` strings are `shall`-direct
    // and unencrypted per Table 15), which is why the encoder here is
    // unconditionally the identity one.
    serialize::write_object(
        out,
        &Object::Dict(trailer.clone()),
        ObjId::new(0, 0),
        &[],
        &IdentityEncoder,
    );
    out.extend_from_slice(b"\nstartxref\n");
    out.extend_from_slice(section_offset.to_string().as_bytes());
    out.extend_from_slice(b"\n%%EOF");
    write_trailing_eol(out, trailing);
}

/// Emit the §7.5.8.1 tail for a cross-reference-stream revision:
/// `startxref` + offset + `%%EOF`, with **no** `trailer` keyword.
///
/// §7.5.8.1: in a file that uses cross-reference streams, *"the
/// keywords `xref` and `trailer` shall no longer be used"* — the stream
/// dictionary carries the trailer's keys instead. `startxref` names the
/// offset of the **stream object** (the first byte of `N G obj`), not
/// of an `xref` keyword.
///
/// `trailing` is `EOL-A2`, exactly as for [`write_classic_tail`] — the
/// ambiguity is about the file's last byte and is therefore identical
/// whichever cross-reference form produced the revision.
pub fn write_stream_tail(out: &mut Vec<u8>, section_offset: u64, trailing: TrailingEol) {
    out.extend_from_slice(b"startxref\n");
    out.extend_from_slice(section_offset.to_string().as_bytes());
    out.extend_from_slice(b"\n%%EOF");
    write_trailing_eol(out, trailing);
}

/// Append (or deliberately do not append) the `EOL-A2` byte after
/// `%%EOF`.
///
/// One function rather than two copies of a two-line `match`, because the
/// classic and stream tails must never be able to disagree about the
/// file's final byte — a divergence there would show up as "the same
/// document ends differently depending on which cross-reference form it
/// happened to use", which is the least debuggable shape a byte-level
/// difference can take.
fn write_trailing_eol(out: &mut Vec<u8>, trailing: TrailingEol) {
    match trailing {
        TrailingEol::Lf => out.push(b'\n'),
        TrailingEol::None => {}
    }
}

/// Field widths for a cross-reference stream's `/W` (§7.5.8.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Widths(pub [usize; 3]);

impl Widths {
    /// Total bytes per entry — §7.5.8.2: *"The sum of the items shall be
    /// the total length of each entry."*
    #[must_use]
    pub const fn row_len(self) -> usize {
        self.0[0] + self.0[1] + self.0[2]
    }

    /// The narrowest widths that can represent every entry in
    /// `entries`, widened to at least `preferred` where that is legal.
    ///
    /// Two rules are load-bearing and both come from Table 17:
    ///
    /// 1. **`W[1]` is never 0.** Table 17 defines a default only for
    ///    field 1 (type ⇒ 1) and, for type-1 rows, field 3 (⇒ 0). Field
    ///    2 has **no defined default at all**, so a zero width there is
    ///    semantically undefined — the RAG records it as an explicit
    ///    NEGATIVE RESULT and pdfcer never emits it.
    /// 2. **`W[2]` must be wide enough for every row**, because the
    ///    width is fixed for the whole stream. A single generation of
    ///    257, or one object stream holding 300 objects, forces
    ///    `W[2] = 2` for every entry in the section.
    ///
    /// `preferred` is the base file's own `/W`, so an unchanged
    /// document re-emits the widths it already had (minimal diff);
    /// widening happens only when the data no longer fits, which is a
    /// value change forced by the file, not a normalization.
    #[must_use]
    pub fn fit(entries: &BTreeMap<u32, XrefEntry>, preferred: [usize; 3]) -> Self {
        let mut max_f2: u64 = 0;
        let mut max_f3: u64 = 0;
        for entry in entries.values() {
            match *entry {
                XrefEntry::InUse { offset, generation } => {
                    max_f2 = max_f2.max(offset);
                    max_f3 = max_f3.max(u64::from(generation));
                }
                XrefEntry::Free {
                    next_free,
                    generation,
                } => {
                    max_f2 = max_f2.max(u64::from(next_free));
                    max_f3 = max_f3.max(u64::from(generation));
                }
                XrefEntry::InStream { stream_num, index } => {
                    max_f2 = max_f2.max(u64::from(stream_num));
                    max_f3 = max_f3.max(u64::from(index));
                }
            }
        }
        // Field 1 holds a type in 0..=2, so one byte always suffices;
        // it is never 0 here because a zero-width type field forces the
        // Table 17 default of 1, which would misread every free and
        // every compressed entry.
        let w0 = preferred[0].clamp(1, 8);
        let w1 = byte_width(max_f2).max(preferred[1]).clamp(1, 8);
        let w2 = byte_width(max_f3).max(preferred[2]).clamp(1, 8);
        Self([w0, w1, w2])
    }
}

/// Minimum number of bytes needed to hold `v` big-endian (at least 1).
fn byte_width(v: u64) -> usize {
    let mut n = 1;
    let mut limit = 0xFFu64;
    while v > limit && n < 8 {
        n += 1;
        limit = (limit << 8) | 0xFF;
    }
    n
}

/// A serialized cross-reference stream, ready to append.
#[derive(Debug)]
pub struct XrefStreamOut {
    /// The complete `N G obj … endobj` definition bytes.
    pub bytes: Vec<u8>,
}

/// Build a cross-reference stream object (§7.5.8) holding `entries`.
///
/// `id` is the stream's own identifier — reused from the base file's
/// newest section when there is one, because that object number is
/// already spent on exactly this role and allocating a fresh one would
/// raise `/Size` for nothing. `trailer_keys` supplies `/Root`, `/Info`,
/// `/Prev`, `/ID` and friends: §7.5.8.1 says the stream dictionary
/// *"carries what a trailer would carry"*.
///
/// The stream is Flate-encoded — §7.5.8.4: *"In practice, both streams
/// should be Flate-encoded"* (a `should`, honoured). **No predictor is
/// applied.** ISO 32000-1 never mentions predictors in §7.5.8 at all;
/// `/Predictor 12` is a widespread convention, not a requirement, and
/// `/Columns == sum(W)` is likewise convention. Emitting plain Flate is
/// conforming, smaller in code, and removes an encoder that would need
/// its own fuzz coverage.
///
/// # Errors
///
/// [`XrefOutError::StreamSelfCheck`] if the row arithmetic disagrees
/// with the bytes produced, or [`XrefOutError::Compress`] if Flate
/// encoding fails.
pub fn build_xref_stream(
    id: ObjId,
    entries: &BTreeMap<u32, XrefEntry>,
    widths: Widths,
    trailer_keys: &Dict,
) -> Result<XrefStreamOut, XrefOutError> {
    let runs = build_runs(entries);
    let row_len = widths.row_len();

    let mut raw = Vec::with_capacity(entries.len() * row_len);
    for run in &runs {
        for entry in &run.entries {
            write_stream_row(&mut raw, entry, widths);
        }
    }

    // Identity-writer invariant 9: the single cheapest correctness
    // check available before emitting an xref stream.
    let rows: usize = runs.iter().map(|r| r.entries.len()).sum();
    if rows.saturating_mul(row_len) != raw.len() {
        return Err(XrefOutError::StreamSelfCheck {
            rows,
            row_len,
            actual: raw.len(),
        });
    }

    let encoded = flate_encode(&raw)?;

    // Table 17, in emission order. Every value is DIRECT — §7.5.8.2:
    // "all entries shown in Table 17 shall be direct objects", and that
    // includes every element of the arrays.
    let mut dict = Dict::new();
    dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"XRef")));
    for (key, value) in trailer_keys.iter() {
        // `/Type` is ours; `/Length` is computed below; the caller's
        // `/W`, `/Index` and `/Filter` are superseded by what we
        // actually emitted, so none of them may be copied through.
        if matches!(
            key.as_bytes(),
            b"Type" | b"Length" | b"W" | b"Index" | b"Filter" | b"DecodeParms" | b"XRefStm"
        ) {
            continue;
        }
        dict.insert(key.clone(), value.clone());
    }
    dict.insert(
        Name::from(b"W"),
        Object::Array(
            widths
                .0
                .iter()
                .map(|&w| Object::Integer(i64::try_from(w).unwrap_or(0)))
                .collect(),
        ),
    );
    // §7.5.8.2: `/Index` pairs "shall be sorted in ascending order by
    // object number". `build_runs` walks a BTreeMap, so they are.
    let mut index_items = Vec::with_capacity(runs.len() * 2);
    for run in &runs {
        index_items.push(Object::Integer(i64::from(run.first)));
        index_items.push(Object::Integer(
            i64::try_from(run.entries.len()).unwrap_or(0),
        ));
    }
    dict.insert(Name::from(b"Index"), Object::Array(index_items));
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"FlateDecode")),
    );
    // §7.5.8.2 permits an indirect `/Length` by omission (it is a
    // Table 5 key, not a Table 17 one) — and an indirect `/Length` on
    // an xref stream is unbootstrappable, because resolving it needs
    // the xref this stream *is*. Always direct. (Checklist rule E14.)
    dict.insert(
        Name::from(b"Length"),
        Object::Integer(i64::try_from(encoded.len()).unwrap_or(i64::MAX)),
    );

    let mut bytes = Vec::with_capacity(encoded.len() + 256);
    bytes.extend_from_slice(id.num.to_string().as_bytes());
    bytes.push(b' ');
    bytes.extend_from_slice(id.generation.to_string().as_bytes());
    bytes.extend_from_slice(b" obj\n");
    serialize::write_object(&mut bytes, &Object::Dict(dict), id, &[], &IdentityEncoder);
    // §7.3.8.1 framing: `stream` followed by LF (never CR alone); an
    // EOL after the data that is NOT counted in `/Length`.
    bytes.extend_from_slice(b"\nstream\n");
    bytes.extend_from_slice(&encoded);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");

    Ok(XrefStreamOut { bytes })
}

/// Write one packed entry row (§7.5.8.3 Table 18), big-endian.
///
/// §7.5.8.3: *"Each field shall be stored with the high-order byte
/// first."* A zero-width field is written as zero bytes, which is what
/// Table 17's "shall not be present" means physically.
fn write_stream_row(out: &mut Vec<u8>, entry: &XrefEntry, widths: Widths) {
    let (f1, f2, f3): (u64, u64, u64) = match *entry {
        // Type 1: offset, generation.
        XrefEntry::InUse { offset, generation } => (1, offset, u64::from(generation)),
        // Type 0: next free object number, next generation to use.
        XrefEntry::Free {
            next_free,
            generation,
        } => (0, u64::from(next_free), u64::from(generation)),
        // Type 2: container object number, index within it.
        XrefEntry::InStream { stream_num, index } => (2, u64::from(stream_num), u64::from(index)), // NO WILDCARD ARM — see `write_classic_entry`. A future entry
                                                                                                   // type must break this match rather than silently degrade to a
                                                                                                   // free entry, because a wrong row here is indistinguishable
                                                                                                   // from a correct one on inspection.
    };
    push_be(out, f1, widths.0[0]);
    push_be(out, f2, widths.0[1]);
    push_be(out, f3, widths.0[2]);
}

/// Append the low `width` bytes of `v`, high-order byte first.
fn push_be(out: &mut Vec<u8>, v: u64, width: usize) {
    for i in (0..width).rev() {
        let shift = i * 8;
        let byte = if shift >= 64 {
            0
        } else {
            ((v >> shift) & 0xFF) as u8
        };
        out.push(byte);
    }
}

/// zlib-compress `data` (§7.4.4 delegates FlateDecode to RFC 1950).
///
/// Uses the same pure-Rust `flate2`/`miniz_oxide` backend the decoder
/// does — never a C backend, per the single-static-binary packaging
/// invariant (`ARCHITECTURE.md` §6) and the WASM fork.
fn flate_encode(data: &[u8]) -> Result<Vec<u8>, XrefOutError> {
    use flate2::{Compress, Compression, FlushCompress, Status};

    let mut c = Compress::new(Compression::default(), true);
    // zlib expands incompressible input by ~0.03% plus a small header;
    // +64 covers the worst case for any realistic xref stream.
    let mut out = Vec::with_capacity(data.len() / 2 + 64);
    let mut consumed = 0usize;
    loop {
        let before_in = c.total_in();
        let before_out = c.total_out();
        out.resize(out.len().max(64) * 2, 0);
        let written = usize::try_from(before_out).unwrap_or(0);
        let status = c
            .compress(
                data.get(consumed..).unwrap_or(&[]),
                out.get_mut(written..).unwrap_or(&mut []),
                FlushCompress::Finish,
            )
            .map_err(|e| XrefOutError::Compress(e.to_string()))?;
        consumed += usize::try_from(c.total_in() - before_in).unwrap_or(0);
        match status {
            Status::StreamEnd => {
                out.truncate(usize::try_from(c.total_out()).unwrap_or(0));
                return Ok(out);
            }
            // BufError with no progress would spin; the resize above
            // guarantees the output buffer grows every iteration, so
            // progress is structural.
            Status::Ok | Status::BufError => {}
        }
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

    fn table(pairs: &[(u32, XrefEntry)]) -> BTreeMap<u32, XrefEntry> {
        pairs.iter().copied().collect()
    }

    fn in_use(offset: u64) -> XrefEntry {
        XrefEntry::InUse {
            offset,
            generation: 0,
        }
    }

    #[test]
    fn classic_entry_is_exactly_twenty_bytes() {
        // Decision 007 W10: assert the length against a literal, NOT
        // by round-tripping through pdfcer's own lenient parser, which
        // would accept a 19-byte bare-LF variant and produce a false
        // green.
        let mut out = Vec::new();
        write_classic_entry(&mut out, &in_use(1234), 1, XrefEntryEol::default()).unwrap();
        assert_eq!(out.len(), CLASSIC_ENTRY_LEN);
        assert_eq!(&out, b"0000001234 00000 n \n");

        let mut out = Vec::new();
        write_classic_entry(
            &mut out,
            &XrefEntry::Free {
                next_free: 0,
                generation: 65_535,
            },
            0,
            XrefEntryEol::default(),
        )
        .unwrap();
        assert_eq!(out.len(), CLASSIC_ENTRY_LEN);
        assert_eq!(&out, b"0000000000 65535 f \n");
    }

    #[test]
    fn classic_eol_is_a_permitted_two_byte_pair() {
        // §7.5.4 permits exactly SP CR, SP LF, CR LF. Anything else —
        // notably SP CR LF (21 bytes) or a bare LF (19) — is
        // non-conforming.
        let mut out = Vec::new();
        write_classic_entry(&mut out, &in_use(0), 1, XrefEntryEol::default()).unwrap();
        let eol = &out[18..20];
        assert!(
            eol == b" \n" || eol == b" \r" || eol == b"\r\n",
            "illegal xref EOL pair {eol:?}"
        );
    }

    #[test]
    fn classic_table_groups_consecutive_numbers_into_subsections() {
        let t = table(&[
            (
                0,
                XrefEntry::Free {
                    next_free: 0,
                    generation: 65_535,
                },
            ),
            (1, in_use(17)),
            (2, in_use(90)),
            // Gap at 3–6.
            (7, in_use(200)),
        ]);
        let mut out = Vec::new();
        write_classic_table(&mut out, &t, XrefEntryEol::default()).unwrap();
        let text = String::from_utf8_lossy(&out).into_owned();
        assert!(text.starts_with("xref\n0 3\n"), "{text}");
        assert!(text.contains("\n7 1\n"), "{text}");
        // Body length: 4 entries × 20 bytes, plus the three headers.
        assert_eq!(out.len(), 5 + 4 + 3 * 20 + 4 + 20);
    }

    #[test]
    fn every_permitted_entry_terminator_keeps_the_entry_at_twenty_bytes() {
        // `EOL-A1` (R169). §7.5.4 permits exactly three two-byte forms and
        // states no preference, so all three are offered — and the whole
        // reason `XrefEntryEol` is an enum rather than a `[u8; 2]` is that
        // an operator must not be able to reach the 21-byte `SP CR LF`
        // form, which is decision 007 W10's named failure mode.
        for (eol, want) in [
            (XrefEntryEol::SpaceLf, &b" \n"[..]),
            (XrefEntryEol::SpaceCr, &b" \r"[..]),
            (XrefEntryEol::CrLf, &b"\r\n"[..]),
        ] {
            let mut out = Vec::new();
            write_classic_entry(&mut out, &in_use(1234), 1, eol).unwrap();
            assert_eq!(
                out.len(),
                CLASSIC_ENTRY_LEN,
                "{eol:?} changed the entry length"
            );
            assert!(out.ends_with(want), "{eol:?} emitted {out:?}");
            // Bytes 0..18 are the fields and must not vary with the
            // terminator — a setting that shifted the `n`/`f` keyword
            // would produce a file every reader repairs silently.
            assert_eq!(&out[..18], b"0000001234 00000 n");
        }
    }

    #[test]
    fn the_default_entry_terminator_is_the_one_pdfcer_always_emitted() {
        // The no-behaviour-change guard for `EOL-A1`: adding the knob must
        // not move the default off `SP LF`.
        let mut out = Vec::new();
        write_classic_entry(&mut out, &in_use(1234), 1, XrefEntryEol::default()).unwrap();
        assert_eq!(&out, b"0000001234 00000 n \n");
    }

    #[test]
    fn the_trailing_eol_is_the_only_byte_that_moves() {
        // `EOL-A2` (R169). §7.5.1 says every line is terminated; §7.5.5
        // says the last line contains only `%%EOF`. Both readings are
        // legitimate, so both are offered — and the difference must be
        // exactly one byte, on both tail forms, or something other than
        // the ambiguity changed.
        let mut trailer = Dict::new();
        trailer.insert(Name::from(b"Size"), Object::Integer(3));

        let mut with = Vec::new();
        write_classic_tail(&mut with, &trailer, 4096, TrailingEol::Lf);
        let mut without = Vec::new();
        write_classic_tail(&mut without, &trailer, 4096, TrailingEol::None);
        assert!(with.ends_with(b"%%EOF\n"));
        assert!(without.ends_with(b"%%EOF"));
        assert_eq!(with.len(), without.len() + 1);
        assert_eq!(&with[..without.len()], &without[..]);

        let mut with = Vec::new();
        write_stream_tail(&mut with, 77, TrailingEol::Lf);
        let mut without = Vec::new();
        write_stream_tail(&mut without, 77, TrailingEol::None);
        assert!(with.ends_with(b"%%EOF\n"));
        assert!(without.ends_with(b"%%EOF"));
        assert_eq!(with.len(), without.len() + 1);
    }

    #[test]
    fn the_default_tail_still_terminates_the_last_line() {
        // The no-behaviour-change guard for `EOL-A2`.
        let mut trailer = Dict::new();
        trailer.insert(Name::from(b"Size"), Object::Integer(3));
        let mut out = Vec::new();
        write_classic_tail(&mut out, &trailer, 4096, TrailingEol::default());
        assert!(out.ends_with(b"%%EOF\n"));
    }

    #[test]
    fn classic_table_never_emits_a_comment() {
        // §7.5.8.4: "PDF comments shall not be included in a
        // cross-reference table or in cross-reference streams."
        let t = table(&[
            (
                0,
                XrefEntry::Free {
                    next_free: 0,
                    generation: 65_535,
                },
            ),
            (1, in_use(9)),
        ]);
        let mut out = Vec::new();
        write_classic_table(&mut out, &t, XrefEntryEol::default()).unwrap();
        assert!(!out.contains(&b'%'), "comment leaked into an xref table");
    }

    #[test]
    fn classic_table_refuses_a_compressed_entry_by_name() {
        // A type-2 entry has no `n`/`f` representation; refusing beats
        // inventing one (R27 applied to the writer).
        let t = table(&[(
            4,
            XrefEntry::InStream {
                stream_num: 9,
                index: 2,
            },
        )]);
        let mut out = Vec::new();
        assert!(matches!(
            write_classic_table(&mut out, &t, XrefEntryEol::default()),
            Err(XrefOutError::CompressedInClassicTable {
                num: 4,
                container: 9
            })
        ));
    }

    #[test]
    fn classic_table_refuses_an_offset_that_cannot_fit_ten_digits() {
        let t = table(&[(1, in_use(MAX_CLASSIC_OFFSET + 1))]);
        let mut out = Vec::new();
        assert!(matches!(
            write_classic_table(&mut out, &t, XrefEntryEol::default()),
            Err(XrefOutError::OffsetTooLarge { .. })
        ));
        // The boundary value itself must still be writable.
        let t = table(&[(1, in_use(MAX_CLASSIC_OFFSET))]);
        let mut out = Vec::new();
        write_classic_table(&mut out, &t, XrefEntryEol::default()).unwrap();
        assert!(out.ends_with(b"9999999999 00000 n \n"));
    }

    #[test]
    fn classic_tail_puts_startxref_and_eof_on_their_own_lines() {
        // §7.5.5: "one per line and in order"; "%%EOF" alone on the
        // last line. `startxref 1234` on one line is non-conforming.
        let mut trailer = Dict::new();
        trailer.insert(Name::from(b"Size"), Object::Integer(9));
        trailer.insert(Name::from(b"Root"), Object::Reference(ObjId::new(1, 0)));
        let mut out = Vec::new();
        write_classic_tail(&mut out, &trailer, 4096, TrailingEol::default());
        let text = String::from_utf8(out).unwrap();
        assert_eq!(
            text,
            "trailer\n<</Size 9/Root 1 0 R>>\nstartxref\n4096\n%%EOF\n"
        );
    }

    #[test]
    fn stream_tail_omits_the_trailer_keyword() {
        // §7.5.8.1: in an xref-stream file "the keywords xref and
        // trailer shall no longer be used".
        let mut out = Vec::new();
        write_stream_tail(&mut out, 77, TrailingEol::default());
        assert_eq!(String::from_utf8(out).unwrap(), "startxref\n77\n%%EOF\n");
    }

    #[test]
    fn widths_never_emit_a_zero_second_field() {
        // Table 17 defines no default for field 2, so W[1] == 0 is
        // semantically undefined. Even an all-zero table must widen.
        let t = table(&[(
            0,
            XrefEntry::Free {
                next_free: 0,
                generation: 0,
            },
        )]);
        let w = Widths::fit(&t, [0, 0, 0]);
        assert_eq!(w.0[0], 1);
        assert_eq!(w.0[1], 1);
        assert_eq!(w.0[2], 1);
    }

    #[test]
    fn widths_grow_to_fit_and_honour_the_base_files_preference() {
        let t = table(&[(1, in_use(0x01_0000)), (2, in_use(5))]);
        // 0x010000 needs three bytes.
        assert_eq!(Widths::fit(&t, [1, 1, 1]).0, [1, 3, 1]);
        // The base file's wider choice is preserved (minimal diff):
        // re-emitting [1 4 2] beats silently shrinking to [1 3 1].
        assert_eq!(Widths::fit(&t, [1, 4, 2]).0, [1, 4, 2]);
        // A generation of 300 forces W[2] = 2 for the whole stream.
        let t = table(&[(
            1,
            XrefEntry::InUse {
                offset: 5,
                generation: 300,
            },
        )]);
        assert_eq!(Widths::fit(&t, [1, 1, 1]).0, [1, 1, 2]);
    }

    #[test]
    fn byte_width_boundaries() {
        assert_eq!(byte_width(0), 1);
        assert_eq!(byte_width(255), 1);
        assert_eq!(byte_width(256), 2);
        assert_eq!(byte_width(65_535), 2);
        assert_eq!(byte_width(65_536), 3);
        assert_eq!(byte_width(u64::MAX), 8);
    }

    #[test]
    fn stream_rows_are_big_endian() {
        // §7.5.8.3: "Each field shall be stored with the high-order
        // byte first." Getting this backwards produces a file that
        // loads and then resolves every object to garbage.
        let mut out = Vec::new();
        write_stream_row(
            &mut out,
            &XrefEntry::InUse {
                offset: 0x0102_0304,
                generation: 0x0506,
            },
            Widths([1, 4, 2]),
        );
        assert_eq!(out, vec![1, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06]);
    }

    #[test]
    fn stream_row_types_match_table_18() {
        let w = Widths([1, 2, 2]);
        let mut out = Vec::new();
        write_stream_row(
            &mut out,
            &XrefEntry::Free {
                next_free: 3,
                generation: 7,
            },
            w,
        );
        assert_eq!(out, vec![0, 0, 3, 0, 7]);

        let mut out = Vec::new();
        write_stream_row(
            &mut out,
            &XrefEntry::InStream {
                stream_num: 9,
                index: 4,
            },
            w,
        );
        assert_eq!(out, vec![2, 0, 9, 0, 4]);
    }

    #[test]
    fn xref_stream_index_is_ascending_and_length_is_direct() {
        let t = table(&[
            (
                0,
                XrefEntry::Free {
                    next_free: 0,
                    generation: 65_535,
                },
            ),
            (1, in_use(9)),
            (5, in_use(400)),
            (6, in_use(500)),
        ]);
        let mut keys = Dict::new();
        keys.insert(Name::from(b"Size"), Object::Integer(7));
        keys.insert(Name::from(b"Root"), Object::Reference(ObjId::new(1, 0)));
        let out = build_xref_stream(ObjId::new(6, 0), &t, Widths([1, 2, 1]), &keys).unwrap();
        let text = String::from_utf8_lossy(&out.bytes).into_owned();
        assert!(text.contains("/Index [0 2 5 2]"), "{text}");
        assert!(text.contains("/W [1 2 1]"), "{text}");
        assert!(text.contains("/Filter /FlateDecode"), "{text}");
        // E14: never an indirect /Length on an xref stream.
        assert!(text.contains("/Length "), "{text}");
        assert!(!text.contains("/Length 0 R"), "{text}");
        assert!(text.starts_with("6 0 obj\n"), "{text}");
    }

    #[test]
    fn xref_stream_drops_inherited_keys_it_must_own() {
        // A base trailer's /W, /Index, /Filter, /Length and /XRefStm
        // describe the OLD section; copying them forward would produce
        // a dictionary that contradicts the bytes beneath it.
        let t = table(&[(
            0,
            XrefEntry::Free {
                next_free: 0,
                generation: 65_535,
            },
        )]);
        let mut keys = Dict::new();
        keys.insert(Name::from(b"W"), Object::Array(vec![Object::Integer(9)]));
        keys.insert(Name::from(b"Index"), Object::Array(vec![]));
        keys.insert(Name::from(b"Length"), Object::Integer(999_999));
        keys.insert(Name::from(b"XRefStm"), Object::Integer(1));
        keys.insert(Name::from(b"Size"), Object::Integer(1));
        let out = build_xref_stream(ObjId::new(1, 0), &t, Widths([1, 1, 1]), &keys).unwrap();
        let text = String::from_utf8_lossy(&out.bytes).into_owned();
        assert!(!text.contains("999999"), "stale /Length survived: {text}");
        assert!(!text.contains("XRefStm"), "stale /XRefStm survived: {text}");
        assert!(text.contains("/W [1 1 1]"), "{text}");
    }

    #[test]
    fn xref_stream_data_decodes_back_to_the_rows_written() {
        let t = table(&[
            (
                0,
                XrefEntry::Free {
                    next_free: 0,
                    generation: 65_535,
                },
            ),
            (1, in_use(0xABCD)),
        ]);
        let mut keys = Dict::new();
        keys.insert(Name::from(b"Size"), Object::Integer(2));
        let widths = Widths([1, 2, 2]);
        let out = build_xref_stream(ObjId::new(1, 0), &t, widths, &keys).unwrap();
        // Pull the stream payload back out and inflate it.
        let body = &out.bytes;
        let start = body.windows(8).position(|w| w == b"\nstream\n").unwrap() + 8;
        let end = body
            .windows(11)
            .rposition(|w| w == b"\nendstream\n")
            .unwrap();
        let inflated = crate::filters::flate::decode(body.get(start..end).unwrap(), None).unwrap();
        assert_eq!(inflated.len(), 2 * widths.row_len());
        assert_eq!(&inflated[0..5], &[0, 0, 0, 255, 255]);
        assert_eq!(&inflated[5..10], &[1, 0xAB, 0xCD, 0, 0]);
    }

    #[test]
    fn flate_encode_round_trips_including_empty_and_incompressible() {
        for probe in [
            vec![],
            b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_vec(),
            (0u8..=255).cycle().take(9_000).collect::<Vec<u8>>(),
        ] {
            let enc = flate_encode(&probe).unwrap();
            let dec = crate::filters::flate::decode(&enc, None).unwrap();
            assert_eq!(dec, probe);
        }
    }

    #[test]
    fn empty_table_emits_a_legal_zero_entry_section() {
        // §7.5.8.4 EXAMPLE 2 blesses `xref` / `0 0` / `trailer` — an
        // update section need not contain any entries. `build_runs` of
        // an empty map yields no subsections at all, which is the same
        // thing with one fewer line.
        let t: BTreeMap<u32, XrefEntry> = BTreeMap::new();
        let mut out = Vec::new();
        write_classic_table(&mut out, &t, XrefEntryEol::default()).unwrap();
        assert_eq!(&out, b"xref\n");
    }
}
