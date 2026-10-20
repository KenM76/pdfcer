//! Byte attribution: object spans, object-stream shares and the gaps between.

use std::collections::{BTreeMap, BTreeSet};

use super::{SpaceCategory, name_of};
use crate::document::Document;
use crate::object::{ObjId, Object, Provenance};
use crate::objstm::ObjectStream;

/// Where the file's bytes went, before classification by role.
#[derive(Debug, Default)]
pub(super) struct Attribution {
    /// Bytes owned by each current object.
    pub per_object: BTreeMap<ObjId, u64>,
    /// Bytes outside every current object, by gap category.
    pub gaps: BTreeMap<SpaceCategory, u64>,
    /// Object-stream containers whose bytes were shared out to members.
    pub distributed: BTreeSet<u32>,
    /// Compressed object → its container's number.
    pub container_of: BTreeMap<ObjId, u32>,
    /// What could not be measured.
    pub notes: Vec<String>,
}

/// Split the file's bytes among objects and gaps, each byte exactly once.
pub(super) fn attribute(doc: &Document) -> Attribution {
    let buf = doc.bytes();
    let mut a = Attribution::default();
    let mut spans: Vec<(usize, usize, ObjId)> = doc
        .objects()
        .filter_map(|o| o.file_span().map(|s| (s.start, s.end(), o.id)))
        .map(|(s, e, id)| (s.min(buf.len()), e.min(buf.len()), id))
        .collect();
    spans.sort_unstable_by_key(|&(s, e, _)| (s, e));

    let mut cursor = 0usize;
    for (start, end, id) in spans {
        let start = start.max(cursor);
        if start > cursor {
            classify_gap(buf, cursor, start, &mut a.gaps);
        }
        let end = end.max(start);
        a.per_object.insert(id, (end - start) as u64);
        cursor = end;
    }
    if cursor < buf.len() {
        classify_gap(buf, cursor, buf.len(), &mut a.gaps);
    }
    share_object_streams(doc, &mut a);
    a
}

/// Spread each object-stream container's bytes across its members.
fn share_object_streams(doc: &Document, a: &mut Attribution) {
    let mut current: BTreeMap<u32, BTreeMap<u32, ObjId>> = BTreeMap::new();
    for obj in doc.objects() {
        if let Provenance::ObjectStream { container, index } = obj.provenance {
            current
                .entry(container.num)
                .or_default()
                .insert(index, obj.id);
            a.container_of.insert(obj.id, container.num);
        }
    }
    let view = doc.view();
    for obj in doc.objects() {
        let Object::Stream(stream) = &obj.value else {
            continue;
        };
        if name_of(&stream.dict, b"Type").as_deref() != Some("ObjStm") {
            continue;
        }
        let Some(&len) = a.per_object.get(&obj.id) else {
            continue;
        };
        let parsed = view
            .slice(stream.data_span)
            .ok_or(())
            .and_then(|raw| ObjectStream::parse(&stream.dict, raw).map_err(|_| ()));
        let Ok(container) = parsed else {
            a.notes.push(format!(
                "Object stream {} could not be decoded; its {len} bytes count as file overhead.",
                obj.id.num
            ));
            continue;
        };
        let members = current.remove(&obj.id.num).unwrap_or_default();
        let shares = proportional(len, &container.member_lengths());
        let numbers: Vec<u32> = container.member_numbers().collect();
        for (index, share) in shares.into_iter().enumerate() {
            let owner = u32::try_from(index)
                .ok()
                .and_then(|i| members.get(&i))
                .filter(|id| numbers.get(index) == Some(&id.num));
            match owner {
                Some(id) => *a.per_object.entry(*id).or_default() += share,
                None => *a.gaps.entry(SpaceCategory::Superseded).or_default() += share,
            }
        }
        a.per_object.remove(&obj.id);
        a.distributed.insert(obj.id.num);
    }
}

/// Split `total` in proportion to `weights`; the remainder goes to the last
/// share, so the shares always sum to `total`. Equal split if every weight
/// is 0. Empty `weights` gives one share holding everything, which lands in
/// [`SpaceCategory::Superseded`] because it has no owner.
fn proportional(total: u64, weights: &[usize]) -> Vec<u64> {
    let n = weights.len().max(1);
    let sum: u128 = weights.iter().map(|&w| w as u128).sum();
    let mut out = Vec::with_capacity(n);
    let mut given = 0u64;
    for i in 0..n.saturating_sub(1) {
        let w = weights.get(i).copied().unwrap_or(0) as u128;
        let share = (u128::from(total) * w)
            .checked_div(sum)
            .map_or(total / n as u64, |s| u64::try_from(s).unwrap_or(0));
        given += share;
        out.push(share);
    }
    out.push(total - given.min(total));
    out
}

/// Attribute `buf[start..end]`, which lies outside every current object.
fn classify_gap(buf: &[u8], start: usize, end: usize, gaps: &mut BTreeMap<SpaceCategory, u64>) {
    let mut pos = start;
    while pos < end {
        let Some(header) = next_obj_header(buf, pos, end) else {
            add(gaps, keyword_category(buf, pos, end), end - pos);
            return;
        };
        if header > pos {
            add(gaps, keyword_category(buf, pos, header), header - pos);
        }
        let body_end = find(buf, b"endobj", header, end).map_or(end, |e| (e + 6).min(end));
        let category = if find(buf, b"/XRef", header, body_end).is_some() {
            SpaceCategory::CrossReference
        } else {
            SpaceCategory::Superseded
        };
        add(gaps, category, body_end - header);
        pos = body_end;
    }
}

fn add(gaps: &mut BTreeMap<SpaceCategory, u64>, category: SpaceCategory, n: usize) {
    *gaps.entry(category).or_default() += n as u64;
}

fn keyword_category(buf: &[u8], start: usize, end: usize) -> SpaceCategory {
    if find(buf, b"xref", start, end).is_some() || find(buf, b"trailer", start, end).is_some() {
        SpaceCategory::CrossReference
    } else {
        SpaceCategory::FileOverhead
    }
}

fn find(buf: &[u8], needle: &[u8], start: usize, end: usize) -> Option<usize> {
    buf.get(start..end)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + start)
}

/// Start of the next `N G obj` header in `buf[from..end]`.
fn next_obj_header(buf: &[u8], from: usize, end: usize) -> Option<usize> {
    let mut at = from;
    while let Some(p) = find(buf, b"obj", at, end) {
        let after = buf.get(p + 3).copied();
        let ends_token = after.is_none_or(|b| !b.is_ascii_alphanumeric());
        if ends_token && let Some(start) = header_start(buf, p, from) {
            return Some(start);
        }
        at = p + 3;
    }
    None
}

/// Walk back from `obj` at `p` over `ws digits ws digits`.
fn header_start(buf: &[u8], p: usize, lo: usize) -> Option<usize> {
    let ws = |b: u8| matches!(b, b' ' | b'\n' | b'\r' | b'\t' | b'\x0c' | 0);
    let digit = |b: u8| b.is_ascii_digit();
    let mut i = p;
    for class in [ws, digit, ws, digit] {
        let before = i;
        while i > lo && buf.get(i - 1).copied().is_some_and(class) {
            i -= 1;
        }
        if i == before {
            return None;
        }
    }
    let prev = if i > lo {
        buf.get(i - 1).copied()
    } else {
        None
    };
    prev.is_none_or(|b| !b.is_ascii_alphanumeric() && b != b'.')
        .then_some(i)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn proportional_shares_sum_to_the_total() {
        assert_eq!(proportional(100, &[1, 1, 2]), [25, 25, 50]);
        assert_eq!(proportional(10, &[1, 1, 1]), [3, 3, 4]);
        assert_eq!(proportional(9, &[0, 0, 0]), [3, 3, 3]);
        assert_eq!(proportional(7, &[]), [7]);
    }

    #[test]
    fn a_gap_splits_into_superseded_and_cross_reference() {
        let buf = b"\n4 0 obj\n<< /A 1 >>\nendobj\nxref\n0 1\ntrailer\n<<>>\n";
        let mut gaps = BTreeMap::new();
        classify_gap(buf, 0, buf.len(), &mut gaps);
        assert_eq!(gaps[&SpaceCategory::FileOverhead], 1);
        assert_eq!(gaps[&SpaceCategory::Superseded], 25);
        let total: u64 = gaps.values().sum();
        assert_eq!(total, buf.len() as u64);
    }

    #[test]
    fn endobj_and_glued_numbers_are_not_headers() {
        assert_eq!(next_obj_header(b"x endobj y", 0, 10), None);
        assert_eq!(next_obj_header(b"a1 0 obj", 0, 8), None);
        assert_eq!(next_obj_header(b" 12 0 obj", 0, 9), Some(1));
    }
}
