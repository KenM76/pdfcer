//! What an embedded file stream says about its file besides the bytes: the
//! media type (`/Subtype`) and the dates in `/Params` (ISO 32000-2 §7.11.4,
//! Tables 45 and 46), shared by every route that embeds a file; and the
//! `/EmbeddedFiles` name-tree read and rebuild `attach_file_with` uses.

use super::{EditError, EditSession, MarkupNote};
use crate::crypto::PermissionBit;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};

/// One `/Names` key and its value (§7.9.6).
type NameTreeEntry = (Vec<u8>, Object);

impl EditSession {
    /// The guards every object-creating verb applies, in their order
    /// (encryption, certification, a filtering `/Size`), then the catalog.
    pub(super) fn attachment_catalog(&self) -> Result<(ObjId, Dict), EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::Annotate]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification_for_annotation()?;
        let suppressed = self.base.suppressed_object_count();
        if suppressed > 0 {
            return Err(EditError::ObjectCreationWouldExposeHiddenObjects { count: suppressed });
        }
        let catalog_id = self.graph().catalog_id().ok_or(EditError::NotADictionary {
            id: ObjId::new(0, 0),
            key: "Root",
        })?;
        match self.value(catalog_id) {
            Some(Object::Dict(d)) => Ok((catalog_id, d.clone())),
            _ => Err(EditError::NotADictionary {
                id: catalog_id,
                key: "Root",
            }),
        }
    }

    /// The catalog's existing `/EmbeddedFiles` tree, as written, and its
    /// entries. A root holding `/Kids` (or not a dictionary) is refused:
    /// inserting into a multi-node tree means repairing every `/Limits` up
    /// the chain, which is not built.
    pub(super) fn embedded_files_entries(
        &self,
        names_dict: &Dict,
    ) -> Result<(Option<Object>, Vec<NameTreeEntry>), EditError> {
        let existing_tree = names_dict.get(b"EmbeddedFiles").cloned();
        let mut entries = Vec::new();
        if let Some(obj) = &existing_tree {
            let node = self
                .deref_dict(Some(obj))
                .ok_or(EditError::AttachmentTreeUnsupported)?;
            if node.contains_key(b"Kids") {
                return Err(EditError::AttachmentTreeUnsupported);
            }
            if let Some(Object::Array(arr)) = self.deref_value(node.get(b"Names")) {
                // Pairs, per §7.9.6: [key1 value1 key2 value2 ...].
                for pair in arr.as_chunks::<2>().0 {
                    if let [Object::String(k), value] = pair {
                        entries.push((k.clone(), value.clone()));
                    }
                }
            }
        }
        Ok((existing_tree, entries))
    }
}

/// A single-node name-tree root holding `entries` with `key` set to `value`
/// (replacing any entry of that key), sorted as §7.9.6 requires: "lexically
/// in ascending order by key. Shorter keys shall appear before longer ones",
/// which a byte-slice sort gives, since a prefix compares less.
pub(super) fn name_tree_root(mut entries: Vec<NameTreeEntry>, key: Vec<u8>, value: Object) -> Dict {
    entries.retain(|(k, _)| k != &key);
    entries.push((key, value));
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut flat = Vec::with_capacity(entries.len() * 2);
    for (k, v) in entries {
        flat.push(Object::String(k));
        flat.push(v);
    }
    let mut root = Dict::new();
    root.insert(Name::from(b"Names"), Object::Array(flat));
    root
}

/// Options for [`EditSession::attach_file_with`](super::EditSession::attach_file_with).
///
/// # Examples
///
/// ```
/// use pdfcer_core::edit::AttachFileOptions;
///
/// let opts = AttachFileOptions::default()
///     .with_description("Quarterly figures")
///     .with_modified("D:20261128120000Z");
/// assert_eq!(opts.modified.as_deref(), Some("D:20261128120000Z"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct AttachFileOptions {
    /// The file specification's `/Desc` (§7.11.3, Table 44).
    pub description: Option<String>,
    /// A PDF date string (§7.9.4), written verbatim as the embedded file's
    /// `/Params /ModDate` and `/CreationDate`. pdfcer reads no clock, for
    /// the reasons on [`MarkupNote`]; `None` writes neither key.
    pub modified: Option<String>,
}

impl AttachFileOptions {
    /// Set the description.
    #[must_use]
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Set the date written to `/Params /ModDate` and `/CreationDate`.
    #[must_use]
    pub fn with_modified(mut self, pdf_date: impl Into<String>) -> Self {
        self.modified = Some(pdf_date.into());
        self
    }

    /// Check the date's shape, by the same rule as [`MarkupNote::validate`].
    ///
    /// # Errors
    ///
    /// [`EditError::MarkupDateMalformed`] naming the offending string.
    pub fn validate(&self) -> Result<(), EditError> {
        match &self.modified {
            Some(d) => MarkupNote::new("").at(d.clone()).validate(),
            None => Ok(()),
        }
    }
}

/// The media type [`EditSession::attach_file`](super::EditSession::attach_file)
/// and the file attachment annotation write as an embedded file's `/Subtype`:
/// looked up from the name's extension in a fixed table of common types
/// (RFC 2046 names, as Table 45 asks), or `None` for anything else, in which
/// case no `/Subtype` is written.
///
/// The extension is matched case-insensitively. The table, not content
/// sniffing, decides: the name is what the operator chose to call the file.
///
/// # Examples
///
/// ```
/// use pdfcer_core::edit::media_type_for_name;
///
/// assert_eq!(media_type_for_name("table.CSV"), Some("text/csv"));
/// assert_eq!(media_type_for_name("notes.unknownext"), None);
/// ```
pub fn media_type_for_name(name: &str) -> Option<&'static str> {
    let (_, ext) = name.rsplit_once('.')?;
    let ext = ext.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "csv" => "text/csv",
        "htm" | "html" => "text/html",
        "xml" => "application/xml",
        "json" => "application/json",
        "zip" => "application/zip",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "tif" | "tiff" => "image/tiff",
        "svg" => "image/svg+xml",
        "rtf" => "application/rtf",
        "eml" => "message/rfc822",
        "doc" => "application/msword",
        "xls" => "application/vnd.ms-excel",
        "ppt" => "application/vnd.ms-powerpoint",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => return None,
    };
    Some(mime)
}

/// Write `/Subtype` (when the name has a known media type) on an embedded
/// file stream dictionary, and the dates (when one is given) on its
/// `/Params`. The name writer escapes the media type's `/` as `#2F`
/// (§7.3.5), as Table 45 requires.
pub(crate) fn describe_embedded_file(
    file_dict: &mut Dict,
    params: &mut Dict,
    name: &str,
    date: Option<&str>,
) {
    if let Some(m) = media_type_for_name(name) {
        file_dict.insert(
            Name::from(b"Subtype"),
            Object::Name(Name(m.as_bytes().to_vec())),
        );
    }
    if let Some(d) = date {
        params.insert(
            Name::from(b"ModDate"),
            Object::String(d.as_bytes().to_vec()),
        );
        params.insert(
            Name::from(b"CreationDate"),
            Object::String(d.as_bytes().to_vec()),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_extensions_map_and_unknown_ones_do_not() {
        assert_eq!(media_type_for_name("table.csv"), Some("text/csv"));
        assert_eq!(media_type_for_name("Report.PDF"), Some("application/pdf"));
        assert_eq!(media_type_for_name("a.b.JPEG"), Some("image/jpeg"));
        assert_eq!(media_type_for_name("notes.unknownext"), None);
        assert_eq!(media_type_for_name("README"), None);
        assert_eq!(media_type_for_name("trailing."), None);
    }
}
