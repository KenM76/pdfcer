//! Evidence bounds (decision 227): a codestream that declares more geometry
//! than its own bytes could encode is refused before anything is sized from
//! that geometry. Every bound is the fewest bytes a conforming encoder can
//! spend, so a file that passes is never one these checks could misjudge.
//!
//! JBIG2 and JXL admit no such bound; Flate-coded image streams are bounded
//! separately by their expected length.

use crate::peak::dct::header;
use crate::{Codec, ImageCodecError};

/// The geometry a refused codestream declared, as named in
/// [`ImageCodecError::DeclaredGeometryExceedsData`].
///
/// # Examples
///
/// ```
/// use pdfcer_image_codec::DeclaredGeometry;
///
/// let g = DeclaredGeometry::Tiles { count: 65_536 };
/// assert_eq!(g.to_string(), "65536 tiles");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DeclaredGeometry {
    /// A frame of `width` x `height` pixels (DCT).
    Pixels {
        /// Declared width.
        width: u32,
        /// Declared height.
        height: u32,
    },
    /// A tile grid of `count` tiles (JPX).
    Tiles {
        /// Declared tile count.
        count: u64,
    },
    /// `rows` scan lines (CCITT).
    Rows {
        /// Declared row count.
        rows: u32,
    },
}

impl std::fmt::Display for DeclaredGeometry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pixels { width, height } => write!(f, "{width} x {height} pixels"),
            Self::Tiles { count } => write!(f, "{count} tiles"),
            Self::Rows { rows } => write!(f, "{rows} rows"),
        }
    }
}

impl ImageCodecError {
    /// A stable, greppable key naming why the image was refused, for
    /// counting refusals by cause: `image/evidence/dct`, `image/evidence/jpx`
    /// or `image/evidence/ccitt` for
    /// [`ImageCodecError::DeclaredGeometryExceedsData`]. `None` for errors
    /// that carry no refusal key.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_image_codec::{Codec, DeclaredGeometry, ImageCodecError};
    ///
    /// let e = ImageCodecError::DeclaredGeometryExceedsData {
    ///     codec: Codec::Ccitt,
    ///     declared: DeclaredGeometry::Rows { rows: 9000 },
    ///     min_bytes: 1125,
    ///     have_bytes: 40,
    /// };
    /// assert_eq!(e.refusal_key(), Some("image/evidence/ccitt"));
    /// assert_eq!(ImageCodecError::Cancelled.refusal_key(), None);
    /// ```
    #[must_use]
    pub fn refusal_key(&self) -> Option<&'static str> {
        match self {
            Self::DeclaredGeometryExceedsData { codec, .. } => Some(match codec {
                Codec::Dct => "image/evidence/dct",
                Codec::Jpx => "image/evidence/jpx",
                Codec::Ccitt => "image/evidence/ccitt",
                Codec::Jbig2 => "image/evidence/jbig2",
                Codec::Jxl => "image/evidence/jxl",
            }),
            _ => None,
        }
    }
}

fn refuse(
    codec: Codec,
    declared: DeclaredGeometry,
    min_bytes: u64,
    have: usize,
) -> ImageCodecError {
    ImageCodecError::DeclaredGeometryExceedsData {
        codec,
        declared,
        min_bytes,
        have_bytes: u64::try_from(have).unwrap_or(u64::MAX),
    }
}

/// Minimum entropy-coded bits for a Huffman DCT stream's first scan.
///
/// A component with sampling `(Hc, Vc)` has `ceil(ceil(X*Hc/Hmax)/8) *
/// ceil(ceil(Y*Vc/Vmax)/8)` blocks (T.81 A.1.1, A.2.4). Every block of the
/// first scan costs at least one Huffman code word for its DC difference
/// (F.1.2.1; Annex C code lengths are 1-16 bits) and, in a sequential scan,
/// at least one more for its AC coefficients, an EOB or a coded coefficient
/// (F.1.2.2). A progressive first scan is a DC scan (G.1.1.1.1), one code
/// word per block; its AC scans can cover thousands of blocks with one
/// EOBRUN code (G.1.2.2), so only the first scan is counted.
///
/// A DNL segment after the first scan may redefine a non-zero `Y` (B.2.5);
/// the bound trusts the frame header's figure, which is what the decoder
/// sizes its buffers from.
///
/// `Ok(None)` means no bound applies: a DNL-deferred height (`Y = 0`,
/// B.2.2), an unparsed header, or an inconsistent one the decoder will
/// reject on its own terms.
fn dct_min_bits(data: &[u8]) -> Result<Option<(u64, DeclaredGeometry)>, ImageCodecError> {
    let Some(h) = header(data) else {
        return Ok(None);
    };
    if h.process == 2 && h.first_scan_ss != 0 {
        return Err(ImageCodecError::Corrupt {
            codec: Codec::Dct,
            detail: format!(
                "progressive frame's first scan starts at coefficient {}; the DC scan \
                 must come first (T.81 G.1.1.1.1)",
                h.first_scan_ss
            ),
        });
    }
    if !matches!(h.process, 0..=2) || h.height == 0 || h.first_scan_selectors.is_empty() {
        return Ok(None);
    }
    let hmax = h.sampling.iter().map(|s| s.0).max().unwrap_or(0);
    let vmax = h.sampling.iter().map(|s| s.1).max().unwrap_or(0);
    if hmax == 0 || vmax == 0 {
        return Ok(None);
    }
    let mut fewest: Option<u64> = None;
    for cs in &h.first_scan_selectors {
        let Some(at) = h.ids.iter().position(|id| id == cs) else {
            return Ok(None);
        };
        let Some(&(hc, vc)) = h.sampling.get(at) else {
            return Ok(None);
        };
        let x = (h.width * hc).div_ceil(hmax).div_ceil(8);
        let y = (h.height * vc).div_ceil(vmax).div_ceil(8);
        let blocks = x.saturating_mul(y);
        fewest = Some(fewest.map_or(blocks, |f| f.min(blocks)));
    }
    let per_block = if h.process == 2 { 1 } else { 2 };
    let declared = DeclaredGeometry::Pixels {
        width: u32::try_from(h.width).unwrap_or(u32::MAX),
        height: u32::try_from(h.height).unwrap_or(u32::MAX),
    };
    Ok(fewest.map(|b| (b.saturating_mul(per_block), declared)))
}

/// DCT evidence: `8 * data.len()` must reach the first scan's minimum bits.
///
/// # Errors
///
/// [`ImageCodecError::DeclaredGeometryExceedsData`] when it does not;
/// [`ImageCodecError::Corrupt`] for a progressive frame whose first scan is
/// not a DC scan, which would make the bound unsound.
pub(crate) fn dct(data: &[u8]) -> Result<(), ImageCodecError> {
    let Some((bits, declared)) = dct_min_bits(data)? else {
        return Ok(());
    };
    let have_bits = u64::try_from(data.len())
        .unwrap_or(u64::MAX)
        .saturating_mul(8);
    if have_bits >= bits {
        return Ok(());
    }
    Err(refuse(Codec::Dct, declared, bits.div_ceil(8), data.len()))
}

#[cfg(feature = "jpx")]
/// Bytes every JPX tile-part takes at least: a 12-byte SOT marker segment
/// (Lsot is fixed at 10) plus the 2-byte SOD marker (T.800 A.4.2).
const JPX_TILE_PART_BYTES: u64 = 14;

#[cfg(feature = "jpx")]
/// JPX evidence: `tiles` may exceed `floor` (the work bound below which no
/// evidence is asked for) only when the data holds a minimal tile-part for
/// each tile, `tiles <= max(floor, len / 14)`.
///
/// # Errors
///
/// [`ImageCodecError::DeclaredGeometryExceedsData`] when it does not.
pub(crate) fn jpx_tiles(tiles: u64, floor: u64, len: usize) -> Result<(), ImageCodecError> {
    let min_bytes = tiles.saturating_mul(JPX_TILE_PART_BYTES);
    if tiles <= floor || u64::try_from(len).unwrap_or(u64::MAX) >= min_bytes {
        return Ok(());
    }
    Err(refuse(
        Codec::Jpx,
        DeclaredGeometry::Tiles { count: tiles },
        min_bytes,
        len,
    ))
}

/// Most rows `len` bytes of CCITT data can code: every row takes at least
/// one bit, the 1-bit V0 code of T.6 Table 1 (the shortest code word of
/// T.4 and T.6), so `8 * len + 1` with one row of slack.
pub(crate) fn ccitt_max_rows(len: usize) -> u64 {
    u64::try_from(len)
        .unwrap_or(u64::MAX)
        .saturating_mul(8)
        .saturating_add(1)
}

/// CCITT evidence: a declared row count must fit [`ccitt_max_rows`].
///
/// # Errors
///
/// [`ImageCodecError::DeclaredGeometryExceedsData`] when it does not.
pub(crate) fn ccitt_rows(rows: u32, len: usize) -> Result<(), ImageCodecError> {
    if u64::from(rows) <= ccitt_max_rows(len) {
        return Ok(());
    }
    let min_bytes = u64::from(rows - 1).div_ceil(8);
    Err(refuse(
        Codec::Ccitt,
        DeclaredGeometry::Rows { rows },
        min_bytes,
        len,
    ))
}
