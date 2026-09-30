//! The PRC container: file header, file-structure descriptions, section
//! extents and inflation [WD 6.1-6.2, 10.2].
//!
//! Section extents are not stored; each runs to the next section's start
//! offset and the last one to the model file's start `[PRCRS decompress.rs]`.
//! Inflation stops at the zlib stream's end, so padding between sections is
//! tolerated.

use std::io::Read;

use crate::PrcError;

/// The PRC version this reader implements (Adobe PRC 8137).
pub const PRC_READER_VERSION: u32 = 8137;

/// The most file structures [`PrcFile::parse`] accepts.
pub const MAX_FILE_STRUCTURES: usize = 65_536;

/// Ceiling on the total inflated size of every section in one PRC stream.
pub const MAX_INFLATED_BYTES: usize = 512 * 1024 * 1024;

/// Sections per file structure [WD 6.1.2]: header, then the five in
/// [`SectionKind`] order.
const SECTIONS: usize = 6;

/// A 128-bit PRC identifier [WD 10.2.4]: four little-endian `u32`s.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UniqueId(pub [u32; 4]);

/// The compressed sections of one file structure, in file order
/// [WD 6.1.2, 6.2.1].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionKind {
    /// Starts with the file structure's schema, then its globals entity.
    Globals,
    /// The product-structure tree.
    Tree,
    /// Tessellated (mesh) data.
    Tessellation,
    /// Exact (B-rep) geometry.
    Geometry,
    /// Supplementary geometry.
    ExtraGeometry,
}

impl SectionKind {
    /// Every kind, in file order.
    pub const ALL: [SectionKind; 5] = [
        SectionKind::Globals,
        SectionKind::Tree,
        SectionKind::Tessellation,
        SectionKind::Geometry,
        SectionKind::ExtraGeometry,
    ];

    /// The section's name, as used in errors.
    pub fn name(self) -> &'static str {
        match self {
            SectionKind::Globals => "globals",
            SectionKind::Tree => "tree",
            SectionKind::Tessellation => "tessellation",
            SectionKind::Geometry => "geometry",
            SectionKind::ExtraGeometry => "extra geometry",
        }
    }
}

/// The uncompressed file header [WD 6.1.1].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PrcHeader {
    /// The oldest reader version the producer says can read the file.
    pub min_version_for_read: u32,
    /// The producer's PRC version.
    pub authoring_version: u32,
    /// The file's identifier.
    pub file_id: UniqueId,
    /// The producing application's identifier (all zero = unregistered).
    pub application_id: UniqueId,
}

/// One file structure: its uncompressed header fields and its five
/// inflated sections [WD 6.2].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FileStructure {
    /// The identifier; equals the file header's description of it.
    pub id: UniqueId,
    /// Per-structure reader version floor; may differ from the file's.
    pub min_version_for_read: u32,
    /// Per-structure producer version.
    pub authoring_version: u32,
    /// Picture payloads (JPEG, PNG, ...) carried uncompressed in the header,
    /// referenced by index from picture entities [WD 6.1.3].
    pub pictures: Vec<Vec<u8>>,
    sections: [Vec<u8>; 5],
}

impl FileStructure {
    /// The inflated bytes of one section.
    pub fn section(&self, kind: SectionKind) -> &[u8] {
        let i = SectionKind::ALL
            .iter()
            .position(|k| *k == kind)
            .unwrap_or(0);
        self.sections.get(i).map_or(&[], Vec::as_slice)
    }
}

/// A parsed PRC stream: header, file structures and model-file section.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PrcFile {
    /// The file header.
    pub header: PrcHeader,
    /// File structures in physical order (referenced before referencing).
    pub file_structures: Vec<FileStructure>,
    /// The inflated model-file section: its schema, then the model file
    /// entity.
    pub model_file: Vec<u8>,
    /// Number of `UncompressedFiles` blocks after the header.
    pub uncompressed_files: usize,
}

struct Bytes<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Bytes<'a> {
    fn take(&mut self, n: usize, what: &'static str) -> Result<&'a [u8], PrcError> {
        let end = self.pos.checked_add(n).ok_or(PrcError::Truncated(what))?;
        let s = self
            .data
            .get(self.pos..end)
            .ok_or(PrcError::Truncated(what))?;
        self.pos = end;
        Ok(s)
    }

    fn u32(&mut self, what: &'static str) -> Result<u32, PrcError> {
        let s = self.take(4, what)?;
        let mut b = [0u8; 4];
        b.copy_from_slice(s);
        Ok(u32::from_le_bytes(b))
    }

    fn id(&mut self, what: &'static str) -> Result<UniqueId, PrcError> {
        Ok(UniqueId([
            self.u32(what)?,
            self.u32(what)?,
            self.u32(what)?,
            self.u32(what)?,
        ]))
    }

    fn magic(&mut self) -> Result<(), PrcError> {
        match self.take(3, "magic") {
            Ok(b"PRC") => Ok(()),
            _ => Err(PrcError::NotPrc),
        }
    }

    /// `UncompressedBlock`s: a count, then size-prefixed blocks [WD 10.2.2-3].
    fn blocks(&mut self, what: &'static str) -> Result<Vec<Vec<u8>>, PrcError> {
        let n = self.u32(what)? as usize;
        // Each block needs at least its 4-byte size.
        if n > self.data.len().saturating_sub(self.pos) / 4 {
            return Err(PrcError::Truncated(what));
        }
        (0..n)
            .map(|_| {
                let len = self.u32(what)? as usize;
                Ok(self.take(len, what)?.to_vec())
            })
            .collect()
    }
}

fn offset(v: u32) -> usize {
    v as usize
}

fn inflate(
    data: &[u8],
    range: (usize, usize),
    section: &'static str,
    budget: &mut usize,
    limit: usize,
) -> Result<Vec<u8>, PrcError> {
    let (start, end) = range;
    if start == end {
        return Ok(Vec::new());
    }
    let src = data.get(start..end).ok_or_else(|| {
        PrcError::Malformed(format!(
            "{section} section {start}..{end} is outside the data"
        ))
    })?;
    let mut out = Vec::new();
    let cap = u64::try_from(*budget).unwrap_or(u64::MAX).saturating_add(1);
    flate2::read::ZlibDecoder::new(src)
        .take(cap)
        .read_to_end(&mut out)
        .map_err(|e| PrcError::Inflate {
            section,
            reason: e.to_string(),
        })?;
    if out.len() > *budget {
        return Err(PrcError::TooLarge { limit });
    }
    *budget -= out.len();
    Ok(out)
}

impl PrcFile {
    /// Parse a PRC stream (the decoded data of a `/3D` stream with
    /// `/Subtype /PRC`).
    ///
    /// Newer `min_version_for_read` values are not refused: the fields read
    /// here are unchanged across published versions, and the caller sees the
    /// versions in [`PrcHeader`].
    ///
    /// # Errors
    /// [`PrcError::NotPrc`] without the magic; [`PrcError::Truncated`] or
    /// [`PrcError::Malformed`] for a damaged header or offset table;
    /// [`PrcError::Inflate`] for a section that is not zlib;
    /// [`PrcError::TooLarge`] past [`MAX_INFLATED_BYTES`] in total.
    ///
    /// # Examples
    /// ```
    /// use pdfcer_3d::{PrcError, PrcFile};
    /// assert_eq!(PrcFile::parse(b"%PDF").unwrap_err(), PrcError::NotPrc);
    /// ```
    pub fn parse(data: &[u8]) -> Result<Self, PrcError> {
        Self::parse_with_limit(data, MAX_INFLATED_BYTES)
    }

    /// [`Self::parse`] with a caller-chosen ceiling on the total inflated
    /// size, for callers with less memory to spend than
    /// [`MAX_INFLATED_BYTES`].
    ///
    /// # Errors
    /// As [`Self::parse`]; [`PrcError::TooLarge`] carries `limit`.
    pub fn parse_with_limit(data: &[u8], limit: usize) -> Result<Self, PrcError> {
        let mut r = Bytes { data, pos: 0 };
        r.magic()?;
        let header = PrcHeader {
            min_version_for_read: r.u32("file header")?,
            authoring_version: r.u32("file header")?,
            file_id: r.id("file header")?,
            application_id: r.id("file header")?,
        };
        let n_fs = r.u32("file structure count")? as usize;
        if n_fs == 0 || n_fs > MAX_FILE_STRUCTURES {
            return Err(PrcError::Malformed(format!("{n_fs} file structures")));
        }
        let mut descriptions = Vec::new();
        for _ in 0..n_fs {
            let id = r.id("file structure description")?;
            if r.u32("file structure description")? != 0 {
                return Err(PrcError::Malformed(
                    "file structure reserved field is not 0".into(),
                ));
            }
            let count = r.u32("file structure description")? as usize;
            if count < SECTIONS {
                return Err(PrcError::Malformed(format!(
                    "{count} sections in a file structure"
                )));
            }
            let mut offs = Vec::with_capacity(SECTIONS);
            for i in 0..count {
                let o = r.u32("section offset")?;
                if i < SECTIONS {
                    offs.push(offset(o));
                }
            }
            descriptions.push((id, offs));
        }
        let mf_start = offset(r.u32("model file offsets")?);
        let mf_end = offset(r.u32("model file offsets")?);
        let uncompressed_files = r.blocks("uncompressed files")?.len();

        let mut budget = limit;
        let mut file_structures = Vec::with_capacity(n_fs);
        for (id, offs) in descriptions {
            let last = mf_start.min(data.len());
            let bound = |i: usize| offs.get(i).copied().unwrap_or(last);
            if offs.windows(2).any(|w| w.first() > w.get(1)) || bound(SECTIONS - 1) > last {
                return Err(PrcError::Malformed(
                    "file structure section offsets are not ascending".into(),
                ));
            }
            let mut h = Bytes {
                data: data.get(..bound(1)).unwrap_or(&[]),
                pos: bound(0),
            };
            h.magic()?;
            let min_version_for_read = h.u32("file structure header")?;
            let authoring_version = h.u32("file structure header")?;
            let fs_id = h.id("file structure header")?;
            if fs_id != id {
                return Err(PrcError::Malformed(
                    "file structure id differs from its description".into(),
                ));
            }
            let _application = h.id("file structure header")?;
            let pictures = h.blocks("file structure pictures")?;
            let mut sections: [Vec<u8>; 5] = Default::default();
            for (i, (slot, kind)) in sections.iter_mut().zip(SectionKind::ALL).enumerate() {
                let end = if i + 2 < SECTIONS { bound(i + 2) } else { last };
                *slot = inflate(data, (bound(i + 1), end), kind.name(), &mut budget, limit)?;
            }
            file_structures.push(FileStructure {
                id,
                min_version_for_read,
                authoring_version,
                pictures,
                sections,
            });
        }
        if mf_end < mf_start {
            return Err(PrcError::Malformed(
                "model file ends before it starts".into(),
            ));
        }
        let model_file = inflate(
            data,
            (mf_start, mf_end.min(data.len())),
            "model file",
            &mut budget,
            limit,
        )?;
        Ok(PrcFile {
            header,
            file_structures,
            model_file,
            uncompressed_files,
        })
    }
}
