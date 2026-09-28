//! The zip container Office Open XML packages are stored in (ECMA-376
//! Part 2, Open Packaging Conventions §8 / ISO/IEC 29500-2: a ZIP file per
//! PKWARE APPNOTE 6.3, no ZIP64, Deflate or Stored entries).
//!
//! Every entry is Deflate-compressed with a fixed 1980-01-01 00:00
//! timestamp, so identical input gives identical bytes.

use std::io::Write as _;

use flate2::Compression;
use flate2::write::DeflateEncoder;

/// Why a package could not be written.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum PackageError {
    /// An entry, the archive or the entry count exceeds what a zip without
    /// ZIP64 can record (4 GiB, 65 535 entries).
    #[error("package too large for a zip without ZIP64")]
    TooLarge,
    /// Compression failed.
    #[error("deflate: {0}")]
    Deflate(#[from] std::io::Error),
}

/// DOS date 1980-01-01 (year offset 0, month 1, day 1).
const DOS_DATE: u16 = (1 << 5) | 1;
const VERSION: u16 = 20;
const METHOD_DEFLATE: u16 = 8;

struct Entry {
    name: String,
    crc: u32,
    compressed: u32,
    size: u32,
    offset: u32,
}

/// Builds a zip archive in memory, entry by entry.
pub(crate) struct ZipWriter {
    out: Vec<u8>,
    entries: Vec<Entry>,
}

fn u32_of(n: usize) -> Result<u32, PackageError> {
    u32::try_from(n).map_err(|_| PackageError::TooLarge)
}

fn u16_of(n: usize) -> Result<u16, PackageError> {
    u16::try_from(n).map_err(|_| PackageError::TooLarge)
}

impl ZipWriter {
    /// An empty archive.
    pub(crate) fn new() -> Self {
        Self {
            out: Vec::new(),
            entries: Vec::new(),
        }
    }

    /// Appends `name` (an ASCII part name, no leading `/`) holding `data`.
    pub(crate) fn add(&mut self, name: &str, data: &[u8]) -> Result<(), PackageError> {
        let mut crc = flate2::Crc::new();
        crc.update(data);
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::best());
        enc.write_all(data)?;
        let packed = enc.finish()?;
        let entry = Entry {
            name: name.to_owned(),
            crc: crc.sum(),
            compressed: u32_of(packed.len())?,
            size: u32_of(data.len())?,
            offset: u32_of(self.out.len())?,
        };
        let name_len = u16_of(name.len())?;
        let o = &mut self.out;
        o.extend_from_slice(&0x0403_4b50u32.to_le_bytes());
        o.extend_from_slice(&VERSION.to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes()); // flags
        o.extend_from_slice(&METHOD_DEFLATE.to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes()); // time
        o.extend_from_slice(&DOS_DATE.to_le_bytes());
        o.extend_from_slice(&entry.crc.to_le_bytes());
        o.extend_from_slice(&entry.compressed.to_le_bytes());
        o.extend_from_slice(&entry.size.to_le_bytes());
        o.extend_from_slice(&name_len.to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes()); // extra length
        o.extend_from_slice(name.as_bytes());
        o.extend_from_slice(&packed);
        self.entries.push(entry);
        Ok(())
    }

    /// Writes the central directory and returns the archive.
    pub(crate) fn finish(mut self) -> Result<Vec<u8>, PackageError> {
        let cd_start = u32_of(self.out.len())?;
        for e in &self.entries {
            let o = &mut self.out;
            o.extend_from_slice(&0x0201_4b50u32.to_le_bytes());
            o.extend_from_slice(&VERSION.to_le_bytes()); // made by
            o.extend_from_slice(&VERSION.to_le_bytes()); // needed
            o.extend_from_slice(&0u16.to_le_bytes()); // flags
            o.extend_from_slice(&METHOD_DEFLATE.to_le_bytes());
            o.extend_from_slice(&0u16.to_le_bytes()); // time
            o.extend_from_slice(&DOS_DATE.to_le_bytes());
            o.extend_from_slice(&e.crc.to_le_bytes());
            o.extend_from_slice(&e.compressed.to_le_bytes());
            o.extend_from_slice(&e.size.to_le_bytes());
            o.extend_from_slice(&u16_of(e.name.len())?.to_le_bytes());
            o.extend_from_slice(&[0; 8]); // extra, comment, disk, internal attrs
            o.extend_from_slice(&0u32.to_le_bytes()); // external attrs
            o.extend_from_slice(&e.offset.to_le_bytes());
            o.extend_from_slice(e.name.as_bytes());
        }
        let cd_size = u32_of(self.out.len())? - cd_start;
        let count = u16_of(self.entries.len())?;
        let o = &mut self.out;
        o.extend_from_slice(&0x0605_4b50u32.to_le_bytes());
        o.extend_from_slice(&[0; 4]); // this disk, cd disk
        o.extend_from_slice(&count.to_le_bytes());
        o.extend_from_slice(&count.to_le_bytes());
        o.extend_from_slice(&cd_size.to_le_bytes());
        o.extend_from_slice(&cd_start.to_le_bytes());
        o.extend_from_slice(&0u16.to_le_bytes()); // comment length
        u32_of(self.out.len())?;
        Ok(self.out)
    }
}

/// Escapes `s` for XML element content or a double-quoted attribute, and
/// drops characters XML 1.0 forbids (C0 controls other than tab, newline
/// and carriage return; U+FFFE; U+FFFF). Returns how many were dropped.
pub(crate) fn escape_into(out: &mut String, s: &str) -> usize {
    let mut dropped = 0;
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if (c as u32) < 0x20 || c == '\u{FFFE}' || c == '\u{FFFF}' => dropped += 1,
            c => out.push(c),
        }
    }
    dropped
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
pub(crate) mod tests {
    use super::*;
    use std::io::Read as _;

    /// Reads every entry back through the central directory.
    pub(crate) fn read_zip(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let le16 = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]) as usize;
        let le32 = |i: usize| {
            u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize
        };
        let eocd = bytes.len() - 22;
        assert_eq!(le32(eocd), 0x0605_4b50);
        let count = le16(eocd + 10);
        let mut at = le32(eocd + 16);
        let mut out = Vec::new();
        for _ in 0..count {
            assert_eq!(le32(at), 0x0201_4b50);
            let crc = le32(at + 16) as u32;
            let csize = le32(at + 20);
            let size = le32(at + 24);
            let nlen = le16(at + 28);
            let local = le32(at + 42);
            let name = String::from_utf8(bytes[at + 46..at + 46 + nlen].to_vec()).unwrap();
            assert_eq!(le32(local), 0x0403_4b50);
            let data_at = local + 30 + le16(local + 26) + le16(local + 28);
            let mut data = Vec::new();
            flate2::read::DeflateDecoder::new(&bytes[data_at..data_at + csize])
                .read_to_end(&mut data)
                .unwrap();
            assert_eq!(data.len(), size);
            let mut c = flate2::Crc::new();
            c.update(&data);
            assert_eq!(c.sum(), crc, "{name}");
            out.push((name, data));
            at += 46 + nlen;
        }
        out
    }

    #[test]
    fn entries_round_trip() {
        let mut z = ZipWriter::new();
        z.add("a.xml", b"<a/>").unwrap();
        z.add("dir/b.txt", &[7u8; 5000]).unwrap();
        let bytes = z.finish().unwrap();
        let back = read_zip(&bytes);
        assert_eq!(back.len(), 2);
        assert_eq!(back[0], ("a.xml".to_owned(), b"<a/>".to_vec()));
        assert_eq!(back[1].0, "dir/b.txt");
        assert_eq!(back[1].1, vec![7u8; 5000]);
    }

    #[test]
    fn escape_drops_illegal_controls() {
        let mut s = String::new();
        assert_eq!(escape_into(&mut s, "a<b&\"c\u{1}\td"), 1);
        assert_eq!(s, "a&lt;b&amp;&quot;c\td");
    }
}
