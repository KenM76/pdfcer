use crate::{RemuxError, interleave};
use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
use zune_jpeg::JpegDecoder;
use zune_jpeg::zune_core::bytestream::ZCursor;

const CEILING: usize = 1 << 20;

/// A smooth gradient with a hard diagonal edge, `w` by `h` RGB.
fn image(w: usize, h: usize) -> Vec<u8> {
    let mut px = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let edge = if x > y { 200 } else { 30 };
            px.extend([(x * 255 / w) as u8, (y * 255 / h) as u8, edge]);
        }
    }
    px
}

fn encode(
    w: usize,
    h: usize,
    color: ColorType,
    sampling: SamplingFactor,
    optimized: bool,
    restart: u16,
) -> Vec<u8> {
    let px = match color {
        ColorType::Luma => image(w, h).iter().step_by(3).copied().collect(),
        _ => image(w, h),
    };
    let mut out = Vec::new();
    let mut e = Encoder::new(&mut out, 85);
    e.set_sampling_factor(sampling);
    e.set_optimized_huffman_tables(optimized);
    if restart > 0 {
        e.set_restart_interval(restart);
    }
    e.encode(&px, w as u16, h as u16, color).unwrap();
    out
}

fn decode(data: &[u8]) -> Vec<u8> {
    JpegDecoder::new(ZCursor::new(data)).decode().unwrap()
}

fn scans(data: &[u8]) -> usize {
    data.windows(2).filter(|p| p == &[0xFF, 0xDA]).count()
}

/// The optimized and plain encodes share quantization and DCT, so their
/// coefficients are equal; a lossless remux must therefore decode to the
/// plain (already interleaved) encode's pixels exactly.
fn assert_lossless(w: usize, h: usize, sampling: SamplingFactor, restart: u16) {
    let opt = encode(w, h, ColorType::Rgb, sampling, true, restart);
    assert!(scans(&opt) > 1, "the encoder wrote one scan per component");
    let fixed = interleave(&opt, CEILING).unwrap().expect("rewritten");
    assert_eq!(scans(&fixed), 1);
    let plain = encode(w, h, ColorType::Rgb, sampling, false, restart);
    assert_eq!(decode(&fixed), decode(&plain), "{w}x{h}");
    assert_eq!(interleave(&fixed, CEILING), Ok(None), "idempotent");
}

#[test]
fn a_420_optimized_encode_decodes_exactly_after_the_rewrite() {
    for (w, h) in [(64, 48), (37, 23), (17, 9), (1, 1)] {
        assert_lossless(w, h, SamplingFactor::F_2_2, 0);
    }
}

#[test]
fn a_422_optimized_encode_decodes_exactly_after_the_rewrite() {
    assert_lossless(45, 30, SamplingFactor::F_2_1, 0);
}

/// `jpeg-encoder` cannot write restarts with optimized tables (its
/// optimizer trips a debug assertion), so this builds the non-interleaved
/// restart layout from the parsed coefficients: one scan per component,
/// `RSTm` every `interval` blocks, Annex K tables.
fn split_with_restarts(opt: &[u8], interval: usize) -> Vec<u8> {
    use crate::bits::BitWriter;
    use crate::huffman::{Encoder, annex_k};
    use crate::scan::{encode_block, push_segment};
    let p = crate::frame::read(opt, CEILING).unwrap().unwrap();
    let mut out = vec![0xFF, 0xD8];
    p.kept.iter().for_each(|s| out.extend_from_slice(s));
    let tables = annex_k();
    let body: Vec<u8> = tables
        .iter()
        .flat_map(|(c, d, s)| {
            std::iter::once(c << 4 | d)
                .chain(s.bits)
                .chain(s.vals.clone())
        })
        .collect();
    push_segment(&mut out, 0xC4, &body);
    push_segment(&mut out, 0xDD, &(interval as u16).to_be_bytes());
    let [dc_l, dc_c, ac_l, ac_c] = tables.map(|(_, _, s)| Encoder::new(&s));
    for (i, c) in p.comps.iter().enumerate() {
        let t = if i == 0 { 0x00 } else { 0x11 };
        push_segment(&mut out, 0xDA, &[1, c.id, t, 0, 63, 0]);
        let (dc, ac) = if i == 0 {
            (&dc_l, &ac_l)
        } else {
            (&dc_c, &ac_c)
        };
        let mut w = BitWriter::default();
        let mut pred = 0;
        for n in 0..c.own_w * c.own_h {
            if n > 0 && n % interval == 0 {
                out.extend(std::mem::take(&mut w).finish());
                out.extend([0xFF, 0xD0 + ((n / interval - 1) % 8) as u8]);
                pred = 0;
            }
            let at = (n / c.own_w) * c.grid_w + n % c.own_w;
            encode_block(&mut w, dc, ac, &mut pred, &c.blocks[at]).unwrap();
        }
        out.extend(w.finish());
    }
    out.extend([0xFF, 0xD9]);
    out
}

#[test]
fn restart_markers_in_the_input_are_honoured() {
    let opt = encode(70, 40, ColorType::Rgb, SamplingFactor::F_2_2, true, 0);
    let split = split_with_restarts(&opt, 3);
    assert_eq!(scans(&split), 3);
    let fixed = interleave(&split, CEILING).unwrap().expect("rewritten");
    let plain = encode(70, 40, ColorType::Rgb, SamplingFactor::F_2_2, false, 0);
    assert_eq!(decode(&fixed), decode(&plain));
}

#[test]
fn streams_that_need_no_rewrite_return_none() {
    let plain = encode(32, 32, ColorType::Rgb, SamplingFactor::F_2_2, false, 0);
    assert_eq!(interleave(&plain, CEILING), Ok(None), "already interleaved");
    let gray = encode(32, 32, ColorType::Luma, SamplingFactor::F_1_1, true, 0);
    assert_eq!(interleave(&gray, CEILING), Ok(None), "one component");
    let full = encode(32, 32, ColorType::Rgb, SamplingFactor::F_1_1, true, 0);
    assert_eq!(interleave(&full, CEILING), Ok(None), "no subsampling");
    assert_eq!(interleave(b"not a jpeg", CEILING), Ok(None));
}

#[test]
fn the_block_ceiling_is_enforced() {
    let opt = encode(64, 64, ColorType::Rgb, SamplingFactor::F_2_2, true, 0);
    assert_eq!(
        interleave(&opt, 10),
        Err(RemuxError::TooLarge {
            needed: 16 * 6,
            ceiling: 10
        })
    );
}

#[test]
fn damaged_input_never_panics() {
    let opt = split_with_restarts(
        &encode(40, 40, ColorType::Rgb, SamplingFactor::F_2_2, true, 0),
        2,
    );
    for cut in (0..opt.len()).step_by(7) {
        let _ = interleave(&opt[..cut], CEILING);
    }
    let mut flipped = opt.clone();
    for i in (20..flipped.len()).step_by(13) {
        flipped[i] ^= 0x5A;
        let _ = interleave(&flipped, CEILING);
    }
}
