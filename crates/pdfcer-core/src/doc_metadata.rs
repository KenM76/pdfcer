//! Every place a document carries metadata or hidden information, listed
//! item by item so each can be removed on its own
//! ([`EditSession::remove_metadata`](crate::edit::EditSession::remove_metadata)).
//!
//! Carriers, by clause (ISO 32000-2):
//! - the document information dictionary, one item per key (§14.3.3 Table 349);
//! - XMP metadata streams on the catalog and on any other object (§14.3.2);
//! - page-piece dictionaries `/PieceInfo` (§14.5) and page thumbnails `/Thumb` (§12.3.4);
//! - JavaScript actions (§12.6.4.17): `/OpenAction`, `/A`, each `/AA` trigger,
//!   and the catalog's `/Names /JavaScript` tree;
//! - document-level attachments (§7.11.4), comments (markup annotations,
//!   §12.5.6.2), layers hidden by default (§8.11.4), form field values (§12.7.4);
//! - earlier revisions kept by incremental update (§7.5.6) and the file
//!   identifier (§14.4).

mod scan;

use std::fmt;

use crate::object::ObjId;

pub use scan::metadata_inventory;

/// What kind of carrier a [`MetadataItem`] is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum MetadataKind {
    /// One key of the document information dictionary (`Title`,
    /// `Producer`, a custom key, …).
    InfoEntry,
    /// The catalog's XMP metadata stream.
    DocumentXmp,
    /// An XMP metadata stream on any other object (a page, image, font, …).
    ObjectXmp,
    /// A `/PieceInfo` dictionary: private data a producer left on the
    /// catalog, a page or a form XObject.
    PieceInfo,
    /// A page's embedded thumbnail image.
    Thumbnail,
    /// A JavaScript action, or the document-level script tree.
    JavaScript,
    /// A document-level attachment.
    Attachment,
    /// A comment (markup annotation), with its pop-up.
    Comment,
    /// A layer that is hidden when the document opens, and its content.
    HiddenLayer,
    /// The values typed into the form's fields.
    FormData,
    /// The earlier revisions an incremental update keeps in the file.
    EarlierRevisions,
    /// The file identifier pair in the trailer.
    DocumentId,
}

impl MetadataKind {
    /// Every kind, in inventory order.
    pub const ALL: [Self; 12] = [
        Self::InfoEntry,
        Self::DocumentXmp,
        Self::ObjectXmp,
        Self::PieceInfo,
        Self::Thumbnail,
        Self::JavaScript,
        Self::Attachment,
        Self::Comment,
        Self::HiddenLayer,
        Self::FormData,
        Self::EarlierRevisions,
        Self::DocumentId,
    ];

    /// A stable lower-case name: `info`, `document-xmp`, `object-xmp`,
    /// `pieceinfo`, `thumbnail`, `javascript`, `attachment`, `comment`,
    /// `hidden-layer`, `form-data`, `earlier-revisions`, `document-id`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InfoEntry => "info",
            Self::DocumentXmp => "document-xmp",
            Self::ObjectXmp => "object-xmp",
            Self::PieceInfo => "pieceinfo",
            Self::Thumbnail => "thumbnail",
            Self::JavaScript => "javascript",
            Self::Attachment => "attachment",
            Self::Comment => "comment",
            Self::HiddenLayer => "hidden-layer",
            Self::FormData => "form-data",
            Self::EarlierRevisions => "earlier-revisions",
            Self::DocumentId => "document-id",
        }
    }
}

/// Names one [`MetadataItem`] across an inventory and a removal.
///
/// The text form is stable for the same document state and is what the
/// CLI takes: `info/Author`, `xmp/12-0`, `pieceinfo/3-0`, `thumb/7-0`,
/// `js/5-0/OpenAction`, `js/30-0/AA/K`, `js/names`, `attachment/report.txt`,
/// `comment/30-0`, `layer/8-0`, `form-data`, `revisions`, `document-id`.
/// Bytes outside printable ASCII, and `#` and `/` inside a key, are written
/// `#xx` as in a PDF name (§7.3.5).
///
/// ```
/// use pdfcer_core::doc_metadata::MetadataItemId;
///
/// let id = MetadataItemId::new("info/Author");
/// assert_eq!(id.as_str(), "info/Author");
/// assert_eq!(id.to_string(), "info/Author");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct MetadataItemId(String);

impl MetadataItemId {
    /// Wrap an id's text form, as printed by an inventory.
    #[must_use]
    pub fn new(text: impl Into<String>) -> Self {
        Self(text.into())
    }

    /// The text form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The id naming `target`.
    pub(crate) fn of(target: &Target) -> Self {
        Self(target.to_text())
    }

    /// The carrier this id names, or `None` when it does not parse.
    pub(crate) fn target(&self) -> Option<Target> {
        Target::parse(&self.0)
    }
}

impl fmt::Display for MetadataItemId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// One carrier of metadata or hidden information.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct MetadataItem {
    /// What removes it.
    pub id: MetadataItemId,
    /// The carrier's kind.
    pub kind: MetadataKind,
    /// Where it is, in words: `document information`, `catalog`,
    /// `page 3`, `image object 12 0`, `annotation 30 0 on page 2`, ….
    pub location: String,
    /// A short look at the content: an info value, the start of a script,
    /// an attachment's name, a count. At most [`PREVIEW_CHARS`] characters.
    pub preview: String,
    /// What it occupies in the file: a stream's encoded data plus its
    /// dictionary, a value's serialised form, an attachment's or a
    /// revision span's bytes. Approximate for a direct value (written as
    /// pdfcer would write it).
    pub bytes: u64,
}

/// The longest [`MetadataItem::preview`], in characters.
pub const PREVIEW_CHARS: usize = 80;

/// What [`metadata_inventory`] found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct MetadataInventory {
    /// Every carrier, in a stable order: info keys, XMP, piece info,
    /// thumbnails, scripts, attachments, comments, layers, form data,
    /// earlier revisions, the identifier.
    pub items: Vec<MetadataItem>,
    /// The object walk stopped at its budget
    /// ([`MAX_REACHABLE_OBJECTS`](crate::edit::MAX_REACHABLE_OBJECTS)):
    /// the list is partial.
    pub truncated: bool,
}

/// What [`remove_metadata`](crate::edit::EditSession::remove_metadata) does
/// with the file identifier when it is asked to remove it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum DocumentIdAction {
    /// Write a fresh random pair: nothing links the file to its earlier
    /// copies, and readers that expect an `/ID` still find one (it is
    /// required in PDF 2.0, §14.4).
    #[default]
    Regenerate,
    /// Delete `/ID` from the trailer.
    Remove,
}

/// Options for [`remove_metadata`](crate::edit::EditSession::remove_metadata).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct MetadataRemoveOptions {
    /// What happens to a `document-id` item.
    pub document_id: DocumentIdAction,
}

impl MetadataRemoveOptions {
    /// Set [`Self::document_id`].
    #[must_use]
    pub const fn with_document_id(mut self, action: DocumentIdAction) -> Self {
        self.document_id = action;
        self
    }
}

/// An item [`remove_metadata`](crate::edit::EditSession::remove_metadata)
/// was asked to remove and did not.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct NotRemoved {
    /// The item.
    pub id: MetadataItemId,
    /// Why, in words.
    pub reason: String,
}

/// What [`remove_metadata`](crate::edit::EditSession::remove_metadata) did.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct MetadataRemoval {
    /// Items removed.
    pub removed: Vec<MetadataItemId>,
    /// Ids that name nothing in the current inventory.
    pub not_found: Vec<MetadataItemId>,
    /// Items found but not removed.
    pub not_removed: Vec<NotRemoved>,
    /// Objects deleted because nothing referred to them any more.
    pub objects_freed: usize,
    /// What the caller must tell the operator: always that only a full
    /// rewrite takes removed data out of the file, plus anything a removal
    /// left behind (a field's default value, a skipped signature field).
    pub disclosures: Vec<String>,
}

/// Which `/AA`-or-action slot a script sits in.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum JsSlot {
    OpenAction,
    Action,
    Additional(Vec<u8>),
}

/// The parsed form of a [`MetadataItemId`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) enum Target {
    Info(Vec<u8>),
    Xmp(ObjId),
    PieceInfo(ObjId),
    Thumb(ObjId),
    Js(ObjId, JsSlot),
    JsNames,
    Attachment(Vec<u8>),
    Comment(ObjId),
    Layer(ObjId),
    FormData,
    Revisions,
    DocumentId,
}

impl Target {
    fn to_text(&self) -> String {
        match self {
            Self::Info(k) => format!("info/{}", escape(k)),
            Self::Xmp(id) => format!("xmp/{}", obj_text(*id)),
            Self::PieceInfo(id) => format!("pieceinfo/{}", obj_text(*id)),
            Self::Thumb(id) => format!("thumb/{}", obj_text(*id)),
            Self::Js(id, JsSlot::OpenAction) => format!("js/{}/OpenAction", obj_text(*id)),
            Self::Js(id, JsSlot::Action) => format!("js/{}/A", obj_text(*id)),
            Self::Js(id, JsSlot::Additional(t)) => {
                format!("js/{}/AA/{}", obj_text(*id), escape(t))
            }
            Self::JsNames => "js/names".to_owned(),
            Self::Attachment(k) => format!("attachment/{}", escape(k)),
            Self::Comment(id) => format!("comment/{}", obj_text(*id)),
            Self::Layer(id) => format!("layer/{}", obj_text(*id)),
            Self::FormData => "form-data".to_owned(),
            Self::Revisions => "revisions".to_owned(),
            Self::DocumentId => "document-id".to_owned(),
        }
    }

    fn parse(text: &str) -> Option<Self> {
        match text {
            "js/names" => return Some(Self::JsNames),
            "form-data" => return Some(Self::FormData),
            "revisions" => return Some(Self::Revisions),
            "document-id" => return Some(Self::DocumentId),
            _ => {}
        }
        let (head, rest) = text.split_once('/')?;
        match head {
            "info" => Some(Self::Info(unescape(rest)?)),
            "attachment" => Some(Self::Attachment(unescape(rest)?)),
            "xmp" => Some(Self::Xmp(parse_obj(rest)?)),
            "pieceinfo" => Some(Self::PieceInfo(parse_obj(rest)?)),
            "thumb" => Some(Self::Thumb(parse_obj(rest)?)),
            "comment" => Some(Self::Comment(parse_obj(rest)?)),
            "layer" => Some(Self::Layer(parse_obj(rest)?)),
            "js" => {
                let (obj, slot) = rest.split_once('/')?;
                let slot = match slot {
                    "OpenAction" => JsSlot::OpenAction,
                    "A" => JsSlot::Action,
                    _ => JsSlot::Additional(unescape(slot.strip_prefix("AA/")?)?),
                };
                Some(Self::Js(parse_obj(obj)?, slot))
            }
            _ => None,
        }
    }
}

fn obj_text(id: ObjId) -> String {
    format!("{}-{}", id.num, id.generation)
}

fn parse_obj(text: &str) -> Option<ObjId> {
    let (n, g) = text.split_once('-')?;
    Some(ObjId::new(n.parse().ok()?, g.parse().ok()?))
}

/// `bytes` as name text: printable ASCII kept, `#`, `/` and everything else
/// as `#xx` (§7.3.5).
pub(crate) fn escape(bytes: &[u8]) -> String {
    let mut out = String::new();
    for &b in bytes {
        if (0x21..=0x7e).contains(&b) && b != b'#' && b != b'/' {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("#{b:02X}"));
        }
    }
    out
}

fn unescape(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut bytes = text.bytes();
    while let Some(b) = bytes.next() {
        if b == b'#' {
            let hi = char::from(bytes.next()?).to_digit(16)?;
            let lo = char::from(bytes.next()?).to_digit(16)?;
            out.push(u8::try_from(hi * 16 + lo).ok()?);
        } else {
            out.push(b);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_target_round_trips_through_its_text() {
        let id = ObjId::new(30, 2);
        let all = [
            Target::Info(b"My Key/#\xe9".to_vec()),
            Target::Xmp(id),
            Target::PieceInfo(id),
            Target::Thumb(id),
            Target::Js(id, JsSlot::OpenAction),
            Target::Js(id, JsSlot::Action),
            Target::Js(id, JsSlot::Additional(b"K".to_vec())),
            Target::JsNames,
            Target::Attachment(b"a b.txt".to_vec()),
            Target::Comment(id),
            Target::Layer(id),
            Target::FormData,
            Target::Revisions,
            Target::DocumentId,
        ];
        for t in all {
            assert_eq!(MetadataItemId::of(&t).target(), Some(t));
        }
        assert_eq!(MetadataItemId::new("nonsense/1").target(), None);
    }
}
