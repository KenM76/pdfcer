//! Merging subsets of one face into one program. Every table but the glyph
//! data must agree, and a glyph two subsets both carry must be the same
//! record, so each subset's users draw exactly what they drew before
//! (OpenType 1.9.1 `glyf`, `loca`, `head`, `name`).

use std::collections::BTreeMap;

use pdfcer_core::optimize::resubset::{ResubsetProgram, ResubsetRefusal};

use super::malformed;
use super::zero::Glyphs;
use crate::font::augment::glyf::{record, write_loca};
use crate::font::program::FontProgram;
use crate::font::sfnt::{Directory, assemble};

/// One program holding every glyph record of `programs`, with every glyph
/// ID kept. `name` is taken from the first program: subsets differ in it by
/// their tag, and nothing draws with it. A table only some carry (a simple
/// font's `cmap`, which a CIDFont ignores) is kept.
pub(super) fn merge(programs: &[&[u8]]) -> Result<ResubsetProgram, ResubsetRefusal> {
    let dirs = programs
        .iter()
        .map(|p| Directory::parse(p).ok_or_else(malformed))
        .collect::<Result<Vec<_>, _>>()?;
    let glyphs = dirs
        .iter()
        .map(|d| Glyphs::read(d).ok_or_else(malformed))
        .collect::<Result<Vec<_>, _>>()?;
    let (Some(dir), Some(first)) = (dirs.first(), glyphs.first()) else {
        return Err(malformed());
    };
    if dirs.iter().any(|d| d.flavor != dir.flavor)
        || glyphs
            .iter()
            .any(|g| g.total != first.total || g.long != first.long)
    {
        return Err(different("glyph count or glyph-offset format"));
    }
    let (glyf, offsets, kept) = union(&glyphs)?;
    let (loca, long) = write_loca(&offsets, first.long);
    if long != first.long {
        return Err(malformed());
    }
    let mut tables = shared_tables(&dirs)?;
    tables.push((*b"glyf", glyf));
    tables.push((*b"loca", loca));
    let out = assemble(dir.flavor, tables);
    verify(&out, &glyphs)?;
    Ok(ResubsetProgram::new(
        out,
        u32::try_from(kept).unwrap_or(u32::MAX),
        u32::try_from(first.total).unwrap_or(u32::MAX),
    ))
}

/// An sfnt's tables by tag, as [`assemble`] takes them.
type Tables = Vec<([u8; 4], Vec<u8>)>;

fn different(what: &str) -> ResubsetRefusal {
    ResubsetRefusal::new(
        "different-face",
        format!("the subsets are not cut from one font program: their {what} differ"),
    )
}

/// `head` with the bytes subsets of one face may differ in zeroed:
/// `checkSumAdjustment` and the `created` and `modified` dates.
fn head_core(head: &[u8]) -> Vec<u8> {
    let mut h = head.to_vec();
    for range in [8..12, 20..36] {
        if let Some(bytes) = h.get_mut(range) {
            bytes.fill(0);
        }
    }
    h
}

/// Every table but `glyf`, `loca` and `DSIG` (whose signature covers the old
/// bytes), each agreeing across the programs that carry it.
fn shared_tables(dirs: &[Directory<'_>]) -> Result<Tables, ResubsetRefusal> {
    let mut out: BTreeMap<[u8; 4], &[u8]> = BTreeMap::new();
    for (tag, data) in dirs.iter().flat_map(|d| d.tables.iter()) {
        if matches!(tag, b"glyf" | b"loca" | b"DSIG") {
            continue;
        }
        let Some(&seen) = out.get(tag) else {
            out.insert(*tag, data);
            continue;
        };
        let agree = match tag {
            b"name" => true,
            b"head" => head_core(seen) == head_core(data),
            _ => seen == *data,
        };
        if !agree {
            return Err(different(&format!(
                "'{}' tables",
                String::from_utf8_lossy(tag).trim_end()
            )));
        }
    }
    Ok(out.into_iter().map(|(t, d)| (t, d.to_vec())).collect())
}

/// The merged `glyf`, its offsets and its count of non-empty records. In the
/// short `loca` format every record starts on an even offset.
fn union(glyphs: &[Glyphs<'_>]) -> Result<(Vec<u8>, Vec<usize>, usize), ResubsetRefusal> {
    let total = glyphs.first().map_or(0, |g| g.total);
    let long = glyphs.first().is_some_and(|g| g.long);
    let mut glyf = Vec::new();
    let mut offsets = Vec::with_capacity(total + 1);
    let mut kept = 0;
    for gid in 0..total {
        offsets.push(glyf.len());
        let mut chosen: Option<&[u8]> = None;
        for g in glyphs {
            let rec = record(g.glyf, &g.offsets, gid).ok_or_else(malformed)?;
            match chosen {
                _ if rec.is_empty() => {}
                Some(c) if c != rec => {
                    return Err(different(&format!("outlines for glyph {gid}")));
                }
                _ => chosen = Some(rec),
            }
        }
        if let Some(rec) = chosen {
            glyf.extend_from_slice(rec);
            kept += 1;
            if !long && glyf.len() % 2 == 1 {
                glyf.push(0);
            }
        }
    }
    offsets.push(glyf.len());
    Ok((glyf, offsets, kept))
}

/// The merge re-parses, and every record of every input is in it unchanged.
fn verify(out: &[u8], inputs: &[Glyphs<'_>]) -> Result<(), ResubsetRefusal> {
    let dir = Directory::parse(out).ok_or_else(malformed)?;
    let new = Glyphs::read(&dir).ok_or_else(malformed)?;
    let same = inputs.iter().all(|g| {
        g.total == new.total
            && (0..g.total).all(|gid| match record(g.glyf, &g.offsets, gid) {
                Some([]) => true,
                Some(rec) => record(new.glyf, &new.offsets, gid) == Some(rec),
                None => false,
            })
    });
    if !same || FontProgram::parse(out).is_err() {
        return Err(malformed());
    }
    Ok(())
}
