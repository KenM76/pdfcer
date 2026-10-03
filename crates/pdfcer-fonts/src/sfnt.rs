//! Reading single tables out of an sfnt (TrueType / OpenType) program.
//!
//! Every read is bounds-checked against both the buffer and the table's
//! declared length; a malformed program yields `None`, never a panic.

use crate::fontinfo::MAX_SFNT_TABLES;

/// The bytes of table `tag` in `program`, or `None` when the program is not
/// a single-face sfnt, the directory is malformed, or the table is absent or
/// runs past the end of the buffer. A collection (`ttcf`) is `None`: which
/// face a PDF meant is not recorded.
#[must_use]
pub fn table<'a>(program: &'a [u8], tag: &[u8; 4]) -> Option<&'a [u8]> {
    match be32(program, 0)? {
        0x0001_0000 | 0x4F54_544F | 0x7472_7565 => {}
        _ => return None,
    }
    let count = usize::from(be16(program, 4)?);
    if count > MAX_SFNT_TABLES {
        return None;
    }
    (0..count).find_map(|i| {
        let rec = 12 + i * 16;
        if program.get(rec..rec + 4)? != tag.as_slice() {
            return None;
        }
        let offset = usize::try_from(be32(program, rec + 8)?).ok()?;
        let length = usize::try_from(be32(program, rec + 12)?).ok()?;
        program.get(offset..offset.checked_add(length)?)
    })
}

/// The decoration metrics a font program declares, in thousandths of an em,
/// each measured to the **centre** of its stroke (the tables give the top;
/// half the thickness is subtracted).
///
/// Sources (OpenType 1.9.1): `post.underlinePosition` @8 and
/// `underlineThickness` @10; `OS/2.yStrikeoutSize` @26 and
/// `yStrikeoutPosition` @28; scaled by `head.unitsPerEm` @18. A field whose
/// table is absent, or whose thickness is not positive, is `None`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
#[non_exhaustive]
pub struct LineMetrics {
    /// Underline centre, below the baseline when negative.
    pub underline_centre: Option<f64>,
    /// Underline thickness.
    pub underline_thickness: Option<f64>,
    /// Strikeout centre above the baseline.
    pub strike_centre: Option<f64>,
    /// Strikeout thickness.
    pub strike_thickness: Option<f64>,
}

/// Read [`LineMetrics`] from `program`; `None` when it has no usable
/// `head.unitsPerEm` (absent, or outside 16..=16384 as OpenType requires).
#[must_use]
pub fn line_metrics(program: &[u8]) -> Option<LineMetrics> {
    let upem = be16(table(program, b"head")?, 18)?;
    if !(16..=16384).contains(&upem) {
        return None;
    }
    let scale = 1000.0 / f64::from(upem);
    let pair = |t: &[u8], pos_at: usize, size_at: usize| -> Option<(f64, f64)> {
        let pos = f64::from(be16(t, pos_at)? as i16) * scale;
        let size = f64::from(be16(t, size_at)? as i16) * scale;
        (size > 0.0).then_some((pos - size / 2.0, size))
    };
    let underline = table(program, b"post").and_then(|t| pair(t, 8, 10));
    let strike = table(program, b"OS/2").and_then(|t| pair(t, 28, 26));
    Some(LineMetrics {
        underline_centre: underline.map(|u| u.0),
        underline_thickness: underline.map(|u| u.1),
        strike_centre: strike.map(|s| s.0),
        strike_thickness: strike.map(|s| s.1),
    })
}

fn be16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        d.get(at..at.checked_add(2)?)?.try_into().ok()?,
    ))
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        d.get(at..at.checked_add(4)?)?.try_into().ok()?,
    ))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    /// A program with `head` (unitsPerEm 2048), `post` (underline top −150,
    /// thickness 100) and `OS/2` (strikeout top 600, size 100).
    fn program(with_os2: bool) -> Vec<u8> {
        let mut head = vec![0u8; 54];
        head[18..20].copy_from_slice(&2048u16.to_be_bytes());
        let mut post = vec![0u8; 32];
        post[8..10].copy_from_slice(&(-150i16).to_be_bytes());
        post[10..12].copy_from_slice(&100i16.to_be_bytes());
        let mut os2 = vec![0u8; 78];
        os2[26..28].copy_from_slice(&100i16.to_be_bytes());
        os2[28..30].copy_from_slice(&600i16.to_be_bytes());
        let mut tables: Vec<(&[u8; 4], Vec<u8>)> = vec![(b"head", head), (b"post", post)];
        if with_os2 {
            tables.push((b"OS/2", os2));
        }
        let mut out = 0x0001_0000u32.to_be_bytes().to_vec();
        out.extend_from_slice(&u16::try_from(tables.len()).unwrap().to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        let mut offset = 12 + 16 * tables.len();
        let mut body = Vec::new();
        for (tag, data) in &tables {
            out.extend_from_slice(*tag);
            out.extend_from_slice(&[0; 4]);
            out.extend_from_slice(&u32::try_from(offset).unwrap().to_be_bytes());
            out.extend_from_slice(&u32::try_from(data.len()).unwrap().to_be_bytes());
            offset += data.len();
            body.extend_from_slice(data);
        }
        out.extend_from_slice(&body);
        out
    }

    #[test]
    fn the_tables_give_centres_in_thousandths_of_an_em() {
        let m = line_metrics(&program(true)).unwrap();
        let k = 1000.0 / 2048.0;
        let close = |a: Option<f64>, b: f64| (a.unwrap() - b).abs() < 1e-9;
        assert!(close(m.underline_centre, -200.0 * k));
        assert!(close(m.underline_thickness, 100.0 * k));
        assert!(close(m.strike_centre, 550.0 * k));
        assert!(close(m.strike_thickness, 100.0 * k));
    }

    #[test]
    fn a_missing_table_leaves_its_fields_empty() {
        let m = line_metrics(&program(false)).unwrap();
        assert!(m.underline_centre.is_some());
        assert_eq!(m.strike_centre, None);
    }

    #[test]
    fn a_truncated_directory_reads_nothing() {
        let p = program(true);
        for len in [0, 4, 11, 20, 40] {
            assert_eq!(table(&p[..len], b"head"), None, "{len}");
        }
        assert_eq!(line_metrics(b"ttcf\0\0\0\0"), None);
    }
}
