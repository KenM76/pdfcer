//! The object graph side of a rebuild: which objects are carried, which are
//! reachable, which are identical, and the dense renumbering.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use super::RebuildReport;
use crate::document::Document;
use crate::object::{Dict, Name, ObjId, Object};
use crate::writer::IdentityEncoder;
use crate::writer::serialize;

/// Merging stops after this many rounds even if a round still merged
/// something. Each round can only merge objects one level above the last
/// round's merges, and real graphs settle in two or three.
const MAX_MERGE_ROUNDS: usize = 32;

/// Every indirect object of `doc` that a rebuild carries over. Input object
/// streams, cross-reference streams and linearization dictionaries are
/// recorded in `report.structural_dropped` and left out.
pub(super) fn live_objects<'a>(
    doc: &'a Document,
    report: &mut RebuildReport,
) -> BTreeMap<ObjId, &'a Object> {
    let mut live = BTreeMap::new();
    for obj in doc.objects() {
        if is_structural(&obj.value) {
            report.structural_dropped.push(obj.id);
        } else {
            live.insert(obj.id, &obj.value);
        }
    }
    live
}

fn is_structural(value: &Object) -> bool {
    let dict = match value {
        Object::Stream(s) => &s.dict,
        Object::Dict(d) => d,
        _ => return false,
    };
    if dict.get(b"Linearized").is_some() {
        return true;
    }
    matches!(value, Object::Stream(_)) && matches!(type_of(dict), Some(b"ObjStm" | b"XRef"))
}

fn type_of(dict: &Dict) -> Option<&[u8]> {
    dict.get(b"Type")
        .and_then(Object::as_name)
        .map(Name::as_bytes)
}

/// Call `f` for every reference inside `value`, without following them.
/// A stream's `/Length` is skipped: the writer always emits a direct one,
/// so an indirect length object is not needed by the output.
pub(super) fn for_each_reference(value: &Object, mut f: impl FnMut(ObjId)) {
    let mut stack: Vec<&Object> = vec![value];
    while let Some(obj) = stack.pop() {
        match obj {
            Object::Reference(id) => f(*id),
            Object::Array(items) => stack.extend(items.iter()),
            Object::Dict(d) => stack.extend(d.iter().map(|(_, v)| v)),
            Object::Stream(s) => stack.extend(
                s.dict
                    .iter()
                    .filter(|(k, _)| k.as_bytes() != b"Length")
                    .map(|(_, v)| v),
            ),
            Object::Null
            | Object::Boolean(_)
            | Object::Integer(_)
            | Object::Real(_)
            | Object::String(_)
            | Object::Name(_) => {}
        }
    }
}

/// The objects of `live` reachable from the trailer's `/Root` and `/Info`
/// (§7.5.5). Iterative, with a visited set, so cycles and deep graphs are
/// safe.
pub(super) fn reachable(trailer: &Dict, live: &BTreeMap<ObjId, &Object>) -> BTreeSet<ObjId> {
    let mut seen = BTreeSet::new();
    let mut todo: Vec<ObjId> = Vec::new();
    for key in [b"Root".as_slice(), b"Info"] {
        if let Some(Object::Reference(id)) = trailer.get(key) {
            todo.push(*id);
        }
    }
    while let Some(id) = todo.pop() {
        let Some(value) = live.get(&id) else { continue };
        if !seen.insert(id) {
            continue;
        }
        for_each_reference(value, |r| {
            if !seen.contains(&r) {
                todo.push(r);
            }
        });
    }
    seen
}

/// Find identical objects among `live`, to a fixpoint. Returns
/// `dropped → kept`, every chain already collapsed, and records the pairs
/// in `report.merged`. The kept object of each group is the lowest-numbered
/// one.
pub(super) fn merge_duplicates(
    doc: &Document,
    live: &BTreeMap<ObjId, &Object>,
    report: &mut RebuildReport,
) -> BTreeMap<ObjId, ObjId> {
    let eligible = eligible_ids(live);
    let mut canon: BTreeMap<ObjId, ObjId> = BTreeMap::new();
    for _ in 0..MAX_MERGE_ROUNDS {
        let mut groups: HashMap<(Vec<u8>, &[u8]), ObjId> = HashMap::new();
        let mut merged_this_round = false;
        for &id in &eligible {
            if canon.contains_key(&id) {
                continue;
            }
            let Some(value) = live.get(&id) else { continue };
            let key = merge_key(value, &canon, doc.bytes());
            match groups.get(&key) {
                Some(&kept) => {
                    canon.insert(id, kept);
                    merged_this_round = true;
                }
                None => {
                    groups.insert(key, id);
                }
            }
        }
        if !merged_this_round {
            break;
        }
    }
    report.merged = canon.iter().map(|(&d, &k)| (d, k)).collect();
    canon
}

/// The comparison key: the object serialized with its references mapped
/// through `canon`, plus a stream's stored data (borrowed, not copied).
fn merge_key<'a>(
    value: &Object,
    canon: &BTreeMap<ObjId, ObjId>,
    source: &'a [u8],
) -> (Vec<u8>, &'a [u8]) {
    let mut bytes = Vec::new();
    let mapped = map_references(value, &mut |id| {
        Object::Reference(canon.get(&id).copied().unwrap_or(id))
    });
    match &mapped {
        Object::Stream(s) => {
            let mut dict = s.dict.clone();
            dict.remove(b"Length");
            bytes.push(b'S');
            serialize::write_object(
                &mut bytes,
                &Object::Dict(dict),
                ObjId::new(0, 0),
                &[],
                &IdentityEncoder,
            );
            (bytes, s.data_span.slice(source).unwrap_or(&[]))
        }
        other => {
            serialize::write_object(&mut bytes, other, ObjId::new(0, 0), &[], &IdentityEncoder);
            (bytes, &[])
        }
    }
}

/// Which objects may be merged with an identical twin, ascending. Objects
/// whose identity carries meaning (pages, annotations, fields, layers,
/// structure elements, the catalog) never are, and nor are page content
/// streams, so that editing one page cannot change another.
fn eligible_ids(live: &BTreeMap<ObjId, &Object>) -> Vec<ObjId> {
    let mut identity = BTreeSet::new();
    let mut page_contents = BTreeSet::new();
    for (&id, value) in live {
        let Object::Dict(d) = value else { continue };
        if has_identity(d) {
            identity.insert(id);
        }
        if type_of(d) == Some(b"Page")
            && let Some(contents) = d.get(b"Contents")
        {
            for_each_reference(contents, |r| {
                page_contents.insert(r);
            });
        }
    }
    live.iter()
        .filter(|(id, value)| match value {
            Object::Stream(_) => !page_contents.contains(id),
            Object::Dict(d) => matches!(
                type_of(d),
                Some(b"Font" | b"FontDescriptor" | b"ExtGState" | b"Encoding")
            ),
            Object::Array(items) => !items.is_empty() && !names_identity(value, &identity),
            Object::Reference(_) => false,
            _ => true,
        })
        .map(|(id, _)| *id)
        .collect()
}

fn has_identity(d: &Dict) -> bool {
    let typed = matches!(
        type_of(d),
        Some(b"Page" | b"Pages" | b"Catalog" | b"Annot" | b"OCG" | b"OCMD" | b"StructElem")
    );
    // An annotation's /Type is optional (Table 166); /Rect plus /Subtype
    // identifies one. A field is anything with /FT, or a /T naming a node.
    let annot = d.get(b"Subtype").is_some() && d.get(b"Rect").is_some();
    let field = d.get(b"FT").is_some() || (d.get(b"T").is_some() && d.get(b"Kids").is_some());
    typed || annot || field
}

fn names_identity(value: &Object, identity: &BTreeSet<ObjId>) -> bool {
    let mut found = false;
    for_each_reference(value, |r| found |= identity.contains(&r));
    found
}

/// A copy of `value` with every reference replaced by `f(reference)`.
/// Recursive: direct nesting is bounded by
/// [`crate::parser::MAX_NESTING_DEPTH`].
pub(super) fn map_references(value: &Object, f: &mut impl FnMut(ObjId) -> Object) -> Object {
    match value {
        Object::Reference(id) => f(*id),
        Object::Array(items) => Object::Array(items.iter().map(|o| map_references(o, f)).collect()),
        Object::Dict(d) => Object::Dict(map_dict(d, f)),
        Object::Stream(s) => {
            let mut s = s.clone();
            s.dict = map_dict(&s.dict, f);
            Object::Stream(s)
        }
        other => other.clone(),
    }
}

fn map_dict(d: &Dict, f: &mut impl FnMut(ObjId) -> Object) -> Dict {
    let mut out = Dict::new();
    for (k, v) in d.iter() {
        out.insert(k.clone(), map_references(v, f));
    }
    out
}

/// Old object id → new dense number, generation 0.
pub(super) struct Renumbering {
    new_num: BTreeMap<ObjId, u32>,
    canon: BTreeMap<ObjId, ObjId>,
}

impl Renumbering {
    /// Number `kept` 1..=n in ascending old-id order; a merged object maps
    /// to its kept twin's number.
    pub(super) fn new(kept: &BTreeSet<ObjId>, canon: &BTreeMap<ObjId, ObjId>) -> Self {
        let new_num = kept.iter().zip(1u32..).map(|(&id, n)| (id, n)).collect();
        Self {
            new_num,
            canon: canon.clone(),
        }
    }

    /// The new id for `old`, or `None` when the output does not define it.
    pub(super) fn get(&self, old: ObjId) -> Option<ObjId> {
        let id = self.canon.get(&old).copied().unwrap_or(old);
        self.new_num.get(&id).map(|&n| ObjId::new(n, 0))
    }

    /// Kept objects in new-number order, as `(old, new)`.
    pub(super) fn kept(&self) -> impl Iterator<Item = (ObjId, ObjId)> + '_ {
        let mut pairs: Vec<(ObjId, ObjId)> = self
            .new_num
            .iter()
            .map(|(&old, &n)| (old, ObjId::new(n, 0)))
            .collect();
        pairs.sort_by_key(|&(_, new)| new.num);
        pairs.into_iter()
    }

    /// Number of objects written before any object or xref stream.
    pub(super) fn len(&self) -> u32 {
        u32::try_from(self.new_num.len()).unwrap_or(u32::MAX)
    }
}
