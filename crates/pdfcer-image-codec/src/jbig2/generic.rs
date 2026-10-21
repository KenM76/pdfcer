//! JBIG2 generic-region encoder: template 0, TPGDON, nominal AT pixels,
//! wrapped as a PDF-embedded segment stream (ITU-T T.88 §6.2.5, §7.2,
//! §7.4.6, §7.4.8, Annex D.3).
//!
//! Spec source: `filters/filter__jbig2_generic_encoder.md` §3 and §6 in
//! the PDF-spec RAG.

use super::mq::MqEncoder;

/// The 16-bit context of Figure 8, under this module's bit ordering
/// (RAG §3.5). SLTP is coded in the same bin as a pixel with this
/// neighbourhood: one 65536-entry context array serves both.
const SLTP_CONTEXT: usize = 0x9B25;

/// Encodes 1-bit samples as a lossless JBIG2 generic region.
///
/// `samples` holds `height` rows of `width` pixels, one bit per pixel,
/// MSB first, each row padded to a whole byte (the PDF image sample
/// layout, ISO 32000-2 §8.9.3). Bits past the end of `samples` read as 0.
///
/// The output is a complete `/JBIG2Decode` stream in the embedded
/// organisation (T.88 Annex D.3): a page-information segment for a
/// `width` x `height` page, then one immediate lossless generic-region
/// segment covering it (template 0, typical prediction on, nominal AT
/// pixels, MQ coding). It needs no `/JBIG2Globals`, so no
/// `/DecodeParms`. A PDF sample 1 is coded as a JBIG2 0 (white), so the
/// decoded samples equal `samples` and the image keeps its `/Decode`.
/// The image needs `/BitsPerComponent 1` and `/Width` and `/Height`
/// equal to `width` and `height`, and must be an XObject: §7.4.7 does
/// not allow the filter on an inline image.
///
/// # Examples
///
/// ```
/// use pdfcer_image_codec::jbig2::encode_generic;
///
/// let stream = encode_generic(&[0xFF, 0x00], 8, 2);
/// // Segment 0 is the page information, type 48.
/// assert_eq!(&stream[..5], &[0, 0, 0, 0, 48]);
/// assert!(stream.ends_with(&[0xFF, 0xAC]));
/// ```
#[must_use]
pub fn encode_generic(samples: &[u8], width: u32, height: u32) -> Vec<u8> {
    let data = code_region(samples, width, height);
    let mut out = Vec::with_capacity(data.len() + 56);
    // Page information (§7.4.8): segment 0, type 48, page 1, 19 bytes.
    segment_header(&mut out, 0, 48, 19);
    region_info(&mut out, width, height);
    // Page flags: eventually lossless; default pixel 0; OR combination.
    out.push(0x01);
    // Page striping: none.
    out.extend_from_slice(&[0, 0]);
    // Immediate lossless generic region (§7.4.6.1): segment 1, type 39.
    let length = u32::try_from(data.len() + 26).unwrap_or(u32::MAX);
    segment_header(&mut out, 1, 39, length);
    region_info(&mut out, width, height);
    // Region flags: external combination operator OR.
    out.push(0x00);
    // Generic flags: MMR 0, template 0, TPGDON 1.
    out.push(0x08);
    // The nominal AT pixels A1-A4 (§6.2.5.4, Table 5).
    out.extend_from_slice(&[0x03, 0xFF, 0xFD, 0xFF, 0x02, 0xFE, 0xFE, 0xFE]);
    out.extend_from_slice(&data);
    out
}

/// A short-form segment header (§7.2): number, type, no referred-to
/// segments, page association 1, data length.
fn segment_header(out: &mut Vec<u8>, number: u32, kind: u8, length: u32) {
    out.extend_from_slice(&number.to_be_bytes());
    out.extend_from_slice(&[kind, 0x00, 0x01]);
    out.extend_from_slice(&length.to_be_bytes());
}

/// Width, height, and X and Y location 0 (§7.4.1 and §7.4.8 share it).
fn region_info(out: &mut Vec<u8>, width: u32, height: u32) {
    out.extend_from_slice(&width.to_be_bytes());
    out.extend_from_slice(&height.to_be_bytes());
    out.extend_from_slice(&[0; 8]);
}

/// The MQ-coded region data, ending with the `0xFF 0xAC` marker.
fn code_region(samples: &[u8], width: u32, height: u32) -> Vec<u8> {
    let w = width as usize;
    let stride = w.div_ceil(8);
    let zero = vec![0u8; w];
    let mut mq = MqEncoder::new(1 << 16);
    let (mut above2, mut above) = (zero.clone(), zero);
    let mut ltp = false;
    for y in 0..height as usize {
        let row = unpack(
            samples.get(y.saturating_mul(stride)..).unwrap_or_default(),
            w,
        );
        // TPGD (§6.2.5.7, encoder side per RAG §3.4).
        let sltp = (row == above) != ltp;
        mq.encode(SLTP_CONTEXT, sltp);
        ltp ^= sltp;
        if !ltp {
            code_row(&mut mq, &above2, &above, &row);
        }
        above2 = std::mem::replace(&mut above, row);
    }
    mq.finish()
}

/// JBIG2 pixels of one row: 1 is black, a PDF sample 0.
fn unpack(line: &[u8], width: usize) -> Vec<u8> {
    (0..width)
        .map(|x| {
            let byte = line.get(x / 8).copied().unwrap_or(0);
            1 - ((byte >> (7 - x % 8)) & 1)
        })
        .collect()
}

/// Codes every pixel of `row` with the template 0 context (Figure 3,
/// RAG §3.5): bits 15-11 are row -2 at x-2..=x+2 (A4, three fixed, A3),
/// bits 10-4 row -1 at x-3..=x+3 (A2, five fixed, A1), bits 3-0 row 0 at
/// x-4..=x-1. Pixels outside the bitmap are 0 (§6.2.5.2).
fn code_row(mq: &mut MqEncoder, above2: &[u8], above: &[u8], row: &[u8]) {
    let at = |r: &[u8], x: usize, plus: usize| -> usize {
        x.checked_add(plus)
            .and_then(|i| i.checked_sub(4))
            .and_then(|i| r.get(i))
            .map_or(0, |&p| usize::from(p))
    };
    // Windows primed for x = 0: what sits left of x+3 (row -2), x+4
    // (row -1) and x (row 0), offset by 4 so the arithmetic stays unsigned.
    let mut w2 = (0..4).fold(0, |w, i| (w << 1) | at(above2, 0, i + 2));
    let mut w1 = (0..6).fold(0, |w, i| (w << 1) | at(above, 0, i + 1));
    let mut w0 = 0;
    for (x, &pixel) in row.iter().enumerate() {
        w2 = ((w2 << 1) | at(above2, x, 6)) & 0x1F;
        w1 = ((w1 << 1) | at(above, x, 7)) & 0x7F;
        mq.encode((w2 << 11) | (w1 << 4) | w0, pixel == 1);
        w0 = ((w0 << 1) | usize::from(pixel)) & 0x0F;
    }
}
