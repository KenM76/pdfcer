//! `hmtx`, `hhea`, `maxp` and `head` for decision 173 §4: the subset's
//! metrics, extended by the appended glyphs, with the derived maxima and
//! bounding box recomputed.

use crate::font::sfnt::{put, read_i16, read_u16};

use super::AugmentError;
use super::glyf::{components, instruction_len, record};

/// Per-glyph horizontal metrics: advance and left side bearing.
pub(crate) type HMetric = (u16, i16);

/// Every glyph's metric from `hmtx` (a trailing run of bare side bearings
/// takes the last advance, per OpenType `hmtx`).
pub(crate) fn read_hmtx(hmtx: &[u8], long_count: usize, num_glyphs: usize) -> Option<Vec<HMetric>> {
    let mut out = Vec::with_capacity(num_glyphs);
    let mut last = 0;
    for gid in 0..num_glyphs {
        if gid < long_count {
            last = read_u16(hmtx, gid * 4)?;
            out.push((last, read_i16(hmtx, gid * 4 + 2)));
        } else {
            let at = long_count * 4 + (gid - long_count) * 2;
            read_u16(hmtx, at)?;
            out.push((last, read_i16(hmtx, at)));
        }
    }
    Some(out)
}

/// `hmtx` with one full entry per glyph (`numberOfHMetrics` = `numGlyphs`).
pub(crate) fn write_hmtx(metrics: &[HMetric]) -> Vec<u8> {
    metrics
        .iter()
        .flat_map(|&(aw, lsb)| aw.to_be_bytes().into_iter().chain(lsb.to_be_bytes()))
        .collect()
}

/// A record's bounding box `(xMin, yMin, xMax, yMax)`; `None` when empty.
pub(crate) fn bbox(rec: &[u8]) -> Option<[i16; 4]> {
    (rec.len() >= 10).then(|| {
        [
            read_i16(rec, 2),
            read_i16(rec, 4),
            read_i16(rec, 6),
            read_i16(rec, 8),
        ]
    })
}

/// `hhea` with `numberOfHMetrics`, `advanceWidthMax`, the side-bearing minima
/// and `xMaxExtent` recomputed over every glyph (OpenType `hhea`: the
/// bearing and extent fields consider only glyphs with contours).
pub(crate) fn write_hhea(old: &[u8], metrics: &[HMetric], boxes: &[Option<[i16; 4]>]) -> Vec<u8> {
    let mut out = old.to_vec();
    let aw_max = metrics.iter().map(|m| m.0).max().unwrap_or(0);
    let (mut min_lsb, mut min_rsb, mut extent) = (i32::MAX, i32::MAX, i32::MIN);
    for (&(aw, lsb), b) in metrics.iter().zip(boxes) {
        let Some([x_min, _, x_max, _]) = *b else {
            continue;
        };
        let width = i32::from(x_max) - i32::from(x_min);
        min_lsb = min_lsb.min(i32::from(lsb));
        min_rsb = min_rsb.min(i32::from(aw) - i32::from(lsb) - width);
        extent = extent.max(i32::from(lsb) + width);
    }
    let clamp =
        |v: i32| i16::try_from(v.clamp(i32::from(i16::MIN), i32::from(i16::MAX))).unwrap_or(0);
    put(&mut out, 10, aw_max.to_be_bytes());
    if extent != i32::MIN {
        put(&mut out, 12, clamp(min_lsb).to_be_bytes());
        put(&mut out, 14, clamp(min_rsb).to_be_bytes());
        put(&mut out, 16, clamp(extent).to_be_bytes());
    }
    let n = u16::try_from(metrics.len()).unwrap_or(u16::MAX);
    put(&mut out, 34, n.to_be_bytes());
    out
}

/// The `maxp` limits one glyph contributes, composites flattened.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct GlyphLimits {
    pub(crate) points: u16,
    pub(crate) contours: u16,
    pub(crate) composite: bool,
    pub(crate) elements: u16,
    pub(crate) depth: u16,
    pub(crate) instructions: u16,
}

/// `gid`'s limits in `glyf`/`loca`, components resolved recursively
/// (`closure` has already refused cycles and excess depth).
pub(crate) fn limits(glyf: &[u8], loca: &[usize], gid: u16) -> Result<GlyphLimits, AugmentError> {
    let rec = record(glyf, loca, usize::from(gid)).unwrap_or(&[]);
    let instructions = u16::try_from(instruction_len(rec)).unwrap_or(u16::MAX);
    match components(rec)? {
        None if rec.is_empty() => Ok(GlyphLimits::default()),
        None => {
            let contours = u16::try_from(read_i16(rec, 0)).unwrap_or(0);
            let points = match contours {
                0 => 0,
                n => read_u16(rec, 10 + 2 * usize::from(n) - 2).map_or(0, |e| e.saturating_add(1)),
            };
            Ok(GlyphLimits {
                points,
                contours,
                instructions,
                ..GlyphLimits::default()
            })
        }
        Some((parts, _)) => {
            let mut l = GlyphLimits {
                composite: true,
                elements: u16::try_from(parts.len()).unwrap_or(u16::MAX),
                depth: 1,
                instructions,
                ..GlyphLimits::default()
            };
            for c in parts {
                let sub = limits(glyf, loca, c.gid)?;
                l.points = l.points.saturating_add(sub.points);
                l.contours = l.contours.saturating_add(sub.contours);
                l.elements = l.elements.max(sub.elements);
                l.depth = l
                    .depth
                    .max(sub.depth.saturating_add(u16::from(sub.composite)));
            }
            Ok(l)
        }
    }
}

/// `maxp` with `numGlyphs` set and, for version 1.0, each glyph maximum
/// taking `max` with `added`; with `face_maxp`, the instruction-program
/// maxima take `max(subset, face)` (instructions copied, §4).
pub(crate) fn write_maxp(
    old: &[u8],
    num_glyphs: u16,
    added: &[GlyphLimits],
    face_maxp: Option<&[u8]>,
) -> Vec<u8> {
    let mut out = old.to_vec();
    let raise = |out: &mut Vec<u8>, at: usize, v: u16| {
        if let Some(cur) = read_u16(out, at)
            && v > cur
        {
            put(out, at, v.to_be_bytes());
        }
    };
    if let Some(d) = out.get_mut(4..6) {
        d.copy_from_slice(&num_glyphs.to_be_bytes());
    }
    if out.len() < 32 {
        return out;
    }
    for l in added {
        if l.composite {
            raise(&mut out, 10, l.points);
            raise(&mut out, 12, l.contours);
            raise(&mut out, 28, l.elements);
            raise(&mut out, 30, l.depth);
        } else {
            raise(&mut out, 6, l.points);
            raise(&mut out, 8, l.contours);
        }
    }
    if let Some(face) = face_maxp.filter(|f| f.len() >= 32) {
        for at in [14, 16, 18, 20, 22, 24, 26] {
            raise(&mut out, at, read_u16(face, at).unwrap_or(0));
        }
        for l in added {
            raise(&mut out, 26, l.instructions);
        }
    }
    out
}

/// `head` with the bounding box unioned with `boxes` and `indexToLocFormat`
/// set; `modified` and `fontRevision` untouched (deterministic output).
pub(crate) fn write_head(old: &[u8], boxes: &[Option<[i16; 4]>], long_loca: bool) -> Vec<u8> {
    let mut out = old.to_vec();
    if out.len() < 54 {
        return out;
    }
    let mut b = [
        read_i16(&out, 36),
        read_i16(&out, 38),
        read_i16(&out, 40),
        read_i16(&out, 42),
    ];
    for nb in boxes.iter().flatten() {
        b = [
            b[0].min(nb[0]),
            b[1].min(nb[1]),
            b[2].max(nb[2]),
            b[3].max(nb[3]),
        ];
    }
    for (i, v) in b.iter().enumerate() {
        put(&mut out, 36 + 2 * i, v.to_be_bytes());
    }
    put(&mut out, 50, i16::from(long_loca).to_be_bytes());
    out
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

    #[test]
    fn a_trailing_bearing_run_is_expanded_with_the_last_advance() {
        let hmtx = [0, 100, 0, 5, 0, 200, 0, 6, 0, 7, 0, 8];
        let m = read_hmtx(&hmtx, 2, 4).unwrap();
        assert_eq!(m, [(100, 5), (200, 6), (200, 7), (200, 8)]);
        assert_eq!(write_hmtx(&m).len(), 16);
    }

    #[test]
    fn hhea_extremes_ignore_empty_glyphs() {
        let hhea = vec![0u8; 36];
        let out = write_hhea(
            &hhea,
            &[(500, 0), (600, 10)],
            &[None, Some([10, 0, 500, 700])],
        );
        assert_eq!(read_u16(&out, 10), Some(600));
        assert_eq!(read_i16(&out, 12), 10);
        assert_eq!(read_i16(&out, 14), 100);
        assert_eq!(read_i16(&out, 16), 500);
        assert_eq!(read_u16(&out, 34), Some(2));
    }

    #[test]
    fn maxp_only_rises() {
        let mut maxp = vec![0u8; 32];
        maxp[6..8].copy_from_slice(&50u16.to_be_bytes());
        let small = GlyphLimits {
            points: 10,
            contours: 1,
            ..GlyphLimits::default()
        };
        let out = write_maxp(&maxp, 9, &[small], None);
        assert_eq!(read_u16(&out, 4), Some(9));
        assert_eq!(read_u16(&out, 6), Some(50));
        assert_eq!(read_u16(&out, 8), Some(1));
    }
}
