//! Reading and writing the sfnt container: the table directory, table
//! checksums and `head.checkSumAdjustment` (OpenType 1.9.1, "The OpenType
//! font file").

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
        let base = if data.starts_with(b"ttcf") {
            usize::try_from(read_u32(data, 12)?).ok()?
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
