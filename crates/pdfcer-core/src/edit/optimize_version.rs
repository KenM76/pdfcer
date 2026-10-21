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

    /// The write setting the catalog's `/Version` to `to`, when that is
    /// above the session's version and the catalog is an indirect
    /// dictionary.
    pub(super) fn version_write(&self, to: PdfVersion) -> Option<ObjectWrite> {
        if to <= self.effective_version() {
            return None;
        }
        let id = self.catalog_id()?;
        let before = self.state.get(&id).cloned();
        let mut catalog = self.value(id)?.as_dict()?.clone();
        let name = format!("{}.{}", to.major, to.minor);
        catalog.insert(
            Name::from(b"Version"),
            Object::Name(Name::from(name.as_bytes())),
        );
        Some(ObjectWrite {
            id,
            before,
            after: Some(Object::Dict(catalog)),
        })
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
