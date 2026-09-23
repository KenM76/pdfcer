//! A web-loadable subset of an embedded sfnt program, for SVG `<text>`
//! export (G033).
//!
//! A font embedded in a PDF is built for a PDF reader. It is often
//! subsetted, often has no `cmap` (a PDF maps codes to glyphs itself), and
//! may lack `OS/2`, `name` or `post`. A browser loads an `@font-face` only
//! after its sanitiser (OTS) accepts the file, and OTS rejects a TrueType
//! or CFF-flavoured sfnt missing any of `cmap`, `head`, `hhea`, `hmtx`,
//! `maxp`, `name`, `OS/2` or `post`.
//!
//! So [`build`]:
//!
//! 1. subsets the program to the glyphs actually shown (`subsetter`, which
//!    keeps `glyf`/`loca` or `CFF `, `head`, `hhea`, `hmtx`, `maxp`,
//!    `name`, `post` and the hinting tables, and drops `cmap` and `OS/2`);
//! 2. writes a fresh `cmap` from the Unicode → glyph mapping the export
//!    observed (format 4, under both (0,3) and (3,1));
//! 3. carries the source `OS/2` with its first/last character index
//!    patched, or synthesises a version-4 one from `head`/`hhea`;
//! 4. adds a minimal `name` when the subset has none, and rewrites `post`
//!    as version 3;
//! 5. rewrites the table directory, table checksums and
//!    `head.checkSumAdjustment`.
//!
//! A font whose `OS/2.fsType` sets the restricted-licence bit (0x0002) is
//! refused: re-embedding it in another file is exactly what that bit
//! forbids. Only BMP characters are mapped, which is all format 4 can carry;
//! the caller already refuses anything else.

use std::collections::BTreeMap;

/// Why a font could not be turned into a web font.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WebFontError {
    /// Not an sfnt, or its table directory is unreadable.
    NotSfnt,
    /// `OS/2.fsType` forbids embedding (restricted licence).
    Restricted,
    /// `subsetter` refused the program.
    Subset,
    /// `head` or `hhea` missing or too short.
    MissingMetrics,
    /// More glyphs or characters than the formats written can carry.
    TooLarge,
}

/// A built web font.
#[derive(Debug, Clone)]
pub(crate) struct WebFont {
    /// The sfnt bytes.
    pub data: Vec<u8>,
    /// `true` for a CFF-flavoured (`OTTO`) font, `false` for TrueType.
    pub cff: bool,
}

/// Build a web font holding the glyphs in `chars` (character → glyph id in
/// the source program), named `family` if the program carries no `name`.
pub(crate) fn build(
    program: &[u8],
    chars: &BTreeMap<char, u16>,
    family: &str,
) -> Result<WebFont, WebFontError> {
    let source = Directory::parse(program).ok_or(WebFontError::NotSfnt)?;
    let source_os2 = source.table(*b"OS/2");
    if let Some(os2) = source_os2
        && let Some(fs_type) = read_u16(os2, 8)
        && fs_type & 0x000F == 0x0002
    {
        return Err(WebFontError::Restricted);
    }

    let mut mapper = subsetter::GlyphRemapper::new();
    let mut new_map = BTreeMap::new();
    for (&c, &gid) in chars {
        let c16 = u16::try_from(u32::from(c)).map_err(|_| WebFontError::TooLarge)?;
        new_map.insert(c16, mapper.remap(gid));
    }
    let subset = subsetter::subset(program, 0, &mapper).map_err(|_| WebFontError::Subset)?;
    let dir = Directory::parse(&subset).ok_or(WebFontError::Subset)?;

    let head = dir.table(*b"head").filter(|h| h.len() >= 54);
    let hhea = dir.table(*b"hhea").filter(|h| h.len() >= 36);
    let (Some(head), Some(hhea)) = (head, hhea) else {
        return Err(WebFontError::MissingMetrics);
    };

    let mut tables: Vec<([u8; 4], Vec<u8>)> = dir
        .tables
        .iter()
        .filter(|(tag, _)| !matches!(tag, b"cmap" | b"OS/2" | b"post"))
        .map(|(tag, data)| (*tag, data.to_vec()))
        .collect();
    tables.push((*b"cmap", cmap(&new_map)?));
    let first = new_map.keys().next().copied().unwrap_or(0x20);
    let last = new_map.keys().next_back().copied().unwrap_or(0x20);
    let os2 = match source_os2.filter(|t| t.len() >= 78) {
        Some(src) => {
            let mut t = src.to_vec();
            t[64..66].copy_from_slice(&first.to_be_bytes());
            t[66..68].copy_from_slice(&last.to_be_bytes());
            t
        }
        None => synthesize_os2(head, hhea, first, last),
    };
    tables.push((*b"OS/2", os2));
    if dir.table(*b"name").is_none() {
        tables.push((*b"name", name_table(family)));
    }
    tables.push((*b"post", post_v3(dir.table(*b"post"))));
    Ok(WebFont {
        data: assemble(dir.flavor, tables),
        cff: dir.flavor == u32::from_be_bytes(*b"OTTO"),
    })
}

/// An sfnt table directory over borrowed bytes.
struct Directory<'a> {
    flavor: u32,
    tables: Vec<([u8; 4], &'a [u8])>,
}

impl<'a> Directory<'a> {
    fn parse(data: &'a [u8]) -> Option<Self> {
        let flavor = read_u32(data, 0)?;
        if !matches!(flavor, 0x0001_0000 | 0x7472_7565 | 0x4F54_544F) {
            return None;
        }
        let count = usize::from(read_u16(data, 4)?);
        let mut tables = Vec::with_capacity(count);
        for i in 0..count {
            let rec = 12 + i * 16;
            let tag: [u8; 4] = data.get(rec..rec + 4)?.try_into().ok()?;
            let offset = usize::try_from(read_u32(data, rec + 8)?).ok()?;
            let length = usize::try_from(read_u32(data, rec + 12)?).ok()?;
            tables.push((tag, data.get(offset..offset.checked_add(length)?)?));
        }
        Some(Self { flavor, tables })
    }

    fn table(&self, tag: [u8; 4]) -> Option<&'a [u8]> {
        self.tables.iter().find(|(t, _)| *t == tag).map(|(_, d)| *d)
    }
}

fn read_u16(d: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(d.get(at..at + 2)?.try_into().ok()?))
}

fn read_i16(d: &[u8], at: usize) -> i16 {
    read_u16(d, at).map_or(0, |v| v as i16)
}

fn read_u32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

/// The OpenType table checksum: the sum of big-endian `u32`s, the table
/// zero-padded to a multiple of four.
fn checksum(data: &[u8]) -> u32 {
    data.chunks(4).fold(0u32, |sum, c| {
        let mut w = [0u8; 4];
        w[..c.len()].copy_from_slice(c);
        sum.wrapping_add(u32::from_be_bytes(w))
    })
}

/// Write the sfnt: header, directory sorted by tag, each table 4-aligned,
/// and `head.checkSumAdjustment` = 0xB1B0AFBA − the whole file's checksum.
fn assemble(flavor: u32, mut tables: Vec<([u8; 4], Vec<u8>)>) -> Vec<u8> {
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
        if tag == b"head" {
            data[8..12].fill(0);
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
        out[at + 8..at + 12].copy_from_slice(&adjust.to_be_bytes());
    }
    out
}

/// A `cmap` with one format-4 subtable shared by (0,3) and (3,1).
///
/// Segments group characters that are consecutive AND map to consecutive
/// glyphs, so each needs only an `idDelta`; the mandatory final segment
/// maps 0xFFFF to glyph 0.
fn cmap(map: &BTreeMap<u16, u16>) -> Result<Vec<u8>, WebFontError> {
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
    let seg_count = u16::try_from(segs.len()).map_err(|_| WebFontError::TooLarge)?;
    let mut entry_selector = 0u16;
    while (2u32 << entry_selector) <= u32::from(seg_count) {
        entry_selector += 1;
    }
    let search_range = 2 * (1u16 << entry_selector);
    let seg_x2 = seg_count * 2;
    let range_shift = seg_x2 - search_range;
    let length = 16 + 8 * usize::from(seg_count);
    let length = u16::try_from(length).map_err(|_| WebFontError::TooLarge)?;

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

    let mut out = Vec::new();
    for v in [0u16, 2] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    // Records sorted by (platformID, encodingID); both at offset 20.
    for (platform, encoding) in [(0u16, 3u16), (3, 1)] {
        out.extend_from_slice(&platform.to_be_bytes());
        out.extend_from_slice(&encoding.to_be_bytes());
        out.extend_from_slice(&20u32.to_be_bytes());
    }
    out.extend_from_slice(&sub);
    Ok(out)
}

/// A version-4 `OS/2` (96 bytes) from `head` and `hhea`, for a program
/// that carried none. Weight 400, width 5, `fsType` 0 (installable),
/// `fsSelection` REGULAR; vertical metrics copied from `hhea`.
fn synthesize_os2(head: &[u8], hhea: &[u8], first: u16, last: u16) -> Vec<u8> {
    let upem = read_u16(head, 18).unwrap_or(1000);
    let ascender = read_i16(hhea, 4);
    let descender = read_i16(hhea, 6);
    let line_gap = read_i16(hhea, 8);
    let y_max = read_i16(head, 42);
    let y_min = read_i16(head, 38);
    let upem_i = i16::try_from(upem).unwrap_or(i16::MAX);
    let sub_size = upem_i / 2;

    let mut t = Vec::with_capacity(96);
    let mut w16 = |v: u16| t.extend_from_slice(&v.to_be_bytes());
    w16(4); // version
    w16((upem / 2).max(1)); // xAvgCharWidth (not recomputed; see module docs)
    w16(400); // usWeightClass
    w16(5); // usWidthClass
    w16(0); // fsType
    for v in [sub_size, sub_size, 0, upem_i / 7] {
        w16(v as u16); // ySubscript X/Y size, X/Y offset
    }
    for v in [sub_size, sub_size, 0, upem_i / 3] {
        w16(v as u16); // ySuperscript X/Y size, X/Y offset
    }
    w16((upem / 20).max(1)); // yStrikeoutSize
    w16((upem_i / 4) as u16); // yStrikeoutPosition
    w16(0); // sFamilyClass
    t.extend_from_slice(&[0; 10]); // panose
    t.extend_from_slice(&[0; 16]); // ulUnicodeRange1..4
    t.extend_from_slice(b"NONE"); // achVendID
    let mut w16 = |v: u16| t.extend_from_slice(&v.to_be_bytes());
    w16(0x0040); // fsSelection: REGULAR
    w16(first);
    w16(last);
    w16(ascender as u16); // sTypoAscender
    w16(descender as u16); // sTypoDescender
    w16(line_gap as u16); // sTypoLineGap
    w16(y_max.max(0) as u16); // usWinAscent
    w16(y_min.min(0).unsigned_abs()); // usWinDescent
    t.extend_from_slice(&1u32.to_be_bytes()); // ulCodePageRange1: Latin 1
    t.extend_from_slice(&0u32.to_be_bytes()); // ulCodePageRange2
    let mut w16 = |v: u16| t.extend_from_slice(&v.to_be_bytes());
    w16(0); // sxHeight
    w16(0); // sCapHeight
    w16(0); // usDefaultChar
    w16(0x20); // usBreakChar
    w16(1); // usMaxContext
    t
}

/// A format-0 `name` table: IDs 1, 2, 4 and 6, Windows Unicode BMP,
/// English (US).
fn name_table(family: &str) -> Vec<u8> {
    let ps: String = family
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || "-._~".contains(*c))
        .take(63)
        .collect();
    let ps = if ps.is_empty() {
        "pdfcer".to_owned()
    } else {
        ps
    };
    let records: [(u16, String); 4] = [
        (1, family.to_owned()),
        (2, "Regular".to_owned()),
        (4, family.to_owned()),
        (6, ps),
    ];
    let mut storage = Vec::new();
    let mut out = Vec::new();
    let count = records.len() as u16;
    for v in [0u16, count, 6 + 12 * count] {
        out.extend_from_slice(&v.to_be_bytes());
    }
    for (id, text) in &records {
        let bytes: Vec<u8> = text.encode_utf16().flat_map(u16::to_be_bytes).collect();
        for v in [
            3u16,
            1,
            0x0409,
            *id,
            u16::try_from(bytes.len()).unwrap_or(0),
            u16::try_from(storage.len()).unwrap_or(0),
        ] {
            out.extend_from_slice(&v.to_be_bytes());
        }
        storage.extend_from_slice(&bytes);
    }
    out.extend_from_slice(&storage);
    out
}

/// A version-3 `post`: no glyph names, upright, proportional.
/// A version-3 `post` (no glyph names), keeping the source's italic angle
/// and underline metrics when it has them. Always rewritten: OTS rejects
/// version 2.5 and any version-2 table in a CFF font, and the web font has
/// no use for glyph names.
fn post_v3(source: Option<&[u8]>) -> Vec<u8> {
    let mut t = match source.and_then(|s| s.get(..32)) {
        Some(header) => header.to_vec(),
        None => vec![0; 32],
    };
    t[..4].copy_from_slice(&0x0003_0000u32.to_be_bytes());
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmap_groups_consecutive_runs_and_maps_every_character() {
        let map: BTreeMap<u16, u16> = [(0x41, 3), (0x42, 4), (0x43, 5), (0x61, 9), (0x20, 1)]
            .into_iter()
            .collect();
        let t = cmap(&map).unwrap();
        // Header + 2 records, then the subtable.
        let sub = &t[20..];
        assert_eq!(read_u16(sub, 0), Some(4));
        let seg_x2 = usize::from(read_u16(sub, 6).unwrap());
        // 0x20, 0x41-0x43, 0x61, 0xFFFF.
        assert_eq!(seg_x2, 8);
        assert_eq!(usize::from(read_u16(sub, 2).unwrap()), sub.len());
        let lookup = |c: u16| -> u16 {
            let segs = seg_x2 / 2;
            for i in 0..segs {
                let end = read_u16(sub, 14 + 2 * i).unwrap();
                let start = read_u16(sub, 16 + seg_x2 + 2 * i).unwrap();
                let delta = read_u16(sub, 16 + 2 * seg_x2 + 2 * i).unwrap();
                if c >= start && c <= end {
                    return c.wrapping_add(delta);
                }
            }
            0
        };
        for (&c, &g) in &map {
            assert_eq!(lookup(c), g, "char {c:#x}");
        }
        assert_eq!(lookup(0xFFFF), 0);
        assert_eq!(lookup(0x44), 0);
    }

    #[test]
    fn assembled_font_has_valid_checksums() {
        let mut head = vec![0u8; 54];
        head[12..16].copy_from_slice(&0x5F0F_3CF5u32.to_be_bytes());
        let tables = vec![(*b"head", head), (*b"abcd", vec![1, 2, 3])];
        let font = assemble(0x0001_0000, tables);
        // The whole file sums to the magic constant.
        assert_eq!(checksum(&font), 0xB1B0_AFBA);
        let dir = Directory::parse(&font).unwrap();
        assert_eq!(dir.tables[0].0, *b"abcd", "sorted by tag");
        for i in 0..2 {
            let rec = 12 + i * 16;
            let mut data = dir.tables[i].1.to_vec();
            if dir.tables[i].0 == *b"head" {
                data[8..12].fill(0);
            }
            assert_eq!(read_u32(&font, rec + 4), Some(checksum(&data)));
        }
    }

    #[test]
    fn restricted_fonts_are_refused() {
        let mut os2 = vec![0u8; 78];
        os2[8..10].copy_from_slice(&2u16.to_be_bytes());
        let font = assemble(0x0001_0000, vec![(*b"OS/2", os2)]);
        let map = [('A', 1u16)].into_iter().collect();
        assert_eq!(
            build(&font, &map, "X").unwrap_err(),
            WebFontError::Restricted
        );
    }
}
