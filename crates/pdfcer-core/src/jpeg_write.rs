//! Baseline and progressive `/DCTDecode` encoding of 8-bit interleaved
//! samples (ISO 32000-2 §7.4.8), shared by image import and optimisation.
//!
//! Four components are written as Adobe transform 2 (YCCK) from the
//! complement of the samples, so a pdfcer decode returns the samples
//! unchanged and the image's `/Decode` keeps its meaning (decision 006,
//! Adobe TN #5116 §13.1).

use std::fmt;

use jpeg_encoder::{ColorType, Encoder, SamplingFactor};

/// Chroma subsampling for a JPEG with three components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum Chroma {
    /// 4:4:4 at quality 90 and above, else 4:2:0.
    #[default]
    Auto,
    /// 4:4:4: full-resolution colour.
    Full,
    /// 4:2:0: colour at half resolution on each axis.
    Half,
}

impl Chroma {
    /// Every value, in spelling order.
    pub const ALL: [Self; 3] = [Self::Auto, Self::Full, Self::Half];

    /// The profile spelling: `auto`, `444` or `420`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Full => "444",
            Self::Half => "420",
        }
    }

    /// The value spelt `name`.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == name)
    }
}

impl fmt::Display for Chroma {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How [`encode`] writes the codestream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JpegOptions {
    /// IJG quality, 1 to 100.
    pub(crate) quality: u8,
    pub(crate) chroma: Chroma,
    pub(crate) progressive: bool,
    /// Per-image Huffman tables: lossless, smaller, one more pass.
    pub(crate) optimize_huffman: bool,
}

/// Encode `samples` (row-major, `width x height x components` bytes, no
/// row padding) as a JPEG codestream.
///
/// # Errors
///
/// A message when `components` is not 1, 3 or 4, the buffer length does not
/// match, a side exceeds 65 535, or the encoder fails.
pub(crate) fn encode(
    samples: &[u8],
    width: u32,
    height: u32,
    components: u8,
    opts: JpegOptions,
) -> Result<Vec<u8>, String> {
    let (Ok(w16), Ok(h16)) = (u16::try_from(width), u16::try_from(height)) else {
        return Err(format!("{width}x{height} exceeds JPEG's 65535 limit"));
    };
    let expected = (width as usize)
        .checked_mul(height as usize)
        .and_then(|px| px.checked_mul(usize::from(components)));
    if expected != Some(samples.len()) {
        return Err(format!(
            "{} sample bytes for {width}x{height}x{components}",
            samples.len()
        ));
    }
    let mut out = Vec::new();
    let mut encoder = Encoder::new(&mut out, opts.quality);
    // jpeg-encoder's YCCK table subsamples Y and K, not C/M/Y, so a
    // subsampled CMYK image would lose ink-edge resolution: always 1:1.
    let sampling = match (components, opts.chroma) {
        (4, _) | (_, Chroma::Full) => Some(SamplingFactor::F_1_1),
        (_, Chroma::Half) => Some(SamplingFactor::F_2_2),
        (_, Chroma::Auto) => None,
    };
    if let Some(s) = sampling {
        encoder.set_sampling_factor(s);
    }
    encoder.set_progressive(opts.progressive);
    encoder.set_optimized_huffman_tables(opts.optimize_huffman);
    let inverted: Vec<u8>;
    let (payload, color_type) = match components {
        1 => (samples, ColorType::Luma),
        3 => (samples, ColorType::Rgb),
        4 => {
            inverted = samples.iter().map(|&v| 255 - v).collect();
            (inverted.as_slice(), ColorType::CmykAsYcck)
        }
        n => return Err(format!("{n} components have no /DCTDecode layout")),
    };
    encoder
        .encode(payload, w16, h16, color_type)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

/// Unpack §8.9.3-packed samples into one byte per component, dropping row
/// padding.
///
/// # The three cases, and why each scales the way it does
///
/// - **8 bits** — already one byte per component; only the row padding
///   (which is zero for 8-bit data anyway) is stepped over.
/// - **16 bits** — the high byte is taken. This is a real loss of precision
///   and it is unavoidable: §7.4.8 and Table 89 both fix `/DCTDecode` at
///   8-bit samples, so there is no 16-bit JPEG to write. Taking the high
///   byte rather than rounding is deliberate — `(v >> 8)` is exactly
///   `floor(v × 255 / 65535)` to within one unit and needs no arithmetic
///   that could overflow, and the difference is far below the quantisation
///   the encoder is about to apply anyway.
/// - **1, 2 or 4 bits** — samples are packed *"from high-order to low-order
///   bits"* (§7.4.4.4, §8.9.3). A colour value is scaled onto 0..=255 by
///   `v × 255 / (2ⁿ − 1)`, which maps the extremes exactly (0→0, max→255) —
///   the naive `v << (8 − n)` would map 4-bit 15 to 240 and quietly darken
///   every white pixel. **Indexed images are handled by the caller**, which
///   must NOT scale: there the value is a palette index.
///
/// Everything past the last full row, and every bit past the last sample in
/// a row, is padding the caller never sees.
pub(crate) fn unpack_to_bytes(
    packed: &[u8],
    width: u32,
    height: u32,
    components: usize,
    bits: u8,
    stride: usize,
) -> Vec<u8> {
    let per_row = (width as usize).saturating_mul(components);
    let mut out = Vec::with_capacity(per_row.saturating_mul(height as usize));

    for y in 0..height as usize {
        let row = packed
            .get(y.saturating_mul(stride)..)
            .and_then(|r| r.get(..stride))
            .unwrap_or(&[]);
        match bits {
            8 => out.extend_from_slice(row.get(..per_row).unwrap_or(row)),
            16 => {
                for i in 0..per_row {
                    out.push(row.get(i.saturating_mul(2)).copied().unwrap_or(0));
                }
            }
            b @ (1 | 2 | 4) => {
                let n = u32::from(b);
                let max = (1u32 << n) - 1;
                let per_byte = 8 / n as usize;
                let mask = max as u8;
                for i in 0..per_row {
                    let byte = row.get(i / per_byte).copied().unwrap_or(0);
                    // High-order first: sample 0 of a byte lives in its top
                    // `n` bits.
                    let shift = 8 - n as usize - (i % per_byte) * n as usize;
                    let v = u32::from((byte >> shift) & mask);
                    // Exact endpoint mapping; see the doc comment.
                    out.push(((v * 255) / max) as u8);
                }
            }
            // Table 89 admits only 1, 2, 4, 8 and 16. Anything else has
            // already been refused by the format importers, so this arm is
            // unreachable; emitting the row verbatim keeps the function
            // total without inventing a scaling rule for a depth that
            // cannot occur.
            _ => out.extend_from_slice(row.get(..per_row).unwrap_or(row)),
        }
    }
    out
}
