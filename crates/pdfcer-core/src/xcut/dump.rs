//! The canonical property dump the gate test compares (decision 228).
//!
//! A [`PropertyDump`] maps a structural path to a [`Canon`] leaf over an
//! object's whole closure. It is reference-blind: an indirect reference is
//! replaced by the path its target is reached at, so a renumbered copy
//! dumps identically. A stream is its dictionary (minus `/Length`, an
//! encoding detail) plus a SHA-256 of its decoded data. A `null` dictionary
//! entry is skipped (§7.3.7: equivalent to absent). Pages, page-tree nodes
//! and the catalog are boundaries: recorded by type, never walked, so a
//! `/P` does not pull the document into an annotation's dump.

use std::collections::{BTreeMap, HashMap};

use sha2::{Digest, Sha256};

use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};

use super::Change;

/// Nesting deeper than this records [`Canon::Deep`] instead of recursing.
const MAX_DEPTH: usize = 64;

/// One leaf of a [`PropertyDump`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Canon {
    Bool(bool),
    /// Integer and real alike: `1` and `1.0` are the same number.
    Num(f64),
    Str(Vec<u8>),
    Name(Vec<u8>),
    Dict(usize),
    Array(usize),
    /// A stream's decoded data, hashed; `None` when it would not decode.
    Data(Option<[u8; 32]>),
    /// A page, page-tree node or catalog, by `/Type`.
    Boundary(Vec<u8>),
    /// A second reference to an object first reached at this path.
    Back(String),
    /// A reference that does not resolve (§7.3.10: null).
    Dangling,
    Deep,
}

/// Path → leaf over an object's closure.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct PropertyDump(pub(crate) BTreeMap<String, Canon>);

impl PropertyDump {
    /// Dump `root` (typically an annotation reference) as read in `graph`.
    pub(crate) fn of<G: ObjectGraph + ?Sized>(graph: &G, root: &Object) -> Self {
        let mut walk = Walk {
            graph,
            seen: HashMap::new(),
            out: BTreeMap::new(),
        };
        walk.value(String::new(), root, 0);
        Self(walk.out)
    }
}

struct Walk<'g, G: ?Sized> {
    graph: &'g G,
    seen: HashMap<ObjId, String>,
    out: BTreeMap<String, Canon>,
}

impl<G: ObjectGraph + ?Sized> Walk<'_, G> {
    fn value(&mut self, path: String, value: &Object, depth: usize) {
        if depth > MAX_DEPTH {
            self.out.insert(path, Canon::Deep);
            return;
        }
        let leaf = match value {
            Object::Null => return,
            Object::Boolean(b) => Canon::Bool(*b),
            Object::Integer(i) => Canon::Num(*i as f64),
            Object::Real(r) => Canon::Num(*r),
            Object::String(s) => Canon::Str(s.clone()),
            Object::Name(n) => Canon::Name(n.as_bytes().to_vec()),
            Object::Array(items) => {
                for (i, item) in items.iter().enumerate() {
                    self.value(format!("{path}[{i}]"), item, depth + 1);
                }
                Canon::Array(items.len())
            }
            Object::Dict(dict) => {
                let mut count = 0;
                for (key, item) in dict.iter() {
                    if !matches!(item, Object::Null) {
                        count += 1;
                    }
                    let sub = format!("{path}/{}", String::from_utf8_lossy(key.as_bytes()));
                    self.value(sub, item, depth + 1);
                }
                Canon::Dict(count)
            }
            Object::Stream(stream) => {
                let mut dict = stream.dict.clone();
                dict.remove(b"Length");
                self.value(path.clone(), &Object::Dict(dict), depth);
                let data = self.graph.stream_raw(stream).and_then(|raw| {
                    crate::filters::decode_stream(&stream.dict, raw)
                        .ok()
                        .map(|d| Sha256::digest(d).into())
                });
                self.out.insert(format!("{path}#data"), Canon::Data(data));
                return;
            }
            Object::Reference(id) => return self.reference(path, *id, depth),
        };
        self.out.insert(path, leaf);
    }

    fn reference(&mut self, path: String, id: ObjId, depth: usize) {
        if let Some(first) = self.seen.get(&id) {
            self.out.insert(path, Canon::Back(first.clone()));
            return;
        }
        let Some(target) = self.graph.value(id).cloned() else {
            self.out.insert(path, Canon::Dangling);
            return;
        };
        if let Some(kind) = boundary(&target) {
            self.out.insert(path, Canon::Boundary(kind));
            return;
        }
        self.seen.insert(id, path.clone());
        self.value(path, &target, depth + 1);
    }
}

fn boundary(target: &Object) -> Option<Vec<u8>> {
    let dict = match target {
        Object::Dict(d) => d,
        _ => return None,
    };
    match dict.get(b"Type") {
        Some(Object::Name(n)) if matches!(n.as_bytes(), b"Page" | b"Pages" | b"Catalog") => {
            Some(n.as_bytes().to_vec())
        }
        _ => None,
    }
}

/// How `after` differs from `before` beyond the declared `changes`, with
/// `(dx, dy)` the offset a [`Change::Translated`] key must move by. Empty
/// means the cell holds. [`Change::Dropped`] keys may be absent after; the
/// caller checks the disclosure counter.
pub(crate) fn compare(
    before: &PropertyDump,
    after: &PropertyDump,
    changes: &[Change],
    offset: (f64, f64),
) -> Vec<String> {
    let mut faults = Vec::new();
    let paths: std::collections::BTreeSet<&String> =
        before.0.keys().chain(after.0.keys()).collect();
    for path in paths {
        let (b, a) = (before.0.get(path), after.0.get(path));
        if let Some(fault) = check(path, b, a, changes, offset) {
            faults.push(fault);
        }
    }
    faults
}

fn top_key(path: &str) -> &str {
    let rest = path.strip_prefix('/').unwrap_or("");
    let end = rest.find(['/', '[', '#']).unwrap_or(rest.len());
    rest.get(..end).unwrap_or("")
}

fn check(
    path: &str,
    before: Option<&Canon>,
    after: Option<&Canon>,
    changes: &[Change],
    (dx, dy): (f64, f64),
) -> Option<String> {
    let key = top_key(path);
    let declared = changes.iter().find(|c| match c {
        Change::Translated(k) | Change::Rebound(k) | Change::Regenerated(k) => *k == key,
        Change::Dropped { key: k, .. } => *k == key,
        Change::Rebaked { .. } | Change::Renumbered | Change::Unbound { .. } => false,
    });
    let fault = |why: &str| {
        Some(format!(
            "{path}: {why} (before {before:?}, after {after:?})"
        ))
    };
    match (declared, before, after) {
        (Some(Change::Translated(_)), Some(Canon::Num(b)), Some(Canon::Num(a))) => {
            let index = path
                .rsplit('[')
                .next()
                .and_then(|t| t.trim_end_matches(']').parse::<usize>().ok());
            let shift = if index.unwrap_or(0).is_multiple_of(2) {
                dx
            } else {
                dy
            };
            ((a - b - shift).abs() > 1e-9).then(|| format!("{path}: moved {} not {shift}", a - b))
        }
        (Some(Change::Rebound(_)), _, Some(Canon::Boundary(_))) => None,
        (Some(Change::Rebound(_)), _, _) => fault("not rebound to a page"),
        (Some(Change::Regenerated(_)), Some(_), None) => fault("not regenerated"),
        (Some(Change::Regenerated(_)), _, _) => None,
        (Some(Change::Dropped { .. }), _, None) => None,
        // A dictionary's entry count is implied by its entries' own paths.
        (_, Some(Canon::Dict(_)), Some(Canon::Dict(_))) => None,
        _ if before == after => None,
        _ => fault("changed"),
    }
}
