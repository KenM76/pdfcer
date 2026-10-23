//! The catalog `/Version` raise an optimize plan commits.

use super::{EditSession, ObjectWrite};
use crate::PdfVersion;
use crate::graph::ObjectGraph;
use crate::object::{Name, Object};

impl EditSession {
    /// The session's effective version: the base document's, raised by a
    /// catalog `/Version` this session wrote (ISO 32000-2 §7.7.2, the
    /// greater of header and catalog).
    pub(crate) fn effective_version(&self) -> PdfVersion {
        let base = self.base.version();
        let written = self
            .catalog_id()
            .and_then(|id| self.value(id))
            .and_then(Object::as_dict)
            .and_then(|d| d.get(b"Version"))
            .and_then(Object::as_name)
            .and_then(|n| parse(n.as_bytes()));
        written.map_or(base, |v| v.max(base))
    }

    /// The catalog writes an optimize plan commits: `/Version` set to
    /// `raise` when that is above the session's version, and the Brotli
    /// extension when `brotli`. Empty unless the catalog is an indirect
    /// dictionary and something changes.
    pub(super) fn catalog_writes(
        &self,
        raise: Option<PdfVersion>,
        brotli: bool,
    ) -> Vec<ObjectWrite> {
        let Some(id) = self.catalog_id() else {
            return Vec::new();
        };
        let Some(mut catalog) = self.value(id).and_then(Object::as_dict).cloned() else {
            return Vec::new();
        };
        let mut changed = false;
        if let Some(to) = raise.filter(|to| *to > self.effective_version()) {
            let name = format!("{}.{}", to.major, to.minor);
            catalog.insert(
                Name::from(b"Version"),
                Object::Name(Name::from(name.as_bytes())),
            );
            changed = true;
        }
        let mut writes = Vec::new();
        if brotli {
            let (in_catalog, indirect) = self.add_brotli_extension(&mut catalog);
            changed |= in_catalog;
            writes.extend(indirect);
        }
        if changed {
            writes.push(ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(Object::Dict(catalog)),
            });
        }
        writes
    }

    /// Remove the catalog `/Version` (ISO 32000-2 §7.7.2) as one undoable
    /// command, so the header alone declares the version. Returns whether
    /// there was one to remove.
    pub(crate) fn drop_catalog_version(&mut self) -> bool {
        let Some(id) = self.catalog_id() else {
            return false;
        };
        let Some(mut catalog) = self.value(id).and_then(Object::as_dict).cloned() else {
            return false;
        };
        if catalog.remove(b"Version").is_none() {
            return false;
        }
        let objects = vec![ObjectWrite {
            id,
            before: self.state.get(&id).cloned(),
            after: Some(Object::Dict(catalog)),
        }];
        self.commit(super::Command {
            kind: super::CommandKind::Optimize { count: 1 },
            objects,
            removals: Vec::new(),
            trailer: None,
        });
        true
    }
}

/// A `/Version` name such as `1.4`.
fn parse(name: &[u8]) -> Option<PdfVersion> {
    let (major, minor) = std::str::from_utf8(name).ok()?.split_once('.')?;
    Some(PdfVersion {
        major: major.parse().ok()?,
        minor: minor.parse().ok()?,
    })
}
