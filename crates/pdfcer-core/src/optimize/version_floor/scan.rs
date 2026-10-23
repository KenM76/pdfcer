//! The two passes: propagate context bits along edges to a fixpoint, then
//! apply the key, value and custom rules to every dictionary.

use std::collections::{BTreeSet, HashMap, VecDeque};

use super::ctx::{self, ENCRYPT, FONT_DESC, SIGNATURE, TRAILER};
use super::{checks, keys};
use crate::document::Document;
use crate::object::{Dict, ObjId, Object};

/// How far direct dictionaries and arrays are followed inside one object.
const MAX_DEPTH: usize = 32;

/// The `(version, construct)` pairs one object needs.
pub(super) type Notes = BTreeSet<(u8, String)>;

/// The context bits every indirect object earns from the edges reaching
/// it, from the trailer down. Bits only grow, so the worklist ends.
pub(super) fn reached(doc: &Document) -> HashMap<ObjId, u64> {
    let mut bits: HashMap<ObjId, u64> = HashMap::new();
    let mut queue: VecDeque<ObjId> = doc.objects().map(|o| o.id).collect();
    let mut found = Vec::new();
    dict_edges(doc.trailer(), TRAILER, 0, &mut found);
    grow(&mut bits, &mut found, &mut queue);
    while let Some(id) = queue.pop_front() {
        let Some(obj) = doc.get(id) else { continue };
        let inherited = bits_of(&bits, id);
        match &obj.value {
            Object::Dict(d) => dict_edges(d, inherited, 0, &mut found),
            Object::Stream(s) => dict_edges(&s.dict, inherited | ctx::STREAM, 0, &mut found),
            _ => {}
        }
        grow(&mut bits, &mut found, &mut queue);
    }
    bits
}

fn grow(
    bits: &mut HashMap<ObjId, u64>,
    found: &mut Vec<(ObjId, u64)>,
    queue: &mut VecDeque<ObjId>,
) {
    for (id, b) in found.drain(..) {
        let e = bits.entry(id).or_insert(0);
        if *e | b != *e {
            *e |= b;
            queue.push_back(id);
        }
    }
}

fn bits_of(bits: &HashMap<ObjId, u64>, id: ObjId) -> u64 {
    bits.get(&id).copied().unwrap_or(0)
}

fn dict_edges(d: &Dict, inherited: u64, depth: usize, out: &mut Vec<(ObjId, u64)>) {
    let bits = ctx::own(d, inherited & ctx::STREAM != 0) | inherited;
    for (k, v) in d.iter() {
        child_edges(v, ctx::edge(bits, k.as_bytes()), depth, out);
    }
}

fn child_edges(v: &Object, bits: u64, depth: usize, out: &mut Vec<(ObjId, u64)>) {
    if depth > MAX_DEPTH {
        return;
    }
    match v {
        Object::Reference(r) if bits != 0 => out.push((*r, bits)),
        Object::Dict(d) => dict_edges(d, bits & !ctx::STREAM, depth + 1, out),
        Object::Array(a) => {
            for (i, e) in a.iter().enumerate() {
                child_edges(e, bits | ctx::colour_space_element(a, i), depth + 1, out);
            }
        }
        _ => {}
    }
}

/// Streams the content pass decodes, found while scanning.
#[derive(Default)]
pub(super) struct Jobs {
    /// Font programs (`/FontFile2`, `/FontFile3`) to sniff.
    pub fonts: Vec<ObjId>,
}

/// Apply every rule to dictionary `d` (context `inherited` plus its own
/// entries) and to the direct dictionaries and arrays inside it.
pub(super) fn scan_dict(
    d: &Dict,
    inherited: u64,
    depth: usize,
    notes: &mut Notes,
    jobs: &mut Jobs,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let raw = ctx::own(d, inherited & ctx::STREAM != 0) | inherited;
    let bits = ctx::settled(raw);
    checks::custom(d, bits, notes);
    for (k, v) in d.iter() {
        let key = k.as_bytes();
        if let Some(ver) = keys::key(key).and_then(|r| keys::applies(r, bits)) {
            notes.insert((ver, format!("/{}", lossy(key))));
        }
        if bits & FONT_DESC != 0 && matches!(key, b"FontFile2" | b"FontFile3") {
            jobs.fonts.extend(v.as_reference());
        }
        let binary = bits & (ENCRYPT | SIGNATURE | TRAILER) != 0;
        scan_value(
            key,
            v,
            (bits, ctx::edge(raw, key)),
            binary,
            depth,
            notes,
            jobs,
        );
    }
}

fn scan_value(
    key: &[u8],
    v: &Object,
    (bits, child): (u64, u64),
    binary: bool,
    depth: usize,
    notes: &mut Notes,
    jobs: &mut Jobs,
) {
    match v {
        Object::Name(n) => {
            if let Some(ver) = keys::value(key, n.as_bytes(), bits) {
                notes.insert((ver, format!("/{} /{}", lossy(key), lossy(n.as_bytes()))));
            }
        }
        Object::String(s) if !binary && s.starts_with(&[0xEF, 0xBB, 0xBF]) => {
            notes.insert((20, "UTF-8 text string".to_owned()));
        }
        Object::Dict(d) => scan_dict(d, child & !ctx::STREAM, depth + 1, notes, jobs),
        Object::Array(a) => {
            for (i, e) in a.iter().enumerate() {
                let elem = child | ctx::colour_space_element(a, i);
                match e {
                    Object::Name(n) if key == b"Filter" => {
                        if let Some(ver) = keys::filter(n.as_bytes()) {
                            notes.insert((ver, format!("/Filter /{}", lossy(n.as_bytes()))));
                        }
                    }
                    _ => scan_value(key, e, (bits, elem), binary, depth + 1, notes, jobs),
                }
            }
        }
        _ => {}
    }
}

pub(super) fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}
