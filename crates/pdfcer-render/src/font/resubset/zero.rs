//! Emptying glyph records: `glyf` keeps only the kept records, `loca` is
//! rewritten in its original format so `head` is unchanged, and every
//! other table is copied (OpenType 1.9.1 `glyf`, `loca`).

use std::collections::BTreeSet;

use pdfcer_core::optimize::resubset::ResubsetRefusal;

use super::malformed;
use crate::font::augment::glyf::{closure, read_loca, record, write_loca};
use crate::font::program::FontProgram;
use crate::font::sfnt::{Directory, assemble, read_i16, read_u16};

/// A cut program and its glyph counts.
pub(super) struct Cut {
    pub(super) program: Vec<u8>,
    pub(super) kept: u32,
    pub(super) total: u32,
}

/// The tables a cut reads: `head.indexToLocFormat`, `maxp.numGlyphs`, and
/// `glyf` with its offsets.
struct Glyphs<'a> {
    long: bool,
    total: usize,
    glyf: &'a [u8],
    offsets: Vec<usize>,
}

impl<'a> Glyphs<'a> {
    fn read(dir: &Directory<'a>) -> Option<Self> {
        let long = read_i16(dir.table(*b"head")?, 50) != 0;
        let total = usize::from(read_u16(dir.table(*b"maxp")?, 4)?);
        let glyf = dir.table(*b"glyf")?;
        let offsets = read_loca(dir.table(*b"loca")?, long, total)?;
        Some(Self {
            long,
            total,
            glyf,
            offsets,
        })
    }
}

/// `program` with every record outside `roots` and their composite
/// closure emptied, verified before it is returned.
///
/// `DSIG` is dropped: its signature covers the old bytes and would be
/// false over the new ones.
pub(super) fn zero(program: &[u8], roots: &BTreeSet<u16>) -> Result<Cut, ResubsetRefusal> {
    let dir = Directory::parse(program).ok_or_else(malformed)?;
    let old = Glyphs::read(&dir).ok_or_else(malformed)?;
    let roots: Vec<u16> = roots
        .iter()
        .copied()
        .filter(|&g| usize::from(g) < old.total)
        .collect();
    let keep: BTreeSet<u16> = closure(old.glyf, &old.offsets, &roots)
        .map_err(|_| malformed())?
        .into_iter()
        .collect();
    let (glyf, offsets) = rebuild(&old, &keep)?;
    let (loca, long) = write_loca(&offsets, old.long);
    if long != old.long {
        return Err(malformed());
    }
    let tables = dir
        .tables
        .iter()
        .filter(|(tag, _)| tag != b"DSIG")
        .map(|(tag, data)| match tag {
            b"glyf" => (*tag, glyf.clone()),
            b"loca" => (*tag, loca.clone()),
            _ => (*tag, data.to_vec()),
        })
        .collect();
    let out = assemble(dir.flavor, tables);
    verify(&out, &old, &keep)?;
    Ok(Cut {
        program: out,
        kept: u32::try_from(keep.len()).unwrap_or(u32::MAX),
        total: u32::try_from(old.total).unwrap_or(u32::MAX),
    })
}

/// The new `glyf` and its offsets. In the short `loca` format every
/// record starts on an even offset, as that format requires.
fn rebuild(
    old: &Glyphs<'_>,
    keep: &BTreeSet<u16>,
) -> Result<(Vec<u8>, Vec<usize>), ResubsetRefusal> {
    let mut glyf = Vec::new();
    let mut offsets = Vec::with_capacity(old.total + 1);
    for gid in 0..old.total {
        offsets.push(glyf.len());
        if u16::try_from(gid).is_ok_and(|g| keep.contains(&g)) {
            glyf.extend_from_slice(record(old.glyf, &old.offsets, gid).ok_or_else(malformed)?);
            if !old.long && glyf.len() % 2 == 1 {
                glyf.push(0);
            }
        }
    }
    offsets.push(glyf.len());
    Ok((glyf, offsets))
}

/// The cut re-parses, has the same glyph count and loca format, and every
/// kept record is byte-identical to its original.
fn verify(out: &[u8], old: &Glyphs<'_>, keep: &BTreeSet<u16>) -> Result<(), ResubsetRefusal> {
    let dir = Directory::parse(out).ok_or_else(malformed)?;
    let new = Glyphs::read(&dir).ok_or_else(malformed)?;
    let same = new.total == old.total
        && new.long == old.long
        && keep.iter().all(|&g| {
            let g = usize::from(g);
            let before = record(old.glyf, &old.offsets, g);
            before.is_some() && before == record(new.glyf, &new.offsets, g)
        });
    if !same || FontProgram::parse(out).is_err() {
        return Err(malformed());
    }
    Ok(())
}
