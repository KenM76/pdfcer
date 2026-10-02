//! `glyf`/`loca` for decision 173 §4: read glyph records, take a composite's
//! component closure, rewrite a record for its new home, and append records
//! after a subset's last glyph without touching any existing byte.

use std::collections::HashMap;

use crate::font::sfnt::{put, read_i16, read_u16};

use super::AugmentError;

const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
const WE_HAVE_A_SCALE: u16 = 0x0008;
const MORE_COMPONENTS: u16 = 0x0020;
const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;
const WE_HAVE_INSTRUCTIONS: u16 = 0x0100;

/// Composite nesting deeper than this is refused as malformed (decision 173 §4).
pub(crate) const MAX_COMPONENT_DEPTH: usize = 16;

/// The `loca` offsets (`numGlyphs + 1` of them) of a `glyf` table.
pub(crate) fn read_loca(loca: &[u8], long: bool, num_glyphs: usize) -> Option<Vec<usize>> {
    (0..=num_glyphs)
        .map(|i| {
            if long {
                crate::font::sfnt::read_u32(loca, i * 4).and_then(|v| usize::try_from(v).ok())
            } else {
                read_u16(loca, i * 2).map(|v| usize::from(v) * 2)
            }
        })
        .collect()
}

/// `loca` for `offsets`: short unless `long` already, or an offset exceeds
/// what the short form can hold (`0x1FFFE`). Returns the table and its form.
pub(crate) fn write_loca(offsets: &[usize], long: bool) -> (Vec<u8>, bool) {
    let long = long || offsets.iter().any(|&o| o > 0x1FFFE);
    let mut out = Vec::with_capacity(offsets.len() * if long { 4 } else { 2 });
    for &o in offsets {
        if long {
            out.extend_from_slice(&u32::try_from(o).unwrap_or(u32::MAX).to_be_bytes());
        } else {
            out.extend_from_slice(&u16::try_from(o / 2).unwrap_or(u16::MAX).to_be_bytes());
        }
    }
    (out, long)
}

/// The record of glyph `gid`; empty for a glyph with no outline.
pub(crate) fn record<'a>(glyf: &'a [u8], loca: &[usize], gid: usize) -> Option<&'a [u8]> {
    let (start, end) = (*loca.get(gid)?, *loca.get(gid + 1)?);
    if end < start {
        return None;
    }
    glyf.get(start..end)
}

/// One component of a composite record: where its `glyphIndex` sits, and
/// the glyph it names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Component {
    pub(crate) index_at: usize,
    pub(crate) gid: u16,
}

/// A composite record's components and the byte offset just past the last
/// one (where its instructions, if any, begin). `None` for a simple or
/// empty record; `Err` when the record is truncated.
pub(crate) fn components(rec: &[u8]) -> Result<Option<(Vec<Component>, usize)>, AugmentError> {
    if rec.is_empty() || read_i16(rec, 0) >= 0 {
        return Ok(None);
    }
    let malformed = || AugmentError::MalformedFace {
        detail: "a composite glyph record is truncated".into(),
    };
    let mut at = 10;
    let mut out = Vec::new();
    loop {
        let flags = read_u16(rec, at).ok_or_else(malformed)?;
        let gid = read_u16(rec, at + 2).ok_or_else(malformed)?;
        out.push(Component {
            index_at: at + 2,
            gid,
        });
        at += 4 + if flags & ARG_1_AND_2_ARE_WORDS != 0 {
            4
        } else {
            2
        };
        at += if flags & WE_HAVE_A_SCALE != 0 {
            2
        } else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 {
            4
        } else if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
            8
        } else {
            0
        };
        if at > rec.len() {
            return Err(malformed());
        }
        if flags & MORE_COMPONENTS == 0 {
            return Ok(Some((out, at)));
        }
    }
}

/// `roots` and every glyph their composites reach, each once, roots first
/// then components in discovery order. Refuses a cycle or nesting deeper
/// than [`MAX_COMPONENT_DEPTH`].
pub(crate) fn closure(
    glyf: &[u8],
    loca: &[usize],
    roots: &[u16],
) -> Result<Vec<u16>, AugmentError> {
    let mut order: Vec<u16> = Vec::new();
    for &r in roots {
        visit(glyf, loca, r, &mut Vec::new(), &mut order)?;
    }
    let mut sorted: Vec<u16> = roots.iter().copied().fold(Vec::new(), |mut v, g| {
        if !v.contains(&g) {
            v.push(g);
        }
        v
    });
    sorted.extend(order.into_iter().filter(|g| !roots.contains(g)));
    Ok(sorted)
}

fn visit(
    glyf: &[u8],
    loca: &[usize],
    gid: u16,
    path: &mut Vec<u16>,
    order: &mut Vec<u16>,
) -> Result<(), AugmentError> {
    if path.contains(&gid) || path.len() > MAX_COMPONENT_DEPTH {
        return Err(AugmentError::MalformedFace {
            detail: format!(
                "glyph {gid}'s components nest in a cycle or deeper than {MAX_COMPONENT_DEPTH}"
            ),
        });
    }
    if order.contains(&gid) {
        return Ok(());
    }
    let rec = record(glyf, loca, usize::from(gid)).ok_or_else(|| AugmentError::MalformedFace {
        detail: format!("glyph {gid} is outside the face's loca"),
    })?;
    path.push(gid);
    if let Some((parts, _)) = components(rec)? {
        for c in parts {
            visit(glyf, loca, c.gid, path, order)?;
        }
    }
    path.pop();
    order.push(gid);
    Ok(())
}

/// `rec` for its new home: component glyph ids mapped through `remap`, and
/// instructions removed when `strip` (outline unchanged, §5 `Strip`).
pub(crate) fn rewrite(
    rec: &[u8],
    remap: &HashMap<u16, u16>,
    strip: bool,
) -> Result<Vec<u8>, AugmentError> {
    if rec.is_empty() {
        return Ok(Vec::new());
    }
    match components(rec)? {
        Some((parts, end)) => {
            let mut out = rec.to_vec();
            for c in &parts {
                let to = remap
                    .get(&c.gid)
                    .ok_or_else(|| AugmentError::MalformedFace {
                        detail: format!("component glyph {} was not carried", c.gid),
                    })?;
                put(&mut out, c.index_at, to.to_be_bytes());
            }
            if strip {
                for c in &parts {
                    let at = c.index_at.saturating_sub(2);
                    let flags = read_u16(&out, at).unwrap_or(0) & !WE_HAVE_INSTRUCTIONS;
                    put(&mut out, at, flags.to_be_bytes());
                }
                out.truncate(end);
            }
            Ok(out)
        }
        None if strip => strip_simple(rec),
        None => Ok(rec.to_vec()),
    }
}

/// A simple record with `instructionLength` set to 0 and its bytecode removed.
fn strip_simple(rec: &[u8]) -> Result<Vec<u8>, AugmentError> {
    let contours = usize::try_from(read_i16(rec, 0)).unwrap_or(0);
    let len_at = 10 + 2 * contours;
    let len = read_u16(rec, len_at).ok_or_else(|| AugmentError::MalformedFace {
        detail: "a simple glyph record is truncated".into(),
    })?;
    let rest = len_at + 2 + usize::from(len);
    let (Some(head), Some(tail)) = (rec.get(..len_at), rec.get(rest..)) else {
        return Err(AugmentError::MalformedFace {
            detail: "a glyph's instructions run past its record".into(),
        });
    };
    let mut out = head.to_vec();
    out.extend_from_slice(&[0, 0]);
    out.extend_from_slice(tail);
    Ok(out)
}

/// The instruction byte count of `rec` (simple: `instructionLength`;
/// composite: the trailing program after `WE_HAVE_INSTRUCTIONS`).
pub(crate) fn instruction_len(rec: &[u8]) -> usize {
    match components(rec) {
        Ok(Some((_, end))) => read_u16(rec, end).map_or(0, usize::from),
        Ok(None) if !rec.is_empty() => {
            let contours = usize::try_from(read_i16(rec, 0)).unwrap_or(0);
            read_u16(rec, 10 + 2 * contours).map_or(0, usize::from)
        }
        _ => 0,
    }
}

/// `old`'s records (copied verbatim) with `added` appended, and the `loca`
/// offsets extended to match. The first new record starts where the last
/// old one ends, so no old record's length changes; each new record is
/// padded so the next starts 4-byte aligned.
pub(crate) fn append(old: &[u8], old_loca: &[usize], added: &[Vec<u8>]) -> (Vec<u8>, Vec<usize>) {
    let end = old_loca.last().copied().unwrap_or(0).min(old.len());
    let mut glyf = old.get(..end).unwrap_or(old).to_vec();
    let mut loca = old_loca.to_vec();
    for rec in added {
        glyf.extend_from_slice(rec);
        glyf.resize(glyf.len().next_multiple_of(4), 0);
        loca.push(glyf.len());
    }
    (glyf, loca)
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

    fn composite(parts: &[(u16, u16)], instructions: &[u8]) -> Vec<u8> {
        let mut r = vec![0xFF, 0xFF, 0, 0, 0, 0, 0, 0, 0, 0];
        for (i, &(gid, extra)) in parts.iter().enumerate() {
            let mut flags = extra;
            if i + 1 < parts.len() {
                flags |= MORE_COMPONENTS;
            } else if !instructions.is_empty() {
                flags |= WE_HAVE_INSTRUCTIONS;
            }
            r.extend_from_slice(&flags.to_be_bytes());
            r.extend_from_slice(&gid.to_be_bytes());
            let args = if flags & ARG_1_AND_2_ARE_WORDS != 0 {
                4
            } else {
                2
            };
            let tf = if flags & WE_HAVE_A_TWO_BY_TWO != 0 {
                8
            } else {
                0
            };
            r.extend(std::iter::repeat_n(7u8, args + tf));
        }
        if !instructions.is_empty() {
            r.extend_from_slice(&u16::try_from(instructions.len()).unwrap().to_be_bytes());
            r.extend_from_slice(instructions);
        }
        r
    }

    #[test]
    fn components_honour_every_argument_and_transform_size() {
        let rec = composite(
            &[(3, ARG_1_AND_2_ARE_WORDS), (4, WE_HAVE_A_TWO_BY_TWO)],
            &[],
        );
        let (parts, end) = components(&rec).unwrap().unwrap();
        assert_eq!(parts.iter().map(|c| c.gid).collect::<Vec<_>>(), [3, 4]);
        assert_eq!(end, rec.len());
    }

    #[test]
    fn rewrite_remaps_components_and_strips_composite_instructions() {
        let rec = composite(&[(3, 0), (4, 0)], &[0xB0, 0x01]);
        let remap = HashMap::from([(3, 9), (4, 10)]);
        let kept = rewrite(&rec, &remap, false).unwrap();
        assert_eq!(instruction_len(&kept), 2);
        let stripped = rewrite(&rec, &remap, true).unwrap();
        let (parts, end) = components(&stripped).unwrap().unwrap();
        assert_eq!(parts.iter().map(|c| c.gid).collect::<Vec<_>>(), [9, 10]);
        assert_eq!(end, stripped.len());
        assert_eq!(
            read_u16(&stripped, parts[1].index_at - 2).unwrap() & WE_HAVE_INSTRUCTIONS,
            0
        );
    }

    #[test]
    fn a_cycle_is_refused() {
        let a = composite(&[(1, 0)], &[]);
        let mut glyf = a.clone();
        glyf.extend_from_slice(&composite(&[(0, 0)], &[]));
        let loca = vec![0, a.len(), glyf.len()];
        assert!(matches!(
            closure(&glyf, &loca, &[0]),
            Err(AugmentError::MalformedFace { .. })
        ));
    }

    #[test]
    fn loca_turns_long_only_past_the_short_limit() {
        assert!(!write_loca(&[0, 0x1FFFE], false).1);
        let (bytes, long) = write_loca(&[0, 0x20000], false);
        assert!(long);
        assert_eq!(read_loca(&bytes, true, 1).unwrap(), [0, 0x20000]);
    }

    #[test]
    fn append_keeps_old_bytes_and_aligns_new_records() {
        let old = [1u8, 2, 3, 4, 5, 6, 0, 0];
        let (glyf, loca) = append(&old, &[0, 6], &[vec![9; 5], vec![8; 4]]);
        assert_eq!(&glyf[..6], &old[..6]);
        assert_eq!(loca, [0, 6, 12, 16], "glyph 0 keeps its length");
    }
}
