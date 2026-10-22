//! Emptying CFF charstrings (Adobe TN5176, TN5177): every charstring
//! outside the kept set becomes a bare `endchar`, and every glyph ID,
//! subroutine, charset, encoding and Private DICT is kept byte for byte.
//!
//! Only three structures are rewritten: the CharStrings INDEX, the Top DICT
//! INDEX and, for a CID-keyed font, the Font DICT INDEX (FDArray). The two
//! DICT INDEXes are rewritten because they carry absolute offsets, which
//! are re-encoded as 5-byte integers so a DICT's size does not depend on
//! the offsets it holds (TN5176 §4 permits any integer form). Every other
//! byte is copied in place, and each offset is moved by the change in size
//! of the rewritten structures before it.

use std::collections::BTreeSet;
use std::ops::Range;

use pdfcer_core::optimize::resubset::ResubsetRefusal;

use super::cff_glyphs;
use super::malformed;
use super::zero::Cut;
use crate::font::sfnt::{Directory, assemble, read_u16};
use crate::font::type1_cff::index;

/// Top DICT `charset` (TN5176 Table 9); 0 to 2 name predefined charsets.
const CHARSET: u16 = 15;
/// Top DICT `Encoding`; 0 and 1 name predefined encodings.
const ENCODING: u16 = 16;
const CHARSTRINGS: u16 = 17;
/// `Private`: size, then offset.
const PRIVATE: u16 = 18;
/// Private DICT `Subrs`, an offset from the Private DICT's start.
const SUBRS: u16 = 19;
const CHARSTRING_TYPE: u16 = 0x0C06;
const FDARRAY: u16 = 0x0C24;
const FDSELECT: u16 = 0x0C25;

/// A charstring holding only `endchar` (TN5177 §4.1, operator 14).
const ENDCHAR: [u8; 1] = [14];

/// One DICT entry: its operator and its operands' spans in the DICT.
struct Entry {
    op: u16,
    operands: Vec<Range<usize>>,
}

/// An INDEX in a CFF: where it lies, and each object's span, absolute.
struct Index {
    span: Range<usize>,
    items: Vec<Range<usize>>,
}

/// The structures of a CFF the cut reads or rewrites.
struct Layout<'a> {
    data: &'a [u8],
    top_index: Index,
    top: Vec<Entry>,
    charstrings: Index,
    fdarray: Option<(Index, Vec<Vec<Entry>>)>,
    /// Each Private DICT's span and its local Subrs INDEX's offset.
    privates: Vec<PrivateSpan>,
}

/// `program`, an sfnt with a `CFF ` table or a bare CFF, with every
/// charstring outside `roots` and the `seac` parts of kept glyphs emptied,
/// verified before it is returned.
pub(super) fn zero(program: &[u8], roots: &BTreeSet<u16>) -> Result<Cut, ResubsetRefusal> {
    if !program.starts_with(b"OTTO") {
        return zero_bare(program, roots);
    }
    let dir = Directory::parse(program).ok_or_else(malformed)?;
    let table = dir.table(*b"CFF ").ok_or_else(malformed)?;
    let cut = zero_bare(table, roots)?;
    let tables = dir
        .tables
        .iter()
        .filter(|(tag, _)| tag != b"DSIG")
        .map(|(tag, data)| match tag {
            b"CFF " => (*tag, cut.program.clone()),
            _ => (*tag, data.to_vec()),
        })
        .collect();
    let out = assemble(dir.flavor, tables);
    if crate::font::program::FontProgram::parse(&out).is_err() {
        return Err(malformed());
    }
    Ok(Cut {
        program: out,
        ..cut
    })
}

/// A bare CFF cut.
fn zero_bare(cff: &[u8], roots: &BTreeSet<u16>) -> Result<Cut, ResubsetRefusal> {
    let layout = Layout::read(cff).ok_or_else(malformed)?;
    let total = layout.charstrings.items.len();
    let keep = cff_glyphs::closure(cff, roots, total)?;
    let charstrings: Vec<&[u8]> = (0..total)
        .map(|gid| {
            let kept = u16::try_from(gid).is_ok_and(|g| keep.contains(&g));
            match layout.charstrings.items.get(gid) {
                Some(span) if kept => cff.get(span.clone()),
                _ => Some(ENDCHAR.as_slice()),
            }
        })
        .collect::<Option<_>>()
        .ok_or_else(malformed)?;
    let out = layout
        .rewrite(&index(&charstrings))
        .ok_or_else(layout_refusal)?;
    cff_glyphs::verify(cff, &out, &keep)?;
    Ok(Cut {
        program: out,
        kept: u32::try_from(keep.len()).unwrap_or(u32::MAX),
        total: u32::try_from(total).unwrap_or(u32::MAX),
        keep,
    })
}

fn layout_refusal() -> ResubsetRefusal {
    ResubsetRefusal::new(
        "cff-layout",
        "the CFF program's structures are laid out in a way the cut cannot move safely",
    )
}

impl<'a> Layout<'a> {
    /// The layout of a single-font CFF with Type 2 charstrings; `None` for
    /// anything else or anything malformed.
    fn read(data: &'a [u8]) -> Option<Self> {
        let header = usize::from(*data.get(2)?);
        let names = read_index(data, header)?;
        let top_index = read_index(data, names.span.end)?;
        let ([_], [top]) = (names.items.as_slice(), top_index.items.as_slice()) else {
            return None;
        };
        let top = entries(data.get(top.clone())?)?;
        if offset(data, &top, top_span(&top_index), CHARSTRING_TYPE).is_some_and(|t| t != 2) {
            return None;
        }
        let strings = read_index(data, top_index.span.end)?;
        read_index(data, strings.span.end)?;
        let at = |op| offset(data, &top, top_span(&top_index), op);
        let charstrings = read_index(data, at(CHARSTRINGS)?)?;
        let mut privates = Vec::new();
        privates.extend(private(data, &top, top_span(&top_index))?);
        let fdarray = match at(FDARRAY) {
            None => None,
            Some(start) => {
                let fds = read_index(data, start)?;
                let mut dicts = Vec::new();
                for item in &fds.items {
                    let dict = entries(data.get(item.clone())?)?;
                    privates.extend(private(data, &dict, item.clone())?);
                    dicts.push(dict);
                }
                Some((fds, dicts))
            }
        };
        Some(Self {
            data,
            top_index,
            top,
            charstrings,
            fdarray,
            privates,
        })
    }

    /// The CFF with `charstrings` as its CharStrings INDEX and every offset
    /// moved to match; `None` when an offset points inside a rewritten
    /// structure or a Private DICT and its Subrs straddle one.
    fn rewrite(&self, charstrings: &[u8]) -> Option<Vec<u8>> {
        let mut replaced = vec![
            (
                self.top_index.span.clone(),
                index(&[&self.top_dict(|_| Some(0))?]).len(),
            ),
            (self.charstrings.span.clone(), charstrings.len()),
        ];
        if let Some((fds, dicts)) = &self.fdarray {
            let len = index(&refs(&self.font_dicts(dicts, |_| Some(0))?)).len();
            replaced.push((fds.span.clone(), len));
        }
        replaced.sort_by_key(|(r, _)| r.start);
        if pairs(&replaced).any(|(a, b)| a.0.end > b.0.start) || self.straddles(&replaced) {
            return None;
        }
        let moved = |o: usize| moved(o, &replaced);
        let mut spans = vec![
            (
                self.top_index.span.clone(),
                index(&[&self.top_dict(moved)?]),
            ),
            (self.charstrings.span.clone(), charstrings.to_vec()),
        ];
        if let Some((fds, dicts)) = &self.fdarray {
            spans.push((
                fds.span.clone(),
                index(&refs(&self.font_dicts(dicts, moved)?)),
            ));
        }
        spans.sort_by_key(|(r, _)| r.start);
        let mut out = Vec::with_capacity(self.data.len());
        let mut cursor = 0;
        for (span, bytes) in spans {
            out.extend_from_slice(self.data.get(cursor..span.start)?);
            out.extend_from_slice(&bytes);
            cursor = span.end;
        }
        out.extend_from_slice(self.data.get(cursor..)?);
        Some(out)
    }

    /// Whether a rewritten structure overlaps a Private DICT or lies
    /// between it and its Subrs INDEX, whose distance the cut would change.
    fn straddles(&self, replaced: &[(Range<usize>, usize)]) -> bool {
        let shift = |o: usize| {
            let n = i64::try_from(moved(o, replaced)?).ok()?;
            Some(n - i64::try_from(o).ok()?)
        };
        self.privates.iter().any(|(span, subrs)| {
            let overlaps = replaced
                .iter()
                .any(|(r, _)| r.start < span.end && span.start < r.end);
            let at = shift(span.start);
            overlaps || at.is_none() || subrs.is_some_and(|s| shift(s) != at)
        })
    }

    /// The Top DICT with its offsets passed through `moved`.
    fn top_dict(&self, moved: impl Fn(usize) -> Option<usize>) -> Option<Vec<u8>> {
        let span = top_span(&self.top_index);
        write_dict(self.data.get(span)?, &self.top, &moved)
    }

    /// Each Font DICT with its Private offset passed through `moved`.
    fn font_dicts(
        &self,
        dicts: &[Vec<Entry>],
        moved: impl Fn(usize) -> Option<usize>,
    ) -> Option<Vec<Vec<u8>>> {
        let (fds, _) = self.fdarray.as_ref()?;
        fds.items
            .iter()
            .zip(dicts)
            .map(|(span, dict)| write_dict(self.data.get(span.clone())?, dict, &moved))
            .collect::<Option<Vec<_>>>()
    }
}

/// The Top DICT's span in the CFF.
fn top_span(top_index: &Index) -> Range<usize> {
    top_index.items.first().cloned().unwrap_or(0..0)
}

/// `o` after the rewritten structures before it change size; `None` when
/// `o` points inside one.
fn moved(o: usize, replaced: &[(Range<usize>, usize)]) -> Option<usize> {
    let mut n = i64::try_from(o).ok()?;
    for (r, len) in replaced {
        if r.end <= o {
            n += i64::try_from(*len).ok()? - i64::try_from(r.len()).ok()?;
        } else if r.start < o {
            return None;
        }
    }
    usize::try_from(n).ok()
}

/// Which operand of `op` is an absolute offset, if any.
fn offset_operand(op: u16, value: i64) -> Option<usize> {
    match op {
        CHARSTRINGS | FDARRAY | FDSELECT => Some(0),
        CHARSET if value > 2 => Some(0),
        ENCODING if value > 1 => Some(0),
        PRIVATE => Some(1),
        _ => None,
    }
}

/// `dict` (spanning `bytes`) re-encoded with every absolute offset passed
/// through `moved` as a 5-byte integer and every other operand copied.
fn write_dict(
    bytes: &[u8],
    dict: &[Entry],
    moved: &impl Fn(usize) -> Option<usize>,
) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(bytes.len() + 16);
    for e in dict {
        let first = e.operands.first().and_then(|r| int(bytes.get(r.clone())?));
        let target = offset_operand(e.op, first.unwrap_or(0));
        for (i, r) in e.operands.iter().enumerate() {
            if Some(i) == target {
                let old = usize::try_from(int(bytes.get(r.clone())?)?).ok()?;
                out.push(29);
                out.extend_from_slice(&i32::try_from(moved(old)?).ok()?.to_be_bytes());
            } else {
                out.extend_from_slice(bytes.get(r.clone())?);
            }
        }
        match u8::try_from(e.op) {
            Ok(op) => out.push(op),
            Err(_) => out.extend_from_slice(&e.op.to_be_bytes()),
        }
    }
    Some(out)
}

/// The integer operand of `op` in `dict` (spanning `span` of `data`),
/// taken as an offset or count; `None` when absent or not an integer.
fn offset(data: &[u8], dict: &[Entry], span: Range<usize>, op: u16) -> Option<usize> {
    let bytes = data.get(span)?;
    let e = dict.iter().find(|e| e.op == op)?;
    let n = int(bytes.get(e.operands.first()?.clone())?)?;
    usize::try_from(n).ok()
}

/// `dict`'s Private DICT span and its Subrs INDEX's offset, absolute;
/// `Some(None)` when the DICT has no Private entry.
fn private(data: &[u8], dict: &[Entry], span: Range<usize>) -> Option<Option<PrivateSpan>> {
    let bytes = data.get(span)?;
    let Some(e) = dict.iter().find(|e| e.op == PRIVATE) else {
        return Some(None);
    };
    let [size, at] = e.operands.as_slice() else {
        return None;
    };
    let size = usize::try_from(int(bytes.get(size.clone())?)?).ok()?;
    let at = usize::try_from(int(bytes.get(at.clone())?)?).ok()?;
    let range = at..at.checked_add(size)?;
    let private = data.get(range.clone())?;
    let subrs = match entries(private)?.iter().find(|e| e.op == SUBRS) {
        None => None,
        Some(e) => {
            let rel = int(private.get(e.operands.first()?.clone())?)?;
            Some(at.checked_add(usize::try_from(rel).ok()?)?)
        }
    };
    Some(Some((range, subrs)))
}

/// A Private DICT's span and its Subrs INDEX's offset.
type PrivateSpan = (Range<usize>, Option<usize>);

/// Each item of `v` with the one after it.
fn pairs<T>(v: &[T]) -> impl Iterator<Item = (&T, &T)> {
    v.iter().zip(v.iter().skip(1))
}

/// Borrowed views of `items`, as [`index`] takes them.
fn refs(items: &[Vec<u8>]) -> Vec<&[u8]> {
    items.iter().map(Vec::as_slice).collect()
}

/// The INDEX at `start` (TN5176 §5); `None` when malformed.
fn read_index(data: &[u8], start: usize) -> Option<Index> {
    let count = usize::from(read_u16(data, start)?);
    if count == 0 {
        return Some(Index {
            span: start..start + 2,
            items: Vec::new(),
        });
    }
    let off_size = usize::from(*data.get(start + 2)?);
    if !(1..=4).contains(&off_size) {
        return None;
    }
    let offsets_at = start + 3;
    let base = offsets_at + (count + 1) * off_size - 1;
    let mut offsets = Vec::with_capacity(count + 1);
    for k in 0..=count {
        let at = offsets_at + k * off_size;
        let bytes = data.get(at..at + off_size)?;
        offsets.push(bytes.iter().fold(0usize, |n, &b| (n << 8) | usize::from(b)));
    }
    if offsets.first() != Some(&1) || pairs(&offsets).any(|(a, b)| b < a) {
        return None;
    }
    let end = base + *offsets.last()?;
    if end > data.len() {
        return None;
    }
    let items = pairs(&offsets).map(|(a, b)| base + a..base + b).collect();
    Some(Index {
        span: start..end,
        items,
    })
}

/// A DICT's entries (TN5176 §4); `None` when malformed or when operands
/// trail the last operator.
fn entries(dict: &[u8]) -> Option<Vec<Entry>> {
    let mut out = Vec::new();
    let mut operands = Vec::new();
    let mut i = 0;
    while let Some(&b0) = dict.get(i) {
        let len = match b0 {
            12 => {
                let op = 0x0C00 | u16::from(*dict.get(i + 1)?);
                out.push(Entry {
                    op,
                    operands: std::mem::take(&mut operands),
                });
                i += 2;
                continue;
            }
            0..=21 => {
                out.push(Entry {
                    op: u16::from(b0),
                    operands: std::mem::take(&mut operands),
                });
                i += 1;
                continue;
            }
            28 => 3,
            29 => 5,
            30 => real_len(dict.get(i..)?)?,
            32..=246 => 1,
            247..=254 => 2,
            _ => return None,
        };
        if i + len > dict.len() {
            return None;
        }
        operands.push(i..i + len);
        i += len;
    }
    operands.is_empty().then_some(out)
}

/// The length of the real operand `bytes` starts with: through the byte
/// holding its end nibble.
fn real_len(bytes: &[u8]) -> Option<usize> {
    bytes
        .iter()
        .skip(1)
        .position(|b| b >> 4 == 0x0F || b & 0x0F == 0x0F)
        .map(|p| p + 2)
}

/// An integer operand's value (TN5176 Table 3); `None` for a real.
fn int(bytes: &[u8]) -> Option<i64> {
    let b = |k: usize| bytes.get(k).copied().map(i64::from);
    match bytes.first()? {
        &b0 @ 32..=246 => Some(i64::from(b0) - 139),
        &b0 @ 247..=250 => Some((i64::from(b0) - 247) * 256 + b(1)? + 108),
        &b0 @ 251..=254 => Some(-(i64::from(b0) - 251) * 256 - b(1)? - 108),
        28 => Some(i64::from(i16::from_be_bytes([
            *bytes.get(1)?,
            *bytes.get(2)?,
        ]))),
        29 => Some(i64::from(i32::from_be_bytes(
            bytes.get(1..5)?.try_into().ok()?,
        ))),
        _ => None,
    }
}

#[cfg(test)]
#[path = "cff_tests.rs"]
mod tests;
