//! The Brotli developer extension an optimize plan that writes
//! `/BrotliDecode` adds to the catalog (EXTN-BROTLI-1; ISO 32000-2 §7.12.3
//! Table 49 shape; decision 194).

use super::{EditSession, ObjectWrite};
use crate::object::{Dict, Name, Object};

/// The plan note a Brotli change carries.
pub(super) const BROTLI_NOTE: &str = "Brotli streams need a reader that supports the PDF \
    Association Brotli extension; the catalog gets /Extensions /PDFa and /Version 2.0";

/// The extension's registered URL, which identifies an existing entry.
const URL: &[u8] = b"https://pdfa.org/resource/extension-brotli";

/// The `/PDFa` developer-extensions dictionary for Brotli.
fn brotli_extension() -> Object {
    let name = |s: &[u8]| Object::Name(Name::from(s));
    let mut d = Dict::new();
    d.insert(Name::from(b"Type"), name(b"DeveloperExtensions"));
    d.insert(Name::from(b"BaseVersion"), name(b"2.0"));
    d.insert(Name::from(b"ExtensionLevel"), Object::Integer(1));
    d.insert(
        Name::from(b"ExtensionRevision"),
        Object::String(b"2026".to_vec()),
    );
    d.insert(Name::from(b"URL"), Object::String(URL.to_vec()));
    Object::Dict(d)
}

impl EditSession {
    /// Add the Brotli extension to `catalog`'s `/Extensions`, keeping every
    /// other developer's entry and any earlier `/PDFa` one. Returns whether
    /// `catalog` changed, and the write for an indirect `/Extensions`.
    pub(super) fn add_brotli_extension(&self, catalog: &mut Dict) -> (bool, Option<ObjectWrite>) {
        match catalog.get(b"Extensions") {
            Some(Object::Reference(r)) => {
                let id = *r;
                let Some(ext) = self.value(id).and_then(Object::as_dict) else {
                    return (false, None);
                };
                let write = self.merged_extensions(ext).map(|d| ObjectWrite {
                    id,
                    before: self.state.get(&id).cloned(),
                    after: Some(Object::Dict(d)),
                });
                (false, write)
            }
            Some(Object::Dict(ext)) => match self.merged_extensions(ext) {
                Some(d) => {
                    catalog.insert(Name::from(b"Extensions"), Object::Dict(d));
                    (true, None)
                }
                None => (false, None),
            },
            _ => {
                let mut d = Dict::new();
                d.insert(Name::from(b"PDFa"), brotli_extension());
                catalog.insert(Name::from(b"Extensions"), Object::Dict(d));
                (true, None)
            }
        }
    }

    /// `ext` with the Brotli entry added, or `None` when it is already there.
    fn merged_extensions(&self, ext: &Dict) -> Option<Dict> {
        let mut out = ext.clone();
        let value = match ext.get(b"PDFa") {
            None => brotli_extension(),
            Some(v) if self.names_brotli(v) => return None,
            Some(Object::Array(items)) => {
                let mut items = items.clone();
                items.push(brotli_extension());
                Object::Array(items)
            }
            Some(other) => Object::Array(vec![other.clone(), brotli_extension()]),
        };
        out.insert(Name::from(b"PDFa"), value);
        Some(out)
    }

    /// Whether a `/PDFa` value (a dictionary, or an array of them, either
    /// possibly indirect) already declares the Brotli extension.
    fn names_brotli(&self, value: &Object) -> bool {
        let resolve = |o: &Object| match o {
            Object::Reference(r) => self.value(*r).cloned(),
            other => Some(other.clone()),
        };
        let is_brotli = |o: &Object| {
            resolve(o)
                .as_ref()
                .and_then(Object::as_dict)
                .and_then(|d| d.get(b"URL"))
                .is_some_and(|u| matches!(u, Object::String(s) if s == URL))
        };
        match resolve(value) {
            Some(Object::Array(items)) => items.iter().any(is_brotli),
            Some(other) => is_brotli(&other),
            None => false,
        }
    }
}
