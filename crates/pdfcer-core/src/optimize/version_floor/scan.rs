//! The two passes: propagate context bits along edges to a fixpoint, then
//! apply the key, value and custom rules to every dictionary.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};

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
            array @ Object::Array(_) => child_edges(array, inherited, 0, &mut found),
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

/// The document being scanned, what the scan has resolved, and the streams
/// the content pass decodes.
pub(super) struct Walk<'a> {
    doc: &'a Document,
    /// Font programs (`/FontFile2`, `/FontFile3`) to sniff.
    pub fonts: Vec<ObjId>,
    /// References resolved for the current object, under the key naming
    /// them: the cycle guard, cleared by [`Walk::next_object`].
    seen: HashSet<(ObjId, Vec<u8>)>,
    /// What a shared indirect `/Filter` needs, read once.
    filters: HashMap<ObjId, Notes>,
}

impl<'a> Walk<'a> {
    pub(super) fn new(doc: &'a Document) -> Self {
        Walk {
            doc,
            fonts: Vec::new(),
            seen: HashSet::new(),
            filters: HashMap::new(),
        }
    }

    /// Start crediting a new object: a value it references counts for it
    /// even if an earlier object referenced the same value.
    pub(super) fn next_object(&mut self) {
        self.seen.clear();
    }
}

/// Apply every rule to dictionary `d` (context `inherited` plus its own
/// entries) and to the direct dictionaries and arrays inside it.
pub(super) fn scan_dict(
    d: &Dict,
    inherited: u64,
    depth: usize,
    notes: &mut Notes,
    walk: &mut Walk<'_>,
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
            walk.fonts.extend(v.as_reference());
        }
        let binary = bits & (ENCRYPT | TRAILER) != 0
            || (bits & SIGNATURE != 0 && matches!(key, b"Contents" | b"Cert"));
        scan_value(
            key,
            v,
            (bits, ctx::edge(raw, key)),
            binary,
            depth,
            notes,
            walk,
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
    walk: &mut Walk<'_>,
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
        Object::Dict(d) => scan_dict(d, child & !ctx::STREAM, depth + 1, notes, walk),
        Object::Reference(r) => {
            referenced(key, *r, (bits, child), binary, depth + 1, notes, walk);
        }
        Object::Array(a) => {
            for (i, e) in a.iter().enumerate() {
                let elem = child | ctx::colour_space_element(a, i);
                match e {
                    Object::Name(n) if key == b"Filter" => {
                        if let Some(ver) = keys::filter(n.as_bytes()) {
                            notes.insert((ver, format!("/Filter /{}", lossy(n.as_bytes()))));
                        }
                    }
                    _ => scan_value(key, e, (bits, elem), binary, depth + 1, notes, walk),
                }
            }
        }
        _ => {}
    }
}

/// A value given by reference, judged under the key that names it. A
/// dictionary or stream is skipped: it is scanned as its own object.
fn referenced(
    key: &[u8],
    id: ObjId,
    ctx: (u64, u64),
    binary: bool,
    depth: usize,
    notes: &mut Notes,
    walk: &mut Walk<'_>,
) {
    let doc = walk.doc;
    let Some(obj) = doc.get(id) else { return };
    if depth > MAX_DEPTH
        || matches!(obj.value, Object::Dict(_) | Object::Stream(_))
        || !walk.seen.insert((id, key.to_vec()))
    {
        return;
    }
    if key != b"Filter" {
        scan_value(key, &obj.value, ctx, binary, depth, notes, walk);
        return;
    }
    if !walk.filters.contains_key(&id) {
        let mut found = Notes::new();
        scan_value(key, &obj.value, ctx, binary, depth, &mut found, walk);
        walk.filters.insert(id, found);
    }
    notes.extend(walk.filters.get(&id).into_iter().flatten().cloned());
}

/// `b` as text for a construct name; invalid UTF-8 is replaced, not refused.
pub(super) fn lossy(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}
