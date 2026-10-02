//! Reading and writing the sfnt container: the table directory, table
//! checksums and `head.checkSumAdjustment` (OpenType 1.9.1, "The OpenType
//! font file").

use std::collections::BTreeMap;

/// An sfnt table directory over borrowed bytes.
pub(crate) struct Directory<'a> {
    pub(crate) flavor: u32,
    pub(crate) tables: Vec<([u8; 4], &'a [u8])>,
}

impl<'a> Directory<'a> {
    /// A plain sfnt, or face 0 of a collection (`ttcf`). A collection's
    /// table offsets count from the start of the whole file, so face 0's
    /// tables are sliced from `data` as they are for a plain sfnt; face 0 is
    /// the face the renderer draws with.
    pub(crate) fn parse(data: &'a [u8]) -> Option<Self> {
        Self::parse_face(data, 0)
    }

    /// Face `index` of a collection, or the sfnt itself when `data` is not a
    /// collection (any `index` then reads the one face).
    pub(crate) fn parse_face(data: &'a [u8], index: u32) -> Option<Self> {
        let base = if data.starts_with(b"ttcf") {
            if index >= read_u32(data, 8)? {
                return None;
            }
            let at = 12 + usize::try_from(index).ok()? * 4;
            usize::try_from(read_u32(data, at)?).ok()?
        } else {
            0
        };
        let flavor = read_u32(data, base)?;
        if !matches!(flavor, 0x0001_0000 | 0x7472_7565 | 0x4F54_544F) {
            return None;
        }
        let count = usize::from(read_u16(data, base + 4)?);
        let mut tables = Vec::with_capacity(count);
        for i in 0..count {
            let rec = base + 12 + i * 16;
            let tag: [u8; 4] = data.get(rec..rec + 4)?.try_into().ok()?;
            let offset = usize::try_from(read_u32(data, rec + 8)?).ok()?;
            let length = usize::try_from(read_u32(data, rec + 12)?).ok()?;
            tables.push((tag, data.get(offset..offset.checked_add(length)?)?));
        }
        Some(Self { flavor, tables })
    }

    /// The bytes of table `tag`, if present.
    pub(crate) fn table(&self, tag: [u8; 4]) -> Option<&'a [u8]> {
        self.tables.iter().find(|(t, _)| *t == tag).map(|(_, d)| *d)
    }
}

/// Overwrite the two bytes at `at` with `v`; out of bounds writes nothing.
pub(crate) fn put(out: &mut [u8], at: usize, v: [u8; 2]) {
    if let Some(d) = out.get_mut(at..at.saturating_add(2)) {
        d.copy_from_slice(&v);
    }
}

/// The big-endian `u16` at `at`, if in bounds.
pub(crate) fn read_u16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

/// The big-endian `i16` at `at`; 0 when out of bounds.
pub(crate) fn read_i16(d: &[u8], at: usize) -> i16 {
    read_u16(d, at).map_or(0, |v| v as i16)
}

/// The big-endian `u32` at `at`, if in bounds.
pub(crate) fn read_u32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// The OpenType table checksum: the sum of big-endian `u32`s, the table
/// zero-padded to a multiple of four.
pub(crate) fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut w = [0u8; 4];
        for (d, s) in w.iter_mut().zip(c) {
            *d = *s;
        }
        sum.wrapping_add(u32::from_be_bytes(w))
    })
}

/// Write the sfnt: header, directory sorted by tag, each table 4-aligned,
/// and `head.checkSumAdjustment` = 0xB1B0AFBA − the whole file's checksum.
pub(crate) fn assemble(flavor: u32, mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
    tables.sort_by_key(|(tag, _)| *tag);
    let count = u16::try_from(tables.len()).unwrap_or(u16::MAX);
    let mut entry_selector = 0u16;
    while (2u16 << entry_selector) <= count {
        entry_selector += 1;
    }
    let search_range = (1u16 << entry_selector) * 16;
    let range_shift = count * 16 - search_range;

    let mut out = Vec::new();
    out.extend_from_slice(&flavor.to_be_bytes());
    for v in [count, search_range, entry_selector, range_shift] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    let mut offset = 12 + tables.len() * 16;
    let mut head_at = None;
    for (tag, data) in &mut tables {
        if tag == b"head"
            && let Some(adjust) = data.get_mut(8..12)
        {
            adjust.fill(0);
            head_at = Some(offset);
        }
        out.extend_from_slice(tag);
        out.extend_from_slice(&checksum(data).to_be_bytes());
        out.extend_from_slice(&u32::try_from(offset).unwrap_or(0).to_be_bytes());
        out.extend_from_slice(&u32::try_from(data.len()).unwrap_or(0).to_be_bytes());
        offset += data.len().next_multiple_of(4);
    }
    for (_, data) in &tables {
        out.extend_from_slice(data);
        out.resize(out.len().next_multiple_of(4), 0);
    }
    if let Some(at) = head_at {
        let adjust = 0xB1B0_AFBAu32.wrapping_sub(checksum(&out));
        if let Some(d) = out.get_mut(at + 8..at + 12) {
            d.copy_from_slice(&adjust.to_be_bytes());
        }
    }
    out
}

/// A format-4 `cmap` subtable for `map` (OpenType `cmap` format 4).
///
/// Segments group characters that are consecutive AND map to consecutive
/// glyphs, so each needs only an `idDelta`; the mandatory final segment maps
/// 0xFFFF to glyph 0. `None` when the subtable would exceed 64 KiB.
pub(crate) fn format4(map: &BTreeMap<u16, u16>) -> Option<Vec<u8>> {
    let mut segs: Vec<(u16, u16, u16)> = Vec::new(); // (start, end, first gid)
    for (&c, &g) in map {
        if c == 0xFFFF {
            continue;
        }
        match segs.last_mut() {
            Some((start, end, g0))
                if u32::from(*end) + 1 == u32::from(c)
                    && u32::from(*g0) + u32::from(c - *start) == u32::from(g) =>
            {
                *end = c;
            }
            _ => segs.push((c, c, g)),
        }
    }
    segs.push((0xFFFF, 0xFFFF, 0));
    let seg_count = u16::try_from(segs.len()).ok()?;
    let mut entry_selector = 0u16;
    while (2u32 << entry_selector) <= u32::from(seg_count) {
        entry_selector += 1;
    }
    let search_range = 2 * (1u16 << entry_selector);
    let seg_x2 = seg_count * 2;
    let range_shift = seg_x2 - search_range;
    let length = 16 + 8 * usize::from(seg_count);
    let length = u16::try_from(length).ok()?;

    let mut sub = Vec::with_capacity(usize::from(length));
    for v in [
        4,
        length,
        0,
        seg_x2,
        search_range,
        entry_selector,
        range_shift,
    ] {
        sub.extend_from_slice(&v.to_be_bytes());
    }
    for (_, end, _) in &segs {
        sub.extend_from_slice(&end.to_be_bytes());
    }
    sub.extend_from_slice(&0u16.to_be_bytes()); // reservedPad
    for (start, _, _) in &segs {
        sub.extend_from_slice(&start.to_be_bytes());
    }
    for (start, _, g0) in &segs {
        // The final segment's delta is 1: 0xFFFF + 1 wraps to glyph 0.
        let delta = if *start == 0xFFFF {
            1
        } else {
            g0.wrapping_sub(*start)
        };
        sub.extend_from_slice(&delta.to_be_bytes());
    }
    for _ in &segs {
        sub.extend_from_slice(&0u16.to_be_bytes()); // idRangeOffset
    }
    Some(sub)
}
