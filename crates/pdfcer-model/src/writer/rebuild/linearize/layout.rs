//! Numbering and byte layout of a linearized file (F.3.1), in two
//! arithmetic passes: object offsets are first taken without the hint
//! stream, the hint tables are built from them (F.4 counts offsets as if
//! the hint stream were absent), and everything at or after the hint
//! stream then moves by its length. Every value in the linearization
//! dictionary and the first-page trailer is padded to a fixed width, so
//! neither part changes size between the passes.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::Graph;
use super::classify::Parts;
use super::hint::{self, Spans, Tables};
use crate::PdfVersion;
use crate::filters::flate;
use crate::object::{ObjId, Object};
use crate::writer::rebuild::RebuildReport;
use crate::writer::rebuild::graph::map_references;
use crate::writer::{IdentityEncoder, serialize};

/// The input trailer entries the output keeps.
pub(super) struct TrailerKeys<'a> {
    pub root: ObjId,
    pub info: Option<ObjId>,
    pub id: Option<&'a Object>,
}

/// Width of every padded number.
const PAD: usize = 20;

struct Numbers {
    new: BTreeMap<ObjId, u32>,
    /// Old ids in file order, after the hint stream.
    second: Vec<ObjId>,
    lin: u32,
    hint: u32,
    last: u32,
}

/// Group 2 (parts 7-9) takes 1..k-1; then the linearization dictionary,
/// part 4, the hint stream and part 6 take k..m (F.3.1, as qpdf numbers).
fn number(parts: &Parts) -> Numbers {
    let mut new = BTreeMap::new();
    let mut next = 1u32;
    let mut assign = |ids: &mut dyn Iterator<Item = &ObjId>, next: &mut u32| {
        for &id in ids {
            new.insert(id, *next);
            *next += 1;
        }
    };
    let group2: Vec<ObjId> = parts
        .part7
        .iter()
        .flatten()
        .chain(&parts.part8)
        .chain(&parts.part9)
        .copied()
        .collect();
    assign(&mut group2.iter(), &mut next);
    let lin = next;
    next += 1;
    assign(&mut parts.part4.iter(), &mut next);
    let hint = next;
    next += 1;
    assign(&mut parts.part6.iter(), &mut next);
    let mut second = parts.part6.clone();
    second.extend(group2);
    Numbers {
        new,
        second,
        lin,
        hint,
        last: next - 1,
    }
}

/// The linearized file and its object count.
pub(super) fn write(
    graph: &Graph<'_>,
    parts: &Parts,
    trailer: &TrailerKeys<'_>,
    version: PdfVersion,
    source: &[u8],
    report: &mut RebuildReport,
) -> (Vec<u8>, usize) {
    let nums = number(parts);
    let id = |old: ObjId| nums.new.get(&old).copied();
    let body = serialize_all(graph, &nums, source, report);
    let mut header = format!("%PDF-{version}\n").into_bytes();
    // §7.5.2: a comment of four bytes >= 128 marks the file as binary.
    header.extend_from_slice(b"%\xE2\xE3\xCF\xD3\n");
    let fixed = Fixed {
        nums: &nums,
        trailer,
        id: &id,
        pages: parts.part7.len() + 1,
        first_page: parts.part6.first().and_then(|&p| id(p)).unwrap_or(0),
    };
    let lin_len = fixed.lin_dict(&[0; 6]).len() as u64;
    let part3_len = fixed.part3(&BTreeMap::new(), 0).len() as u64;

    let first_half: Vec<u32> = parts.part4.iter().filter_map(|&o| id(o)).collect();
    let second: Vec<u32> = nums.second.iter().filter_map(|&o| id(o)).collect();
    let start = header.len() as u64 + lin_len + part3_len;
    let (spans, hint_at, end_pass1) = pass1(&body, start, &first_half, &second);

    let hint_obj = hint_object(nums.hint, &tables(parts, &id), &spans);
    let h = hint_obj.len() as u64;
    let real = |offset: u64| {
        if offset >= hint_at {
            offset + h
        } else {
            offset
        }
    };
    let mut offsets: BTreeMap<u32, u64> = spans.iter().map(|(&n, &(o, _))| (n, real(o))).collect();
    offsets.insert(nums.lin, header.len() as u64);
    offsets.insert(nums.hint, hint_at);

    let main_at = end_pass1 + h;
    let part11 = fixed.part11(&offsets, header.len() as u64 + lin_len);
    let total = main_at + part11.len() as u64;
    let part6_len: u64 = parts
        .part6
        .iter()
        .filter_map(|&o| id(o))
        .filter_map(|n| spans.get(&n))
        .map(|s| s.1)
        .sum();
    let t = main_at + format!("xref\n0 {}", nums.lin).len() as u64;
    let values = [total, hint_at, h, hint_at + h + part6_len, t, main_at];

    let mut out = header;
    out.extend(fixed.lin_dict(&values));
    out.extend(fixed.part3(&offsets, main_at));
    for n in first_half {
        out.extend(body.get(&n).into_iter().flatten());
    }
    out.extend(hint_obj);
    for n in second {
        out.extend(body.get(&n).into_iter().flatten());
    }
    out.extend(part11);
    (out, nums.last as usize)
}

/// Every object's definition under its new number, references renumbered
/// and any dangling one written as `null`.
fn serialize_all(
    graph: &Graph<'_>,
    nums: &Numbers,
    source: &[u8],
    report: &mut RebuildReport,
) -> BTreeMap<u32, Vec<u8>> {
    let mut body = BTreeMap::new();
    for (&old, &num) in &nums.new {
        let Some(value) = graph.value(old) else {
            continue;
        };
        let new = |r| graph.target(r).and_then(|t| nums.new.get(&t).copied());
        let mapped = map_references(value, &mut |r| match new(r) {
            Some(n) => Object::Reference(ObjId::new(n, 0)),
            None => {
                report.dangling += 1;
                Object::Null
            }
        });
        let mut bytes = Vec::new();
        let id = ObjId::new(num, 0);
        serialize::write_indirect(&mut bytes, id, &mapped, source, &IdentityEncoder);
        body.insert(num, bytes);
    }
    body
}

/// Offsets and lengths without the hint stream, starting at `start`; also
/// where the hint stream goes and where the body ends.
fn pass1(
    body: &BTreeMap<u32, Vec<u8>>,
    start: u64,
    first_half: &[u32],
    second: &[u32],
) -> (Spans, u64, u64) {
    let mut spans = Spans::new();
    let mut at = start;
    let mut place = |nums: &[u32], at: &mut u64| {
        for n in nums {
            let len = body.get(n).map_or(0, |b| b.len() as u64);
            spans.insert(*n, (*at, len));
            *at += len;
        }
    };
    place(first_half, &mut at);
    let hint_at = at;
    place(second, &mut at);
    (spans, hint_at, at)
}

fn tables(parts: &Parts, id: &dyn Fn(ObjId) -> Option<u32>) -> Tables {
    let nums = |ids: &[ObjId]| ids.iter().filter_map(|&o| id(o)).collect::<Vec<u32>>();
    let shared: Vec<u32> = nums(&parts.part6)
        .into_iter()
        .chain(nums(&parts.part8))
        .collect();
    let index: BTreeMap<u32, u32> = shared
        .iter()
        .enumerate()
        .filter_map(|(i, &n)| Some((n, u32::try_from(i).ok()?)))
        .collect();
    let mut page_shared = vec![Vec::new()];
    page_shared.extend(parts.shared.iter().map(|s| {
        nums(s)
            .iter()
            .filter_map(|n| index.get(n).copied())
            .collect()
    }));
    let mut pages = vec![nums(&parts.part6)];
    pages.extend(parts.part7.iter().map(|g| nums(g)));
    Tables {
        pages,
        page_shared,
        first_page_shared: parts.part6.len(),
        shared,
        outlines: parts.outlines.and_then(|(o, count)| Some((id(o)?, count))),
    }
}

fn hint_object(num: u32, tables: &Tables, spans: &Spans) -> Vec<u8> {
    let (data, s, o) = hint::encode(tables, spans);
    let data = flate::encode_with_level(&data, 9);
    let mut dict = format!("<< /Filter /FlateDecode /Length {} /S {s}", data.len());
    if let Some(o) = o {
        let _ = write!(dict, " /O {o}");
    }
    let mut out = format!("{num} 0 obj\n{dict} >>\nstream\n").into_bytes();
    out.extend(data);
    out.extend_from_slice(b"\nendstream\nendobj\n");
    out
}

struct Fixed<'a> {
    nums: &'a Numbers,
    trailer: &'a TrailerKeys<'a>,
    id: &'a dyn Fn(ObjId) -> Option<u32>,
    pages: usize,
    first_page: u32,
}

impl Fixed<'_> {
    /// `values`: /L, /H offset, /H length, /E, /T, unused.
    fn lin_dict(&self, v: &[u64; 6]) -> Vec<u8> {
        let [l, h0, h1, e, t, _] = *v;
        format!(
            "{} 0 obj\n<< /Linearized 1 /L {l:>PAD$} /H [ {h0:>PAD$} {h1:>PAD$} ] /O {} /E {e:>PAD$} /N {} /T {t:>PAD$} >>\nendobj\n",
            self.nums.lin, self.first_page, self.pages
        )
        .into_bytes()
    }

    /// The first-page cross-reference section and trailer.
    fn part3(&self, offsets: &BTreeMap<u32, u64>, prev: u64) -> Vec<u8> {
        let (lin, last) = (self.nums.lin, self.nums.last);
        let mut out = format!("xref\n{lin} {}\n", last - lin + 1).into_bytes();
        entries(&mut out, offsets, lin..=last);
        let mut trailer = format!(
            "trailer\n<< /Size {} /Root {} 0 R",
            last + 1,
            (self.id)(self.trailer.root).unwrap_or(0)
        );
        if let Some(info) = self.trailer.info.and_then(self.id) {
            let _ = write!(trailer, " /Info {info} 0 R");
        }
        out.extend(trailer.into_bytes());
        if let Some(id) = self.trailer.id {
            out.extend_from_slice(b" /ID ");
            serialize::write_object(&mut out, id, ObjId::new(0, 0), &[], &IdentityEncoder);
        }
        out.extend(format!(" /Prev {prev:>PAD$} >>\nstartxref\n0\n%%EOF\n").into_bytes());
        out
    }

    /// The main cross-reference section and trailer.
    fn part11(&self, offsets: &BTreeMap<u32, u64>, first_xref: u64) -> Vec<u8> {
        let lin = self.nums.lin;
        let mut out = format!("xref\n0 {lin}\n0000000000 65535 f \n").into_bytes();
        entries(&mut out, offsets, 1..=lin.saturating_sub(1));
        let size = self.nums.last + 1;
        out.extend(
            format!("trailer\n<< /Size {size} >>\nstartxref\n{first_xref}\n%%EOF\n").into_bytes(),
        );
        out
    }
}

fn entries(out: &mut Vec<u8>, offsets: &BTreeMap<u32, u64>, range: std::ops::RangeInclusive<u32>) {
    for n in range {
        let offset = offsets.get(&n).copied().unwrap_or(0);
        out.extend(format!("{offset:010} 00000 n \n").into_bytes());
    }
}
