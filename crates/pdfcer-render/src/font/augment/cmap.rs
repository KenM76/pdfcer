//! `cmap` for decision 173 §4: add the appended glyphs to every Unicode
//! subtable — `(0,*)` except variation sequences, `(3,1)`, `(3,10)` — and to
//! `(1,0)` for a character Mac OS Roman encodes. Formats 0, 4, 6 and 12 are
//! rewritten; any other subtable is copied byte-identical.

use std::collections::BTreeMap;

use pdfcer_core::fontdata::{BaseEncoding, encoding_glyph_name, glyph_name_to_unicode};

use crate::font::sfnt::{format4, read_u16, read_u32};

use super::AugmentError;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Unicode,
    MacRoman,
    Other,
}

fn kind(platform: u16, encoding: u16, format: u16) -> Kind {
    match (platform, encoding) {
        (0, _) if format != 14 => Kind::Unicode,
        (3, 1 | 10) => Kind::Unicode,
        (1, 0) => Kind::MacRoman,
        _ => Kind::Other,
    }
}

/// The Mac OS Roman code for `ch`, read from PDF's `MacRomanEncoding`.
fn mac_roman(ch: char) -> Option<u32> {
    (1..=255u8).find_map(|c| {
        let name = encoding_glyph_name(BaseEncoding::MacRoman, c)?;
        (glyph_name_to_unicode(name) == Some(ch)).then_some(u32::from(c))
    })
}

/// `cmap` with each `(ch, gid)` added. A character above U+FFFF reaches only
/// format-12 subtables. Refuses when no `(3,1)` subtable exists, or a
/// Unicode subtable has a format this cannot rewrite.
pub(crate) fn add_entries(cmap: &[u8], adds: &[(char, u16)]) -> Result<Vec<u8>, AugmentError> {
    let malformed = || AugmentError::MalformedFace {
        detail: "the subset's cmap is truncated".into(),
    };
    let count = usize::from(read_u16(cmap, 2).ok_or_else(malformed)?);
    let mut records = Vec::with_capacity(count);
    for i in 0..count {
        let at = 4 + i * 8;
        let p = read_u16(cmap, at).ok_or_else(malformed)?;
        let e = read_u16(cmap, at + 2).ok_or_else(malformed)?;
        let off = usize::try_from(read_u32(cmap, at + 4).ok_or_else(malformed)?)
            .map_err(|_| malformed())?;
        records.push((p, e, off));
    }
    if !records.iter().any(|&(p, e, _)| (p, e) == (3, 1)) {
        return Err(AugmentError::MissingUnicodeCmap);
    }
    let mut offsets: Vec<usize> = records.iter().map(|r| r.2).collect();
    offsets.sort_unstable();
    offsets.dedup();
    let mut rewritten: BTreeMap<usize, Vec<u8>> = BTreeMap::new();
    for &off in &offsets {
        let sub = subtable(cmap, off).ok_or_else(malformed)?;
        let format = read_u16(sub, 0).ok_or_else(malformed)?;
        let kinds: Vec<Kind> = records
            .iter()
            .filter(|r| r.2 == off)
            .map(|r| kind(r.0, r.1, format))
            .collect();
        let k = if kinds.contains(&Kind::Unicode) {
            Kind::Unicode
        } else if kinds.contains(&Kind::MacRoman) {
            Kind::MacRoman
        } else {
            Kind::Other
        };
        rewritten.insert(off, rewrite_subtable(sub, format, k, adds)?);
    }
    Ok(assemble(&records, &rewritten))
}

/// The glyph `(3,10)` or `(3,1)` maps `ch` to (decision 173 I7), glyph 0
/// counting as unmapped.
pub(crate) fn unicode_glyph(cmap: &[u8], ch: char) -> Option<u16> {
    let count = usize::from(read_u16(cmap, 2)?);
    let mut found = None;
    for i in 0..count {
        let at = 4 + i * 8;
        let key = (read_u16(cmap, at)?, read_u16(cmap, at + 2)?);
        if key != (3, 10) && key != (3, 1) {
            continue;
        }
        let sub = subtable(cmap, usize::try_from(read_u32(cmap, at + 4)?).ok()?)?;
        if let Some(&g) = read_map(sub, read_u16(sub, 0)?)?.get(&u32::from(ch)) {
            found = Some(g);
            if key == (3, 10) {
                break;
            }
        }
    }
    found
}

/// Every mapping of the `(3,1)` subtable.
pub(crate) fn windows_unicode_map(cmap: &[u8]) -> Option<BTreeMap<u32, u16>> {
    let count = usize::from(read_u16(cmap, 2)?);
    let at = (0..count)
        .map(|i| 4 + i * 8)
        .find(|&at| read_u16(cmap, at) == Some(3) && read_u16(cmap, at + 2) == Some(1))?;
    let sub = subtable(cmap, usize::try_from(read_u32(cmap, at + 4)?).ok()?)?;
    read_map(sub, read_u16(sub, 0)?)
}

fn rewrite_subtable(
    sub: &[u8],
    format: u16,
    k: Kind,
    adds: &[(char, u16)],
) -> Result<Vec<u8>, AugmentError> {
    let codes: Vec<(u32, u16)> = match k {
        Kind::Other => return Ok(sub.to_vec()),
        Kind::Unicode => adds.iter().map(|&(c, g)| (u32::from(c), g)).collect(),
        Kind::MacRoman => adds
            .iter()
            .filter_map(|&(c, g)| Some((mac_roman(c)?, g)))
            .collect(),
    };
    let Some(mut map) = read_map(sub, format) else {
        return Err(AugmentError::UnsupportedCmapFormat { format });
    };
    let mut changed = false;
    for (code, gid) in codes {
        if format == 12 || code <= 0xFFFF {
            map.insert(code, gid);
            changed = true;
        }
    }
    if !changed {
        return Ok(sub.to_vec());
    }
    write_map(&map, format).ok_or(AugmentError::UnsupportedCmapFormat { format })
}

fn subtable(cmap: &[u8], off: usize) -> Option<&[u8]> {
    let format = read_u16(cmap, off)?;
    let len = match format {
        0 | 2 | 4 | 6 => usize::from(read_u16(cmap, off + 2)?),
        _ => usize::try_from(read_u32(cmap, off + 4)?).ok()?,
    };
    cmap.get(off..off.checked_add(len)?)
}

/// Every mapping of a format 0, 4, 6 or 12 subtable, glyph 0 omitted.
fn read_map(sub: &[u8], format: u16) -> Option<BTreeMap<u32, u16>> {
    let mut map = BTreeMap::new();
    match format {
        0 => {
            for c in 0..256usize {
                map.insert(u32::try_from(c).ok()?, u16::from(*sub.get(6 + c)?));
            }
        }
        4 => read_format4(sub, &mut map)?,
        6 => {
            let first = read_u16(sub, 6)?;
            for i in 0..read_u16(sub, 8)? {
                map.insert(
                    u32::from(first) + u32::from(i),
                    read_u16(sub, 10 + 2 * usize::from(i))?,
                );
            }
        }
        12 => {
            for i in 0..usize::try_from(read_u32(sub, 12)?).ok()? {
                let at = 16 + i * 12;
                let (start, end, g0) = (
                    read_u32(sub, at)?,
                    read_u32(sub, at + 4)?,
                    read_u32(sub, at + 8)?,
                );
                if end < start || end - start > 0x10FFFF {
                    return None;
                }
                for c in start..=end {
                    map.insert(c, u16::try_from(g0 + (c - start)).ok()?);
                }
            }
        }
        _ => return None,
    }
    map.retain(|_, g| *g != 0);
    Some(map)
}

fn read_format4(sub: &[u8], map: &mut BTreeMap<u32, u16>) -> Option<()> {
    let seg_x2 = usize::from(read_u16(sub, 6)?);
    let (ends, starts, deltas, ranges) = (14, 16 + seg_x2, 16 + 2 * seg_x2, 16 + 3 * seg_x2);
    for s in 0..seg_x2 / 2 {
        let (end, start) = (read_u16(sub, ends + 2 * s)?, read_u16(sub, starts + 2 * s)?);
        let delta = read_u16(sub, deltas + 2 * s)?;
        let range_at = ranges + 2 * s;
        let range = usize::from(read_u16(sub, range_at)?);
        for c in start..=end.max(start) {
            if c == 0xFFFF {
                break;
            }
            let g = if range == 0 {
                c.wrapping_add(delta)
            } else {
                let at = range_at + range + 2 * usize::from(c - start);
                match read_u16(sub, at)? {
                    0 => 0,
                    g => g.wrapping_add(delta),
                }
            };
            map.insert(u32::from(c), g);
        }
    }
    Some(())
}

/// `map` in `format`, widened to format 4 (or 12 above the BMP) when the
/// original format cannot hold it.
fn write_map(map: &BTreeMap<u32, u16>, format: u16) -> Option<Vec<u8>> {
    let bmp = map.keys().all(|&c| c <= 0xFFFF);
    if format == 0 && map.iter().all(|(&c, &g)| c < 256 && g < 256) {
        let mut out = vec![0, 0, 1, 6, 0, 0];
        let mut glyphs = [0u8; 256];
        for (&c, &g) in map {
            *glyphs.get_mut(usize::try_from(c).ok()?)? = u8::try_from(g).ok()?;
        }
        out.extend_from_slice(&glyphs);
        return Some(out);
    }
    if format == 12 || !bmp {
        return Some(format12(map));
    }
    let narrow: BTreeMap<u16, u16> = map
        .iter()
        .map(|(&c, &g)| (u16::try_from(c).unwrap_or(0xFFFF), g))
        .collect();
    format4(&narrow)
}

fn format12(map: &BTreeMap<u32, u16>) -> Vec<u8> {
    let mut groups: Vec<(u32, u32, u32)> = Vec::new();
    for (&c, &g) in map {
        match groups.last_mut() {
            Some((start, end, g0)) if *end + 1 == c && *g0 + (c - *start) == u32::from(g) => {
                *end = c
            }
            _ => groups.push((c, c, u32::from(g))),
        }
    }
    let len = 16 + 12 * groups.len();
    let mut out = Vec::with_capacity(len);
    out.extend_from_slice(&12u16.to_be_bytes());
    out.extend_from_slice(&0u16.to_be_bytes());
    for v in [len, 0, groups.len()] {
        out.extend_from_slice(&u32::try_from(v).unwrap_or(u32::MAX).to_be_bytes());
    }
    for (s, e, g) in groups {
        for v in [s, e, g] {
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
    out
}

/// The `cmap` table: the original records in their original order, each
/// pointing at its (possibly rewritten, still shared) subtable.
fn assemble(records: &[(u16, u16, usize)], subs: &BTreeMap<usize, Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&0u16.to_be_bytes());
    out.extend_from_slice(&u16::try_from(records.len()).unwrap_or(0).to_be_bytes());
    let mut at = 4 + 8 * records.len();
    let mut placed = BTreeMap::new();
    for (&old, sub) in subs {
        placed.insert(old, at);
        at += sub.len().next_multiple_of(4);
    }
    for &(p, e, old) in records {
        out.extend_from_slice(&p.to_be_bytes());
        out.extend_from_slice(&e.to_be_bytes());
        out.extend_from_slice(
            &u32::try_from(placed.get(&old).copied().unwrap_or(0))
                .unwrap_or(0)
                .to_be_bytes(),
        );
    }
    for sub in subs.values() {
        out.extend_from_slice(sub);
        out.resize(out.len().next_multiple_of(4), 0);
    }
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

    fn table(subs: &[((u16, u16), Vec<u8>)]) -> Vec<u8> {
        let records: Vec<(u16, u16, usize)> = subs
            .iter()
            .enumerate()
            .map(|(i, ((p, e), _))| (*p, *e, i))
            .collect();
        let map = subs
            .iter()
            .enumerate()
            .map(|(i, (_, s))| (i, s.clone()))
            .collect();
        assemble(&records, &map)
    }

    fn map_of(cmap: &[u8], idx: usize) -> BTreeMap<u32, u16> {
        let off = usize::try_from(read_u32(cmap, 4 + idx * 8 + 4).unwrap()).unwrap();
        let sub = subtable(cmap, off).unwrap();
        read_map(sub, read_u16(sub, 0).unwrap()).unwrap()
    }

    #[test]
    fn every_unicode_subtable_gains_the_entry_and_others_are_untouched() {
        let f4 = format4(&BTreeMap::from([(0x41, 1), (0x42, 2)])).unwrap();
        let sym = format4(&BTreeMap::from([(0xF041, 1)])).unwrap();
        let cmap = table(&[((0, 3), f4.clone()), ((3, 0), sym.clone()), ((3, 1), f4)]);
        let out = add_entries(&cmap, &[('\u{C9}', 7)]).unwrap();
        for idx in [0, 2] {
            assert_eq!(map_of(&out, idx).get(&0xC9), Some(&7));
            assert_eq!(map_of(&out, idx).get(&0x41), Some(&1));
        }
        assert_eq!(map_of(&out, 1), BTreeMap::from([(0xF041, 1)]));
    }

    #[test]
    fn mac_roman_takes_the_mac_code() {
        let f4 = format4(&BTreeMap::from([(0x41, 1)])).unwrap();
        let mut f0 = vec![0, 0, 1, 6, 0, 0];
        f0.extend([0u8; 256]);
        f0[6 + 0x41] = 1;
        let out = add_entries(&table(&[((1, 0), f0), ((3, 1), f4)]), &[('\u{C9}', 7)]).unwrap();
        assert_eq!(map_of(&out, 0).get(&0x83), Some(&7), "É is Mac Roman 0x83");
    }

    #[test]
    fn a_cmap_without_3_1_is_refused() {
        let f4 = format4(&BTreeMap::from([(0x41, 1)])).unwrap();
        assert!(matches!(
            add_entries(&table(&[((0, 3), f4)]), &[('B', 2)]),
            Err(AugmentError::MissingUnicodeCmap)
        ));
    }
}
