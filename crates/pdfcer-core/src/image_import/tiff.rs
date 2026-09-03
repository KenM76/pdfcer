//! # TIFF import (TIFF 6.0, 1992-06-03) — the format scanners and CAD tools emit
//!
//! Turns a baseline TIFF into a PDF image XObject. TIFF is the notable gap in
//! the PNG/JPEG/BMP set this module started with, and it is the gap an
//! operator hits first: a flatbed scanner, a large-format plotter's "save as
//! raster", and most CAD export dialogs all offer TIFF before they offer
//! anything else. A drag-and-drop that silently rejects it is a hole in the
//! feature, not a missing nicety.
//!
//! ## TIFF is a container, not a codec — which is the whole difficulty
//!
//! PNG has one layout. JPEG has one codestream. A TIFF is a **directory of
//! tags** that describes an arbitrary sample grid, and the same picture can be
//! stored in dozens of legal ways: two byte orders, five compressions this
//! module accepts and a dozen it does not, strips or tiles, chunky or planar
//! channels, six photometric interpretations, its own predictor, and an
//! arbitrary number of *pages* chained one after another. Nothing in the file
//! is redundant enough to catch a misread — a wrong `PlanarConfiguration`
//! produces a plausible-looking, entirely wrong picture rather than an error.
//!
//! So the shape of this module is: **read the directory, decide whether every
//! tag combination it declares is one pdfcer can represent faithfully, and
//! refuse by name the moment it is not.** The decode itself is the small part.
//!
//! ## What is accepted (the baseline)
//!
//! | Property | Accepted | Tag |
//! |---|---|---|
//! | Byte order | `II` (little) and `MM` (big) | header |
//! | Version | 42 (classic TIFF) | header |
//! | Compression | 1 none, 5 LZW, 8/32946 Deflate, 32773 PackBits | `Compression` (259) |
//! | Photometric | 0 `WhiteIsZero`, 1 `BlackIsZero`, 2 `RGB`, 3 `Palette` | `PhotometricInterpretation` (262) |
//! | Bits per sample | 1, 2, 4, 8, 16 — **uniform across samples** | `BitsPerSample` (258) |
//! | Samples per pixel | colour channels, plus at most **one** extra | `SamplesPerPixel` (277) |
//! | Layout | strips, chunky (`PlanarConfiguration` 1), any `RowsPerStrip` | 273/278/279/284 |
//! | Predictor | 1 none, 2 horizontal differencing | `Predictor` (317) |
//! | Orientation | 1–8, applied in the placement matrix | `Orientation` (274) |
//! | Resolution | inch and centimetre | 282/283/296 |
//! | Pages | the **first** IFD; the rest are counted and disclosed | IFD chain |
//!
//! ## The four traps, in the order they bite
//!
//! ### 1. Endianness is not only the header fields
//!
//! Every IFD field is read in the header's byte order — that part is obvious
//! and every implementation gets it right. The part that is missed is that
//! **16-bit sample data is also stored in the file's byte order**, while
//! ISO 32000-1 §8.9.3 requires PDF image samples to be stored *high-order byte
//! first*. So a 16-bit `II` TIFF's pixels must be **byte-swapped**, and a
//! reader that swaps the tags but not the samples produces an image whose
//! every channel is scrambled into noise that still has the right dimensions.
//! [`Plan::swap16`] is that swap, and it runs **before** un-prediction,
//! because TIFF 6.0 §14's horizontal differencing operates on the 16-bit
//! sample *values* and [`predictor::unpredict`] reads them big-endian
//! (§7.4.4.4 rule 3).
//!
//! ### 2. PackBits is *not* `RunLengthDecode`, despite being the same algorithm
//!
//! ISO 32000-1 §7.4.5 and TIFF 6.0 §9 define byte-identical run semantics —
//! `0..=127` means `L + 1` literal bytes, `129..=255` means `257 − L` copies —
//! and they disagree on exactly one value:
//!
//! | Length byte `128` (`0x80`) | PDF §7.4.5 | TIFF 6.0 §9 |
//! |---|---|---|
//! | meaning | **EOD** — stop | **no-op** — skip it and carry on |
//!
//! TIFF has no end-of-data marker at all; a strip ends when the expected
//! number of *decoded* bytes has been produced. So
//! [`crate::filters::runlength::decode`] cannot be reused: handed a strip
//! containing a `0x80` no-op (which real writers do emit, as alignment
//! padding), it would return a short buffer and the rest of the image would be
//! silently lost. [`packbits`] is therefore a deliberate, documented
//! divergence rather than a duplicated implementation — it is the same run
//! grammar with TIFF's terminator rule and TIFF's length bound.
//!
//! Everything else **is** reused: [`flate::decode`] for compressions 8/32946
//! (both are RFC 1950 zlib, exactly what §7.4.4.1 delegates to),
//! [`lzw::decode`] for compression 5 (TIFF 6.0 §13 is the same MSB-packed,
//! early-change LZW that §7.4.4.2 describes — `weezl`'s
//! `with_tiff_size_switch` is named after it), and [`predictor::unpredict`]
//! for `Predictor 2` (TIFF 6.0 §14 *is* §7.4.4.4's Table 10 value 2,
//! sub-byte expand→difference→repack rule included).
//!
//! ### 3. `WhiteIsZero` inverts the meaning of every grey sample
//!
//! `PhotometricInterpretation` 0 means *"0 is imaged as white"* — the reverse
//! of `/DeviceGray`, where §8.6.4.2 puts 0.0 at black. It is the default
//! polarity for fax-lineage bilevel scans and is extremely common on
//! monochrome CAD output, so ignoring it renders half the world's scans as
//! photographic negatives.
//!
//! **pdfcer inverts the samples.** This is not the "Adobe CMYK inversion" R29
//! forbids, and the distinction is worth stating precisely: R29 concerns a
//! polarity **nothing in the file declares**, where any inversion is a guess.
//! Here the polarity is declared, unambiguously, by a required tag. The
//! alternative — writing a `/Decode [1 0]` array and leaving the samples
//! alone — is exactly equivalent under §8.9.5.2 Table 90 and is arguably the
//! tidier representation, but [`ImportedImage`] has no `/Decode` field and
//! adding one reaches into the writer. Inversion costs nothing here because a
//! TIFF's samples are being rebuilt anyway (see "no passthrough", below), and
//! it is exact at every bit depth: for an unsigned `n`-bit sample,
//! `max − v == !v` within its field, so a byte-wise complement is the correct
//! inversion for 1, 2, 4, 8 **and** big-endian 16-bit samples alike, padding
//! bits included. It is disclosed as
//! [`ImportNotes::tiff_white_is_zero_inverted`].
//!
//! ### 4. `ExtraSamples` distinguishes premultiplied alpha from straight alpha
//!
//! TIFF 6.0 §18 gives `ExtraSamples` (338) three values, and getting them
//! backwards produces compositing that is visibly wrong in a way that looks
//! like a rendering bug rather than an import bug:
//!
//! | Value | Meaning | What pdfcer does |
//! |---|---|---|
//! | **1** | *associated* alpha — **the colour samples are premultiplied** | **un-premultiplies**, then writes the straight alpha as `/SMask`; disclosed |
//! | **2** | *unassociated* alpha — straight, independent of the colour | splits directly into base + `/SMask` |
//! | **0** | *unspecified data* | **dropped**, not treated as opacity; disclosed |
//!
//! A `/SMask` is a straight alpha channel: §11.6.5.3 gives the premultiplied
//! case its own dedicated entry, `/Matte`, precisely because the two are not
//! interchangeable. Feeding premultiplied colour to a straight-alpha mask
//! double-darkens every partly-transparent pixel against the page — the more
//! transparent, the darker — which reads as "pdfcer lost the transparency"
//! rather than as an inverted convention.
//!
//! **The chosen repair, and its cost, stated rather than implied.**
//! Un-premultiplying (`c' = round(c × max / a)`, and `0` where `a == 0`) is
//! the reconstruction, and it is *lossy in the low-alpha tail*: an 8-bit
//! sample premultiplied by an alpha of 8 retains about three bits of colour
//! information, and nothing can put back what the premultiplication
//! quantised away. It is exact at `a == max` and good everywhere the pixel is
//! actually visible. The **faithful** alternative is to keep the
//! premultiplied samples and write `/SMask << … /Matte [0 0 0] >>` — TIFF's
//! associated alpha is premultiplied against black, which is exactly what
//! `/Matte` records — and that is what this should become once [`SoftMask`]
//! can carry a matte through to the writer. Until then the reconstruction is
//! disclosed as [`ImportNotes::tiff_associated_alpha_unpremultiplied`] rather
//! than performed quietly.
//!
//! ## No passthrough, with one narrow exception
//!
//! The parent module's governing rule is *re-encode nothing we do not have
//! to*. TIFF frustrates it in both directions. Two of its three compressions
//! have no PDF-writable counterpart at all — pdfcer writes no LZW and no
//! run-length encoder (R28) — and even Deflate, which pdfcer **can** write,
//! usually cannot be reused because the strip bytes stop meaning what the PDF
//! stream needs them to mean the moment anything has to be transformed: a
//! multi-strip image is several independent zlib streams, `Predictor 2` needs
//! parameters [`ImportFilter`] has no variant for, a 16-bit `II` image needs
//! its samples swapped, and `WhiteIsZero` needs them inverted.
//!
//! The exception is the case where **none** of that applies: a single-strip,
//! Deflate-compressed, `Predictor 1`, chunky, non-`WhiteIsZero`, alpha-free
//! TIFF's strip payload **is** a conforming plain `/FlateDecode` stream, byte
//! for byte, and is passed through unchanged. It is verified first — the strip
//! is decompressed and its length checked against `rows × stride` — and only
//! then are the *original* bytes stored. A passthrough that is not checked is
//! a way to embed a stream whose real geometry differs from its dictionary,
//! which renders as garbage with no error anywhere.
//!
//! Every other TIFF reports [`RecompressReason::SourceCodecNotReusable`] (or
//! [`RecompressReason::AlphaSplit`] when opacity forced the split), so
//! "pdfcer re-compressed my file" is never something to discover by diffing
//! bytes.
//!
//! ## What is refused, and why each refusal is by name
//!
//! Per R27, every refusal carries a stable feature key and never a generic
//! "decode failed". Half a TIFF reader is worse than none: the failure mode of
//! guessing at these is a wrong-looking image, not an error message.
//!
//! | Refused | Key | Why |
//! |---|---|---|
//! | BigTIFF (version 43) | `TIFF/bigtiff` | 8-byte offsets and a different IFD layout — a different parser, not a superset. Refused at [`super::sniff`] as its own *format*, so the message names BigTIFF. |
//! | Tiled | `TIFF/tiled` | `TileWidth`/`TileLength` replace strips with a 2-D grid whose tiles are padded to the tile size; reassembly is a different algorithm, and a tiled file read as strips is noise. |
//! | `PlanarConfiguration 2` | `TIFF/planar-separate` | Channels stored in separate planes. PDF images are interleaved (§8.9.3), so this needs a re-interleave pass; read as chunky it produces three overlaid ghosts. |
//! | CCITT G3/G4/RLE (2, 3, 4) | `TIFF/ccitt-g3` … | pdfcer **has** a fuzzed CCITT decoder ([`crate::image_codec`]) but it is reachable only through a PDF image dictionary, and `/CCITTFaxDecode` passthrough needs an [`ImportFilter`] variant the writer would have to learn. Named separately per `K`-class so the counts are meaningful; this is the single highest-value TIFF follow-up, since it is what fax-lineage scanners emit. |
//! | JPEG-in-TIFF (6, 7) | `TIFF/jpeg-old`, `TIFF/jpeg` | Compression 6 is the withdrawn, incompatible original; 7 is TechNote 2's. Both put tables in tags rather than in the codestream, so the strip bytes are not a self-contained `/DCTDecode` payload. |
//! | Other compressions | `TIFF/thunderscan` … `TIFF/unknown-compression` | Need a decoder pdfcer does not have. Rule 13 makes adding one a licence-classified decision, not something a feature Pass does in passing. |
//! | Photometric 4, 5, 6, 8–10, 32803, 34892 | `TIFF/photometric-cmyk` … | CMYK is the near miss — `/DeviceCMYK` exists and the ink polarity already agrees — but it carries its own `InkSet`/`NumberOfInks`/`DotRange` surface, and `YCbCr`/`CIELab` need colour conversions pdfcer would then own. Left out of the baseline deliberately. |
//! | `SampleFormat` 2/3/4 | `TIFF/sample-format-signed` … | Signed and IEEE-float samples (GIS elevation, HDR, CAD depth) are *numbers*, not intensities; read as unsigned they render as noise with a plausible histogram. |
//! | 32-bit samples | `TIFF/32-bit-samples` | §8.9.5 Table 89 caps `/BitsPerComponent` at 16. |
//! | Mixed `BitsPerSample` | `TIFF/mixed-bit-depth` | Table 89 gives an image one `/BitsPerComponent`. |
//! | `FillOrder 2` | `TIFF/fill-order-2` | Bit-reversed bytes. §8.9.3 packs high-order bit first, unconditionally. |
//! | Sub-byte alpha | `TIFF/sub-byte-alpha` | Splitting an interleaved alpha channel below 8 bits per sample is a bit-level de-interleave; refused rather than half-built. |
//! | More than one extra sample | `TIFF/multiple-extra-samples` | Which one is opacity is undeclared once there is a choice. |
//! | 16-bit palette indices | `TIFF/palette-16-bit` | §8.6.6.3 caps `hival` at 255. |
//!
//! ## Multi-page TIFF: the first page, and an honest count of the rest
//!
//! A TIFF's IFDs form a linked list, and a multi-page scan is the ordinary
//! output of a sheet-fed scanner. pdfcer places the **first** page and reports
//! how many it did not, as [`ImportNotes::tiff_pages_ignored`].
//!
//! Refusing outright was considered and rejected: it would turn the single
//! most common scanner output into a dead end, and the operator's picture is
//! right there in the file. Silently dropping the rest is the option rule 4
//! actually forbids — so the count is computed on the way in, before anything
//! is placed, and travels with the import.
//!
//! ## Spec sources
//!
//! - **TIFF 6.0** (Aldus/Adobe, 1992-06-03) — §2 (image file header and IFD
//!   layout, field types), §3–§8 (baseline tags), §9 (PackBits), §13 (LZW),
//!   §14 (Predictor 2 / horizontal differencing), §16–§17 (`ColorMap`,
//!   `Orientation`), §18 (`ExtraSamples` / associated alpha), §21 (Deflate is
//!   a registered extension; `Adobe Deflate` = 8, `Deflate` = 32946).
//!   TIFF 6.0 is **not** an ISO standard and is not in the PDF spec RAG; the
//!   clauses cited here name the specification, not a RAG file.
//! - **ISO 32000-1** — §7.4.4 + Table 8 (`/FlateDecode`), §7.4.4.4 + Table 10
//!   (predictors; value 2 is TIFF's), §7.4.5 (`RunLengthDecode`, for the
//!   PackBits divergence), §8.6.4.2 (`/DeviceGray` polarity), §8.6.6.3
//!   (`/Indexed`, `hival ≤ 255`), §8.9.3 (sample layout, high-order bit first,
//!   16-bit high-order **byte** first), §8.9.4 (row order), §8.9.5 Table 89
//!   (`/BitsPerComponent`), §11.6.5.3 (`/SMask`, `/Matte`).
//!
//! ### Recorded ambiguity (reported, not silently resolved)
//!
//! **`ColorMap` values are specified as 16-bit but are frequently written as
//! 8-bit.** TIFF 6.0 §16 is explicit — *"0 represents the minimum intensity
//! and 65535 represents the maximum"* — yet a long tail of writers stores
//! 0–255 in those `SHORT`s. A reader that trusts the spec renders such a
//! palette almost entirely black; libtiff and every viewer built on it apply
//! the heuristic in [`palette_from_colormap`] instead. pdfcer applies it too,
//! and **discloses when it fired**
//! ([`ImportNotes::tiff_palette_assumed_8bit`]) rather than pretending the
//! file was unambiguous. This is a genuine standard-vs-practice divergence and
//! is a candidate for an operator setting under R169; it is hard-coded here
//! only because this module cannot reach `pdfcer-core`'s settings surface.

use super::{
    DpiSource, ImageFormat, ImageImportError, ImportColorSpace, ImportFilter, ImportNotes,
    ImportedImage, Orientation, PdfFeature, RecompressReason, SoftMask, check_dimensions,
    flate_encode, raise_version, row_bytes,
};
use crate::filters::{FilterNotes, flate, lzw, predictor};

// ---------------------------------------------------------------------------
// Header and directory primitives (TIFF 6.0 §2)
// ---------------------------------------------------------------------------

/// The byte order every multi-byte field in the file is stored in
/// (TIFF 6.0 §2: `"II"` = little-endian, `"MM"` = big-endian).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteOrder {
    /// `II` — least-significant byte first.
    Little,
    /// `MM` — most-significant byte first.
    Big,
}

/// The largest number of IFDs pdfcer will walk while counting pages.
///
/// A pdfcer ceiling, not a TIFF one (`ARCHITECTURE.md` §10.1): the chain is a
/// linked list read from untrusted input, so it needs a bound even with the
/// cycle guard in [`walk_ifds`]. Four thousand pages is beyond any scan an
/// operator places on one page of a PDF, and the count is only a disclosure.
const MAX_IFDS: usize = 4096;

/// `RowsPerStrip`'s default (TIFF 6.0 §8: 2³² − 1, i.e. "the whole image is
/// one strip").
const ROWS_PER_STRIP_DEFAULT: u64 = 0xFFFF_FFFF;

/// A parsed field of one image file directory.
///
/// Deliberately *not* the field's value: a field's payload may be inline (when
/// it fits in the 4-byte value slot) or at an offset elsewhere in the file, and
/// resolving that needs the reader. `field` is the offset of the 4-byte
/// value/offset slot itself, so [`Tiff::values`] can decide.
#[derive(Debug, Clone, Copy)]
struct Entry {
    /// The tag number (TIFF 6.0 §3–§8; e.g. 256 = `ImageWidth`).
    tag: u16,
    /// The field type (1 = `BYTE`, 3 = `SHORT`, 4 = `LONG`, 5 = `RATIONAL`, …).
    kind: u16,
    /// How many values of that type the field holds.
    count: u32,
    /// File offset of the entry's 4-byte value/offset slot.
    field: usize,
}

/// A TIFF file plus the byte order every field in it is read with.
struct Tiff<'a> {
    data: &'a [u8],
    order: ByteOrder,
}

impl Tiff<'_> {
    /// Read a 16-bit field in the file's byte order.
    fn u16_at(&self, off: usize) -> Option<u16> {
        let b: [u8; 2] = self.data.get(off..off.checked_add(2)?)?.try_into().ok()?;
        Some(match self.order {
            ByteOrder::Little => u16::from_le_bytes(b),
            ByteOrder::Big => u16::from_be_bytes(b),
        })
    }

    /// Read a 32-bit field in the file's byte order.
    fn u32_at(&self, off: usize) -> Option<u32> {
        let b: [u8; 4] = self.data.get(off..off.checked_add(4)?)?.try_into().ok()?;
        Some(match self.order {
            ByteOrder::Little => u32::from_le_bytes(b),
            ByteOrder::Big => u32::from_be_bytes(b),
        })
    }

    /// The bytes of `entry`'s payload, wherever they live.
    ///
    /// TIFF 6.0 §2: *"If the value is shorter than 4 bytes, it is
    /// left-justified within the 4-byte Value Offset"* — so a payload of 4
    /// bytes or fewer is **inline** and anything larger is at the offset the
    /// slot holds. Returning `None` for a payload that runs past the end of
    /// the file is what keeps every later `Vec::with_capacity` bounded by the
    /// file's own size rather than by an attacker-chosen `count`.
    fn payload(&self, entry: &Entry) -> Option<&[u8]> {
        let size = type_size(entry.kind)?;
        let total = (entry.count as usize).checked_mul(size)?;
        let start = if total <= 4 {
            entry.field
        } else {
            self.u32_at(entry.field)? as usize
        };
        self.data.get(start..start.checked_add(total)?)
    }

    /// Every value of an integer-typed field, widened to `u64`.
    ///
    /// Signed types are accepted and reinterpreted unsigned: no baseline tag
    /// this module reads is legitimately negative, and a writer that spelled
    /// `SamplesPerPixel` as `SSHORT` has made a type error, not a semantic
    /// one. Non-integer types yield `None` rather than a coerced number —
    /// a `RATIONAL` `Compression` is a malformed file, not a value.
    fn values(&self, entry: &Entry) -> Option<Vec<u64>> {
        let size = type_size(entry.kind)?;
        let bytes = self.payload(entry)?;
        let mut out = Vec::with_capacity(bytes.len() / size.max(1));
        for chunk in bytes.chunks_exact(size) {
            let v = match (entry.kind, chunk) {
                (1 | 2 | 6 | 7, [b]) => u64::from(*b),
                (3 | 8, _) => u64::from(match self.order {
                    ByteOrder::Little => u16::from_le_bytes(chunk.try_into().ok()?),
                    ByteOrder::Big => u16::from_be_bytes(chunk.try_into().ok()?),
                }),
                (4 | 9, _) => u64::from(match self.order {
                    ByteOrder::Little => u32::from_le_bytes(chunk.try_into().ok()?),
                    ByteOrder::Big => u32::from_be_bytes(chunk.try_into().ok()?),
                }),
                _ => return None,
            };
            out.push(v);
        }
        Some(out)
    }

    /// A `RATIONAL` field as a number (TIFF 6.0 §2: two `LONG`s,
    /// numerator then denominator).
    ///
    /// `SHORT`/`LONG` are also accepted, because writers do emit a bare
    /// integer resolution and reading it is strictly better than discarding a
    /// resolution the file plainly stated.
    fn rational(&self, entry: &Entry) -> Option<f64> {
        if entry.kind == 5 || entry.kind == 10 {
            let bytes = self.payload(entry)?;
            let n = self.u32_from(bytes.get(0..4)?)?;
            let d = self.u32_from(bytes.get(4..8)?)?;
            if d == 0 {
                return None;
            }
            return Some(f64::from(n) / f64::from(d));
        }
        #[allow(clippy::cast_precision_loss)] // a resolution, not an identity
        self.values(entry)
            .and_then(|v| v.first().copied())
            .map(|v| v as f64)
    }

    /// Read a 32-bit value out of an already-bounded slice.
    fn u32_from(&self, bytes: &[u8]) -> Option<u32> {
        let b: [u8; 4] = bytes.try_into().ok()?;
        Some(match self.order {
            ByteOrder::Little => u32::from_le_bytes(b),
            ByteOrder::Big => u32::from_be_bytes(b),
        })
    }
}

/// Bytes per value of each TIFF field type (TIFF 6.0 §2, types 1–12).
///
/// `None` for a type this file's version does not define — which is a
/// malformed directory, not an extension: TIFF 6.0 froze the list at 12, and
/// a reader that guesses a size for type 13 mis-locates every field after it.
const fn type_size(kind: u16) -> Option<usize> {
    match kind {
        // BYTE, ASCII, SBYTE, UNDEFINED
        1 | 2 | 6 | 7 => Some(1),
        // SHORT, SSHORT
        3 | 8 => Some(2),
        // LONG, SLONG, FLOAT
        4 | 9 | 11 => Some(4),
        // RATIONAL, SRATIONAL, DOUBLE
        5 | 10 | 12 => Some(8),
        _ => None,
    }
}

// Baseline tag numbers actually read by this module (TIFF 6.0 §3–§8, §16–§18).
const TAG_IMAGE_WIDTH: u16 = 256;
const TAG_IMAGE_LENGTH: u16 = 257;
const TAG_BITS_PER_SAMPLE: u16 = 258;
const TAG_COMPRESSION: u16 = 259;
const TAG_PHOTOMETRIC: u16 = 262;
const TAG_FILL_ORDER: u16 = 266;
const TAG_STRIP_OFFSETS: u16 = 273;
const TAG_ORIENTATION: u16 = 274;
const TAG_SAMPLES_PER_PIXEL: u16 = 277;
const TAG_ROWS_PER_STRIP: u16 = 278;
const TAG_STRIP_BYTE_COUNTS: u16 = 279;
const TAG_X_RESOLUTION: u16 = 282;
const TAG_Y_RESOLUTION: u16 = 283;
const TAG_PLANAR_CONFIG: u16 = 284;
const TAG_RESOLUTION_UNIT: u16 = 296;
const TAG_PREDICTOR: u16 = 317;
const TAG_COLOR_MAP: u16 = 320;
const TAG_TILE_WIDTH: u16 = 322;
const TAG_TILE_LENGTH: u16 = 323;
const TAG_TILE_OFFSETS: u16 = 324;
const TAG_EXTRA_SAMPLES: u16 = 338;
const TAG_SAMPLE_FORMAT: u16 = 339;
const TAG_ICC_PROFILE: u16 = 34675;

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

/// Parse a TIFF into a PDF-ready image XObject payload.
///
/// Only the **first** page (IFD) is placed; any others are counted into
/// [`ImportNotes::tiff_pages_ignored`] before this returns, so a front end can
/// say so before the document changes.
///
/// # Errors
///
/// See [`ImageImportError`]. Every tag combination pdfcer declines is
/// [`ImageImportError::Unsupported`] with a stable key (the table in the
/// module docs is the complete list); a directory that does not parse is
/// [`ImageImportError::Corrupt`].
pub fn import(data: &[u8]) -> Result<ImportedImage, ImageImportError> {
    let (order, first_ifd) = read_header(data)?;
    let tiff = Tiff { data, order };
    let (entries, pages) = walk_ifds(&tiff, first_ifd)?;
    let raster = Raster::read(&tiff, &entries)?;
    assemble(&tiff, &raster, pages.saturating_sub(1))
}

/// Read the 8-byte image file header (TIFF 6.0 §2).
///
/// Layout: byte-order marker (2), version magic (2), offset of the first IFD
/// (4). The version is `42` — *"an arbitrary but carefully chosen number"* —
/// and is the only value classic TIFF defines. BigTIFF's `43` is refused at
/// [`super::sniff`] as its own format name, so reaching it here means the
/// caller bypassed the sniffer; it is refused again rather than trusted.
fn read_header(data: &[u8]) -> Result<(ByteOrder, usize), ImageImportError> {
    let order = match data.get(0..2) {
        Some(b"II") => ByteOrder::Little,
        Some(b"MM") => ByteOrder::Big,
        _ => {
            return Err(corrupt(
                "the file does not begin with a TIFF byte-order mark",
            ));
        }
    };
    let probe = Tiff { data, order };
    match probe.u16_at(2) {
        Some(42) => {}
        Some(43) => {
            return Err(ImageImportError::Unsupported {
                feature: "TIFF/bigtiff",
            });
        }
        _ => return Err(corrupt("the TIFF version magic is not 42")),
    }
    let Some(first) = probe.u32_at(4).map(|v| v as usize) else {
        return Err(corrupt("the TIFF header is truncated"));
    };
    Ok((order, first))
}

/// Walk the IFD chain: return the **first** directory's entries and the total
/// number of directories (pages) in the file.
///
/// An IFD is a `u16` entry count, that many 12-byte entries, and a `u32`
/// offset to the next IFD (`0` = end) — TIFF 6.0 §2.
///
/// # Why a damaged chain does not lose the first page
///
/// The walk stops — rather than failing — on a cycle or on [`MAX_IFDS`]. The
/// first directory has already been read and completely describes a placeable
/// image; refusing it because a *later* `NextIFD` pointer loops would discard
/// a good picture over metadata damage, which is the same posture
/// [`super::png`] takes toward a wrong ancillary-chunk CRC. In that
/// (pathological) case the page count is a lower bound, which is the honest
/// direction for a disclosure to be wrong in.
fn walk_ifds(tiff: &Tiff<'_>, first: usize) -> Result<(Vec<Entry>, usize), ImageImportError> {
    let mut entries = Vec::new();
    let mut seen: Vec<usize> = Vec::new();
    let mut pages = 0usize;
    let mut offset = first;

    while offset != 0 && pages < MAX_IFDS && !seen.contains(&offset) {
        seen.push(offset);
        let Some(count) = tiff.u16_at(offset) else {
            // The FIRST directory must exist; a later one that does not is
            // damage past the image and is treated as the end of the chain.
            if pages == 0 {
                return Err(corrupt(
                    "the first TIFF directory is past the end of the file",
                ));
            }
            break;
        };
        let table = offset.saturating_add(2);
        let span = usize::from(count).saturating_mul(12);
        if tiff
            .data
            .get(table..table.saturating_add(span).saturating_add(4))
            .is_none()
            && pages == 0
        {
            return Err(corrupt("a TIFF directory runs past the end of the file"));
        }

        if pages == 0 {
            entries.reserve(usize::from(count));
            for i in 0..usize::from(count) {
                let at = table.saturating_add(i.saturating_mul(12));
                let (Some(tag), Some(kind), Some(n)) =
                    (tiff.u16_at(at), tiff.u16_at(at + 2), tiff.u32_at(at + 4))
                else {
                    return Err(corrupt("a TIFF directory entry is truncated"));
                };
                entries.push(Entry {
                    tag,
                    kind,
                    count: n,
                    field: at + 8,
                });
            }
        }
        pages += 1;
        offset = tiff
            .u32_at(table.saturating_add(span))
            .map_or(0, |v| v as usize);
    }

    if pages == 0 {
        return Err(corrupt("the TIFF has no image file directory"));
    }
    Ok((entries, pages))
}

/// Find one tag in a directory.
fn find(entries: &[Entry], tag: u16) -> Option<&Entry> {
    entries.iter().find(|e| e.tag == tag)
}

/// One tag's first value, or `default` when the tag is absent or unreadable.
fn scalar(tiff: &Tiff<'_>, entries: &[Entry], tag: u16, default: u64) -> u64 {
    find(entries, tag)
        .and_then(|e| tiff.values(e))
        .and_then(|v| v.first().copied())
        .unwrap_or(default)
}

fn corrupt(detail: &str) -> ImageImportError {
    ImageImportError::Corrupt {
        detail: detail.to_owned(),
    }
}

// ---------------------------------------------------------------------------
// The validated description of one page
// ---------------------------------------------------------------------------

/// Which colour channel layout the photometric interpretation implies, and
/// what pdfcer must do to the samples because of it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Colour {
    /// `PhotometricInterpretation` 0 or 1 — one channel, `/DeviceGray`.
    /// `invert` is true for 0 (`WhiteIsZero`).
    Gray { invert: bool },
    /// `PhotometricInterpretation` 2 — three channels, `/DeviceRGB`.
    Rgb,
    /// `PhotometricInterpretation` 3 — one channel of indices into `ColorMap`.
    Palette {
        /// `[/Indexed /DeviceRGB hival lookup]`'s `hival`.
        hival: u8,
        /// Consecutive RGB triples (§8.6.6.3).
        lookup: Vec<u8>,
        /// The `ColorMap` values were 8-bit despite TIFF 6.0 §16's 16-bit
        /// definition, and pdfcer used them as stored — see the module docs'
        /// recorded ambiguity.
        assumed_8bit: bool,
    },
}

impl Colour {
    /// Colour channels, excluding any extra sample.
    const fn channels(&self) -> u32 {
        match self {
            Self::Gray { .. } | Self::Palette { .. } => 1,
            Self::Rgb => 3,
        }
    }
}

/// How the one extra sample (if any) is to be read (TIFF 6.0 §18).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Extra {
    /// `ExtraSamples` 1 — alpha, with the colour samples premultiplied by it.
    Associated,
    /// `ExtraSamples` 2 — alpha, independent of the colour samples.
    Unassociated,
    /// `ExtraSamples` 0, or absent — "unspecified data". Dropped, never read
    /// as opacity: a channel of unknown meaning treated as alpha can make an
    /// image entirely invisible, which is the same trap
    /// [`super::bmp`]'s fourth byte names.
    Unspecified,
}

/// Everything one page of a TIFF declares, after every combination pdfcer
/// cannot represent has already been refused.
///
/// Built by [`Raster::read`], which is where all validation lives — so by the
/// time [`assemble`] runs, "can pdfcer do this?" is already answered and the
/// remaining code is only arithmetic.
struct Raster {
    width: u32,
    height: u32,
    /// Bits per **sample**, uniform across all samples: 1, 2, 4, 8 or 16.
    bits: u32,
    /// `SamplesPerPixel` — colour channels plus any extra sample.
    samples: u32,
    colour: Colour,
    extra: Option<Extra>,
    compression: u64,
    predictor: u64,
    strip_offsets: Vec<u64>,
    strip_counts: Vec<u64>,
    rows_per_strip: u64,
    orientation: Orientation,
    dpi: Option<(f64, f64)>,
    icc: bool,
}

impl Raster {
    /// Read and validate one directory.
    ///
    /// The order of the checks is deliberate: **container-level refusals
    /// first** (compression, tiling, planar, sample format), then geometry,
    /// then colour. An operator handed a tiled CMYK JPEG-in-TIFF should be
    /// told about the codec, which is the thing they can act on by re-saving,
    /// rather than about the colour space.
    fn read(tiff: &Tiff<'_>, entries: &[Entry]) -> Result<Self, ImageImportError> {
        let compression = scalar(tiff, entries, TAG_COMPRESSION, 1);
        if let Some(feature) = compression_refusal(compression) {
            return Err(ImageImportError::Unsupported { feature });
        }
        if find(entries, TAG_TILE_WIDTH).is_some()
            || find(entries, TAG_TILE_LENGTH).is_some()
            || find(entries, TAG_TILE_OFFSETS).is_some()
        {
            return Err(ImageImportError::Unsupported {
                feature: "TIFF/tiled",
            });
        }
        if scalar(tiff, entries, TAG_PLANAR_CONFIG, 1) != 1 {
            return Err(ImageImportError::Unsupported {
                feature: "TIFF/planar-separate",
            });
        }
        if scalar(tiff, entries, TAG_FILL_ORDER, 1) != 1 {
            return Err(ImageImportError::Unsupported {
                feature: "TIFF/fill-order-2",
            });
        }
        // `SampleFormat` (339) may carry one value per sample; any non-unsigned
        // one poisons the whole image, so the check is over all of them.
        if let Some(formats) = find(entries, TAG_SAMPLE_FORMAT).and_then(|e| tiff.values(e)) {
            for f in formats {
                if let Some(feature) = sample_format_refusal(f) {
                    return Err(ImageImportError::Unsupported { feature });
                }
            }
        }
        let predictor = scalar(tiff, entries, TAG_PREDICTOR, 1);
        match predictor {
            1 | 2 => {}
            3 => {
                return Err(ImageImportError::Unsupported {
                    feature: "TIFF/predictor-float",
                });
            }
            _ => return Err(corrupt("unknown TIFF Predictor value")),
        }

        // --- geometry ---------------------------------------------------
        let (Ok(width), Ok(height)) = (
            u32::try_from(scalar(tiff, entries, TAG_IMAGE_WIDTH, 0)),
            u32::try_from(scalar(tiff, entries, TAG_IMAGE_LENGTH, 0)),
        ) else {
            return Err(corrupt("the TIFF declares an out-of-range image size"));
        };

        let samples = u32::try_from(scalar(tiff, entries, TAG_SAMPLES_PER_PIXEL, 1))
            .ok()
            .filter(|&s| s >= 1)
            .ok_or_else(|| corrupt("the TIFF declares an out-of-range SamplesPerPixel"))?;

        // `BitsPerSample` is one value per sample and they must agree —
        // Table 89 gives a PDF image exactly one `/BitsPerComponent`.
        let depths = find(entries, TAG_BITS_PER_SAMPLE)
            .and_then(|e| tiff.values(e))
            .unwrap_or_else(|| vec![1]);
        let Some(&first_depth) = depths.first() else {
            return Err(corrupt("the TIFF declares an empty BitsPerSample"));
        };
        if depths.iter().any(|&d| d != first_depth) {
            return Err(ImageImportError::Unsupported {
                feature: "TIFF/mixed-bit-depth",
            });
        }
        let bits = match first_depth {
            1 | 2 | 4 | 8 | 16 => u32::try_from(first_depth).unwrap_or(8),
            32 | 64 => {
                return Err(ImageImportError::Unsupported {
                    feature: "TIFF/32-bit-samples",
                });
            }
            _ => return Err(corrupt("unsupported TIFF BitsPerSample")),
        };

        // --- colour -----------------------------------------------------
        let photometric = find(entries, TAG_PHOTOMETRIC)
            .and_then(|e| tiff.values(e))
            .and_then(|v| v.first().copied())
            .ok_or_else(|| corrupt("the TIFF has no PhotometricInterpretation"))?;
        if let Some(feature) = photometric_refusal(photometric) {
            return Err(ImageImportError::Unsupported { feature });
        }
        let colour = match photometric {
            0 => Colour::Gray { invert: true },
            1 => Colour::Gray { invert: false },
            2 => Colour::Rgb,
            _ => {
                if bits > 8 {
                    // §8.6.6.3 caps `hival` at 255, so an index wider than 8
                    // bits has nowhere to go.
                    return Err(ImageImportError::Unsupported {
                        feature: "TIFF/palette-16-bit",
                    });
                }
                let map = find(entries, TAG_COLOR_MAP)
                    .and_then(|e| tiff.values(e))
                    .ok_or_else(|| corrupt("a palette TIFF with no ColorMap"))?;
                palette_from_colormap(&map, bits)?
            }
        };

        // --- the extra sample (TIFF 6.0 §18) ----------------------------
        let channels = colour.channels();
        if samples < channels {
            return Err(corrupt(
                "the TIFF declares fewer samples than its colour model needs",
            ));
        }
        let extra_count = samples - channels;
        let extra = match extra_count {
            0 => None,
            1 => {
                let declared = find(entries, TAG_EXTRA_SAMPLES)
                    .and_then(|e| tiff.values(e))
                    .and_then(|v| v.first().copied());
                let kind = match declared {
                    Some(1) => Extra::Associated,
                    Some(2) => Extra::Unassociated,
                    // 0 = "unspecified", and so is an absent tag: TIFF 6.0
                    // requires `ExtraSamples` when `SamplesPerPixel` exceeds
                    // the colour model's channels, so a missing one is a
                    // producer omission with no declared meaning.
                    _ => Extra::Unspecified,
                };
                if kind != Extra::Unspecified && bits < 8 {
                    // De-interleaving an alpha channel below one byte per
                    // sample is a bit-level split; refused rather than
                    // half-built (R27).
                    return Err(ImageImportError::Unsupported {
                        feature: "TIFF/sub-byte-alpha",
                    });
                }
                Some(kind)
            }
            _ => {
                return Err(ImageImportError::Unsupported {
                    feature: "TIFF/multiple-extra-samples",
                });
            }
        };

        // --- strips -----------------------------------------------------
        let strip_offsets = find(entries, TAG_STRIP_OFFSETS)
            .and_then(|e| tiff.values(e))
            .ok_or_else(|| corrupt("the TIFF has no StripOffsets"))?;
        let strip_counts = find(entries, TAG_STRIP_BYTE_COUNTS)
            .and_then(|e| tiff.values(e))
            .unwrap_or_default();
        let rows_per_strip = match scalar(tiff, entries, TAG_ROWS_PER_STRIP, ROWS_PER_STRIP_DEFAULT)
        {
            // 0 is not a legal strip height; treated as "one strip", which is
            // what `RowsPerStrip`'s own default means.
            0 => ROWS_PER_STRIP_DEFAULT,
            n => n,
        };

        // --- the rest is metadata ---------------------------------------
        let orientation = Orientation::from_exif(
            u16::try_from(scalar(tiff, entries, TAG_ORIENTATION, 1)).unwrap_or(1),
        );
        let dpi = resolution(tiff, entries);

        Ok(Self {
            width,
            height,
            bits,
            samples,
            colour,
            extra,
            compression,
            predictor,
            strip_offsets,
            strip_counts,
            rows_per_strip,
            orientation,
            dpi,
            icc: find(entries, TAG_ICC_PROFILE).is_some(),
        })
    }
}

/// Map a `Compression` value pdfcer declines to its stable feature key.
///
/// `None` for the five it accepts. The named keys matter more than the
/// coverage: R27 counts refusals by name, and "TIFF was refused" is not a
/// number anyone can act on, while "17 CCITT G4 TIFFs were refused" is a
/// roadmap item.
const fn compression_refusal(value: u64) -> Option<&'static str> {
    match value {
        // 1 none, 5 LZW, 8 Adobe Deflate, 32773 PackBits, 32946 Deflate.
        1 | 5 | 8 | 32773 | 32946 => None,
        2 => Some("TIFF/ccitt-rle"),
        3 => Some("TIFF/ccitt-g3"),
        4 => Some("TIFF/ccitt-g4"),
        6 => Some("TIFF/jpeg-old"),
        7 => Some("TIFF/jpeg"),
        9 => Some("TIFF/jbig-t85"),
        10 => Some("TIFF/jbig-t43"),
        32766 => Some("TIFF/next-rle"),
        32809 => Some("TIFF/thunderscan"),
        34712 => Some("TIFF/jpeg2000"),
        34887 => Some("TIFF/lerc"),
        34925 => Some("TIFF/lzma"),
        50000 => Some("TIFF/zstd"),
        50001 => Some("TIFF/webp"),
        _ => Some("TIFF/unknown-compression"),
    }
}

/// Map a `PhotometricInterpretation` pdfcer declines to its stable key.
const fn photometric_refusal(value: u64) -> Option<&'static str> {
    match value {
        0..=3 => None,
        4 => Some("TIFF/photometric-transparency-mask"),
        5 => Some("TIFF/photometric-cmyk"),
        6 => Some("TIFF/photometric-ycbcr"),
        8..=10 => Some("TIFF/photometric-cielab"),
        32803 => Some("TIFF/photometric-cfa"),
        34892 => Some("TIFF/photometric-linear-raw"),
        _ => Some("TIFF/photometric-unknown"),
    }
}

/// Map a `SampleFormat` pdfcer declines to its stable key (TIFF 6.0 TechNote 3).
///
/// 1 is unsigned integer — the baseline, and the only interpretation under
/// which a sample is an *intensity*. 2 (two's-complement), 3 (IEEE float) and
/// 4 (undefined) are numbers that happen to be stored in an image grid; read
/// as intensities they produce noise with a plausible histogram, which is
/// worse than a refusal because it looks like a decode that worked.
const fn sample_format_refusal(value: u64) -> Option<&'static str> {
    match value {
        1 => None,
        2 => Some("TIFF/sample-format-signed"),
        3 => Some("TIFF/sample-format-float"),
        _ => Some("TIFF/sample-format-undefined"),
    }
}

/// Turn a `ColorMap` field into a §8.6.6.3 `/Indexed` lookup.
///
/// TIFF 6.0 §16: the field holds `3 × 2^BitsPerSample` `SHORT`s — the **whole
/// red block, then the whole green block, then the whole blue block**, not
/// interleaved triples — where *"0 represents the minimum intensity and 65535
/// represents the maximum"*. §8.6.6.3 wants the opposite layout (consecutive
/// 8-bit RGB triples), so this both de-blocks and narrows.
///
/// # The 8-bit-in-16-bit heuristic
///
/// See the module docs' recorded ambiguity. A long tail of writers stores
/// 0–255 in those `SHORT`s, and trusting the spec renders such a palette
/// almost black. The rule applied here is libtiff's: **if no value exceeds 255
/// and at least one is non-zero, the values are already 8-bit.** The false
/// positive is a genuine 16-bit palette every one of whose components is
/// darker than 1/257 of full intensity — a palette of near-blacks — which is
/// reported (`assumed_8bit`) rather than assumed away.
fn palette_from_colormap(map: &[u64], bits: u32) -> Result<Colour, ImageImportError> {
    let entries = 1usize << bits;
    if map.len() < entries * 3 {
        return Err(corrupt(
            "the TIFF ColorMap is shorter than its bit depth needs",
        ));
    }
    let assumed_8bit = map.iter().all(|&v| v <= 255) && map.iter().any(|&v| v != 0);
    let narrow = |v: u64| -> u8 {
        if assumed_8bit {
            u8::try_from(v & 0xFF).unwrap_or(0)
        } else {
            u8::try_from((v & 0xFFFF) >> 8).unwrap_or(0)
        }
    };

    let mut lookup = Vec::with_capacity(entries * 3);
    for i in 0..entries {
        let r = map.get(i).copied().unwrap_or(0);
        let g = map.get(entries + i).copied().unwrap_or(0);
        let b = map.get(entries * 2 + i).copied().unwrap_or(0);
        lookup.extend_from_slice(&[narrow(r), narrow(g), narrow(b)]);
    }
    Ok(Colour::Palette {
        hival: u8::try_from(entries.saturating_sub(1)).unwrap_or(255),
        lookup,
        assumed_8bit,
    })
}

/// `XResolution`/`YResolution` under `ResolutionUnit` (TIFF 6.0 §8).
///
/// Unit 2 is inches (the default, so the values are already dpi), 3 is
/// centimetres. **Unit 1 means "no absolute unit"** — the values are an aspect
/// ratio, not a resolution — and yields `None`, exactly as [`super::png`]
/// treats a `pHYs` chunk with unit specifier 0. Reading a ratio as dpi would
/// invent a physical size the file explicitly declined to state.
fn resolution(tiff: &Tiff<'_>, entries: &[Entry]) -> Option<(f64, f64)> {
    let unit = scalar(tiff, entries, TAG_RESOLUTION_UNIT, 2);
    let scale = match unit {
        2 => 1.0,
        3 => 2.54,
        _ => return None,
    };
    let x = find(entries, TAG_X_RESOLUTION).and_then(|e| tiff.rational(e))?;
    let y = find(entries, TAG_Y_RESOLUTION).and_then(|e| tiff.rational(e))?;
    if x > 0.0 && y > 0.0 {
        Some((x * scale, y * scale))
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Decode
// ---------------------------------------------------------------------------

/// The per-strip decode plan, computed once so the strip loop has no decisions
/// left in it.
struct Plan {
    /// Bytes of one byte-padded sample row (§8.9.3 / TIFF 6.0 §8 agree: a row
    /// occupies a whole number of bytes).
    stride: usize,
    /// 16-bit samples stored little-endian must become big-endian for §8.9.3.
    /// Applied **before** un-prediction — see the module docs' trap 1.
    swap16: bool,
    /// `Predictor 2`'s parameters, or `None` for `Predictor 1`.
    unpredict: Option<predictor::Params>,
}

/// Decode every strip into one contiguous, un-predicted, big-endian sample
/// buffer of exactly `stride × height` bytes.
fn decode_strips(
    tiff: &Tiff<'_>,
    raster: &Raster,
    plan: &Plan,
) -> Result<Vec<u8>, ImageImportError> {
    let height = raster.height as usize;
    let total = plan
        .stride
        .checked_mul(height)
        .ok_or(ImageImportError::TooLarge)?;
    let rows_per_strip = usize::try_from(raster.rows_per_strip)
        .unwrap_or(usize::MAX)
        .min(height)
        .max(1);
    let strips = height.div_ceil(rows_per_strip);
    if raster.strip_offsets.len() < strips {
        return Err(corrupt("the TIFF has fewer StripOffsets than strips"));
    }

    let mut out = Vec::with_capacity(total);
    for s in 0..strips {
        let rows = rows_per_strip.min(height - s * rows_per_strip);
        let expected = rows
            .checked_mul(plan.stride)
            .ok_or(ImageImportError::TooLarge)?;

        let start = usize::try_from(raster.strip_offsets.get(s).copied().unwrap_or(0))
            .map_err(|_| corrupt("a TIFF strip offset is out of range"))?;
        // `StripByteCounts` is required by TIFF 6.0 §8, but an uncompressed
        // strip's length is derivable, so a file missing it is still readable
        // rather than refused for a tag that carries no information pdfcer
        // does not already have.
        let len = match raster.strip_counts.get(s).copied() {
            Some(n) => usize::try_from(n).unwrap_or(usize::MAX),
            None if raster.compression == 1 => expected,
            None => return Err(corrupt("the TIFF has no StripByteCounts")),
        };
        let raw = tiff
            .data
            .get(start..start.saturating_add(len))
            .ok_or_else(|| corrupt("a TIFF strip runs past the end of the file"))?;

        let mut strip = decompress(raster.compression, raw, expected)?;
        if strip.len() < expected {
            return Err(corrupt(
                "a TIFF strip decoded to fewer bytes than its rows need",
            ));
        }
        // A trailing strip padded past its own rows is normal; the surplus is
        // not image data and is dropped rather than shifting the next row.
        strip.truncate(expected);

        if plan.swap16 {
            swap16(&mut strip);
        }
        if let Some(params) = plan.unpredict.as_ref() {
            // Per strip, not over the whole image: rows never span strips, and
            // `unpredict` derives its row count from the buffer length.
            strip = predictor::unpredict(strip, params).map_err(|e| ImageImportError::Corrupt {
                detail: format!("a TIFF strip could not be un-predicted: {e}"),
            })?;
        }
        out.extend_from_slice(&strip);
    }
    out.truncate(total);
    if out.len() < total {
        return Err(corrupt(
            "the TIFF's strips do not cover its declared height",
        ));
    }
    Ok(out)
}

/// Run one strip through its compression.
///
/// `expected` is the strip's decoded length, which PackBits needs as its
/// terminator (TIFF has no end-of-data marker) and which the others ignore.
fn decompress(compression: u64, raw: &[u8], expected: usize) -> Result<Vec<u8>, ImageImportError> {
    match compression {
        1 => Ok(raw.to_vec()),
        5 => {
            // TIFF 6.0 §13 and ISO 32000-1 §7.4.4.2 describe the same codec:
            // MSB-packed, 8-bit alphabet, code widths that grow one code early.
            // `filters::lzw` with no parameters is exactly that configuration.
            let mut notes = FilterNotes::default();
            lzw::decode(raw, None, &mut notes).map_err(|e| ImageImportError::Corrupt {
                detail: format!("a TIFF LZW strip could not be decoded: {e}"),
            })
        }
        32773 => packbits(raw, expected),
        // 8 (Adobe Deflate) and 32946 (Deflate) are both RFC 1950 zlib, which
        // is what §7.4.4.1 defines FlateDecode as.
        _ => flate::decode(raw, None).map_err(|e| ImageImportError::Corrupt {
            detail: format!("a TIFF Deflate strip could not be decompressed: {e}"),
        }),
    }
}

/// PackBits (TIFF 6.0 §9), stopping at `expected` bytes.
///
/// **Not** [`crate::filters::runlength::decode`], and the difference is one
/// byte value — see the module docs' trap 2. `128` is a **no-op** here, not
/// end-of-data, and the stream ends when enough bytes have been produced
/// rather than at a marker.
///
/// The run grammar itself is identical to §7.4.5's, including its asymmetry:
/// a literal run is `L + 1` bytes (so `L = 0` means **one** byte, never zero)
/// while a repeat run is `257 − L` copies (so `L = 255` means two).
fn packbits(raw: &[u8], expected: usize) -> Result<Vec<u8>, ImageImportError> {
    let mut out = Vec::with_capacity(expected);
    let mut i = 0usize;
    while out.len() < expected {
        let Some(&length) = raw.get(i) else {
            return Err(corrupt(
                "a PackBits strip ended before its rows were complete",
            ));
        };
        i += 1;
        if length == 128 {
            // TIFF 6.0 §9: "Note that the -128 value is not used; it is a
            // no-op." Reading it as §7.4.5's EOD here is how a strip silently
            // loses everything after its first alignment pad.
            continue;
        }
        if length < 128 {
            let count = usize::from(length) + 1;
            let Some(run) = raw.get(i..i + count) else {
                return Err(corrupt("a PackBits literal run runs past the strip"));
            };
            out.extend_from_slice(run);
            i += count;
        } else {
            let count = 257 - usize::from(length);
            let Some(&byte) = raw.get(i) else {
                return Err(corrupt("a PackBits repeat run runs past the strip"));
            };
            out.resize(out.len() + count, byte);
            i += 1;
        }
    }
    Ok(out)
}

/// Swap every 16-bit sample from little-endian to big-endian (§8.9.3:
/// *"16-bit values are stored high-order byte first"*).
fn swap16(buf: &mut [u8]) {
    for pair in buf.chunks_exact_mut(2) {
        pair.swap(0, 1);
    }
}

// ---------------------------------------------------------------------------
// Assembly
// ---------------------------------------------------------------------------

/// Turn a validated [`Raster`] into an [`ImportedImage`].
fn assemble(
    tiff: &Tiff<'_>,
    raster: &Raster,
    pages_ignored: usize,
) -> Result<ImportedImage, ImageImportError> {
    check_dimensions(raster.width, raster.height, raster.samples, raster.bits)?;

    let stride = row_bytes(raster.width, raster.samples, raster.bits);
    let plan = Plan {
        stride,
        // TIFF stores 16-bit samples in the FILE's byte order; PDF stores them
        // high-order byte first. Only `II` needs the swap.
        swap16: raster.bits == 16 && tiff.order == ByteOrder::Little,
        unpredict: (raster.predictor == 2).then_some(predictor::Params {
            predictor: 2,
            // Horizontal differencing strides by SamplesPerPixel, alpha
            // included: the difference is taken against the same channel of
            // the pixel to the left (TIFF 6.0 §14).
            colors: raster.samples,
            bits_per_component: raster.bits,
            columns: raster.width,
        }),
    };

    let mut notes = ImportNotes {
        colour_profile_dropped: raster.icc,
        dpi_source: if raster.dpi.is_some() {
            DpiSource::TiffResolution
        } else {
            DpiSource::Assumed
        },
        exif_orientation: raster.orientation.exif_value(),
        tiff_pages_ignored: u32::try_from(pages_ignored).unwrap_or(u32::MAX),
        ..ImportNotes::default()
    };
    if raster.bits == 16 {
        raise_version(
            &mut notes.requires_pdf_version,
            PdfFeature::BitsPerComponent16,
        );
    }

    let mut samples = decode_strips(tiff, raster, &plan)?;

    // --- the extra sample, before anything else touches the buffer -------
    //
    // Splitting first is what makes the `WhiteIsZero` complement below safe:
    // a blanket byte-wise inversion over interleaved grey+alpha would invert
    // the OPACITY too, turning every transparent pixel opaque.
    let channels = raster.colour.channels();
    let mut alpha: Option<Vec<u8>> = None;
    if let Some(extra) = raster.extra {
        let depth_bytes = (raster.bits as usize).div_ceil(8);
        let (base, split) = split_extra(
            &samples,
            raster.width as usize,
            raster.height as usize,
            stride,
            channels as usize,
            depth_bytes,
        );
        samples = base;
        match extra {
            Extra::Unassociated => alpha = Some(split),
            Extra::Associated => {
                unpremultiply(&mut samples, &split, channels as usize, raster.bits);
                notes.tiff_associated_alpha_unpremultiplied = true;
                alpha = Some(split);
            }
            Extra::Unspecified => {
                // Dropped, not read as opacity. See [`Extra::Unspecified`].
                notes.tiff_extra_samples_dropped = 1;
            }
        }
    }

    // --- `WhiteIsZero` (TIFF 6.0 §4) -------------------------------------
    if matches!(raster.colour, Colour::Gray { invert: true }) {
        for b in &mut samples {
            // `max − v == !v` for every unsigned n-bit field, so one
            // complement inverts 1-, 2-, 4-, 8- and big-endian-16-bit samples
            // alike, padding bits included.
            *b = !*b;
        }
        notes.tiff_white_is_zero_inverted = true;
    }

    // --- colour space -----------------------------------------------------
    let color_space = match &raster.colour {
        Colour::Gray { .. } => ImportColorSpace::DeviceGray,
        Colour::Rgb => ImportColorSpace::DeviceRgb,
        Colour::Palette {
            hival,
            lookup,
            assumed_8bit,
        } => {
            notes.tiff_palette_assumed_8bit = *assumed_8bit;
            ImportColorSpace::Indexed {
                hival: *hival,
                lookup: lookup.clone(),
            }
        }
    };

    // --- the one passthrough case ----------------------------------------
    let data = if let Some(verbatim) = passthrough_strip(tiff, raster, &plan, &notes) {
        verbatim
    } else {
        notes.recompressed = Some(if alpha.is_some() {
            RecompressReason::AlphaSplit
        } else {
            RecompressReason::SourceCodecNotReusable
        });
        flate_encode(&samples)?
    };

    let soft_mask = alpha
        .map(|a| -> Result<SoftMask, ImageImportError> {
            Ok(SoftMask {
                width: raster.width,
                height: raster.height,
                // The mask keeps the source's precision: a 16-bit RGBA TIFF has
                // 16-bit alpha, and quantising it to 8 would discard data the
                // operator supplied, silently.
                bits_per_component: u8::try_from(raster.bits).unwrap_or(8),
                data: flate_encode(&a)?,
            })
        })
        .transpose()?;
    if soft_mask.is_some() {
        notes.alpha_to_soft_mask = true;
        raise_version(&mut notes.requires_pdf_version, PdfFeature::SoftMask);
    }

    Ok(ImportedImage {
        format: ImageFormat::Tiff,
        width: raster.width,
        height: raster.height,
        bits_per_component: u8::try_from(raster.bits).unwrap_or(8),
        color_space,
        filter: ImportFilter::Flate,
        data,
        soft_mask,
        color_key_mask: None,
        orientation: raster.orientation,
        dpi: raster.dpi,
        notes,
    })
}

/// The source strip's own bytes, when they already **are** a conforming plain
/// `/FlateDecode` stream for this image — see the module docs.
///
/// Returns `None` whenever anything at all had to be done to the samples, and
/// **verifies before it returns bytes**: the strip is decompressed and its
/// length compared against `stride × height`. A passthrough whose geometry was
/// never checked is a way to embed a stream that renders as garbage with no
/// error anywhere in the file.
fn passthrough_strip(
    tiff: &Tiff<'_>,
    raster: &Raster,
    plan: &Plan,
    notes: &ImportNotes,
) -> Option<Vec<u8>> {
    if !matches!(raster.compression, 8 | 32946)
        || raster.predictor != 1
        || plan.swap16
        || raster.extra.is_some()
        || notes.tiff_white_is_zero_inverted
        || matches!(raster.colour, Colour::Gray { invert: true })
    {
        return None;
    }
    // Exactly one strip: two zlib streams concatenated are not one zlib
    // stream, so any multi-strip image has to be rebuilt.
    let rows_per_strip = usize::try_from(raster.rows_per_strip).unwrap_or(usize::MAX);
    if rows_per_strip < raster.height as usize || raster.strip_offsets.len() != 1 {
        return None;
    }

    let start = usize::try_from(*raster.strip_offsets.first()?).ok()?;
    let len = usize::try_from(*raster.strip_counts.first()?).ok()?;
    let raw = tiff.data.get(start..start.checked_add(len)?)?;
    let total = plan.stride.checked_mul(raster.height as usize)?;
    // The verification: the strip must decode to EXACTLY the sample buffer the
    // dictionary will describe — no surplus, no shortfall.
    if flate::decode(raw, None).ok()?.len() != total {
        return None;
    }
    Some(raw.to_vec())
}

/// De-interleave the last sample of each pixel into its own buffer.
///
/// Returns `(colour samples, extra samples)`, both byte-packed with no row
/// padding — the stride collapses because the output rows are narrower than
/// the input's. Only reached at 8 or 16 bits per sample (sub-byte extras are
/// refused as `TIFF/sub-byte-alpha`), so every boundary here is byte-aligned.
fn split_extra(
    samples: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    channels: usize,
    depth_bytes: usize,
) -> (Vec<u8>, Vec<u8>) {
    let px_in = (channels + 1) * depth_bytes;
    let px_colour = channels * depth_bytes;
    let mut base = Vec::with_capacity(width * height * px_colour);
    let mut extra = Vec::with_capacity(width * height * depth_bytes);
    for y in 0..height {
        let row = samples
            .get(y * stride..(y + 1) * stride)
            .unwrap_or_default();
        for x in 0..width {
            let at = x * px_in;
            let px = row.get(at..at + px_in).unwrap_or_default();
            base.extend_from_slice(px.get(..px_colour).unwrap_or_default());
            extra.extend_from_slice(px.get(px_colour..).unwrap_or_default());
        }
    }
    (base, extra)
}

/// Undo TIFF's *associated* (premultiplied) alpha, in place.
///
/// TIFF 6.0 §18 defines associated alpha as colour that *"has been
/// premultiplied by the alpha value"*, against black. A PDF `/SMask` is a
/// straight alpha channel, so the multiplication has to come back out:
/// `c' = round(c × max / a)`, saturating at `max`, and `0` where `a == 0`
/// (a fully transparent pixel's colour carries no information at all — the
/// premultiplication erased it, and any value is equally right).
///
/// Lossy in the low-alpha tail and exact at full opacity — see the module
/// docs, and [`ImportNotes::tiff_associated_alpha_unpremultiplied`], which is
/// set whenever this runs.
fn unpremultiply(base: &mut [u8], alpha: &[u8], channels: usize, bits: u32) {
    if bits == 16 {
        for (px, a) in base
            .chunks_exact_mut(channels * 2)
            .zip(alpha.chunks_exact(2))
        {
            let av = u32::from(u16::from_be_bytes([
                a.first().copied().unwrap_or(0),
                a.get(1).copied().unwrap_or(0),
            ]));
            for c in px.chunks_exact_mut(2) {
                let v = u32::from(u16::from_be_bytes([
                    c.first().copied().unwrap_or(0),
                    c.get(1).copied().unwrap_or(0),
                ]));
                let out = recover(v, av, 65535);
                let bytes = u16::try_from(out).unwrap_or(u16::MAX).to_be_bytes();
                if let Some(hi) = c.first_mut() {
                    *hi = bytes[0];
                }
                if let Some(lo) = c.get_mut(1) {
                    *lo = bytes[1];
                }
            }
        }
        return;
    }
    for (px, a) in base.chunks_exact_mut(channels).zip(alpha.iter()) {
        let av = u32::from(*a);
        for c in px.iter_mut() {
            *c = u8::try_from(recover(u32::from(*c), av, 255)).unwrap_or(u8::MAX);
        }
    }
}

/// `round(v × max / a)`, saturating, with `a == 0` yielding 0.
const fn recover(v: u32, a: u32, max: u32) -> u32 {
    if a == 0 {
        return 0;
    }
    let scaled = v * max + a / 2;
    let out = scaled / a;
    if out > max { max } else { out }
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

    // -----------------------------------------------------------------
    // A TIFF builder, so every fixture in this file is bytes THIS project
    // chose. Fields are emitted in ascending tag order, which TIFF 6.0 §2
    // requires ("The entries must be sorted in ascending order by Tag").
    // -----------------------------------------------------------------

    /// One directory field, as `(tag, type, values)`.
    #[derive(Clone)]
    struct Field(u16, u16, Vec<u64>);

    /// Build a single-page TIFF. `strips` are the already-compressed strip
    /// payloads; `StripOffsets`/`StripByteCounts` are computed and appended.
    fn tiff_file(order: ByteOrder, fields: &[Field], strips: &[Vec<u8>]) -> Vec<u8> {
        tiff_pages(order, &[(fields.to_vec(), strips.to_vec())])
    }

    /// Build a multi-page TIFF: one IFD per element, chained.
    fn tiff_pages(order: ByteOrder, pages: &[(Vec<Field>, Vec<Vec<u8>>)]) -> Vec<u8> {
        let le = order == ByteOrder::Little;
        let u16b = |v: u16| {
            if le {
                v.to_le_bytes().to_vec()
            } else {
                v.to_be_bytes().to_vec()
            }
        };
        let u32b = |v: u32| {
            if le {
                v.to_le_bytes().to_vec()
            } else {
                v.to_be_bytes().to_vec()
            }
        };

        // Header, then all strip data, then the IFDs and their overflow areas.
        let mut out: Vec<u8> = if le { b"II".to_vec() } else { b"MM".to_vec() };
        out.extend_from_slice(&u16b(42));
        let first_ifd_slot = out.len();
        out.extend_from_slice(&u32b(0));

        // Strip payloads first so their offsets are known before any IFD.
        let mut strip_spans: Vec<Vec<(u32, u32)>> = Vec::new();
        for (_, strips) in pages {
            let mut spans = Vec::new();
            for s in strips {
                spans.push((out.len() as u32, s.len() as u32));
                out.extend_from_slice(s);
            }
            strip_spans.push(spans);
        }

        let mut next_slots: Vec<usize> = Vec::new();
        for (page, (fields, _)) in pages.iter().enumerate() {
            let spans = &strip_spans[page];
            let mut all = fields.clone();
            all.push(Field(
                TAG_STRIP_OFFSETS,
                4,
                spans.iter().map(|s| u64::from(s.0)).collect(),
            ));
            all.push(Field(
                TAG_STRIP_BYTE_COUNTS,
                4,
                spans.iter().map(|s| u64::from(s.1)).collect(),
            ));
            all.sort_by_key(|f| f.0);

            // Pad to an even offset (TIFF 6.0 §2 wants word-aligned IFDs).
            if out.len() % 2 == 1 {
                out.push(0);
            }
            let ifd_at = out.len() as u32;
            if page == 0 {
                out[first_ifd_slot..first_ifd_slot + 4].copy_from_slice(&u32b(ifd_at));
            } else {
                let slot = next_slots[page - 1];
                out[slot..slot + 4].copy_from_slice(&u32b(ifd_at));
            }

            let mut table = u16b(all.len() as u16);
            let mut overflow: Vec<u8> = Vec::new();
            // The overflow area starts right after the entry table + next slot.
            let overflow_base = ifd_at as usize + 2 + all.len() * 12 + 4;
            for Field(tag, kind, values) in &all {
                table.extend_from_slice(&u16b(*tag));
                table.extend_from_slice(&u16b(*kind));
                table.extend_from_slice(&u32b(values.len() as u32));
                let size = type_size(*kind).unwrap();
                let mut payload = Vec::new();
                for v in values {
                    match size {
                        1 => payload.push(*v as u8),
                        2 => payload.extend_from_slice(&u16b(*v as u16)),
                        4 => payload.extend_from_slice(&u32b(*v as u32)),
                        _ => {
                            // RATIONAL: numerator then denominator.
                            payload.extend_from_slice(&u32b(*v as u32));
                            payload.extend_from_slice(&u32b(1));
                        }
                    }
                }
                if payload.len() <= 4 {
                    payload.resize(4, 0);
                    table.extend_from_slice(&payload);
                } else {
                    table.extend_from_slice(&u32b((overflow_base + overflow.len()) as u32));
                    overflow.extend_from_slice(&payload);
                }
            }
            let next_slot = ifd_at as usize + table.len();
            table.extend_from_slice(&u32b(0));
            next_slots.push(next_slot);
            out.extend_from_slice(&table);
            out.extend_from_slice(&overflow);
        }
        out
    }

    fn deflate(data: &[u8]) -> Vec<u8> {
        use flate2::{Compression, write::ZlibEncoder};
        use std::io::Write;
        let mut e = ZlibEncoder::new(Vec::new(), Compression::default());
        e.write_all(data).unwrap();
        e.finish().unwrap()
    }

    fn lzw_encode(data: &[u8]) -> Vec<u8> {
        // `weezl`'s encoder is compiled in and used in tests ONLY, exactly as
        // `pdfcer-core`'s manifest documents for the LZW filter's fixtures.
        weezl::encode::Encoder::new(weezl::BitOrder::Msb, 8)
            .encode(data)
            .unwrap()
    }

    /// Minimum fields for a greyscale image.
    fn gray_fields(w: u64, h: u64, bits: u64, compression: u64, photometric: u64) -> Vec<Field> {
        vec![
            Field(TAG_IMAGE_WIDTH, 4, vec![w]),
            Field(TAG_IMAGE_LENGTH, 4, vec![h]),
            Field(TAG_BITS_PER_SAMPLE, 3, vec![bits]),
            Field(TAG_COMPRESSION, 3, vec![compression]),
            Field(TAG_PHOTOMETRIC, 3, vec![photometric]),
            Field(TAG_SAMPLES_PER_PIXEL, 3, vec![1]),
            Field(TAG_ROWS_PER_STRIP, 4, vec![h]),
        ]
    }

    fn decoded(img: &ImportedImage) -> Vec<u8> {
        flate::decode(&img.data, None).unwrap()
    }

    // -----------------------------------------------------------------
    // Byte order — the single most common TIFF bug
    // -----------------------------------------------------------------

    /// The SAME picture in both byte orders must import to identical samples.
    /// A reader that swaps the tags but not the 16-bit sample data passes an
    /// 8-bit version of this test and fails the 16-bit one below.
    #[test]
    fn both_byte_orders_produce_the_same_image() {
        let rows = vec![1u8, 2, 3, 4, 5, 6];
        let mut out = Vec::new();
        for order in [ByteOrder::Little, ByteOrder::Big] {
            let file = tiff_file(
                order,
                &gray_fields(3, 2, 8, 1, 1),
                std::slice::from_ref(&rows),
            );
            let img = import(&file).unwrap();
            assert_eq!(img.width, 3);
            assert_eq!(img.height, 2);
            assert_eq!(img.color_space, ImportColorSpace::DeviceGray);
            out.push(decoded(&img));
        }
        assert_eq!(out[0], rows, "II must decode to the stored samples");
        assert_eq!(out[0], out[1], "II and MM must agree");
    }

    /// 16-bit samples are stored in the FILE's byte order but PDF wants them
    /// high-order byte first (§8.9.3). This is the assertion that fails if the
    /// swap is missing — and it cannot be reached by an 8-bit fixture.
    #[test]
    fn sixteen_bit_samples_become_big_endian_whatever_the_file_said() {
        // Two samples: 0x1234 and 0x5678.
        let be = vec![0x12u8, 0x34, 0x56, 0x78];
        let le = vec![0x34u8, 0x12, 0x78, 0x56];

        let big = tiff_file(
            ByteOrder::Big,
            &gray_fields(2, 1, 16, 1, 1),
            std::slice::from_ref(&be),
        );
        let little = tiff_file(ByteOrder::Little, &gray_fields(2, 1, 16, 1, 1), &[le]);

        assert_eq!(decoded(&import(&big).unwrap()), be);
        assert_eq!(
            decoded(&import(&little).unwrap()),
            be,
            "an II file's 16-bit samples must be byte-swapped into PDF order"
        );
        assert_eq!(
            import(&big).unwrap().notes.requires_pdf_version,
            Some(PdfFeature::BitsPerComponent16)
        );
    }

    // -----------------------------------------------------------------
    // Compression
    // -----------------------------------------------------------------

    #[test]
    fn every_accepted_compression_decodes_to_the_same_samples() {
        let pixels: Vec<u8> = (0..12u8).collect();
        let cases: Vec<(u64, Vec<u8>)> = vec![
            (1, pixels.clone()),
            (5, lzw_encode(&pixels)),
            (8, deflate(&pixels)),
            (32946, deflate(&pixels)),
            (32773, packbits_encode(&pixels)),
        ];
        for (compression, strip) in cases {
            let file = tiff_file(
                ByteOrder::Little,
                &gray_fields(4, 3, 8, compression, 1),
                &[strip],
            );
            let img = import(&file)
                .unwrap_or_else(|e| panic!("compression {compression} must decode: {e}"));
            assert_eq!(decoded(&img), pixels, "compression {compression}");
        }
    }

    /// A minimal PackBits encoder for the fixtures — literal runs only, plus
    /// one deliberate `0x80` no-op (see the next test).
    fn packbits_encode(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        for chunk in data.chunks(64) {
            out.push((chunk.len() - 1) as u8);
            out.extend_from_slice(chunk);
        }
        out
    }

    /// **The PackBits divergence, pinned.** `0x80` is a no-op in TIFF 6.0 §9
    /// and end-of-data in ISO 32000-1 §7.4.5. A TIFF strip decoded through
    /// `filters::runlength` would stop at the pad and lose every byte after
    /// it — here, half the image.
    #[test]
    fn packbits_treats_0x80_as_a_no_op_not_end_of_data() {
        let pixels: Vec<u8> = (0..8u8).collect();
        let mut strip = vec![3u8];
        strip.extend_from_slice(&pixels[0..4]);
        strip.push(128); // the no-op a §7.4.5 decoder would read as EOD
        strip.push(3);
        strip.extend_from_slice(&pixels[4..8]);

        let file = tiff_file(ByteOrder::Big, &gray_fields(4, 2, 8, 32773, 1), &[strip]);
        assert_eq!(decoded(&import(&file).unwrap()), pixels);

        // And the divergence is real, not hypothetical: the shared PDF filter
        // genuinely stops at the pad.
        let mut same = vec![3u8];
        same.extend_from_slice(&pixels[0..4]);
        same.push(128);
        same.push(3);
        same.extend_from_slice(&pixels[4..8]);
        assert_eq!(
            crate::filters::runlength::decode(&same).unwrap().len(),
            4,
            "the PDF filter stops at 0x80 — which is why TIFF needs its own"
        );
    }

    #[test]
    fn packbits_repeat_runs_use_257_minus_l() {
        // 0xFF -> 2 copies, 0x81 -> 128 copies. Six pixels: qq then rrrr.
        let strip = vec![0xFFu8, b'q', 0xFD, b'r'];
        let file = tiff_file(ByteOrder::Big, &gray_fields(6, 1, 8, 32773, 1), &[strip]);
        assert_eq!(decoded(&import(&file).unwrap()), b"qqrrrr".to_vec());
    }

    // -----------------------------------------------------------------
    // Strips
    // -----------------------------------------------------------------

    /// Multi-strip images are the norm for anything a scanner produces, and
    /// each strip is independently compressed. A reader that decompresses the
    /// concatenation instead of the strips gets one strip and an error.
    #[test]
    fn a_multi_strip_image_reassembles_in_order() {
        let rows: Vec<Vec<u8>> = (0..6u8).map(|y| vec![y * 10, y * 10 + 1]).collect();
        // Two rows per strip, three strips, each deflated separately.
        let strips: Vec<Vec<u8>> = rows.chunks(2).map(|pair| deflate(&pair.concat())).collect();
        let mut fields = gray_fields(2, 6, 8, 8, 1);
        fields.retain(|f| f.0 != TAG_ROWS_PER_STRIP);
        fields.push(Field(TAG_ROWS_PER_STRIP, 4, vec![2]));

        let file = tiff_file(ByteOrder::Little, &fields, &strips);
        let img = import(&file).unwrap();
        assert_eq!(decoded(&img), rows.concat());
        assert_eq!(
            img.notes.recompressed,
            Some(RecompressReason::SourceCodecNotReusable),
            "several zlib streams cannot be one PDF stream"
        );
    }

    /// The last strip of an image whose height is not a multiple of
    /// `RowsPerStrip` is SHORT. A reader that assumes every strip is full
    /// reads past the buffer or pads the image with a phantom row.
    #[test]
    fn a_ragged_last_strip_is_honoured() {
        let rows: Vec<Vec<u8>> = (0..5u8).map(|y| vec![y, y]).collect();
        let strips: Vec<Vec<u8>> = rows.chunks(2).map(|c| c.concat()).collect();
        assert_eq!(strips.len(), 3);
        assert_eq!(strips[2].len(), 2, "the last strip holds ONE row");
        let mut fields = gray_fields(2, 5, 8, 1, 1);
        fields.retain(|f| f.0 != TAG_ROWS_PER_STRIP);
        fields.push(Field(TAG_ROWS_PER_STRIP, 4, vec![2]));

        let file = tiff_file(ByteOrder::Big, &fields, &strips);
        assert_eq!(decoded(&import(&file).unwrap()), rows.concat());
    }

    // -----------------------------------------------------------------
    // Photometric interpretation
    // -----------------------------------------------------------------

    /// `WhiteIsZero` is the fax-lineage default and is what monochrome CAD
    /// output uses. Read as `/DeviceGray` without inverting, the picture is a
    /// photographic negative.
    #[test]
    fn white_is_zero_is_inverted_into_devicegray_polarity() {
        let pixels = vec![0x00u8, 0xFF, 0x40];
        let file = tiff_file(ByteOrder::Big, &gray_fields(3, 1, 8, 1, 0), &[pixels]);
        let img = import(&file).unwrap();
        assert_eq!(decoded(&img), vec![0xFF, 0x00, 0xBF]);
        assert!(img.notes.tiff_white_is_zero_inverted);
        assert_eq!(img.color_space, ImportColorSpace::DeviceGray);
    }

    /// The same complement at one bit per sample, where a bilevel scan lives —
    /// padding bits included, because they are not samples and nothing reads
    /// them.
    #[test]
    fn white_is_zero_inverts_bilevel_samples_too() {
        // 6 pixels, 1 bit each: 0b101010 then two padding bits.
        let file = tiff_file(
            ByteOrder::Big,
            &gray_fields(6, 1, 1, 1, 0),
            &[vec![0b1010_1000]],
        );
        let img = import(&file).unwrap();
        assert_eq!(img.bits_per_component, 1);
        assert_eq!(decoded(&img), vec![0b0101_0111]);
    }

    #[test]
    fn black_is_zero_is_left_alone() {
        let pixels = vec![0x00u8, 0xFF, 0x40];
        let file = tiff_file(
            ByteOrder::Big,
            &gray_fields(3, 1, 8, 1, 1),
            std::slice::from_ref(&pixels),
        );
        let img = import(&file).unwrap();
        assert_eq!(decoded(&img), pixels);
        assert!(!img.notes.tiff_white_is_zero_inverted);
    }

    #[test]
    fn rgb_becomes_devicergb() {
        let pixels = vec![1u8, 2, 3, 4, 5, 6];
        let mut fields = gray_fields(2, 1, 8, 1, 2);
        fields.retain(|f| f.0 != TAG_SAMPLES_PER_PIXEL && f.0 != TAG_BITS_PER_SAMPLE);
        fields.push(Field(TAG_SAMPLES_PER_PIXEL, 3, vec![3]));
        fields.push(Field(TAG_BITS_PER_SAMPLE, 3, vec![8, 8, 8]));
        let file = tiff_file(ByteOrder::Little, &fields, std::slice::from_ref(&pixels));
        let img = import(&file).unwrap();
        assert_eq!(img.color_space, ImportColorSpace::DeviceRgb);
        assert_eq!(decoded(&img), pixels);
    }

    /// TIFF 6.0 §16 stores a `ColorMap` as three CONSECUTIVE BLOCKS (all red,
    /// all green, all blue), while §8.6.6.3 wants interleaved triples. A
    /// reader that copies the field straight through gets a palette whose
    /// every colour is wrong.
    #[test]
    fn a_colormap_is_deblocked_into_indexed_triples() {
        // Four entries, 16-bit values: black, red, green, blue.
        let map: Vec<u64> = vec![
            0, 65535, 0, 0, // red block
            0, 0, 65535, 0, // green block
            0, 0, 0, 65535, // blue block
        ];
        let mut fields = gray_fields(4, 1, 2, 1, 3);
        fields.push(Field(TAG_COLOR_MAP, 3, map));
        // 4 pixels at 2 bits: indices 0, 1, 2, 3.
        let file = tiff_file(ByteOrder::Big, &fields, &[vec![0b0001_1011]]);
        let img = import(&file).unwrap();
        assert_eq!(
            img.color_space,
            ImportColorSpace::Indexed {
                hival: 3,
                lookup: vec![0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255],
            }
        );
        assert!(!img.notes.tiff_palette_assumed_8bit);
        assert_eq!(decoded(&img), vec![0b0001_1011]);
    }

    /// The recorded standard-vs-practice ambiguity: a `ColorMap` whose values
    /// are all ≤ 255 is read as already-8-bit, and the assumption is DISCLOSED.
    /// Trusting TIFF 6.0 §16 literally would render this palette near-black.
    #[test]
    fn an_eight_bit_colormap_is_detected_and_disclosed() {
        let map: Vec<u64> = vec![0, 255, 0, 0, 0, 0];
        let mut fields = gray_fields(2, 1, 1, 1, 3);
        fields.push(Field(TAG_COLOR_MAP, 3, map));
        let file = tiff_file(ByteOrder::Big, &fields, &[vec![0b0100_0000]]);
        let img = import(&file).unwrap();
        assert_eq!(
            img.color_space,
            ImportColorSpace::Indexed {
                hival: 1,
                lookup: vec![0, 0, 0, 255, 0, 0],
            }
        );
        assert!(
            img.notes.tiff_palette_assumed_8bit,
            "the heuristic fired and must say so"
        );
    }

    // -----------------------------------------------------------------
    // Alpha — ExtraSamples
    // -----------------------------------------------------------------

    fn rgba_fields(w: u64, h: u64, bits: u64, extra: Option<u64>) -> Vec<Field> {
        let mut f = vec![
            Field(TAG_IMAGE_WIDTH, 4, vec![w]),
            Field(TAG_IMAGE_LENGTH, 4, vec![h]),
            Field(TAG_BITS_PER_SAMPLE, 3, vec![bits; 4]),
            Field(TAG_COMPRESSION, 3, vec![1]),
            Field(TAG_PHOTOMETRIC, 3, vec![2]),
            Field(TAG_SAMPLES_PER_PIXEL, 3, vec![4]),
            Field(TAG_ROWS_PER_STRIP, 4, vec![h]),
        ];
        if let Some(e) = extra {
            f.push(Field(TAG_EXTRA_SAMPLES, 3, vec![e]));
        }
        f
    }

    #[test]
    fn unassociated_alpha_splits_straight_into_a_soft_mask() {
        // Two pixels: (10,20,30,a=128) and (40,50,60,a=255).
        let pixels = vec![10u8, 20, 30, 128, 40, 50, 60, 255];
        let file = tiff_file(ByteOrder::Little, &rgba_fields(2, 1, 8, Some(2)), &[pixels]);
        let img = import(&file).unwrap();
        assert_eq!(img.color_space, ImportColorSpace::DeviceRgb);
        assert_eq!(decoded(&img), vec![10, 20, 30, 40, 50, 60]);
        let mask = img.soft_mask.expect("unassociated alpha is a soft mask");
        assert_eq!(flate::decode(&mask.data, None).unwrap(), vec![128, 255]);
        assert!(img.notes.alpha_to_soft_mask);
        assert!(
            !img.notes.tiff_associated_alpha_unpremultiplied,
            "straight alpha needs no reconstruction"
        );
        assert_eq!(img.notes.recompressed, Some(RecompressReason::AlphaSplit));
    }

    /// **The compositing trap.** Associated alpha is PREMULTIPLIED; a
    /// `/SMask` is straight. Stored as-is, a half-transparent mid-grey would
    /// paint at a quarter intensity rather than a half — visibly wrong, and
    /// wrong in a direction that looks like a renderer bug.
    #[test]
    fn associated_alpha_is_unpremultiplied_and_disclosed() {
        // A pixel whose true colour is (200, 100, 50) at alpha 128. Stored
        // premultiplied: round(c * 128 / 255).
        let stored = [
            (200u32 * 128 / 255) as u8,
            (100u32 * 128 / 255) as u8,
            (50u32 * 128 / 255) as u8,
        ];
        let pixels = vec![stored[0], stored[1], stored[2], 128];
        let file = tiff_file(ByteOrder::Big, &rgba_fields(1, 1, 8, Some(1)), &[pixels]);
        let img = import(&file).unwrap();

        let out = decoded(&img);
        assert!(img.notes.tiff_associated_alpha_unpremultiplied);
        // Exact recovery is impossible (the premultiply quantised), so the
        // assertion is on closeness — and, critically, on the samples being
        // roughly DOUBLE what was stored rather than equal to it.
        for (got, want) in out.iter().zip([200u8, 100, 50]) {
            assert!(
                got.abs_diff(want) <= 2,
                "un-premultiplied {got} should be near {want}"
            );
        }
        assert_eq!(
            flate::decode(&img.soft_mask.unwrap().data, None).unwrap(),
            vec![128]
        );
    }

    /// A fully transparent pixel's premultiplied colour is all-zero and
    /// carries no information; dividing by zero is not the answer.
    #[test]
    fn associated_alpha_at_zero_does_not_divide_by_zero() {
        let pixels = vec![0u8, 0, 0, 0, 9, 9, 9, 255];
        let file = tiff_file(ByteOrder::Big, &rgba_fields(2, 1, 8, Some(1)), &[pixels]);
        let img = import(&file).unwrap();
        assert_eq!(decoded(&img), vec![0, 0, 0, 9, 9, 9]);
    }

    /// `ExtraSamples 0` (and an absent tag) is "unspecified data", not
    /// opacity. Reading it as alpha is how a perfectly good image renders
    /// entirely invisible — the same trap BMP's fourth byte names.
    #[test]
    fn unspecified_extra_samples_are_dropped_not_read_as_alpha() {
        for extra in [Some(0), None] {
            let pixels = vec![10u8, 20, 30, 0, 40, 50, 60, 0];
            let file = tiff_file(ByteOrder::Little, &rgba_fields(2, 1, 8, extra), &[pixels]);
            let img = import(&file).unwrap();
            assert!(
                img.soft_mask.is_none(),
                "unspecified data is not an alpha channel"
            );
            assert_eq!(decoded(&img), vec![10, 20, 30, 40, 50, 60]);
            assert_eq!(img.notes.tiff_extra_samples_dropped, 1);
        }
    }

    #[test]
    fn sixteen_bit_alpha_keeps_sixteen_bits_and_is_byte_swapped() {
        // One pixel, II: R=0x0102 G=0x0304 B=0x0506 A=0x0708, stored LE.
        let pixels = vec![0x02u8, 0x01, 0x04, 0x03, 0x06, 0x05, 0x08, 0x07];
        let file = tiff_file(
            ByteOrder::Little,
            &rgba_fields(1, 1, 16, Some(2)),
            &[pixels],
        );
        let img = import(&file).unwrap();
        assert_eq!(img.bits_per_component, 16);
        assert_eq!(decoded(&img), vec![1, 2, 3, 4, 5, 6]);
        let mask = img.soft_mask.unwrap();
        assert_eq!(mask.bits_per_component, 16);
        assert_eq!(flate::decode(&mask.data, None).unwrap(), vec![7, 8]);
    }

    // -----------------------------------------------------------------
    // Predictor
    // -----------------------------------------------------------------

    /// `Predictor 2` is horizontal differencing (TIFF 6.0 §14) and is what
    /// every LZW/Deflate TIFF written by a serious tool uses. Ignoring it
    /// produces a smeared, monotonically-brightening image.
    #[test]
    fn predictor_2_is_undone_per_channel() {
        // True RGB pixels: (10,20,30) (11,22,33) (12,24,36).
        let truth = vec![10u8, 20, 30, 11, 22, 33, 12, 24, 36];
        // Horizontal differences, per channel.
        let diffed = vec![10u8, 20, 30, 1, 2, 3, 1, 2, 3];

        let mut fields = gray_fields(3, 1, 8, 8, 2);
        fields.retain(|f| f.0 != TAG_SAMPLES_PER_PIXEL && f.0 != TAG_BITS_PER_SAMPLE);
        fields.push(Field(TAG_SAMPLES_PER_PIXEL, 3, vec![3]));
        fields.push(Field(TAG_BITS_PER_SAMPLE, 3, vec![8, 8, 8]));
        fields.push(Field(TAG_PREDICTOR, 3, vec![2]));

        let file = tiff_file(ByteOrder::Little, &fields, &[deflate(&diffed)]);
        assert_eq!(decoded(&import(&file).unwrap()), truth);
    }

    /// A 16-bit `II` image with `Predictor 2`: the swap must happen BEFORE the
    /// un-prediction, because the differences are between 16-bit VALUES and
    /// `predictor::unpredict` reads them big-endian (§7.4.4.4 rule 3).
    #[test]
    fn predictor_2_at_16_bits_swaps_before_it_un_predicts() {
        // True samples 0x0100 and 0x0300; difference 0x0200. Stored LE.
        let diffed_le = vec![0x00u8, 0x01, 0x00, 0x02];
        let mut fields = gray_fields(2, 1, 16, 8, 1);
        fields.push(Field(TAG_PREDICTOR, 3, vec![2]));
        let file = tiff_file(ByteOrder::Little, &fields, &[deflate(&diffed_le)]);
        assert_eq!(
            decoded(&import(&file).unwrap()),
            vec![0x01, 0x00, 0x03, 0x00],
            "swap first, then difference — the other order gives 0x0101"
        );
    }

    // -----------------------------------------------------------------
    // Passthrough
    // -----------------------------------------------------------------

    /// The one case where a TIFF's own compressed bytes are already a legal
    /// PDF stream: single strip, Deflate, no predictor, nothing to transform.
    #[test]
    fn a_single_strip_deflate_tiff_passes_its_bytes_through() {
        let pixels: Vec<u8> = (0..12u8).collect();
        let strip = deflate(&pixels);
        let file = tiff_file(
            ByteOrder::Big,
            &gray_fields(4, 3, 8, 8, 1),
            std::slice::from_ref(&strip),
        );
        let img = import(&file).unwrap();
        assert_eq!(img.data, strip, "the strip's own bytes, unaltered");
        assert_eq!(img.filter, ImportFilter::Flate);
        assert!(img.notes.recompressed.is_none());
        assert_eq!(decoded(&img), pixels);
    }

    /// …and every reason NOT to pass through actually stops it.
    #[test]
    fn a_transform_of_any_kind_cancels_the_passthrough() {
        let pixels: Vec<u8> = (0..12u8).collect();

        // WhiteIsZero: the samples get complemented.
        let inverted = tiff_file(
            ByteOrder::Big,
            &gray_fields(4, 3, 8, 8, 0),
            &[deflate(&pixels)],
        );
        assert_eq!(
            import(&inverted).unwrap().notes.recompressed,
            Some(RecompressReason::SourceCodecNotReusable)
        );

        // LZW: pdfcer writes no LZW encoder (R28), so nothing to reuse.
        let lzw_file = tiff_file(
            ByteOrder::Big,
            &gray_fields(4, 3, 8, 5, 1),
            &[lzw_encode(&pixels)],
        );
        assert_eq!(
            import(&lzw_file).unwrap().notes.recompressed,
            Some(RecompressReason::SourceCodecNotReusable)
        );
    }

    /// A Deflate strip whose decoded length disagrees with the geometry the
    /// dictionary will declare must NOT be passed through — that is a stream
    /// that renders as garbage with no error anywhere.
    #[test]
    fn a_geometry_mismatch_is_never_passed_through() {
        // 4x3 declared, but the strip holds only 6 bytes.
        let short = deflate(&[0u8; 6]);
        let file = tiff_file(ByteOrder::Big, &gray_fields(4, 3, 8, 8, 1), &[short]);
        assert!(matches!(
            import(&file),
            Err(ImageImportError::Corrupt { .. })
        ));
    }

    // -----------------------------------------------------------------
    // Multi-page
    // -----------------------------------------------------------------

    /// The scanner's ordinary output. The first page is placed and the rest
    /// are COUNTED — never silently dropped (rule 4).
    #[test]
    fn a_multi_page_tiff_places_the_first_and_counts_the_rest() {
        let page = |v: u8| (gray_fields(2, 1, 8, 1, 1), vec![vec![v, v]]);
        let file = tiff_pages(ByteOrder::Little, &[page(1), page(2), page(3)]);
        let img = import(&file).unwrap();
        assert_eq!(decoded(&img), vec![1, 1], "the FIRST page");
        assert_eq!(img.notes.tiff_pages_ignored, 2);
    }

    #[test]
    fn a_single_page_tiff_reports_no_ignored_pages() {
        let file = tiff_file(ByteOrder::Big, &gray_fields(2, 1, 8, 1, 1), &[vec![1, 2]]);
        assert_eq!(import(&file).unwrap().notes.tiff_pages_ignored, 0);
    }

    // -----------------------------------------------------------------
    // Metadata
    // -----------------------------------------------------------------

    #[test]
    fn resolution_is_read_in_inches_and_centimetres() {
        for (unit, value, want) in [(2u64, 300u64, 300.0f64), (3, 100, 254.0)] {
            let mut fields = gray_fields(1, 1, 8, 1, 1);
            fields.push(Field(TAG_X_RESOLUTION, 5, vec![value]));
            fields.push(Field(TAG_Y_RESOLUTION, 5, vec![value]));
            fields.push(Field(TAG_RESOLUTION_UNIT, 3, vec![unit]));
            let file = tiff_file(ByteOrder::Little, &fields, &[vec![7]]);
            let img = import(&file).unwrap();
            let (dx, _) = img.dpi.expect("a stated resolution");
            assert!((dx - want).abs() < 0.5, "unit {unit}: {dx}");
            assert_eq!(img.notes.dpi_source, DpiSource::TiffResolution);
        }
    }

    /// `ResolutionUnit 1` means "no absolute unit" — the numbers are an aspect
    /// ratio, not dots per inch. Reading them as dpi invents a physical size
    /// the file explicitly declined to state.
    #[test]
    fn resolution_unit_none_is_not_a_resolution() {
        let mut fields = gray_fields(1, 1, 8, 1, 1);
        fields.push(Field(TAG_X_RESOLUTION, 5, vec![2]));
        fields.push(Field(TAG_Y_RESOLUTION, 5, vec![1]));
        fields.push(Field(TAG_RESOLUTION_UNIT, 3, vec![1]));
        let file = tiff_file(ByteOrder::Big, &fields, &[vec![7]]);
        let img = import(&file).unwrap();
        assert!(img.dpi.is_none());
        assert_eq!(img.notes.dpi_source, DpiSource::Assumed);
    }

    /// TIFF tag 274 and EXIF tag 0x0112 are the same tag with the same eight
    /// values — EXIF is a TIFF IFD. It is applied in the placement matrix, not
    /// to the pixels.
    #[test]
    fn the_orientation_tag_is_applied_in_the_matrix() {
        let mut fields = gray_fields(2, 1, 8, 1, 1);
        fields.push(Field(TAG_ORIENTATION, 3, vec![6]));
        let file = tiff_file(ByteOrder::Little, &fields, &[vec![1, 2]]);
        let img = import(&file).unwrap();
        assert_eq!(img.orientation, Orientation::Rotate90);
        assert_eq!(img.notes.exif_orientation, Some(6));
        assert_eq!(decoded(&img), vec![1, 2], "the PIXELS are untouched");
    }

    #[test]
    fn an_embedded_icc_profile_is_disclosed_not_carried() {
        let mut fields = gray_fields(1, 1, 8, 1, 1);
        fields.push(Field(TAG_ICC_PROFILE, 7, vec![0, 1, 2, 3, 4, 5]));
        let file = tiff_file(ByteOrder::Big, &fields, &[vec![7]]);
        assert!(import(&file).unwrap().notes.colour_profile_dropped);
    }

    // -----------------------------------------------------------------
    // Refusals — every one by name (R27)
    // -----------------------------------------------------------------

    #[test]
    fn declined_compressions_are_refused_by_name() {
        for (compression, feature) in [
            (2u64, "TIFF/ccitt-rle"),
            (3, "TIFF/ccitt-g3"),
            (4, "TIFF/ccitt-g4"),
            (6, "TIFF/jpeg-old"),
            (7, "TIFF/jpeg"),
            (34712, "TIFF/jpeg2000"),
            (99999, "TIFF/unknown-compression"),
        ] {
            let file = tiff_file(
                ByteOrder::Little,
                &gray_fields(2, 1, 8, compression, 1),
                &[vec![0, 0]],
            );
            assert_eq!(
                import(&file).unwrap_err(),
                ImageImportError::Unsupported { feature },
                "compression {compression}"
            );
        }
    }

    #[test]
    fn declined_photometrics_are_refused_by_name() {
        for (photometric, feature) in [
            (4u64, "TIFF/photometric-transparency-mask"),
            (5, "TIFF/photometric-cmyk"),
            (6, "TIFF/photometric-ycbcr"),
            (8, "TIFF/photometric-cielab"),
            (32803, "TIFF/photometric-cfa"),
            (12345, "TIFF/photometric-unknown"),
        ] {
            let file = tiff_file(
                ByteOrder::Big,
                &gray_fields(2, 1, 8, 1, photometric),
                &[vec![0, 0]],
            );
            assert_eq!(
                import(&file).unwrap_err(),
                ImageImportError::Unsupported { feature },
                "photometric {photometric}"
            );
        }
    }

    #[test]
    fn structural_variants_pdfcer_cannot_represent_are_refused_by_name() {
        let base = || gray_fields(2, 1, 8, 1, 1);
        let cases: Vec<(Field, &'static str)> = vec![
            (Field(TAG_TILE_WIDTH, 3, vec![16]), "TIFF/tiled"),
            (Field(TAG_PLANAR_CONFIG, 3, vec![2]), "TIFF/planar-separate"),
            (Field(TAG_FILL_ORDER, 3, vec![2]), "TIFF/fill-order-2"),
            (
                Field(TAG_SAMPLE_FORMAT, 3, vec![3]),
                "TIFF/sample-format-float",
            ),
            (
                Field(TAG_SAMPLE_FORMAT, 3, vec![2]),
                "TIFF/sample-format-signed",
            ),
            (Field(TAG_PREDICTOR, 3, vec![3]), "TIFF/predictor-float"),
        ];
        for (field, feature) in cases {
            let mut fields = base();
            fields.push(field);
            let file = tiff_file(ByteOrder::Little, &fields, &[vec![0, 0]]);
            assert_eq!(
                import(&file).unwrap_err(),
                ImageImportError::Unsupported { feature }
            );
        }
    }

    #[test]
    fn thirty_two_bit_and_mixed_sample_depths_are_refused_by_name() {
        let mut wide = gray_fields(1, 1, 32, 1, 1);
        wide.retain(|f| f.0 != TAG_BITS_PER_SAMPLE);
        wide.push(Field(TAG_BITS_PER_SAMPLE, 3, vec![32]));
        let file = tiff_file(ByteOrder::Big, &wide, &[vec![0; 4]]);
        assert_eq!(
            import(&file).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "TIFF/32-bit-samples"
            }
        );

        let mut mixed = gray_fields(1, 1, 8, 1, 2);
        mixed.retain(|f| f.0 != TAG_BITS_PER_SAMPLE && f.0 != TAG_SAMPLES_PER_PIXEL);
        mixed.push(Field(TAG_SAMPLES_PER_PIXEL, 3, vec![3]));
        mixed.push(Field(TAG_BITS_PER_SAMPLE, 3, vec![5, 6, 5]));
        let file = tiff_file(ByteOrder::Big, &mixed, &[vec![0; 4]]);
        assert_eq!(
            import(&file).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "TIFF/mixed-bit-depth"
            }
        );
    }

    #[test]
    fn sub_byte_alpha_and_extra_extras_are_refused_by_name() {
        // 4-bit grey + a declared alpha sample.
        let mut sub = vec![
            Field(TAG_IMAGE_WIDTH, 4, vec![2]),
            Field(TAG_IMAGE_LENGTH, 4, vec![1]),
            Field(TAG_BITS_PER_SAMPLE, 3, vec![4, 4]),
            Field(TAG_COMPRESSION, 3, vec![1]),
            Field(TAG_PHOTOMETRIC, 3, vec![1]),
            Field(TAG_SAMPLES_PER_PIXEL, 3, vec![2]),
            Field(TAG_ROWS_PER_STRIP, 4, vec![1]),
        ];
        sub.push(Field(TAG_EXTRA_SAMPLES, 3, vec![2]));
        let file = tiff_file(ByteOrder::Big, &sub, &[vec![0]]);
        assert_eq!(
            import(&file).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "TIFF/sub-byte-alpha"
            }
        );

        let mut many = vec![
            Field(TAG_IMAGE_WIDTH, 4, vec![1]),
            Field(TAG_IMAGE_LENGTH, 4, vec![1]),
            Field(TAG_BITS_PER_SAMPLE, 3, vec![8; 5]),
            Field(TAG_COMPRESSION, 3, vec![1]),
            Field(TAG_PHOTOMETRIC, 3, vec![2]),
            Field(TAG_SAMPLES_PER_PIXEL, 3, vec![5]),
            Field(TAG_ROWS_PER_STRIP, 4, vec![1]),
        ];
        many.push(Field(TAG_EXTRA_SAMPLES, 3, vec![2, 2]));
        let file = tiff_file(ByteOrder::Big, &many, &[vec![0; 5]]);
        assert_eq!(
            import(&file).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "TIFF/multiple-extra-samples"
            }
        );
    }

    #[test]
    fn a_sixteen_bit_palette_is_refused_by_name() {
        let mut fields = gray_fields(1, 1, 16, 1, 3);
        fields.push(Field(TAG_COLOR_MAP, 3, vec![0; 6]));
        let file = tiff_file(ByteOrder::Big, &fields, &[vec![0, 0]]);
        assert_eq!(
            import(&file).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "TIFF/palette-16-bit"
            }
        );
    }

    #[test]
    fn bigtiff_is_refused_by_name_even_if_the_sniffer_is_bypassed() {
        let mut file = tiff_file(ByteOrder::Little, &gray_fields(1, 1, 8, 1, 1), &[vec![0]]);
        file[2..4].copy_from_slice(&43u16.to_le_bytes());
        assert_eq!(
            import(&file).unwrap_err(),
            ImageImportError::Unsupported {
                feature: "TIFF/bigtiff"
            }
        );
    }

    // -----------------------------------------------------------------
    // Malformed input
    // -----------------------------------------------------------------

    /// Every truncation must be a diagnosis, never a panic. `pdfcer-core`
    /// denies `indexing_slicing` crate-wide precisely because this input is
    /// untrusted.
    #[test]
    fn a_truncated_tiff_is_corrupt_not_a_panic() {
        let file = tiff_file(
            ByteOrder::Little,
            &gray_fields(4, 4, 8, 8, 1),
            &[deflate(&[0u8; 16])],
        );
        for cut in 0..file.len() {
            let _ = import(file.get(..cut).unwrap());
        }
    }

    /// An IFD chain that points back at itself must terminate the walk rather
    /// than loop for ever — and must still place the first page.
    #[test]
    fn a_looping_ifd_chain_terminates() {
        let mut file = tiff_file(ByteOrder::Big, &gray_fields(2, 1, 8, 1, 1), &[vec![1, 2]]);
        // Point the first IFD's "next" slot back at the first IFD.
        let first = u32::from_be_bytes(file[4..8].try_into().unwrap()) as usize;
        let count = u16::from_be_bytes(file[first..first + 2].try_into().unwrap()) as usize;
        let next_slot = first + 2 + count * 12;
        file[next_slot..next_slot + 4].copy_from_slice(&(first as u32).to_be_bytes());
        let img = import(&file).unwrap();
        assert_eq!(decoded(&img), vec![1, 2]);
    }

    #[test]
    fn a_zero_pixel_tiff_is_refused() {
        let file = tiff_file(ByteOrder::Big, &gray_fields(0, 1, 8, 1, 1), &[vec![]]);
        assert!(matches!(import(&file), Err(ImageImportError::Empty { .. })));
    }

    #[test]
    fn a_palette_tiff_with_no_colormap_is_corrupt() {
        let file = tiff_file(ByteOrder::Big, &gray_fields(1, 1, 8, 1, 3), &[vec![0]]);
        assert!(matches!(
            import(&file),
            Err(ImageImportError::Corrupt { .. })
        ));
    }

    #[test]
    fn a_tiff_with_no_photometric_tag_is_corrupt() {
        let mut fields = gray_fields(1, 1, 8, 1, 1);
        fields.retain(|f| f.0 != TAG_PHOTOMETRIC);
        let file = tiff_file(ByteOrder::Big, &fields, &[vec![0]]);
        assert!(matches!(
            import(&file),
            Err(ImageImportError::Corrupt { .. })
        ));
    }
}
