//! Listing and removing metadata ([`EditSession::metadata_inventory`],
//! [`EditSession::remove_metadata`]).

use std::collections::{BTreeMap, HashSet};

use super::{
    Command, CommandKind, EditError, EditSession, LayerContentPolicy, ObjectWrite, PendingGraph,
    Removal, collect_references, reachable,
};
use crate::crypto::PermissionBit;
use crate::doc_metadata::{
    DocumentIdAction, JsSlot, MetadataInventory, MetadataItemId, MetadataRemoval,
    MetadataRemoveOptions, NotRemoved, Target,
};
use crate::object::{Dict, Name, ObjId, Object};

/// Edits staged for the one direct-edit command.
#[derive(Default)]
struct Staged {
    scratch: BTreeMap<ObjId, Object>,
    trailer: Option<Dict>,
    detached: Vec<Object>,
}

impl EditSession {
    /// Every carrier of metadata or hidden information in the document as
    /// it stands in this session; see
    /// [`doc_metadata::metadata_inventory`](crate::doc_metadata::metadata_inventory).
    /// The earlier-revisions item reflects the file as opened.
    #[must_use]
    pub fn metadata_inventory(&self) -> MetadataInventory {
        crate::doc_metadata::metadata_inventory(&self.view(), self.base.bytes())
    }

    /// Remove the items `ids` names, taken from
    /// [`Self::metadata_inventory`]. One undo entry.
    ///
    /// - Comments, attachments, hidden layers (with their content) and form
    ///   data go through [`Self::delete_annotation`], [`Self::detach_file`],
    ///   [`Self::delete_layer`] and [`Self::reset_form`]; form data resets
    ///   to each field's `/DV` (§12.7.5.3), skipping signatures and
    ///   read-only fields, which is disclosed.
    /// - Every other item deletes its key; objects nothing refers to
    ///   afterwards are deleted too ([`MetadataRemoval::objects_freed`]).
    /// - `document-id` follows [`MetadataRemoveOptions::document_id`], and
    ///   is refused in an encrypted file, whose key derives from it (§7.6.4.3.2).
    /// - `revisions` changes nothing here: earlier revisions leave on a full
    ///   rewrite (`to_full_bytes`), which is also the only save that removes
    ///   anything else, so [`MetadataRemoval::needs_full_rewrite`] is set
    ///   whenever anything was removed.
    ///
    /// An item that fails is reported in [`MetadataRemoval::not_removed`]
    /// and the rest still go; an id not in the inventory is in
    /// [`MetadataRemoval::not_found`].
    ///
    /// # Errors
    ///
    /// [`EditError::DocumentEncrypted`] when the permissions forbid
    /// modifying the document.
    ///
    /// # Examples
    ///
    /// ```
    /// # use pdfcer_core::edit::EditSession;
    /// # use pdfcer_core::doc_metadata::MetadataRemoveOptions;
    /// # fn demo(session: &mut EditSession) -> Result<(), Box<dyn std::error::Error>> {
    /// let ids: Vec<_> = session.metadata_inventory().items.into_iter().map(|i| i.id).collect();
    /// let report = session.remove_metadata(&ids, &MetadataRemoveOptions::default())?;
    /// assert!(report.not_found.is_empty());
    /// # Ok(())
    /// # }
    /// ```
    pub fn remove_metadata(
        &mut self,
        ids: &[MetadataItemId],
        options: &MetadataRemoveOptions,
    ) -> Result<MetadataRemoval, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        let listed: HashSet<MetadataItemId> = self
            .metadata_inventory()
            .items
            .into_iter()
            .map(|i| i.id)
            .collect();
        let mut out = MetadataRemoval::default();
        let mut seen = HashSet::new();
        let mut verbs = Vec::new();
        let mut direct = Vec::new();
        for id in ids.iter().filter(|id| seen.insert(*id)) {
            match id.target().filter(|_| listed.contains(id)) {
                None => out.not_found.push(id.clone()),
                Some(
                    t @ (Target::Comment(_)
                    | Target::Attachment(_)
                    | Target::Layer(_)
                    | Target::FormData),
                ) => {
                    verbs.push((id.clone(), t));
                }
                Some(t) => direct.push((id.clone(), t)),
            }
        }
        let checkpoint = self.checkpoint();
        for (id, target) in verbs {
            let result = self.remove_by_verb(&target, &mut out.disclosures);
            record(&mut out, id, result);
        }
        let mut staged = Staged::default();
        for (id, target) in direct {
            let result = self.stage_metadata(&target, options, &mut staged, &mut out.disclosures);
            record(&mut out, id, result);
        }
        self.drop_emptied_info(&mut staged);
        out.objects_freed = self.commit_metadata_edits(staged);
        let pushed = self.pushed_since(&checkpoint);
        if !self.coalesce_last(pushed, CommandKind::RemoveMetadata) {
            out.disclosures
                .push("the removal takes more than one undo".to_owned());
        }
        out.needs_full_rewrite = !out.removed.is_empty();
        Ok(out)
    }

    fn remove_by_verb(&mut self, target: &Target, notes: &mut Vec<String>) -> Result<(), String> {
        let text = |e: EditError| e.to_string();
        match target {
            Target::Comment(id) => self.delete_annotation(*id).map(drop).map_err(text),
            Target::Attachment(key) => self.detach_file(key).map_err(text),
            Target::Layer(id) => self
                .delete_layer(*id, LayerContentPolicy::RemoveContent)
                .map(drop)
                .map_err(text),
            Target::FormData => {
                let reset = self.reset_form(None).map_err(text)?;
                if reset.values_defaulted > 0 {
                    notes.push(format!(
                        "{} form field(s) went back to their default value, which stays in the file",
                        reset.values_defaulted
                    ));
                }
                if reset.skipped_read_only + reset.skipped_signatures > 0 {
                    notes.push(format!(
                        "{} read-only and {} signature field(s) kept their values",
                        reset.skipped_read_only, reset.skipped_signatures
                    ));
                }
                Ok(())
            }
            _ => Err("not removed by a verb".to_owned()),
        }
    }

    fn stage_metadata(
        &self,
        target: &Target,
        options: &MetadataRemoveOptions,
        st: &mut Staged,
        notes: &mut Vec<String>,
    ) -> Result<(), String> {
        match target {
            Target::Info(key) => self.stage_info(key, st),
            Target::Xmp(owner) => self.strip_metadata_key(st, *owner, &[b"Metadata"]),
            Target::PieceInfo(owner) => self.strip_metadata_key(st, *owner, &[b"PieceInfo"]),
            Target::Thumb(owner) => self.strip_metadata_key(st, *owner, &[b"Thumb"]),
            Target::Js(owner, JsSlot::OpenAction) => {
                self.strip_metadata_key(st, *owner, &[b"OpenAction"])
            }
            Target::Js(owner, JsSlot::Action) => self.strip_metadata_key(st, *owner, &[b"A"]),
            Target::Js(owner, JsSlot::Additional(trigger)) => {
                self.strip_metadata_key(st, *owner, &[b"AA", trigger])
            }
            Target::JsNames => {
                let catalog = self.trailer.get(b"Root").and_then(Object::as_reference);
                self.strip_metadata_key(
                    st,
                    catalog.ok_or("no catalog")?,
                    &[b"Names", b"JavaScript"],
                )
            }
            Target::Revisions => {
                notes.push("earlier revisions are dropped by a full rewrite".to_owned());
                Ok(())
            }
            Target::DocumentId => self.stage_document_id(options.document_id, st, notes),
            _ => Err("not a direct edit".to_owned()),
        }
    }

    fn stage_info(&self, key: &[u8], st: &mut Staged) -> Result<(), String> {
        match self.trailer.get(b"Info") {
            Some(Object::Reference(info)) => self.strip_metadata_key(st, *info, &[key]),
            Some(Object::Dict(_)) => {
                let trailer = st.trailer.get_or_insert_with(|| self.trailer.clone());
                let mut info = trailer.remove(b"Info").ok_or("no document information")?;
                let removed = match &mut info {
                    Object::Dict(d) => d.remove(key),
                    _ => None,
                };
                trailer.insert(Name::from(b"Info"), info);
                removed
                    .map(|v| st.detached.push(v))
                    .ok_or_else(|| "already absent".to_owned())
            }
            _ => Err("no document information".to_owned()),
        }
    }

    fn stage_document_id(
        &self,
        action: DocumentIdAction,
        st: &mut Staged,
        notes: &mut Vec<String>,
    ) -> Result<(), String> {
        if self.trailer.get(b"Encrypt").is_some() {
            return Err("an encrypted file's key is derived from its identifier".to_owned());
        }
        let trailer = st.trailer.get_or_insert_with(|| self.trailer.clone());
        match action {
            DocumentIdAction::Remove => {
                trailer.remove(b"ID").ok_or("already absent")?;
                if self.base.version().major >= 2 {
                    notes.push(
                        "PDF 2.0 requires a file identifier; this file now has none".to_owned(),
                    );
                }
            }
            _ => {
                let pair = (
                    crate::crypto::rng::array::<16>(),
                    crate::crypto::rng::array::<16>(),
                );
                let (Ok(a), Ok(b)) = pair else {
                    return Err(
                        "no random source on this platform to make a new identifier".to_owned()
                    );
                };
                let id = vec![Object::String(a.to_vec()), Object::String(b.to_vec())];
                trailer.insert(Name::from(b"ID"), Object::Array(id));
            }
        }
        Ok(())
    }

    /// Delete the entry at `path` under `owner`, following an indirect
    /// dictionary on the way; a direct dictionary emptied by it goes too.
    fn strip_metadata_key(
        &self,
        st: &mut Staged,
        owner: ObjId,
        path: &[&[u8]],
    ) -> Result<(), String> {
        let mut obj = st
            .scratch
            .get(&owner)
            .or_else(|| self.value(owner))
            .cloned()
            .ok_or("the object no longer exists")?;
        let dict = match &mut obj {
            Object::Dict(d) => d,
            Object::Stream(s) => &mut s.dict,
            _ => return Err("not a dictionary".to_owned()),
        };
        match strip_dict(dict, path, 0)? {
            Stripped::Value(v) => {
                st.detached.push(v);
                st.scratch.insert(owner, obj);
                Ok(())
            }
            Stripped::Follow(next, rest) => self.strip_metadata_key(st, next, rest),
        }
    }

    /// An `/Info` emptied by the removals is dropped from the trailer.
    fn drop_emptied_info(&self, st: &mut Staged) {
        let Some(info) = self.trailer.get(b"Info").and_then(Object::as_reference) else {
            return;
        };
        let empty = st
            .scratch
            .get(&info)
            .and_then(Object::as_dict)
            .is_some_and(Dict::is_empty);
        if empty {
            st.scratch.remove(&info);
            let trailer = st.trailer.get_or_insert_with(|| self.trailer.clone());
            trailer.remove(b"Info");
            st.detached.push(Object::Reference(info));
        }
    }

    /// Commit the staged writes with every object they orphaned removed;
    /// returns how many were removed.
    fn commit_metadata_edits(&mut self, st: Staged) -> usize {
        if st.scratch.is_empty() && st.trailer.is_none() {
            return 0;
        }
        let mut seeds = Vec::new();
        for value in &st.detached {
            collect_references(value, false, 0, &mut seeds);
        }
        let candidates = reachable(&self.graph(), &seeds, &HashSet::new());
        let trailer = st.trailer.as_ref().unwrap_or(&self.trailer);
        let roots: Vec<ObjId> = [b"Root".as_slice(), b"Info"]
            .iter()
            .filter_map(|k| trailer.get(k).and_then(Object::as_reference))
            .collect();
        let none = HashSet::new();
        let pending = PendingGraph {
            session: self,
            scratch: &st.scratch,
            removed: &none,
        };
        let live = reachable(&pending, &roots, &HashSet::new());
        let removals: Vec<Removal> = candidates
            .into_iter()
            .filter(|id| {
                !live.contains(id) && !self.deleted.contains(id) && self.value(*id).is_some()
            })
            .map(|id| Removal {
                id,
                was_deleted: false,
                is_deleted: true,
            })
            .collect();
        let freed = removals.len();
        let objects = st
            .scratch
            .into_iter()
            .map(|(id, after)| ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(after),
            })
            .collect();
        let trailer = st.trailer.map(|after| (self.trailer.clone(), after));
        self.commit(Command {
            kind: CommandKind::RemoveMetadata,
            objects,
            removals,
            trailer,
        });
        freed
    }
}

fn record(out: &mut MetadataRemoval, id: MetadataItemId, result: Result<(), String>) {
    match result {
        Ok(()) => out.removed.push(id),
        Err(reason) => out.not_removed.push(NotRemoved { id, reason }),
    }
}

enum Stripped<'p> {
    Value(Object),
    Follow(ObjId, &'p [&'p [u8]]),
}

/// Paths are at most two keys long; `depth` bounds the recursion anyway.
fn strip_dict<'p>(
    dict: &mut Dict,
    path: &'p [&'p [u8]],
    depth: usize,
) -> Result<Stripped<'p>, String> {
    let [first, rest @ ..] = path else {
        return Err("empty path".to_owned());
    };
    if rest.is_empty() {
        return dict
            .remove(first)
            .map(Stripped::Value)
            .ok_or_else(|| "already absent".to_owned());
    }
    match dict.remove(first) {
        Some(Object::Reference(next)) => {
            dict.insert(Name::from(*first), Object::Reference(next));
            Ok(Stripped::Follow(next, rest))
        }
        Some(Object::Dict(mut inner)) if depth < 4 => {
            let result = strip_dict(&mut inner, rest, depth + 1);
            if !inner.is_empty() {
                dict.insert(Name::from(*first), Object::Dict(inner));
            }
            result
        }
        Some(other) => {
            dict.insert(Name::from(*first), other);
            Err("already absent".to_owned())
        }
        None => Err("already absent".to_owned()),
    }
}
