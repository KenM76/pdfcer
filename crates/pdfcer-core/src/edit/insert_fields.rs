//! Carry form-field definitions across a page insert (`Pass 102.1`), and the
//! field-tree helpers `merge_acroform` shares with it.
//!
//! A widget on an inserted page arrives through the page's `/Annots`; its
//! field lives in `/AcroForm /Fields` and does not (ISO 32000-1 §12.7.3.1).
//! This module walks the SOURCE field tree and carries every terminal field
//! whose widgets **all** arrived, under pruned copies of its ancestors so
//! inherited `/FT`, `/Ff`, `/V`, `/DV`, `/DA` and `/Q` (§12.7.3.2) still
//! reach it. A terminal field split across inserted and non-inserted pages
//! is not carried — half a field is refused rather than trimmed, as in
//! `pageops::assemble` — and is counted in `InsertOutcome::fields_split`.

use std::collections::{BTreeMap, BTreeSet};

use super::{EditError, EditSession, decode_text_string, encode_text_string, unique_field_name};
use crate::forms;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::view::DocumentView;

/// Deepest field tree walked. §12.7.3.1 sets no limit; real forms nest a
/// handful of levels, and a cycle or hostile chain stops here
/// (`ARCHITECTURE.md` §10).
const MAX_FIELD_DEPTH: usize = 32;

/// What [`EditSession::carry_inserted_fields`] did.
#[derive(Debug, Default)]
pub(super) struct FieldCarry {
    /// Terminal fields carried.
    pub(super) carried: usize,
    /// Carried top-level fields renamed on a name collision.
    pub(super) renamed: usize,
    /// Terminal fields with widgets on both inserted and other pages.
    pub(super) split: usize,
    /// Whether the target had no `/AcroForm` and one was created.
    pub(super) created: bool,
    /// Every new object id inside a carried field subtree.
    pub(super) claimed: BTreeSet<ObjId>,
}

/// How a source field's widgets sit relative to the inserted pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coverage {
    /// No widget arrived (or the field has none).
    None,
    /// Some arrived and some did not.
    Partial,
    /// At least one arrived and every widget did.
    Full,
}

/// Whether `dict`, a `/Kids` entry, is a field rather than a widget.
///
/// §12.7.3.1: a non-terminal field's kids are fields, a terminal field's
/// kids are widgets, and only a field carries a partial name. `/Kids` is
/// accepted too so a nameless intermediate node is not mistaken for a
/// widget.
fn is_field_kid(dict: &Dict) -> bool {
    dict.contains_key(b"T") || dict.contains_key(b"Kids")
}

/// The `/Kids` references of a dictionary, in order.
fn kid_refs(view: &DocumentView<'_>, dict: &Dict) -> Vec<ObjId> {
    dict.get(b"Kids")
        .map(|o| view.graph().resolve(o))
        .and_then(Object::as_array)
        .map(|kids| kids.iter().filter_map(Object::as_reference).collect())
        .unwrap_or_default()
}

/// Classify a source field's widgets against `mapping` (what the page copy
/// brought across). A leaf with `/Rect` that did not arrive is a widget
/// left behind; a leaf with neither is a pure container and counts as
/// nothing.
fn coverage(view: &DocumentView<'_>, root: ObjId, mapping: &BTreeMap<ObjId, ObjId>) -> Coverage {
    let (mut on, mut off) = (0usize, 0usize);
    let mut stack = vec![(root, 0usize)];
    let mut visited = BTreeSet::new();
    while let Some((id, depth)) = stack.pop() {
        if depth > MAX_FIELD_DEPTH || !visited.insert(id) {
            continue;
        }
        let Some(dict) = view.graph().resolved(id).as_dict() else {
            continue;
        };
        let kids = kid_refs(view, dict);
        if kids.is_empty() {
            if mapping.contains_key(&id) {
                on += 1;
            } else if dict.contains_key(b"Rect") {
                off += 1;
            }
        } else {
            stack.extend(kids.into_iter().map(|k| (k, depth + 1)));
        }
    }
    match (on, off) {
        (0, _) => Coverage::None,
        (_, 0) => Coverage::Full,
        _ => Coverage::Partial,
    }
}

/// Terminal fields at or below `root` in the source.
fn count_terminals(view: &DocumentView<'_>, root: ObjId) -> usize {
    let mut count = 0usize;
    let mut stack = vec![(root, 0usize)];
    let mut visited = BTreeSet::new();
    while let Some((id, depth)) = stack.pop() {
        if depth > MAX_FIELD_DEPTH || !visited.insert(id) {
            continue;
        }
        let Some(dict) = view.graph().resolved(id).as_dict() else {
            continue;
        };
        let field_kids: Vec<ObjId> = kid_refs(view, dict)
            .into_iter()
            .filter(|k| {
                view.graph()
                    .resolved(*k)
                    .as_dict()
                    .is_some_and(is_field_kid)
            })
            .collect();
        if field_kids.is_empty() {
            count += 1;
        } else {
            stack.extend(field_kids.into_iter().map(|k| (k, depth + 1)));
        }
    }
    count
}

/// The first segments of every fully qualified name in `graph`'s form: the
/// names a new top-level field must not reuse.
///
/// Root names, not terminal names: a target holding `Name.First` and an
/// arriving root `Name` with kid `Last` share the root `Name`, and two root
/// dictionaries with one name are one field by §12.7.3.1. Comparing against
/// terminal names alone passes them both; the group names supply the
/// non-terminal roots.
pub(super) fn taken_root_names<G: ObjectGraph + ?Sized>(graph: &G) -> BTreeSet<String> {
    forms::parse_acroform(graph)
        .map(|f| {
            f.fields
                .iter()
                .map(|x| x.fully_qualified_name.as_str())
                .chain(f.groups.iter().map(|g| g.fully_qualified_name.as_str()))
                .map(|fqn| fqn.split('.').next().unwrap_or(fqn).to_owned())
                .collect()
        })
        .unwrap_or_default()
}

/// Rename a staged top-level field when its `/T` is already in `taken`;
/// records the name it ends with. Returns whether it renamed.
pub(super) fn rename_if_taken(field: &mut Dict, taken: &mut BTreeSet<String>) -> bool {
    let Some(Object::String(bytes)) = field.get(b"T") else {
        return false;
    };
    let original = decode_text_string(bytes).text;
    if taken.contains(&original) {
        let renamed = unique_field_name(&original, taken);
        field.insert(
            Name::from(b"T"),
            Object::String(encode_text_string(&renamed)),
        );
        taken.insert(renamed);
        true
    } else {
        taken.insert(original);
        false
    }
}

/// Point every staged kid below `root` back at its parent, at every level,
/// and return the ids of the subtree (`root` included).
///
/// The copy drops `/Parent` (`import_dict`), so without this a grandchild
/// widget is reachable from its field and not the other way round, and
/// inheritance (§12.7.3.2) stops at the first level.
pub(super) fn reparent_subtree(
    root: ObjId,
    scratch: &mut BTreeMap<ObjId, Object>,
) -> BTreeSet<ObjId> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![(root, 0usize)];
    while let Some((id, depth)) = stack.pop() {
        if depth > MAX_FIELD_DEPTH || !seen.insert(id) {
            continue;
        }
        let kids: Vec<ObjId> = match scratch.get(&id) {
            Some(Object::Dict(d)) => match d.get(b"Kids") {
                Some(Object::Array(k)) => k.iter().filter_map(Object::as_reference).collect(),
                _ => Vec::new(),
            },
            _ => Vec::new(),
        };
        for kid in kids {
            if let Some(Object::Dict(kid_dict)) = scratch.get_mut(&kid) {
                kid_dict.insert(Name::from(b"Parent"), Object::Reference(id));
                stack.push((kid, depth + 1));
            }
        }
    }
    seen
}

/// Widgets on the new pages that no carried field claims, and the subset
/// with no `/T` of their own (a bare kid whose name, type and value were
/// only in an ancestor). Returns `(orphaned, unrecoverable)`.
///
/// Counted from `scratch`: the copy keeps `/T` exactly when the source had
/// it, so the copy answers the same question the source would.
pub(super) fn count_orphans(
    new_pages: &[ObjId],
    claimed: &BTreeSet<ObjId>,
    scratch: &BTreeMap<ObjId, Object>,
) -> (usize, usize) {
    let (mut orphaned, mut unrecoverable) = (0usize, 0usize);
    for page in new_pages {
        let Some(Object::Dict(page)) = scratch.get(page) else {
            continue;
        };
        let annots = match page.get(b"Annots") {
            Some(Object::Array(list)) => list.as_slice(),
            Some(Object::Reference(r)) => match scratch.get(r) {
                Some(Object::Array(list)) => list.as_slice(),
                _ => continue,
            },
            _ => continue,
        };
        for id in annots.iter().filter_map(Object::as_reference) {
            if claimed.contains(&id) {
                continue;
            }
            let Some(Object::Dict(annot)) = scratch.get(&id) else {
                continue;
            };
            if matches!(annot.get(b"Subtype"), Some(Object::Name(n)) if n.as_bytes() == b"Widget") {
                orphaned += 1;
                unrecoverable += usize::from(!annot.contains_key(b"T"));
            }
        }
    }
    (orphaned, unrecoverable)
}

/// Whether any staged node in `ids` is a signature field (`/FT /Sig`).
fn has_signature_field(ids: &BTreeSet<ObjId>, scratch: &BTreeMap<ObjId, Object>) -> bool {
    ids.iter().any(|id| {
        matches!(scratch.get(id), Some(Object::Dict(d))
            if matches!(d.get(b"FT"), Some(Object::Name(n)) if n.as_bytes() == b"Sig"))
    })
}

impl EditSession {
    /// Carry the source fields whose widgets all arrived with the inserted
    /// pages into this document's `/AcroForm`. Writes into `scratch`;
    /// commits nothing. See the module header for the rule.
    pub(super) fn carry_inserted_fields(
        &mut self,
        source: &DocumentView<'_>,
        mapping: &mut BTreeMap<ObjId, ObjId>,
        scratch: &mut BTreeMap<ObjId, Object>,
    ) -> Result<FieldCarry, EditError> {
        let mut out = FieldCarry::default();
        let Some(src_form) = source
            .graph()
            .catalog_dict()
            .and_then(|c| c.get(b"AcroForm"))
            .map(|o| source.graph().resolve(o))
            .and_then(Object::as_dict)
            .cloned()
        else {
            return Ok(out);
        };
        let roots: Vec<ObjId> = src_form
            .get(b"Fields")
            .map(|o| source.graph().resolve(o))
            .and_then(Object::as_array)
            .map(|f| f.iter().filter_map(Object::as_reference).collect())
            .unwrap_or_default();

        let mut new_roots = Vec::new();
        for root in roots {
            if let Some(new_id) = self.carry_field(source, root, 0, mapping, scratch, &mut out)? {
                new_roots.push(new_id);
            }
        }
        if new_roots.is_empty() {
            return Ok(out);
        }

        let mut taken = taken_root_names(&self.graph());
        for &id in &new_roots {
            if let Some(Object::Dict(field)) = scratch.get_mut(&id) {
                field.remove(b"Parent");
                out.renamed += usize::from(rename_if_taken(field, &mut taken));
            }
            out.claimed.extend(reparent_subtree(id, scratch));
        }
        let sig_flags = if has_signature_field(&out.claimed, scratch) {
            match src_form.get(b"SigFlags").map(|o| source.graph().resolve(o)) {
                Some(Object::Integer(n)) => *n,
                _ => 0,
            }
        } else {
            0
        };
        let refs = new_roots.into_iter().map(Object::Reference).collect();
        out.created = self.splice_acroform(source, &src_form, refs, sig_flags, mapping, scratch)?;
        Ok(out)
    }

    /// Carry one source field: whole when every widget arrived, as a pruned
    /// copy holding only its carried field kids when some did, or not at
    /// all. Returns the staged id.
    fn carry_field(
        &mut self,
        source: &DocumentView<'_>,
        id: ObjId,
        depth: usize,
        mapping: &mut BTreeMap<ObjId, ObjId>,
        scratch: &mut BTreeMap<ObjId, Object>,
        out: &mut FieldCarry,
    ) -> Result<Option<ObjId>, EditError> {
        if depth > MAX_FIELD_DEPTH {
            return Ok(None);
        }
        match coverage(source, id, mapping) {
            Coverage::None => Ok(None),
            Coverage::Full => {
                out.carried += count_terminals(source, id);
                Ok(Some(self.import_object(source, id, mapping, scratch)?))
            }
            Coverage::Partial => {
                let Some(dict) = source.graph().resolved(id).as_dict().cloned() else {
                    return Ok(None);
                };
                let field_kids: Vec<ObjId> = kid_refs(source, &dict)
                    .into_iter()
                    .filter(|k| {
                        source
                            .graph()
                            .resolved(*k)
                            .as_dict()
                            .is_some_and(is_field_kid)
                    })
                    .collect();
                if field_kids.is_empty() {
                    out.split += 1;
                    return Ok(None);
                }
                let mut carried_kids = Vec::new();
                for kid in field_kids {
                    if let Some(new_kid) =
                        self.carry_field(source, kid, depth + 1, mapping, scratch, out)?
                    {
                        carried_kids.push(Object::Reference(new_kid));
                    }
                }
                if carried_kids.is_empty() {
                    return Ok(None);
                }
                self.pruned_copy(source, &dict, carried_kids, mapping, scratch)
                    .map(Some)
            }
        }
    }

    /// A new copy of a non-terminal field holding only `kids`. Not entered
    /// in `mapping`: it is not the source object, and a later import of the
    /// source id must not resolve to this subset.
    fn pruned_copy(
        &mut self,
        source: &DocumentView<'_>,
        dict: &Dict,
        kids: Vec<Object>,
        mapping: &mut BTreeMap<ObjId, ObjId>,
        scratch: &mut BTreeMap<ObjId, Object>,
    ) -> Result<ObjId, EditError> {
        let mut copy = Dict::new();
        for (key, value) in dict.iter() {
            if matches!(key.as_bytes(), b"Kids" | b"Parent") {
                continue;
            }
            let imported = self.import_value(source, value, mapping, scratch)?;
            copy.insert(key.clone(), imported);
        }
        copy.insert(Name::from(b"Kids"), Object::Array(kids));
        let new_id = ObjId::new(self.alloc_number()?, 0);
        scratch.insert(new_id, Object::Dict(copy));
        Ok(new_id)
    }

    /// Append `new_fields` to this document's `/AcroForm /Fields`, creating
    /// the dictionary when absent, and fold in the source form's
    /// `/NeedAppearances` (OR), `sig_flags` (bitwise OR), `/DR` (target wins
    /// per key) and `/DA` (when absent). Returns whether it created
    /// `/AcroForm`. Table 218/219; §12.7.3.3 for `/DR`.
    pub(super) fn splice_acroform(
        &mut self,
        source: &DocumentView<'_>,
        src_form: &Dict,
        new_fields: Vec<Object>,
        sig_flags: i64,
        mapping: &mut BTreeMap<ObjId, ObjId>,
        scratch: &mut BTreeMap<ObjId, Object>,
    ) -> Result<bool, EditError> {
        let catalog_id = self.graph().catalog_id().ok_or(EditError::NotADictionary {
            id: ObjId::new(0, 0),
            key: "Root",
        })?;
        let Some(mut catalog) = self.staged_catalog(catalog_id, scratch) else {
            return Err(EditError::NotADictionary {
                id: catalog_id,
                key: "Root",
            });
        };
        let existing = catalog.get(b"AcroForm").cloned();
        let created = existing.is_none();
        let mut form = match &existing {
            Some(obj) => self.deref_dict(Some(obj)).unwrap_or_default(),
            None => Dict::new(),
        };
        let mut fields = form
            .get(b"Fields")
            .map(|o| self.resolve_value(o))
            .and_then(Object::as_array)
            .map(<[Object]>::to_vec)
            .unwrap_or_default();
        fields.extend(new_fields);
        form.insert(Name::from(b"Fields"), Object::Array(fields));
        self.fold_form_keys(source, src_form, sig_flags, &mut form, mapping, scratch)?;
        match existing {
            Some(Object::Reference(form_id)) => {
                scratch.insert(form_id, Object::Dict(form));
            }
            _ => {
                catalog.insert(Name::from(b"AcroForm"), Object::Dict(form));
                scratch.insert(catalog_id, Object::Dict(catalog));
            }
        }
        Ok(created)
    }

    /// The document-level `/AcroForm` keys [`Self::splice_acroform`] folds.
    ///
    /// `/NeedAppearances` and `/SigFlags` are ORed, never overwritten: either
    /// document's "appearances may be stale" or "signature fields exist" is
    /// still true of the union. Carrying `/SigFlags` claims structure only —
    /// a carried signature VALUE is already broken by the renumbering.
    fn fold_form_keys(
        &mut self,
        source: &DocumentView<'_>,
        src_form: &Dict,
        sig_flags: i64,
        form: &mut Dict,
        mapping: &mut BTreeMap<ObjId, ObjId>,
        scratch: &mut BTreeMap<ObjId, Object>,
    ) -> Result<(), EditError> {
        if matches!(
            src_form
                .get(b"NeedAppearances")
                .map(|o| source.graph().resolve(o)),
            Some(Object::Boolean(true))
        ) {
            form.insert(Name::from(b"NeedAppearances"), Object::Boolean(true));
        }
        if sig_flags != 0 {
            let existing = form
                .get(b"SigFlags")
                .and_then(|o| match self.resolve_value(o) {
                    Object::Integer(n) => Some(*n),
                    _ => None,
                })
                .unwrap_or(0);
            form.insert(
                Name::from(b"SigFlags"),
                Object::Integer(existing | sig_flags),
            );
        }
        if let Some(src_dr) = src_form
            .get(b"DR")
            .map(|o| source.graph().resolve(o))
            .and_then(Object::as_dict)
            .cloned()
        {
            let mut dr = form
                .get(b"DR")
                .and_then(|o| self.deref_dict(Some(o)))
                .unwrap_or_default();
            for (key, value) in src_dr.iter() {
                if !dr.contains_key(key.as_bytes()) {
                    let imported = self.import_value(source, value, mapping, scratch)?;
                    dr.insert(key.clone(), imported);
                }
            }
            form.insert(Name::from(b"DR"), Object::Dict(dr));
        }
        if !form.contains_key(b"DA")
            && let Some(da) = src_form.get(b"DA")
        {
            let da = self.import_value(source, da, mapping, scratch)?;
            form.insert(Name::from(b"DA"), da);
        }
        Ok(())
    }
}
