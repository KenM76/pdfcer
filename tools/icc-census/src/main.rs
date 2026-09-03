//! # `icc-census` — what ICC profiles real PDFs actually contain
//!
//! A population census of every ICC profile embedded in pdfcer's rights-cleared
//! corpus, answering the six axes the sibling colour project `iccce` asked for
//! in `request_profile_population_census.md` (2026-08-17).
//!
//! ## Why the requester wants it, in their words
//!
//! `iccce` recommends a **33-node CLUT grid** for 3- and 4-channel sources in
//! its compiled fast path. Their own audit of that constant:
//!
//! > That evidence rests on **one profile pair, one direction, one tag type**
//! > […] So the constant may be fitted to an unrepresentative sample, and I
//! > have no way to find out from inside this repository. Every profile iccce
//! > has ever been tested against was synthetic, OS-shipped, or
//! > standards-body issued.
//!
//! A population distribution from real documents is the only evidence that
//! settles it, and it cannot be manufactured by more careful reasoning on
//! their side. pdfcer has the corpus; they do not.
//!
//! ## ★ Why this tool does not violate the iccce boundary
//!
//! `ARCHITECTURE.md` §12 decision 064 gives `iccce` **all colour conversion**
//! in this ecosystem, and pdfcer must never grow a CMM.
//!
//! **This tool converts nothing.** It reads the 128-byte profile header
//! (ICC.1:2010 §7.2) and the tag **table** (§7.3) — an inventory of which tags
//! are present, their offsets and their sizes — plus, for `curv`/`para`/`mft*`
//! /`mAB `/`mBA `, a handful of **structural** integers from the first bytes
//! of the tag (a curve's point count, a CLUT's grid dimension). It never
//! evaluates a curve, applies a matrix, or interpolates a CLUT. It is an
//! inventory of shapes, not a colour engine.
//!
//! It lives in `tools/` rather than in `pdfcer-core` deliberately, so that the
//! statement pdfcer made to `iccce` on 2026-08-25 — *"nothing in pdfcer has ever
//! decoded an ICC profile"* — stays true of the shipping engine.
//!
//! ## Method, stated because a number without its method is not a claim
//!
//! The requester asked explicitly for the method, and for the parse-failure
//! count, "since a census over the subset that parsed is a different claim
//! from a census over the corpus". Both are reported.
//!
//! ### How profiles are found: by SIGNATURE, not by reference-following
//!
//! Every indirect object in the document is visited. Every one that is a
//! stream is decoded, and a stream is treated as an ICC profile **iff** its
//! decoded bytes are at least 132 long and carry the ASCII signature `acsp`
//! at offset 36 — ICC.1:2010 §7.2.6, which fixes that field's value for every
//! conformant profile of every version.
//!
//! **This is deliberate and it is the opposite of the obvious design.** The
//! obvious design follows the references the request enumerates —
//! `/OutputIntents` `/DestOutputProfile`, `ICCBased` colour spaces, image
//! `/ColorSpace`, "anywhere". That design *under-counts by construction*: it
//! finds only the reference paths its author thought of, and a census whose
//! error direction is "silently missed some" is worse than useless, because
//! the miss looks like a finding about the population.
//!
//! Scanning by signature cannot miss a profile that is present as a stream,
//! whatever names the file uses to reach it. Its own error direction is the
//! safe one: it would over-count a stream that merely *looks* like a profile,
//! and `acsp` at a fixed offset plus a self-consistent size field makes that
//! essentially impossible.
//!
//! The reference paths are still recorded, but as a **classification of what
//! was found**, never as the search. A profile reachable by no path this tool
//! recognises is counted and reported as `unclassified` — a number that is
//! itself informative, since it measures how much of the population a
//! reference-following census would have dropped.
//!
//! ### Object streams
//!
//! Walked. `Document::objects()` yields objects from `/ObjStm` containers
//! exactly as it yields uncompressed ones — a profile cannot itself live in an
//! object stream (§7.5.7 forbids streams inside object streams), but the
//! `ICCBased` array and the colour-space dictionaries that name one routinely
//! do, which is why the reference classification needs them.
//!
//! ### Deduplication — reported BOTH ways, because they answer different questions
//!
//! Byte-identical profiles are deduplicated by SHA-free 64-bit FNV-1a over the
//! decoded bytes plus the length. Both totals are reported:
//!
//! * **embeddings** — every occurrence. Answers *"what does a CMM meet when it
//!   processes this corpus?"*, which is the question a performance constant
//!   like a grid size is really about.
//! * **distinct profiles** — deduplicated. Answers *"how many different things
//!   exist?"*, which is the question about coverage of tag shapes.
//!
//! The requester's own down-payment sample quoted both (98 PDFs → 121
//! embeddings → 20 distinct), so the two are directly comparable.
//!
//! ### Failures
//!
//! Counted, never silently dropped, in three separate buckets: the file did
//! not load, an individual stream did not decode, and the file exceeded its
//! wall-clock budget. A census that quietly excluded the files that broke it
//! would be measuring the easy half of the corpus and calling it the world.
//!
//! ## Usage
//!
//! ```text
//! cargo run --release -- [--tsv <path>] [--budget <secs>] <corpus-dir> [more-dirs …]
//! ```
//!
//! `--tsv` writes one row per DISTINCT profile with every axis, so the
//! requester can re-cut the distribution without re-running the scan. **No
//! profile bytes are ever written** — they asked not to receive them, citing
//! the licensing on several.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[cfg(test)]
mod tests;

use pdfcer_core::document::Document;
use pdfcer_core::object::Object;

/// Default per-file wall-clock budget.
///
/// A corpus is adversarial by nature — it contains the files that break
/// parsers, which is why it is worth having. A budget is not optional.
const DEFAULT_BUDGET_SECS: u64 = 20;

/// ICC.1:2010 §7.2: the profile header is exactly 128 bytes, and the tag
/// count immediately follows it as a `uInt32Number`.
const HEADER_LEN: usize = 128;

/// Offset of the `acsp` profile-file signature within the header
/// (ICC.1:2010 §7.2.6, `Table 14`). Its value is fixed for every conformant
/// profile of every version, which is what makes signature-scanning sound.
const SIG_OFFSET: usize = 36;

// ---------------------------------------------------------------------------
// The six axes the request asked for
// ---------------------------------------------------------------------------

/// One profile's census record — every axis the request named, and nothing
/// that would let the profile itself be reconstructed.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Profile {
    /// Decoded byte length. Reported because a profile's size is the cheapest
    /// proxy for whether it carries CLUTs at all.
    len: usize,
    /// **Axis 1 — `/N`.** Channel count of the DATA colour space, read from
    /// the header's colour-space signature (§7.2.10) rather than from the
    /// PDF's `/N` entry.
    ///
    /// ★ Read from the profile, not from the PDF, **on purpose** — and this
    /// is what makes axis 7 possible at all. §8.6.5.5 requires the PDF's `/N`
    /// to agree with the profile; whether it does in the wild is the
    /// requester's third ask, and it cannot be answered by a tool that trusts
    /// one of the two sources.
    header_channels: Option<u8>,
    /// The `/N` the *PDF* declared for this stream, when it declared one.
    pdf_n: Option<i64>,
    /// **Axis 2 — version**, as `major.minor` from the header's four-byte
    /// version field (§7.2.4; the minor and bug-fix nibbles share one byte).
    version: (u8, u8),
    /// **Axis 3 — device class** (§7.2.5): `mntr`, `prtr`, `scnr`, `link`,
    /// `spac`, `abst`, `nmcl`.
    class: [u8; 4],
    /// The data colour space signature (§7.2.7) — `GRAY`, `RGB `, `CMYK`, …
    space: [u8; 4],
    /// **Axis 5 — PCS** (§7.2.8): `XYZ ` or `Lab `.
    pcs: [u8; 4],
    /// **Axis 4 — the transform tag shape.** THE ONE THE REQUEST SAID IT MOST
    /// NEEDS AND EXPECTED TO BE DROPPED, being "a level deeper than the other
    /// three and requires opening the tag table".
    transform: TransformShape,
    /// **Axis 6 — CLUT grid size**, when a lookup tag carries one.
    ///
    /// `mft1`/`mft2` state a single grid dimension shared by every input
    /// channel (ICC.1:2010 §10.10/§10.11 byte 10). `mAB `/`mBA ` carry a
    /// per-channel array (§10.12/§10.13); the maximum is reported, since a
    /// grid recommendation is about the largest table a CMM must build.
    clut_grid: Option<u8>,
    /// **Axis 6 — the `desc` string**, so repeated real-world profiles can be
    /// named. Truncated, ASCII-filtered, and reported only in aggregate.
    desc: Option<String>,
    /// Every four-character tag signature in the tag table, in file order.
    tags: Vec<[u8; 4]>,
    /// **The requester's THIRD ask, and it is a question about the profile's
    /// internal consistency — not about the PDF.**
    ///
    /// `Some((declared, in_tag))` when a lookup tag's channel count disagrees
    /// with the header's data colour space; `None` when they agree or when
    /// there is no lookup tag to compare against.
    ///
    /// ICC.1:2022 is **silent** on this — the requester established that as a
    /// sourced finding rather than an assumption. No clause requires a LUT
    /// tag's channel count to agree with the header's data colour space, and
    /// none says what a reader should do on mismatch. The only two `shall`s
    /// binding a count are §10.4 `colorantOrderType` and §10.5
    /// `colorantTableType`, neither of which is a LUT.
    ///
    /// So this is not a conformance check. It is a measurement of whether the
    /// disagreement their cross-check defends against **occurs**, which is
    /// the difference between dead code and a noisy warning.
    ///
    /// A2B0 runs device → PCS, so its INPUT channel count is the data space's.
    /// B2A0 runs PCS → device, so its OUTPUT channel count is. Both are
    /// checked; the first disagreement found is reported.
    channel_disagreement: Option<(u8, u8)>,
    /// Whether a LUT tag existed to compare against at all.
    ///
    /// ★ Split out from [`Self::channel_disagreement`] deliberately. The
    /// first draft bucketed *"agree"* and *"there was nothing to compare"*
    /// together, and reported 100 % agreement across 2,494 embeddings — a
    /// number that looks like a strong negative result and would have been
    /// equally consistent with the check never having run. A tally that
    /// cannot distinguish "checked and clean" from "not checked" is the
    /// under-reporting failure this project keeps finding in its own gates.
    channel_comparable: bool,
}

/// The shape of a profile's colour transform — the request's axis 4.
///
/// Determined by which tags are present, in the precedence a CMM would apply
/// them: ICC.1:2010 §8.3 makes `A2B0`/`B2A0` the general path and the
/// matrix/TRC form a special case available only to three-component
/// `XYZ `-PCS profiles (§8.3.2), so a profile carrying both is using the
/// lookup tables.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TransformShape {
    /// `A2B0`/`B2A0` present and typed `mft1` (8-bit LUT, §10.10).
    Mft1,
    /// `A2B0`/`B2A0` present and typed `mft2` (16-bit LUT, §10.11).
    Mft2,
    /// `A2B0` typed `mAB ` (§10.12) — the v4 general form.
    MAb,
    /// `B2A0` typed `mBA ` (§10.13) with no `mAB ` alongside.
    MBa,
    /// Matrix (`rXYZ`/`gXYZ`/`bXYZ`) plus `curv` TRCs — the §8.3.2 shortcut.
    MatrixCurv,
    /// Matrix plus `para` (parametric) TRCs — the v4 spelling of the same.
    MatrixPara,
    /// `kTRC` only, no matrix and no LUT: a monochrome profile (§8.3.3).
    ///
    /// ★ The request called this shape out by name — *"one profile is 4 tags
    /// and `kTRC`-only […] structurally degenerate […] a shape a CMM will get
    /// wrong if it has only ever seen CLUT profiles"*.
    KTrcOnly,
    /// A lookup tag exists but carries a type signature none of the above
    /// names. Reported rather than folded into `Other`, because an unknown
    /// TYPE on a known TAG is a different finding from a missing tag.
    UnknownLutType,
    /// None of the above — no recognised transform tag at all.
    Other,
}

impl TransformShape {
    const fn label(self) -> &'static str {
        match self {
            Self::Mft1 => "mft1",
            Self::Mft2 => "mft2",
            Self::MAb => "mAB ",
            Self::MBa => "mBA ",
            Self::MatrixCurv => "matrix+curv",
            Self::MatrixPara => "matrix+para",
            Self::KTrcOnly => "kTRC-only",
            Self::UnknownLutType => "lut/unknown-type",
            Self::Other => "other",
        }
    }
}

/// Where in the PDF a profile was reached from.
///
/// A **classification of what the signature scan found**, never the search
/// itself — see the module docs on why the search must not be
/// reference-driven.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Reference {
    /// `/OutputIntents[] /DestOutputProfile` (§14.11.5).
    OutputIntent,
    /// The stream named by an `[/ICCBased <stream>]` colour-space array
    /// (§8.6.5.5).
    IccBased,
    /// Found by signature and reachable by no path this tool recognises.
    ///
    /// ★ **This bucket is itself a measurement**: it is how much of the
    /// population a reference-following census would silently have dropped.
    Unclassified,
}

impl Reference {
    const fn label(self) -> &'static str {
        match self {
            Self::OutputIntent => "/OutputIntents /DestOutputProfile",
            Self::IccBased => "ICCBased",
            Self::Unclassified => "unclassified (signature-only)",
        }
    }
}

// ---------------------------------------------------------------------------
// Header + tag-table reading (ICC.1:2010 §7.2, §7.3)
// ---------------------------------------------------------------------------

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at + 4)?;
    Some(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
}

fn sig(b: &[u8], at: usize) -> Option<[u8; 4]> {
    let s = b.get(at..at + 4)?;
    Some([s[0], s[1], s[2], s[3]])
}

/// The channel count implied by a data-colour-space signature (§7.2.7,
/// Table 19), which is where a profile itself states what `/N` must be.
const fn channels_of(space: [u8; 4]) -> Option<u8> {
    match &space {
        b"GRAY" => Some(1),
        b"RGB " | b"XYZ " | b"Lab " | b"Luv " | b"YCbr" | b"Yxy " | b"HSV " | b"HLS " | b"CMY " => {
            Some(3)
        }
        b"CMYK" => Some(4),
        b"2CLR" => Some(2),
        b"3CLR" => Some(3),
        b"4CLR" => Some(4),
        b"5CLR" => Some(5),
        b"6CLR" => Some(6),
        b"7CLR" => Some(7),
        b"8CLR" => Some(8),
        b"9CLR" => Some(9),
        b"ACLR" => Some(10),
        b"BCLR" => Some(11),
        b"CCLR" => Some(12),
        b"DCLR" => Some(13),
        b"ECLR" => Some(14),
        b"FCLR" => Some(15),
        _ => None,
    }
}

/// Parse the header and tag table. `None` when the bytes are not a profile.
///
/// Structural checks only — no colour value is ever computed. Deliberately
/// tolerant about everything except the two facts that make the bytes
/// *identifiable*: the `acsp` signature, and a tag count that fits.
fn parse(bytes: &[u8], pdf_n: Option<i64>) -> Option<Profile> {
    if bytes.len() < HEADER_LEN + 4 || sig(bytes, SIG_OFFSET)? != *b"acsp" {
        return None;
    }
    let version_raw = be_u32(bytes, 8)?;
    // §7.2.4: byte 0 is the major version; byte 1 packs minor (high nibble)
    // and bug-fix (low nibble) as BCD.
    let version = (
        u8::try_from(version_raw >> 24).unwrap_or(0),
        u8::try_from((version_raw >> 20) & 0x0f).unwrap_or(0),
    );
    let class = sig(bytes, 12)?;
    let space = sig(bytes, 16)?;
    let pcs = sig(bytes, 20)?;

    // §7.3: the tag table is a count followed by that many 12-byte entries.
    let count = be_u32(bytes, HEADER_LEN)? as usize;
    // A malformed count must not make this allocate gigabytes. A profile with
    // more than 4,096 tags does not exist; the cap is a guard, not a claim.
    if count > 4096 {
        return None;
    }
    let mut tags = Vec::with_capacity(count);
    let mut located: HashMap<[u8; 4], (usize, usize)> = HashMap::new();
    for i in 0..count {
        let at = HEADER_LEN + 4 + i * 12;
        let Some(name) = sig(bytes, at) else { break };
        let Some(off) = be_u32(bytes, at + 4) else {
            break;
        };
        let Some(size) = be_u32(bytes, at + 8) else {
            break;
        };
        tags.push(name);
        located.insert(name, (off as usize, size as usize));
    }

    let tag_type = |name: &[u8; 4]| -> Option<[u8; 4]> {
        let (off, size) = *located.get(name)?;
        if size < 4 {
            return None;
        }
        sig(bytes, off)
    };
    let has = |name: &[u8; 4]| located.contains_key(name);

    // Axis 4. §8.3 precedence: the lookup tags ARE the transform when
    // present; the matrix/TRC form is the §8.3.2 special case, available only
    // to three-component XYZ-PCS profiles, so a profile carrying both is
    // using the tables.
    let lut = tag_type(b"A2B0").or_else(|| tag_type(b"B2A0"));
    let transform = match lut {
        Some(t) if &t == b"mft1" => TransformShape::Mft1,
        Some(t) if &t == b"mft2" => TransformShape::Mft2,
        Some(t) if &t == b"mAB " => TransformShape::MAb,
        Some(t) if &t == b"mBA " => TransformShape::MBa,
        Some(_) => TransformShape::UnknownLutType,
        None => {
            let matrix = has(b"rXYZ") && has(b"gXYZ") && has(b"bXYZ");
            let trc = tag_type(b"rTRC");
            match (matrix, trc) {
                (true, Some(t)) if &t == b"para" => TransformShape::MatrixPara,
                (true, Some(_)) => TransformShape::MatrixCurv,
                (true, None) => TransformShape::MatrixCurv,
                (false, _) if has(b"kTRC") => TransformShape::KTrcOnly,
                _ => TransformShape::Other,
            }
        }
    };

    // Axis 6 — the CLUT grid dimension, read as a STRUCTURAL integer out of
    // the lookup tag's fixed-position fields. No table is read.
    let clut_grid = located
        .get(b"A2B0")
        .or_else(|| located.get(b"B2A0"))
        .and_then(|&(off, size)| clut_grid_points(bytes, off, size));

    // The requester's third ask. A2B0's INPUT count and B2A0's OUTPUT count
    // are each the data colour space's channel count (ICC.1:2010 Table 41);
    // both LUT families put those two bytes at offsets 8 and 9 of the tag.
    let mut channel_comparable = false;
    let channel_disagreement = channels_of(space).and_then(|declared| {
        for (name, byte) in [(b"A2B0", 8usize), (b"B2A0", 9usize)] {
            let Some(&(off, size)) = located.get(name) else {
                continue;
            };
            if size < 12 {
                continue;
            }
            let Some(t) = sig(bytes, off) else { continue };
            if !matches!(&t, b"mft1" | b"mft2" | b"mAB " | b"mBA ") {
                continue;
            }
            if let Some(&in_tag) = bytes.get(off + byte)
                && in_tag != 0
            {
                channel_comparable = true;
                if in_tag != declared {
                    return Some((declared, in_tag));
                }
            }
        }
        None
    });

    let desc = located
        .get(b"desc")
        .and_then(|&(off, size)| read_desc(bytes, off, size));

    Some(Profile {
        len: bytes.len(),
        header_channels: channels_of(space),
        pdf_n,
        version,
        class,
        space,
        pcs,
        transform,
        clut_grid,
        desc,
        tags,
        channel_disagreement,
        channel_comparable,
    })
}

/// The CLUT grid dimension declared by a lookup tag, if it has one.
///
/// `mft1` (§10.10) and `mft2` (§10.11) both put the number of CLUT grid
/// points at byte **10** of the tag, as a single `uInt8Number` shared by every
/// input channel. `mAB `/`mBA ` (§10.12/§10.13) instead reach a `clut` element
/// carrying a **16-byte per-channel array**, of which the maximum is returned
/// — a grid recommendation is about the largest table a CMM must build.
///
/// Returns `None` for a tag with no CLUT at all, which is a real and
/// interesting answer rather than a failure: a matrix/TRC profile and a
/// `kTRC`-only profile both have no grid, and the request singled the second
/// out as *"a shape a CMM will get wrong if it has only ever seen CLUT
/// profiles"*.
fn clut_grid_points(bytes: &[u8], off: usize, size: usize) -> Option<u8> {
    let tag = bytes.get(off..off.checked_add(size)?)?;
    match sig(tag, 0)? {
        t if &t == b"mft1" || &t == b"mft2" => tag.get(10).copied().filter(|&g| g > 0),
        t if &t == b"mAB " || &t == b"mBA " => {
            // §10.12: the element offsets follow the 8-byte header; the CLUT
            // offset is at byte 24, relative to the tag's own start.
            let clut_off = be_u32(tag, 24)? as usize;
            if clut_off == 0 {
                return None;
            }
            let clut = tag.get(clut_off..)?;
            // §10.12.5: 16 bytes of per-channel grid points, then precision.
            clut.get(0..16)?.iter().copied().filter(|&g| g > 0).max()
        }
        _ => None,
    }
}

/// The `desc` tag's display string, ASCII-filtered and truncated.
///
/// Handles both spellings, because the corpus spans both ICC versions: v2's
/// `desc` (§6.5.17 of ICC.1:2001) puts an ASCII count at byte 8 and the text
/// at byte 12; v4's `mluc` (§10.15) is a record table whose first record's
/// UTF-16BE text is reached through offsets at bytes 20/24.
fn read_desc(bytes: &[u8], off: usize, size: usize) -> Option<String> {
    let tag = bytes.get(off..off.checked_add(size)?)?;
    let out = match sig(tag, 0)? {
        t if &t == b"desc" => {
            let n = be_u32(tag, 8)? as usize;
            let text = tag.get(12..12 + n.min(256))?;
            text.iter()
                .copied()
                .take_while(|&c| c != 0)
                .filter(|&c| (0x20..0x7f).contains(&c))
                .map(char::from)
                .collect::<String>()
        }
        t if &t == b"mluc" => {
            let len = be_u32(tag, 20)? as usize;
            let at = be_u32(tag, 24)? as usize;
            let text = tag.get(at..at + len.min(512))?;
            // UTF-16BE; the census only needs it recognisable, so the high
            // byte of each unit is dropped rather than decoded properly.
            text.chunks_exact(2)
                .map(|p| p[1])
                .take_while(|&c| c != 0)
                .filter(|&c| (0x20..0x7f).contains(&c))
                .map(char::from)
                .collect::<String>()
        }
        _ => return None,
    };
    let trimmed = out.trim();
    (!trimmed.is_empty()).then(|| trimmed.chars().take(72).collect())
}

/// FNV-1a over the decoded bytes — deduplication only, never a security claim.
fn fingerprint(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

// ---------------------------------------------------------------------------
// Scanning one document
// ---------------------------------------------------------------------------

/// Record which object ids a value NAMES as an ICC profile, recursively.
///
/// Depth-guarded rather than trusted: a corpus contains malformed files, and
/// `Document::resolve` is not being used here (this walks parsed values in
/// place, so a cycle through indirect references is impossible) — but a
/// deeply nested array of arrays is not, and 64 is far past any real
/// document's resource nesting.
fn classify_into(
    value: &Object,
    refs: &mut HashMap<pdfcer_core::object::ObjId, Reference>,
    depth: u32,
) {
    if depth > 64 {
        return;
    }
    match value {
        Object::Array(items) => {
            // `[/ICCBased <stream>]` — §8.6.5.5.
            if let (Some(Object::Name(n)), Some(Object::Reference(id))) =
                (items.first(), items.get(1))
                && n.as_bytes() == b"ICCBased"
            {
                refs.entry(*id).or_insert(Reference::IccBased);
            }
            for i in items {
                classify_into(i, refs, depth + 1);
            }
        }
        Object::Dict(d) => {
            // `/OutputIntents` entries — §14.11.5.
            if let Some(Object::Reference(id)) = d.get(b"DestOutputProfile") {
                // An output intent's profile wins over an ICCBased mention:
                // it is the more specific role, and a profile serving both
                // is doing the output-intent job.
                refs.insert(*id, Reference::OutputIntent);
            }
            for (_, v) in d.iter() {
                classify_into(v, refs, depth + 1);
            }
        }
        Object::Stream(st) => {
            for (_, v) in st.dict.iter() {
                classify_into(v, refs, depth + 1);
            }
        }
        _ => {}
    }
}

/// What one file contributed.
#[derive(Debug, Default)]
struct FileResult {
    /// `(fingerprint, profile, reference)` per embedding.
    found: Vec<(u64, Profile, Reference)>,
    /// Streams whose filters would not decode. Counted, never dropped.
    undecodable_streams: usize,
}

/// Scan one already-loaded document.
///
/// Two passes, and the order matters. The reference map is built FIRST so
/// that every profile the signature scan then finds can be classified — the
/// reverse order would need a second walk.
fn scan(doc: &Document) -> FileResult {
    let mut out = FileResult::default();

    // --- pass 1: which object ids are named as profiles, and how ---
    //
    // RECURSIVE, and that is not a refinement. An `[/ICCBased <stream>]`
    // array is very rarely an indirect object of its own: it normally sits
    // inline inside a page's `/Resources /ColorSpace` dictionary, which is
    // itself often inline inside the page. A walk that inspected only
    // top-level object VALUES saw almost none of them and pushed the
    // profiles it found into `Unclassified` — reported on the first smoke
    // run as 1 of 2, which reads as a finding about the corpus and was
    // really a finding about this loop.
    let mut refs: HashMap<pdfcer_core::object::ObjId, Reference> = HashMap::new();
    for io in doc.objects() {
        classify_into(&io.value, &mut refs, 0);
    }
    // `/OutputIntents` reached from the catalog, for the case where the
    // array and its dictionaries are all inline.
    if let Ok(cat) = doc.catalog()
        && let Some(intents) = cat.get(b"OutputIntents")
    {
        classify_into(intents, &mut refs, 0);
    }

    // --- pass 2: the signature scan, which is the actual census ---
    for io in doc.objects() {
        let Object::Stream(stream) = &io.value else {
            continue;
        };
        let Some(raw) = stream.data_span.slice(doc.bytes()) else {
            continue;
        };
        let decoded = match pdfcer_core::filters::decode_stream(&stream.dict, raw) {
            Ok(d) => d,
            Err(_) => {
                // Only counted when the stream could plausibly have been a
                // profile: counting every undecodable stream in the corpus
                // would report a number about filters, not about profiles.
                if stream.dict.get(b"N").is_some() {
                    out.undecodable_streams += 1;
                }
                continue;
            }
        };
        let pdf_n = stream.dict.get(b"N").and_then(Object::as_int);
        let Some(profile) = parse(&decoded, pdf_n) else {
            continue;
        };
        let how = refs.get(&io.id).copied().unwrap_or(Reference::Unclassified);
        out.found.push((fingerprint(&decoded), profile, how));
    }
    out
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

#[derive(Default)]
struct Census {
    files_seen: usize,
    files_failed_load: usize,
    files_timed_out: usize,
    files_panicked: usize,
    files_with_profiles: usize,
    embeddings: usize,
    undecodable_streams: usize,
    /// fingerprint → (profile, how many embeddings, which references)
    distinct: HashMap<u64, (Profile, usize, BTreeMap<Reference, usize>)>,
    /// ★ MEASURED PER EMBEDDING, not extrapolated from one sample per
    /// distinct profile.
    ///
    /// The first draft of this tool weighted a distinct profile's
    /// first-seen `pdf_n` by its embedding count — which silently assumes
    /// every embedding of one profile declares the same `/N`. That is
    /// exactly the assumption the axis exists to test, so the number was
    /// circular. Caught by reading the TSV and noticing that "91 disagree"
    /// came from ONE row.
    ///
    /// `/N` is per-STREAM; the profile bytes are shared. Two embeddings of
    /// the same profile can and do declare different `/N`, and only a
    /// per-embedding tally can say so.
    n_disagree_embeddings: usize,
    /// Every `/N`-disagreeing embedding, named.
    ///
    /// A count alone cannot say WHICH profiles disagree, and the per-distinct
    /// TSV cannot either — its `/N` column is one sample. Without this, the
    /// only honest sentence about the disagreements is "there are two", which
    /// is not enough for the requester to look into them.
    n_disagree_detail: Vec<(String, u8, i64)>,
    n_absent_embeddings: usize,
    n_present_embeddings: usize,
}

impl Census {
    fn absorb(&mut self, r: FileResult) {
        self.undecodable_streams += r.undecodable_streams;
        if !r.found.is_empty() {
            self.files_with_profiles += 1;
        }
        for (fp, profile, how) in r.found {
            self.embeddings += 1;
            match (profile.pdf_n, profile.header_channels) {
                (None, _) => self.n_absent_embeddings += 1,
                (Some(pdf), Some(hdr)) => {
                    self.n_present_embeddings += 1;
                    if pdf != i64::from(hdr) {
                        self.n_disagree_embeddings += 1;
                        self.n_disagree_detail.push((
                            profile
                                .desc
                                .clone()
                                .unwrap_or_else(|| "(no desc)".to_owned()),
                            hdr,
                            pdf,
                        ));
                    }
                }
                (Some(_), None) => self.n_present_embeddings += 1,
            }
            let e = self
                .distinct
                .entry(fp)
                .or_insert_with(|| (profile, 0, BTreeMap::new()));
            e.1 += 1;
            *e.2.entry(how).or_insert(0) += 1;
        }
    }
}

/// Print a `name → count` distribution, sorted by count descending, with the
/// denominator spelled out on every line.
///
/// The denominator is not decoration. This project has repeatedly found that
/// a bare count reads as a rate, and the requester is about to write these
/// numbers into their own `NUMERIC_CLAIMS.md`.
fn distribution(title: &str, counts: &BTreeMap<String, usize>, total: usize) {
    println!("\n{title}  (denominator {total})");
    let mut rows: Vec<_> = counts.iter().collect();
    rows.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (name, n) in rows {
        let pct = if total == 0 {
            0.0
        } else {
            *n as f64 * 100.0 / total as f64
        };
        println!("  {n:>7}  {pct:>5.1}%  {name}");
    }
}

fn ascii(s: [u8; 4]) -> String {
    s.iter()
        .map(|&c| {
            if (0x20..0x7f).contains(&c) {
                char::from(c)
            } else {
                '?'
            }
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Driver
// ---------------------------------------------------------------------------

fn collect_pdfs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_pdfs(&p, out);
        } else if p.extension().is_some_and(|x| x.eq_ignore_ascii_case("pdf")) {
            out.push(p);
        }
    }
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("icc-census: {msg}");
    eprintln!("usage: icc-census [--tsv <path>] [--budget <secs>] <corpus-dir> [more-dirs …]");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let mut dirs: Vec<PathBuf> = Vec::new();
    let mut tsv: Option<PathBuf> = None;
    let mut budget = Duration::from_secs(DEFAULT_BUDGET_SECS);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--tsv" => match args.next() {
                Some(p) => tsv = Some(PathBuf::from(p)),
                None => return usage("--tsv needs a path"),
            },
            "--budget" => match args.next().and_then(|s| s.parse::<u64>().ok()) {
                Some(s) if s > 0 => budget = Duration::from_secs(s),
                _ => return usage("--budget needs a positive number of seconds"),
            },
            other => dirs.push(PathBuf::from(other)),
        }
    }
    if dirs.is_empty() {
        return usage("no corpus directory given");
    }

    let mut files = Vec::new();
    for d in &dirs {
        collect_pdfs(d, &mut files);
    }
    files.sort();
    if files.is_empty() {
        return usage("no .pdf files found under the given directories");
    }
    eprintln!("icc-census: {} file(s)", files.len());

    let started = Instant::now();
    let mut census = Census::default();
    for (i, path) in files.iter().enumerate() {
        if i % 250 == 0 && i > 0 {
            eprintln!(
                "  … {i}/{} ({:.0}s)",
                files.len(),
                started.elapsed().as_secs_f64()
            );
        }
        census.files_seen += 1;

        // Each file is scanned on its own thread with a wall-clock budget: a
        // corpus contains the files that break parsers, and a hang would
        // otherwise cost the whole run. A panic becomes a counted outcome
        // rather than an abort, for the same reason.
        let p = path.clone();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            let r = std::panic::catch_unwind(|| match Document::load(&p) {
                Ok(doc) => Some(scan(&doc)),
                Err(_) => None,
            });
            let _ = tx.send(r);
        });
        match rx.recv_timeout(budget) {
            Ok(Ok(Some(r))) => {
                census.absorb(r);
                let _ = handle.join();
            }
            Ok(Ok(None)) => {
                census.files_failed_load += 1;
                let _ = handle.join();
            }
            Ok(Err(_)) => {
                census.files_panicked += 1;
                let _ = handle.join();
            }
            Err(_) => {
                // The thread is left running and detached: killing it is not
                // possible in safe Rust, and the alternative — waiting — is
                // exactly what the budget exists to avoid.
                census.files_timed_out += 1;
            }
        }
    }

    report(&census, started.elapsed());
    if let Some(path) = tsv
        && let Err(e) = write_tsv(&census, &path)
    {
        eprintln!("icc-census: could not write {}: {e}", path.display());
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

fn report(c: &Census, elapsed: Duration) {
    let parsed = c.files_seen - c.files_failed_load - c.files_timed_out - c.files_panicked;
    println!("\n=== icc-census ===\n");
    println!("files seen                {:>7}", c.files_seen);
    println!("  parsed                  {parsed:>7}");
    println!("  failed to load          {:>7}", c.files_failed_load);
    println!("  exceeded budget         {:>7}", c.files_timed_out);
    println!("  panicked                {:>7}", c.files_panicked);
    println!("files carrying >=1 profile{:>7}", c.files_with_profiles);
    println!("embeddings                {:>7}", c.embeddings);
    println!("distinct profiles         {:>7}", c.distinct.len());
    println!(
        "undecodable /N streams    {:>7}   (streams that declared /N but whose filters failed)",
        c.undecodable_streams
    );
    println!("elapsed                   {:>7.0}s", elapsed.as_secs_f64());

    // Every distribution is reported over BOTH denominators, because they
    // answer different questions — see the module docs.
    for (label, by_embedding) in [("distinct profiles", false), ("embeddings", true)] {
        let total = if by_embedding {
            c.embeddings
        } else {
            c.distinct.len()
        };
        let weight = |n: usize| if by_embedding { n } else { 1 };
        println!("\n\n----- by {label} -----");

        let mut classes = BTreeMap::new();
        let mut versions = BTreeMap::new();
        let mut spaces = BTreeMap::new();
        let mut pcs = BTreeMap::new();
        let mut shapes = BTreeMap::new();
        let mut grids = BTreeMap::new();
        let mut refs = BTreeMap::new();
        let mut chan_disagree = BTreeMap::new();
        let mut n_mismatch = 0usize;
        let mut n_absent = 0usize;
        for (p, n, how) in c.distinct.values() {
            let w = weight(*n);
            *classes.entry(ascii(p.class)).or_insert(0) += w;
            *versions
                .entry(format!("v{}.{}", p.version.0, p.version.1))
                .or_insert(0) += w;
            *spaces
                .entry(match p.header_channels {
                    Some(ch) => format!("{} ({ch} channel)", ascii(p.space)),
                    None => format!("{} (unknown channel count)", ascii(p.space)),
                })
                .or_insert(0) += w;
            *pcs.entry(ascii(p.pcs)).or_insert(0) += w;
            *shapes.entry(p.transform.label().to_owned()).or_insert(0) += w;
            *grids
                .entry(match p.clut_grid {
                    Some(g) => format!("{g} nodes"),
                    None => "no CLUT".to_owned(),
                })
                .or_insert(0) += w;
            for (r, rn) in how {
                *refs.entry(r.label().to_owned()).or_insert(0) +=
                    if by_embedding { *rn } else { 1 };
            }
            if let Some((declared, in_tag)) = p.channel_disagreement {
                *chan_disagree
                    .entry(format!("header {declared} channel(s), LUT tag {in_tag}"))
                    .or_insert(0) += w;
            } else if p.channel_comparable {
                *chan_disagree
                    .entry("CHECKED and agree".to_owned())
                    .or_insert(0) += w;
            } else {
                *chan_disagree
                    .entry("not checkable (no LUT tag carrying a channel count)".to_owned())
                    .or_insert(0) += w;
            }
            if !by_embedding {
                match (p.pdf_n, p.header_channels) {
                    (Some(pdf), Some(hdr)) if pdf != i64::from(hdr) => n_mismatch += 1,
                    (None, _) => n_absent += 1,
                    _ => {}
                }
            }
        }
        distribution("device class (axis 3)", &classes, total);
        distribution("version (axis 2)", &versions, total);
        distribution("data space and channel count (axis 1)", &spaces, total);
        distribution("PCS (axis 5)", &pcs, total);
        distribution("transform tag shape (axis 4)", &shapes, total);
        distribution("CLUT grid size (axis 6)", &grids, total);
        distribution("reached from", &refs, total);
        distribution(
            "★ ask 3 — header data space vs LUT tag channel count",
            &chan_disagree,
            total,
        );
        if by_embedding {
            // MEASURED per embedding, because `/N` lives on the STREAM and
            // the profile bytes are shared: two embeddings of one profile can
            // declare different `/N`, which is the very thing being tested.
            let d = c.n_disagree_embeddings;
            let total_n = c.n_present_embeddings;
            let pct = if total_n == 0 {
                0.0
            } else {
                d as f64 * 100.0 / total_n as f64
            };
            println!(
                "
PDF /N vs header channel count — MEASURED per embedding"
            );
            println!("  {d:>7}  {pct:>5.1}%  DISAGREE  (of {total_n} that declared /N)");
            println!(
                "  {:>7}         no /N declared by the PDF",
                c.n_absent_embeddings
            );
            // Each disagreement NAMED. A count alone cannot say which
            // profiles disagree, and the per-distinct TSV cannot either —
            // its `/N` column is one sample per fingerprint. Without this
            // the only honest sentence about them is "there are two", which
            // is not enough for the requester to go and look.
            for (desc, hdr, pdf) in &c.n_disagree_detail {
                println!(
                    "          profile says {hdr} channel(s), the PDF says /N {pdf}  --  {desc}"
                );
            }
        } else {
            println!(
                "
PDF /N vs header channel count  (denominator {total} distinct)"
            );
            println!("  {n_mismatch:>7}  DISAGREE");
            println!("  {n_absent:>7}  no /N declared by the PDF");
        }
    }

    // The named profiles, for the requester's "so repeated real-world
    // profiles can be named".
    let mut named: Vec<_> = c
        .distinct
        .values()
        .filter_map(|(p, n, _)| {
            p.desc
                .as_ref()
                .map(|d| (*n, d.clone(), p.transform, p.clut_grid))
        })
        .collect();
    named.sort_by_key(|r| std::cmp::Reverse(r.0));
    println!("\n\n----- the 30 most-embedded named profiles -----");
    println!("{:>7}  {:<14} {:<10} desc", "embeds", "transform", "grid");
    for (n, desc, shape, grid) in named.iter().take(30) {
        let g = grid.map_or_else(|| "-".to_owned(), |g| g.to_string());
        println!("{n:>7}  {:<14} {g:<10} {desc}", shape.label());
    }
}

fn write_tsv(c: &Census, path: &Path) -> std::io::Result<()> {
    let mut f = fs::File::create(path)?;
    writeln!(
        f,
        "embeddings\tbytes\tclass\tversion\tspace\theader_channels\tpdf_n\tpcs\ttransform\tclut_grid\ttag_count\tdesc"
    )?;
    let mut rows: Vec<_> = c.distinct.values().collect();
    rows.sort_by_key(|r| std::cmp::Reverse(r.1));
    for (p, n, _) in rows {
        writeln!(
            f,
            "{n}\t{}\t{}\tv{}.{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            p.len,
            ascii(p.class),
            p.version.0,
            p.version.1,
            ascii(p.space),
            p.header_channels
                .map_or_else(|| "?".to_owned(), |v| v.to_string()),
            p.pdf_n.map_or_else(|| "-".to_owned(), |v| v.to_string()),
            ascii(p.pcs),
            p.transform.label(),
            p.clut_grid
                .map_or_else(|| "-".to_owned(), |v| v.to_string()),
            p.tags.len(),
            p.desc.as_deref().unwrap_or("")
        )?;
    }
    Ok(())
}
