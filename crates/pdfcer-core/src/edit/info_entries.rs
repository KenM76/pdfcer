//! Editing the document information dictionary (ISO 32000-1 §14.3.3) and
//! keeping the catalog's XMP packet (§14.3.2) in step with it
//! ([`EditSession::set_info_field`], [`EditSession::set_info_entry`],
//! [`EditSession::info_entries`]).
//!
//! `/Info` key → XMP property (spec RAG `xmp__ref__info_to_xmp_mapping.md`):
//! `Title` → `dc:title` (Alt), `Author` → `dc:creator` (Seq), `Subject` →
//! `dc:description` (Alt), `Keywords` → `pdf:Keywords`, `Creator` →
//! `xmp:CreatorTool`, any other key → `pdfx:<key>` (XMP Part 3 §2.2.1).
//! Dates are never written: pdfcer does not set `/ModDate` or
//! `xmp:ModifyDate` on its own (see [`InfoField`]).

use super::xmp_rewrite::{self, Change, Failure, Packet, Policy, Property, Shape};
use super::{
    Command, CommandKind, EditError, EditSession, InfoField, InfoText, ObjectWrite,
    decode_text_string, encode_text_string,
};
use crate::crypto::PermissionBit;
use crate::object::{Dict, Name, ObjId, Object, Stream};

const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const PDF_NS: &str = "http://ns.adobe.com/pdf/1.3/";
const XMP_NS: &str = "http://ns.adobe.com/xap/1.0/";
const PDFX_NS: &str = "http://ns.adobe.com/pdfx/1.3/";
/// ISO 32000-1 Annex C: the longest name a conforming reader must accept.
const MAX_KEY_BYTES: usize = 127;

/// One entry of the document information dictionary, as
/// [`EditSession::info_entries`] returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InfoEntry {
    /// The key, without the leading `/`, as decoded name bytes.
    pub key: Vec<u8>,
    /// The value.
    pub value: InfoValue,
}

/// The value of an [`InfoEntry`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum InfoValue {
    /// A text string (every standard key except `/Trapped`, and what
    /// [`EditSession::set_info_entry`] writes). Dates are text strings too.
    Text(InfoText),
    /// A name, such as `/Trapped`'s `/True`, `/False` or `/Unknown`.
    Name(Vec<u8>),
    /// Any other object, which no standard key uses.
    Other,
}

/// What an `/Info` edit did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InfoEditOutcome {
    /// Whether `/Info` changed. With [`XmpSync::Updated`] false or true,
    /// one undo entry was recorded when either changed, none otherwise.
    pub info_changed: bool,
    /// What happened to the XMP copy of the value.
    pub xmp: XmpSync,
}

impl InfoEditOutcome {
    /// Whether the edit recorded an undo entry.
    #[must_use]
    pub const fn recorded(&self) -> bool {
        self.info_changed || matches!(self.xmp, XmpSync::Updated { .. })
    }
}

/// What an `/Info` edit did to the catalog's XMP packet.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum XmpSync {
    /// The catalog has no `/Metadata`; there is nothing to keep in step.
    NoPacket,
    /// The property was set or removed, in the same undo entry.
    Updated {
        /// The XMP property, as `prefix:name`.
        property: String,
    },
    /// The packet already said this.
    InStep {
        /// The XMP property, as `prefix:name`.
        property: String,
    },
    /// The packet was left alone and may now disagree with `/Info`.
    Skipped {
        /// The XMP property, as `prefix:name`; `None` when the key has
        /// none (it is not UTF-8).
        property: Option<String>,
        /// Why.
        reason: XmpSkip,
    },
}

/// Why an `/Info` edit left the XMP packet alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum XmpSkip {
    /// [`XmpMirror::Never`] is set.
    Off,
    /// The packet is not a stream, does not decode, is not UTF-8 XML, or
    /// has no `rdf:RDF`.
    Unreadable,
    /// The document is signed and [`SignedPacket::Leave`] is set.
    Signed,
    /// The key is custom and the packet claims PDF/A-1, -2 or -3, where a
    /// `pdfx:` property without an extension schema breaks conformance
    /// (ISO 19005-1 §6.7.9, 19005-2 §6.6.2.3.1; `XI-A4`).
    PdfaCustomKey {
        /// The claimed `pdfaid:part`.
        part: u32,
    },
    /// The value holds a character XML 1.0 cannot carry, or the key is
    /// not UTF-8.
    Unrepresentable,
}

impl XmpSkip {
    /// A stable kebab-case token for a shell to print.
    #[must_use]
    pub const fn token(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Unreadable => "unreadable",
            Self::Signed => "signed",
            Self::PdfaCustomKey { .. } => "pdfa-custom-key",
            Self::Unrepresentable => "unrepresentable",
        }
    }
}

/// How `/Info` edits treat the XMP packet; set with
/// [`EditSession::set_xmp_sync`]. The default is the safe answer to each
/// question the standards leave open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct XmpSyncOptions {
    /// Whether to mirror at all.
    pub mirror: XmpMirror,
    /// What to do when the document is signed.
    pub signed: SignedPacket,
    /// What happens to `dc:title`/`dc:description` items in other
    /// languages (`XI-A3`).
    pub languages: AltLanguages,
    /// How `Author` becomes `dc:creator` items (`XI-A2`).
    pub authors: AuthorItems,
}

/// Whether `/Info` edits are mirrored into XMP.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum XmpMirror {
    /// Mirror (§14.3.2: the two should agree).
    #[default]
    Always,
    /// Write `/Info` only; the outcome says [`XmpSkip::Off`].
    Never,
}

/// Whether a signed document's packet is rewritten.
///
/// An incremental save leaves the signed bytes intact either way, but a
/// metadata change after signing is a change a validator may flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SignedPacket {
    /// Leave it; the outcome says [`XmpSkip::Signed`].
    #[default]
    Leave,
    /// Rewrite it like any other.
    Update,
}

/// What happens to a Language Alternative's non-`x-default` items when
/// its value is set (spec RAG `XI-A3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum AltLanguages {
    /// Drop the item that repeated the old `x-default` value; keep the
    /// rest (the recommended reading).
    #[default]
    ReplaceDuplicates,
    /// Keep every other item.
    Keep,
    /// Keep only the new `x-default` item.
    Drop,
}

/// How `Author` becomes `dc:creator` items (spec RAG `XI-A2`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum AuthorItems {
    /// One item holding the whole string: lossless, and the only form
    /// PDF/A-1 allows.
    #[default]
    Whole,
    /// One item per `;`-separated name. Falls back to
    /// [`AuthorItems::Whole`] in a packet claiming PDF/A-1.
    SplitOnSemicolons,
}

/// An `/Info` key [`EditSession::set_info_entry`] will not write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReservedInfoKey {
    /// Governed by [`ProducerPolicy`](crate::writer::ProducerPolicy) (R41).
    Producer,
    /// A §7.9.4 date with no editor yet.
    CreationDate,
    /// A §7.9.4 date pdfcer never sets on its own.
    ModDate,
    /// A name (`/True`, `/False`, `/Unknown`), not text.
    Trapped,
}

/// Why a key is not a usable `/Info` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum InfoKeyProblem {
    /// No bytes.
    Empty,
    /// A NUL byte, which no PDF name may hold (§7.3.5).
    ContainsNul,
    /// Longer than the 127 bytes ISO 32000-1 Annex C lets a reader limit
    /// names to.
    TooLong,
}

fn reserved(key: &[u8]) -> Option<ReservedInfoKey> {
    match key {
        b"Producer" => Some(ReservedInfoKey::Producer),
        b"CreationDate" => Some(ReservedInfoKey::CreationDate),
        b"ModDate" => Some(ReservedInfoKey::ModDate),
        b"Trapped" => Some(ReservedInfoKey::Trapped),
        _ => None,
    }
}

fn standard_field(key: &[u8]) -> Option<InfoField> {
    InfoField::all().iter().copied().find(|f| f.key() == key)
}

/// The XMP property `key` maps to; `None` for a custom key that is not
/// UTF-8.
fn property_for(key: &[u8]) -> Option<Property> {
    let (ns, prefix, local, shape) = match standard_field(key) {
        Some(InfoField::Title) => (DC_NS, "dc", "title", Shape::LangAlt),
        Some(InfoField::Author) => (DC_NS, "dc", "creator", Shape::Seq),
        Some(InfoField::Subject) => (DC_NS, "dc", "description", Shape::LangAlt),
        Some(InfoField::Keywords) => (PDF_NS, "pdf", "Keywords", Shape::Simple),
        Some(InfoField::Creator) => (XMP_NS, "xmp", "CreatorTool", Shape::Simple),
        None => {
            let local = xmp_rewrite::escape_key(std::str::from_utf8(key).ok()?);
            return Some(Property {
                ns: PDFX_NS,
                prefix: "pdfx",
                local,
                shape: Shape::Simple,
            });
        }
    };
    Some(Property {
        ns,
        prefix,
        local: local.to_owned(),
        shape,
    })
}

/// The planned `/Info` write, and the trailer change when it creates one.
struct InfoWrite {
    write: ObjectWrite,
    trailer: Option<(Dict, Dict)>,
}

/// The catalog's `/Metadata`, as far as it resolves.
enum Metadata {
    Absent,
    Malformed,
    Stream(ObjId, Stream),
}

impl EditSession {
    /// Set or clear one standard document-information field (§14.3.3),
    /// keeping the XMP packet in step.
    ///
    /// `Some(text)` sets it; `None` removes the entry (see [`Dict::remove`]
    /// for why removal rather than an explicit `null`). Text is encoded by
    /// [`encode_text_string`]. When the catalog has `/Metadata`, the
    /// matching XMP property (module docs) is set or removed in the **same**
    /// undo entry, under [`EditSession::xmp_sync`]; the outcome says what
    /// happened to it, including why it was left alone.
    ///
    /// A file with no `/Info` gets one, with the trailer's `/Info`
    /// reference (Table 15). That is not a fingerprint under R41: the
    /// operator asked, by name, for this dictionary to exist.
    ///
    /// Setting a field to the value it has, or clearing an absent one,
    /// records no undo entry and leaves the redo stack alone, unless the
    /// XMP copy still disagrees, which one entry then fixes.
    ///
    /// # Errors
    ///
    /// [`EditError::DocumentEncrypted`] when permissions forbid editing;
    /// [`EditError::NotADictionary`] if `/Info` points at a non-dictionary
    /// and a value is being set; [`EditError::ObjectNumbersExhausted`] or
    /// [`EditError::ObjectCreationWouldExposeHiddenObjects`] when `/Info`
    /// must be created and cannot be.
    pub fn set_info_field(
        &mut self,
        field: InfoField,
        value: Option<&str>,
    ) -> Result<InfoEditOutcome, EditError> {
        let kind = match value {
            Some(_) => CommandKind::SetInfoField(field),
            None => CommandKind::ClearInfoField(field),
        };
        self.edit_info(field.key(), value, kind)
    }

    /// Set or clear any `/Info` entry by key (§14.3.3: *"the dictionary
    /// may contain other keys"*), keeping the XMP packet in step exactly as
    /// [`EditSession::set_info_field`] does. A custom key maps to
    /// `pdfx:<key>`, escaped per XMP Part 3 §2.2.1.
    ///
    /// `key` is the decoded name without `/`. A standard key is recorded as
    /// [`CommandKind::SetInfoField`]; any other as
    /// [`CommandKind::SetInfoEntry`].
    ///
    /// # Example
    ///
    /// ```no_run
    /// # fn demo(session: &mut pdfcer_core::edit::EditSession) -> Result<(), pdfcer_core::edit::EditError> {
    /// let outcome = session.set_info_entry(b"Department", Some("Tooling"))?;
    /// assert!(outcome.info_changed);
    /// # Ok(()) }
    /// ```
    ///
    /// # Errors
    ///
    /// [`EditError::InfoKeyReserved`] for `Producer`, `CreationDate`,
    /// `ModDate` and `Trapped`; [`EditError::InvalidInfoKey`] for an empty,
    /// NUL-holding or over-long key; otherwise as
    /// [`EditSession::set_info_field`].
    pub fn set_info_entry(
        &mut self,
        key: &[u8],
        value: Option<&str>,
    ) -> Result<InfoEditOutcome, EditError> {
        if let Some(key) = reserved(key) {
            return Err(EditError::InfoKeyReserved { key });
        }
        let problem = if key.is_empty() {
            Some(InfoKeyProblem::Empty)
        } else if key.contains(&0) {
            Some(InfoKeyProblem::ContainsNul)
        } else if key.len() > MAX_KEY_BYTES {
            Some(InfoKeyProblem::TooLong)
        } else {
            None
        };
        if let Some(problem) = problem {
            return Err(EditError::InvalidInfoKey { problem });
        }
        let kind = match (standard_field(key), value) {
            (Some(f), Some(_)) => CommandKind::SetInfoField(f),
            (Some(f), None) => CommandKind::ClearInfoField(f),
            (None, Some(_)) => CommandKind::SetInfoEntry,
            (None, None) => CommandKind::ClearInfoEntry,
        };
        self.edit_info(key, value, kind)
    }

    /// Every `/Info` entry with its full value, in dictionary order,
    /// unsaved edits included. Empty when there is no `/Info` dictionary.
    /// An indirect value is resolved one level.
    #[must_use]
    pub fn info_entries(&self) -> Vec<InfoEntry> {
        let Some(Object::Dict(dict)) = self.info_id().and_then(|id| self.value(id)) else {
            return Vec::new();
        };
        dict.iter()
            .map(|(key, value)| {
                let value = match value {
                    Object::Reference(id) => self.value(*id).unwrap_or(&Object::Null),
                    v => v,
                };
                let value = match value {
                    Object::String(bytes) => InfoValue::Text(decode_text_string(bytes)),
                    Object::Name(name) => InfoValue::Name(name.as_bytes().to_vec()),
                    _ => InfoValue::Other,
                };
                InfoEntry {
                    key: key.as_bytes().to_vec(),
                    value,
                }
            })
            .collect()
    }

    /// Set how `/Info` edits treat the XMP packet. Session setting; not
    /// saved in the document.
    pub const fn set_xmp_sync(&mut self, options: XmpSyncOptions) {
        self.xmp_sync = options;
    }

    /// How `/Info` edits treat the XMP packet.
    #[must_use]
    pub const fn xmp_sync(&self) -> XmpSyncOptions {
        self.xmp_sync
    }

    fn edit_info(
        &mut self,
        key: &[u8],
        value: Option<&str>,
        kind: CommandKind,
    ) -> Result<InfoEditOutcome, EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        let info = self.plan_info_write(key, value)?;
        let (xmp, xmp_write) = self.plan_xmp_write(key, value);
        let outcome = InfoEditOutcome {
            info_changed: info.is_some(),
            xmp,
        };
        if info.is_none() && xmp_write.is_none() {
            return Ok(outcome);
        }
        let mut objects = Vec::with_capacity(2);
        let mut trailer = None;
        if let Some(info) = info {
            if info.trailer.is_some() {
                self.next_number = self.next_number.and_then(|n| n.checked_add(1));
            }
            objects.push(info.write);
            trailer = info.trailer;
        }
        objects.extend(xmp_write);
        self.commit(Command {
            kind,
            objects,
            removals: Vec::new(),
            trailer,
        });
        Ok(outcome)
    }

    /// The `/Info` write for this edit, or `None` when `/Info` already
    /// says it.
    fn plan_info_write(
        &self,
        key: &[u8],
        value: Option<&str>,
    ) -> Result<Option<InfoWrite>, EditError> {
        let Some(id) = self.info_id() else {
            return value.map(|text| self.plan_new_info(key, text)).transpose();
        };
        let Some(Object::Dict(current)) = self.value(id) else {
            // Clearing on a malformed /Info is already true; setting must
            // refuse rather than overwrite whatever is really there.
            return match value {
                None => Ok(None),
                Some(_) => Err(EditError::NotADictionary {
                    id,
                    key: standard_field(key).map_or("Info", super::field_key_str),
                }),
            };
        };
        let mut updated = current.clone();
        match value {
            Some(text) => updated.insert(Name::from(key), Object::String(encode_text_string(text))),
            None => {
                updated.remove(key);
            }
        }
        if updated == *current {
            return Ok(None);
        }
        Ok(Some(InfoWrite {
            write: ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(Object::Dict(updated)),
            },
            trailer: None,
        }))
    }

    /// A new `/Info` holding one entry, and the trailer reference to it.
    fn plan_new_info(&self, key: &[u8], text: &str) -> Result<InfoWrite, EditError> {
        // Creating an object raises /Size, which would expose every entry
        // this file's /Size suppresses; see `Document::suppressed_object_count`.
        let suppressed = self.base.suppressed_object_count();
        if suppressed > 0 {
            return Err(EditError::ObjectCreationWouldExposeHiddenObjects { count: suppressed });
        }
        let id = ObjId::new(
            self.next_number.ok_or(EditError::ObjectNumbersExhausted)?,
            0,
        );
        let mut info = Dict::new();
        info.insert(Name::from(key), Object::String(encode_text_string(text)));
        let mut trailer_after = self.trailer.clone();
        trailer_after.insert(Name::from(b"Info"), Object::Reference(id));
        Ok(InfoWrite {
            write: ObjectWrite {
                id,
                before: None,
                after: Some(Object::Dict(info)),
            },
            trailer: Some((self.trailer.clone(), trailer_after)),
        })
    }

    fn metadata(&self) -> Metadata {
        let catalog = self
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .and_then(|id| self.value(id))
            .and_then(Object::as_dict);
        let Some(entry) = catalog.and_then(|c| c.get(b"Metadata")) else {
            return Metadata::Absent;
        };
        match entry.as_reference().map(|id| (id, self.value(id))) {
            Some((id, Some(Object::Stream(s)))) => Metadata::Stream(id, s.clone()),
            // §7.3.10: a reference to nothing is null, i.e. no packet.
            Some((_, None | Some(Object::Null))) => Metadata::Absent,
            _ => Metadata::Malformed,
        }
    }

    /// What happens to the XMP copy, and the metadata stream write when
    /// it changes. Stages the new packet bytes only when it returns a write.
    fn plan_xmp_write(
        &mut self,
        key: &[u8],
        value: Option<&str>,
    ) -> (XmpSync, Option<ObjectWrite>) {
        let property = property_for(key);
        let name = property.as_ref().map(Property::qualified);
        let skip = |reason| XmpSync::Skipped {
            property: name.clone(),
            reason,
        };
        let (id, stream) = match self.metadata() {
            Metadata::Absent => return (XmpSync::NoPacket, None),
            Metadata::Malformed => return (skip(XmpSkip::Unreadable), None),
            Metadata::Stream(id, stream) => (id, stream),
        };
        let Some(property) = property else {
            return (skip(XmpSkip::Unrepresentable), None);
        };
        let options = self.xmp_sync;
        if options.mirror == XmpMirror::Never {
            return (skip(XmpSkip::Off), None);
        }
        if options.signed == SignedPacket::Leave && crate::signature::census(&self.graph()).any() {
            return (skip(XmpSkip::Signed), None);
        }
        let decoded = self
            .view()
            .slice(stream.data_span)
            .and_then(|raw| crate::filters::decode_stream(&stream.dict, raw).ok());
        let custom = standard_field(key).is_none();
        let rewritten = match decoded {
            Some(bytes) => rewrite_packet(&bytes, &property, value, options, custom),
            None => Err(XmpSkip::Unreadable),
        };
        let property = property.qualified();
        match rewritten {
            Err(reason) => (skip(reason), None),
            Ok(None) => (XmpSync::InStep { property }, None),
            Ok(Some(bytes)) => {
                let write = ObjectWrite {
                    id,
                    before: self.state.get(&id).cloned(),
                    after: Some(Object::Stream(self.unfiltered_stream(stream.dict, &bytes))),
                };
                (XmpSync::Updated { property }, Some(write))
            }
        }
    }

    /// `dict`'s stream carrying `data` with no filter (`XP-A3`: a packet
    /// should be readable by a tool that only scans bytes).
    fn unfiltered_stream(&mut self, mut dict: Dict, data: &[u8]) -> Stream {
        for key in [&b"Filter"[..], b"DecodeParms", b"DL"] {
            dict.remove(key);
        }
        dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(data.len()).unwrap_or(i64::MAX)),
        );
        Stream {
            dict,
            data_span: self.stage_bytes(data),
        }
    }
}

/// The packet's new bytes, `None` when it already says this, or why it
/// cannot be updated.
fn rewrite_packet(
    bytes: &[u8],
    property: &Property,
    value: Option<&str>,
    options: XmpSyncOptions,
    custom: bool,
) -> Result<Option<Vec<u8>>, XmpSkip> {
    let unreadable = |_: Failure| XmpSkip::Unreadable;
    let packet = Packet::read(bytes).map_err(unreadable)?;
    let part = xmp_rewrite::pdfa_part(packet.text).map_err(unreadable)?;
    if let Some(part @ 1..=3) = part.filter(|_| custom) {
        return Err(XmpSkip::PdfaCustomKey { part });
    }
    let authors = match part {
        Some(1) => AuthorItems::Whole,
        _ => options.authors,
    };
    let policy = Policy {
        languages: options.languages,
        authors,
    };
    match xmp_rewrite::rewrite(packet.text, property, value, policy) {
        Ok(Change::InStep) => Ok(None),
        Ok(Change::Rewritten(text)) => Ok(Some(packet.encode(&text))),
        Err(Failure::Unreadable) => Err(XmpSkip::Unreadable),
        Err(Failure::Unrepresentable) => Err(XmpSkip::Unrepresentable),
    }
}

#[cfg(test)]
mod tests;
