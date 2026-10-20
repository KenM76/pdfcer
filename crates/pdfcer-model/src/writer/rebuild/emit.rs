//! Byte output of a rebuild: a classic body and table, or object streams
//! plus a cross-reference stream.

use std::collections::BTreeMap;

use super::graph::{Renumbering, map_references};
use super::{RebuildOptions, RebuildReport};
use crate::PdfVersion;
use crate::document::Document;
use crate::filters::flate;
use crate::object::{Dict, Name, ObjId, Object};
use crate::writer::xref_out::{self, Widths};
use crate::writer::{IdentityEncoder, SaveOptions, TrailingEol, WriteError, serialize};
use crate::xref::XrefEntry;

/// Objects per object stream. Large enough that the per-stream overhead is
/// negligible, small enough that a reader decompressing one stream to
/// reach one object does not decompress the whole file.
const OBJECTS_PER_STREAM: usize = 100;

const PDF_1_5: PdfVersion = PdfVersion { major: 1, minor: 5 };

pub(super) fn write(
    doc: &Document,
    live: &BTreeMap<ObjId, &Object>,
    renum: &Renumbering,
    ropts: &RebuildOptions,
    opts: &SaveOptions,
    report: &mut RebuildReport,
) -> Result<Vec<u8>, WriteError> {
    let mut version = doc.version();
    if ropts.object_streams && version < PDF_1_5 {
        version = PDF_1_5;
        report.version_raised = Some(version);
    }
    let mut out = format!("%PDF-{version}\n").into_bytes();
    // §7.5.2: a comment of four bytes >= 128 marks the file as binary.
    out.extend_from_slice(b"%\xE2\xE3\xCF\xD3\n");

    let objects = remapped_objects(live, renum, report);
    let trailer = new_trailer(doc.trailer(), renum, report);
    if ropts.object_streams {
        let packing = Packing {
            kept: renum.len(),
            level: ropts.level,
            source: doc.bytes(),
            trailing: opts.trailing_eol,
        };
        write_packed(&mut out, &objects, trailer, &packing, report)?;
    } else {
        write_classic(&mut out, &objects, trailer, doc.bytes(), opts)?;
        report.objects_out = objects.len();
    }
    Ok(out)
}

/// Each kept object, in new-number order, with its references renumbered
/// and any dangling one written as `null`.
fn remapped_objects(
    live: &BTreeMap<ObjId, &Object>,
    renum: &Renumbering,
    report: &mut RebuildReport,
) -> Vec<(ObjId, Object)> {
    let mut dangling = 0usize;
    let objects = renum
        .kept()
        .filter_map(|(old, new)| {
            let value = live.get(&old)?;
            let mapped = map_references(value, &mut |r| match renum.get(r) {
                Some(id) => Object::Reference(id),
                None => {
                    dangling += 1;
                    Object::Null
                }
            });
            Some((new, mapped))
        })
        .collect();
    report.dangling += dangling;
    objects
}

/// `/Root`, `/Info` and `/ID` of the input trailer, renumbered. `/Size` is
/// added by the caller once the last number is known.
fn new_trailer(old: &Dict, renum: &Renumbering, report: &mut RebuildReport) -> Dict {
    let mut trailer = Dict::new();
    for key in [b"Root".as_slice(), b"Info"] {
        if let Some(Object::Reference(id)) = old.get(key) {
            match renum.get(*id) {
                Some(new) => trailer.insert(Name::from(key), Object::Reference(new)),
                None => report.dangling += 1,
            }
        }
    }
    if let Some(id) = old.get(b"ID") {
        trailer.insert(Name::from(b"ID"), id.clone());
    }
    trailer
}

fn size_object(size: u32) -> Object {
    Object::Integer(i64::from(size))
}

fn offset(out: &[u8]) -> u64 {
    u64::try_from(out.len()).unwrap_or(u64::MAX)
}

fn write_classic(
    out: &mut Vec<u8>,
    objects: &[(ObjId, Object)],
    mut trailer: Dict,
    source: &[u8],
    opts: &SaveOptions,
) -> Result<(), WriteError> {
    let mut entries = BTreeMap::new();
    entries.insert(0, free_head());
    for (id, value) in objects {
        entries.insert(id.num, in_use(offset(out)));
        serialize::write_indirect(out, *id, value, source, &IdentityEncoder);
    }
    let size = u32::try_from(objects.len())
        .unwrap_or(u32::MAX)
        .saturating_add(1);
    trailer.insert(Name::from(b"Size"), size_object(size));
    let xref_at = offset(out);
    xref_out::write_classic_table(out, &entries, opts.xref_entry_eol)?;
    xref_out::write_classic_tail(out, &trailer, xref_at, opts.trailing_eol);
    Ok(())
}

const fn free_head() -> XrefEntry {
    XrefEntry::Free {
        next_free: 0,
        generation: 65535,
    }
}

const fn in_use(offset: u64) -> XrefEntry {
    XrefEntry::InUse {
        offset,
        generation: 0,
    }
}

/// §7.5.7: streams cannot be compressed, and an object that is only a
/// reference must not be.
const fn stays_top_level(value: &Object) -> bool {
    matches!(value, Object::Stream(_) | Object::Reference(_))
}

struct Packing<'a> {
    /// Objects are numbered 1..=kept; containers and the xref stream follow.
    kept: u32,
    level: u8,
    source: &'a [u8],
    trailing: TrailingEol,
}

fn write_packed(
    out: &mut Vec<u8>,
    objects: &[(ObjId, Object)],
    mut trailer: Dict,
    packing: &Packing<'_>,
    report: &mut RebuildReport,
) -> Result<(), WriteError> {
    let Packing {
        kept,
        level,
        source,
        trailing,
    } = *packing;
    let mut entries = BTreeMap::new();
    entries.insert(0, free_head());
    let mut packed: Vec<&(ObjId, Object)> = Vec::new();
    for item in objects {
        if stays_top_level(&item.1) {
            entries.insert(item.0.num, in_use(offset(out)));
            serialize::write_indirect(out, item.0, &item.1, source, &IdentityEncoder);
        } else {
            packed.push(item);
        }
    }
    let mut next = kept.saturating_add(1);
    for chunk in packed.chunks(OBJECTS_PER_STREAM) {
        let container = ObjId::new(next, 0);
        next = next.saturating_add(1);
        for (index, (id, _)) in chunk.iter().enumerate() {
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            entries.insert(
                id.num,
                XrefEntry::InStream {
                    stream_num: container.num,
                    index,
                },
            );
        }
        entries.insert(container.num, in_use(offset(out)));
        write_object_stream(out, container, chunk, level);
        report.object_streams += 1;
    }
    let xref_id = ObjId::new(next, 0);
    let xref_at = offset(out);
    entries.insert(xref_id.num, in_use(xref_at));
    trailer.insert(Name::from(b"Size"), size_object(next.saturating_add(1)));
    let widths = Widths::fit(&entries, [1, 1, 1]);
    let stream = xref_out::build_xref_stream(xref_id, &entries, widths, &trailer)?;
    out.extend_from_slice(&stream.bytes);
    xref_out::write_stream_tail(out, xref_at, trailing);
    report.objects_out = usize::try_from(next).unwrap_or(usize::MAX);
    Ok(())
}

/// One §7.5.7 object stream: `N` pairs of `number offset`, then the
/// objects, offsets counted from `/First`.
fn write_object_stream(out: &mut Vec<u8>, id: ObjId, chunk: &[&(ObjId, Object)], level: u8) {
    let mut header = Vec::new();
    let mut body = Vec::new();
    for (obj_id, value) in chunk {
        if !header.is_empty() {
            header.push(b' ');
        }
        header.extend_from_slice(format!("{} {}", obj_id.num, body.len()).as_bytes());
        // The source buffer is not consulted: nothing packed is a stream.
        serialize::write_object(&mut body, value, *obj_id, &[], &IdentityEncoder);
        body.push(b'\n');
    }
    header.push(b'\n');
    let first = header.len();
    header.extend_from_slice(&body);
    let encoded = flate::encode_with_level(&header, u32::from(level));

    let mut dict = Dict::new();
    dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"ObjStm")));
    dict.insert(Name::from(b"N"), int(chunk.len()));
    dict.insert(Name::from(b"First"), int(first));
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"FlateDecode")),
    );
    dict.insert(Name::from(b"Length"), int(encoded.len()));
    out.extend_from_slice(format!("{} 0 obj\n", id.num).as_bytes());
    serialize::write_object(out, &Object::Dict(dict), id, &[], &IdentityEncoder);
    out.extend_from_slice(b"\nstream\n");
    out.extend_from_slice(&encoded);
    out.extend_from_slice(b"\nendstream\nendobj\n");
}

fn int(v: usize) -> Object {
    Object::Integer(i64::try_from(v).unwrap_or(i64::MAX))
}
