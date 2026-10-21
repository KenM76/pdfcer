//! Estimating the IJG quality a JPEG codestream was written at, from its
//! luminance quantisation table (ITU-T T.81 §B.2.4.1 `DQT`, Annex K
//! Table K.1, and the IJG `jcparam.c` quality scaling).

/// ITU-T T.81 Table K.1, the luminance table, in the zig-zag order a `DQT`
/// segment stores it (T.81 Figure A.6).
const K1_ZIGZAG: [u16; 64] = [
    16, 11, 12, 14, 12, 10, 16, 14, 13, 14, 18, 17, 16, 19, 24, 40, 26, 24, 22, 22, 24, 49, 35, 37,
    29, 40, 58, 51, 61, 60, 57, 51, 56, 55, 64, 72, 92, 78, 64, 68, 87, 69, 55, 56, 80, 109, 81,
    87, 95, 98, 103, 104, 103, 62, 77, 113, 121, 112, 100, 120, 92, 101, 103, 99,
];

/// The most a table may differ, summed over its 64 entries, from the IJG
/// table at the nearest quality and still be read as that quality. Covers
/// encoders that round the IJG scaling differently.
const TOLERANCE: u32 = 64;

/// The IJG quality, 1 to 100, whose scaled Table K.1 is closest to the
/// table quantising the codestream's first frame component (its `Tqi` in
/// the frame header, T.81 §B.2.2); `None` when there is no such table
/// before the first scan or it is not an IJG scaling of K.1.
///
/// Both of IJG's clamps are tried: 1..=255 (`force_baseline`) and
/// 1..=32767 (16-bit tables, written below about quality 24). Reads only
/// marker segments before the first `SOS`, and never past `codestream`.
///
/// ```
/// use pdfcer_core::optimize::estimate_jpeg_quality;
///
/// assert_eq!(estimate_jpeg_quality(b"not a jpeg"), None);
/// ```
#[must_use]
pub fn estimate_jpeg_quality(codestream: &[u8]) -> Option<u8> {
    let table = luma_table(codestream)?;
    let distance = |reference: [u16; 64]| -> u32 {
        reference
            .iter()
            .zip(&table)
            .map(|(a, b)| u32::from(a.abs_diff(*b)))
            .sum()
    };
    let (quality, error) = (1..=100u8)
        .map(|q| {
            (
                q,
                distance(ijg_table(q, 255)).min(distance(ijg_table(q, 32767))),
            )
        })
        .min_by_key(|(q, error)| (*error, u8::MAX - q))?;
    (error <= TOLERANCE).then_some(quality)
}

/// Table K.1 scaled to `quality` as IJG's `jpeg_quality_scaling` and
/// `jpeg_add_quant_table` do, clamped to `1..=max`.
fn ijg_table(quality: u8, max: u32) -> [u16; 64] {
    let q = u32::from(quality.clamp(1, 100));
    let scale = if q < 50 { 5000 / q } else { 200 - 2 * q };
    K1_ZIGZAG.map(|base| {
        let v = (u32::from(base) * scale + 50) / 100;
        u16::try_from(v.clamp(1, max)).unwrap_or(u16::MAX)
    })
}

/// The table in force for the first frame component at the first `SOS`.
fn luma_table(data: &[u8]) -> Option<[u16; 64]> {
    if data.get(..2) != Some(&[0xFF, 0xD8]) {
        return None;
    }
    let mut tables: [Option<[u16; 64]>; 4] = [None; 4];
    let mut luma = 0usize;
    let mut at = 2usize;
    loop {
        // Fill bytes: any number of 0xFF may precede a marker (T.81 B.1.1.2).
        while data.get(at) == Some(&0xFF) && data.get(at + 1) == Some(&0xFF) {
            at += 1;
        }
        if *data.get(at)? != 0xFF {
            return None;
        }
        let marker = *data.get(at + 1)?;
        at += 2;
        match marker {
            // TEM and RST0-7 carry no length.
            0x01 | 0xD0..=0xD7 => continue,
            0xD9 => return None,
            0xDA => return tables.get(luma).copied().flatten(),
            _ => {}
        }
        let len = usize::from(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]));
        let segment = data.get(at + 2..at.checked_add(len)?)?;
        match marker {
            0xDB => read_dqt(segment, &mut tables)?,
            // SOF0-15 except DHT (C4), JPG (C8) and DAC (CC): Table B.1.
            0xC0..=0xCF if !matches!(marker, 0xC4 | 0xC8 | 0xCC) => {
                // P, Y, X, Nf, then C1, H1V1, Tq1 (Table B.2).
                luma = usize::from(*segment.get(8)? & 0x0F);
            }
            _ => {}
        }
        at += len;
    }
}

/// Every table in one `DQT` segment's payload (after `Lq`), by `Tq`.
fn read_dqt(mut payload: &[u8], tables: &mut [Option<[u16; 64]>; 4]) -> Option<()> {
    while let Some((&pq_tq, rest)) = payload.split_first() {
        let wide = pq_tq >> 4 != 0;
        let size = if wide { 128 } else { 64 };
        let values = rest.get(..size)?;
        let mut table = [0u16; 64];
        for (k, slot) in table.iter_mut().enumerate() {
            *slot = if wide {
                u16::from_be_bytes([*values.get(2 * k)?, *values.get(2 * k + 1)?])
            } else {
                u16::from(*values.get(k)?)
            };
        }
        *tables.get_mut(usize::from(pq_tq & 0x0F))? = Some(table);
        payload = rest.get(size..)?;
    }
    Some(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use crate::jpeg_write::{Chroma, JpegOptions, encode};

    fn jpeg(quality: u8, components: u8, progressive: bool) -> Vec<u8> {
        let n = 16 * 16 * usize::from(components);
        let samples: Vec<u8> = (0..n).map(|i| (i * 37 % 251) as u8).collect();
        let opts = JpegOptions {
            quality,
            chroma: Chroma::Auto,
            progressive,
            optimize_huffman: true,
        };
        encode(&samples, 16, 16, components, opts).unwrap()
    }

    #[test]
    fn pdfcer_written_jpegs_read_back_at_their_quality() {
        for q in [1, 10, 20, 40, 49, 50, 51, 60, 75, 80, 90, 95, 100] {
            for (c, p) in [(1, false), (3, false), (3, true), (4, false)] {
                assert_eq!(
                    estimate_jpeg_quality(&jpeg(q, c, p)),
                    Some(q),
                    "q {q} c {c}"
                );
            }
        }
    }

    /// A hand-built codestream: SOI, a DQT holding an 8-bit table 0 of 99s
    /// and a 16-bit table 1 equal to K.1 (quality 50), a frame header whose
    /// first component uses table 1, then SOS.
    fn hand_built(first_component_tq: u8) -> Vec<u8> {
        let mut d = vec![0xFF, 0xD8, 0xFF, 0xFF, 0xDB];
        let len = 2 + (1 + 64) + (1 + 128);
        d.extend_from_slice(&u16::try_from(len).unwrap().to_be_bytes());
        d.push(0x00);
        d.extend_from_slice(&[99; 64]);
        d.push(0x11);
        for v in K1_ZIGZAG {
            d.extend_from_slice(&v.to_be_bytes());
        }
        d.extend_from_slice(&[0xFF, 0xC1, 0, 11, 8, 0, 16, 0, 16, 1, 1, 0x11]);
        d.push(first_component_tq);
        d.extend_from_slice(&[0xFF, 0xDA, 0, 2]);
        d
    }

    #[test]
    fn the_first_components_table_is_read_and_16_bit_precision_accepted() {
        assert_eq!(estimate_jpeg_quality(&hand_built(1)), Some(50));
        // Table 0, all 99s, is no IJG scaling of K.1.
        assert_eq!(estimate_jpeg_quality(&hand_built(0)), None);
    }

    #[test]
    fn a_non_baseline_table_below_quality_24_reads_back() {
        let mut d = vec![0xFF, 0xD8, 0xFF, 0xDB, 0, 2 + 129, 0x10];
        for v in ijg_table(5, 32767) {
            d.extend_from_slice(&v.to_be_bytes());
        }
        d.extend_from_slice(&[0xFF, 0xDA, 0, 2]);
        assert_eq!(estimate_jpeg_quality(&d), Some(5));
    }

    #[test]
    fn truncated_foreign_and_tableless_codestreams_give_none() {
        let good = jpeg(75, 1, false);
        for cut in [0, 1, 2, 3, 4, 10, 20, 40] {
            assert_eq!(estimate_jpeg_quality(&good[..cut]), None, "cut {cut}");
        }
        // A table no IJG quality produces: every entry 7 except the DC.
        let mut d = vec![0xFF, 0xD8, 0xFF, 0xDB, 0, 67, 0x00, 200];
        d.extend_from_slice(&[7; 63]);
        d.extend_from_slice(&[0xFF, 0xDA, 0, 2]);
        assert_eq!(estimate_jpeg_quality(&d), None);
        // A length running past the end.
        assert_eq!(
            estimate_jpeg_quality(&[0xFF, 0xD8, 0xFF, 0xDB, 0xFF, 0xFF]),
            None
        );
        // SOS before any table.
        assert_eq!(estimate_jpeg_quality(&[0xFF, 0xD8, 0xFF, 0xDA, 0, 2]), None);
    }
}
