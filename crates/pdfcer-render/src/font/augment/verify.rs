//! Rule R260: the augmented program, re-parsed by the renderer's own parser,
//! keeps every old glyph and mapping, reaches the face's outline for every
//! new character, and carries valid checksums. Never skipped.

use skrifa::FontRef;

use crate::font::program::FontProgram;
use crate::font::sfnt::{Directory, checksum, read_u16};

use super::metrics::{HMetric, read_hmtx};
use super::{AddedGlyph, AugmentError, cmap, glyf};

fn fail(detail: impl Into<String>) -> AugmentError {
    AugmentError::VerificationFailed {
        detail: detail.into(),
    }
}

/// Check `out` against `subset` and the face, per R260.
pub(crate) fn check(
    subset: &[u8],
    out: &[u8],
    face: &[u8],
    face_index: u32,
    added: &[AddedGlyph],
    num_glyphs: usize,
) -> Result<(), AugmentError> {
    checksums(out)?;
    let program =
        FontProgram::parse(out).map_err(|e| fail(format!("the result does not parse: {e}")))?;
    if usize::try_from(program.num_glyphs()).ok() != Some(num_glyphs) {
        return Err(fail("numGlyphs is not old + added"));
    }
    let (old, new) = (tables(subset)?, tables(out)?);
    let old_n = old.loca.len() - 1;
    for gid in 0..old_n {
        if glyf::record(old.glyf, &old.loca, gid) != glyf::record(new.glyf, &new.loca, gid) {
            return Err(fail(format!("glyph {gid}'s record changed")));
        }
        if old.metrics.get(gid) != new.metrics.get(gid) {
            return Err(fail(format!("glyph {gid}'s metrics changed")));
        }
    }
    for (c, g) in cmap::windows_unicode_map(old.cmap).unwrap_or_default() {
        let remapped = added.iter().any(|a| u32::from(a.ch) == c);
        if !remapped
            && cmap::windows_unicode_map(new.cmap).and_then(|m| m.get(&c).copied()) != Some(g)
        {
            return Err(fail(format!("U+{c:04X}'s old mapping changed")));
        }
    }
    let face = FontRef::from_index(face, face_index)
        .map(FontProgram::Sfnt)
        .map_err(|e| fail(e.to_string()))?;
    for a in added {
        let gid = program.glyph_for_char(a.ch);
        if gid != Some(u32::from(a.gid)) {
            return Err(fail(format!(
                "U+{:04X} does not reach its new glyph",
                u32::from(a.ch)
            )));
        }
        let face_gid = face
            .glyph_for_char(a.ch)
            .ok_or_else(|| fail("the face lost the character"))?;
        let mine = program
            .outline(u32::from(a.gid))
            .map_err(|e| fail(e.to_string()))?;
        let theirs = face.outline(face_gid).map_err(|e| fail(e.to_string()))?;
        let same = match (&mine, &theirs) {
            (Some(m), Some(t)) => m.points() == t.points() && m.verbs() == t.verbs(),
            (None, None) => true,
            _ => false,
        };
        if !same {
            return Err(fail(format!(
                "U+{:04X}'s outline differs from the face's",
                u32::from(a.ch)
            )));
        }
    }
    Ok(())
}

/// Every table checksum, and `checkSumAdjustment` (whole file sums to
/// 0xB1B0AFBA).
fn checksums(out: &[u8]) -> Result<(), AugmentError> {
    let dir = Directory::parse(out).ok_or_else(|| fail("the directory does not parse"))?;
    let count = usize::from(read_u16(out, 4).unwrap_or(0));
    for i in 0..count {
        let rec = 12 + i * 16;
        let Some(&(tag, data)) = dir.tables.get(i) else {
            break;
        };
        let mut d = data.to_vec();
        if &tag == b"head"
            && let Some(adj) = d.get_mut(8..12)
        {
            adj.fill(0);
        }
        if out.get(rec + 4..rec + 8) != Some(&checksum(&d).to_be_bytes()[..]) {
            return Err(fail(format!(
                "the {} checksum is wrong",
                tag.escape_ascii()
            )));
        }
    }
    if checksum(out) != 0xB1B0_AFBA {
        return Err(fail("checkSumAdjustment is wrong"));
    }
    Ok(())
}

struct Tables<'a> {
    glyf: &'a [u8],
    loca: Vec<usize>,
    metrics: Vec<HMetric>,
    cmap: &'a [u8],
}

fn tables(data: &[u8]) -> Result<Tables<'_>, AugmentError> {
    let dir = Directory::parse(data).ok_or_else(|| fail("a directory does not parse"))?;
    let t = |tag: &[u8; 4]| {
        dir.table(*tag)
            .ok_or_else(|| fail(format!("{} is missing", tag.escape_ascii())))
    };
    let n = usize::from(read_u16(t(b"maxp")?, 4).unwrap_or(0));
    let long = read_u16(t(b"head")?, 50) == Some(1);
    Ok(Tables {
        glyf: t(b"glyf")?,
        loca: glyf::read_loca(t(b"loca")?, long, n).ok_or_else(|| fail("loca is truncated"))?,
        metrics: read_hmtx(
            t(b"hmtx")?,
            usize::from(read_u16(t(b"hhea")?, 34).unwrap_or(0)).min(n),
            n,
        )
        .ok_or_else(|| fail("hmtx is truncated"))?,
        cmap: t(b"cmap")?,
    })
}
