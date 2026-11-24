//! The PDF/UA fixups that write document-level metadata: showing the title
//! (ISO 14289-1 §7.1 ¶10, ISO 14289-2 §8.11.2), the XMP `dc:title` it shows
//! (ISO 14289-1 §7.1 ¶8), and the `pdfuaid` identification (ISO 14289-1 §5,
//! ISO 14289-2 §5).

use super::info_entries::Metadata;
use super::xmp_rewrite::{self, Change, Packet, Policy, Property, Shape};
use super::{Command, CommandKind, EditError, EditSession, InfoField, ObjectWrite, PermissionBit};
use crate::PdfVersion;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::PageTreeError;
use crate::preflight::{
    FindingKind, PdfuaPart, PdfuaReport, Severity, pdfua_header_ok, validate_pdfua, xmp_has_title,
};

const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
/// The PDF/UA identification schema (ISO 14289-1 §5 Table 1).
const PDFUAID_NS: &str = "http://www.aiim.org/pdfua/ns/id/";
/// A packet holding nothing yet (ISO 16684-1 §7.3, spec RAG
/// `xmp__ref__packet_and_dates.md`).
const EMPTY_PACKET: &str = "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n \
<rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n  \
<rdf:Description rdf:about=\"\"/>\n \
</rdf:RDF>\n</x:xmpmeta>\n<?xpacket end=\"w\"?>";
/// The findings the identification itself answers, which therefore never
/// withhold it.
const IDENTIFICATION_ROWS: &[&str] = &["PUI-06-002", "PUI2-5-1", "PUI2-5-2", "PUI2-5-5"];

/// Where the title [`EditSession::set_display_doc_title`] shows came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum DocTitleSource {
    /// The XMP metadata already had a `dc:title`.
    Existing,
    /// The caller supplied it; it was written to `/Info /Title` and to
    /// `dc:title`.
    Supplied,
    /// The document's `/Info /Title`, copied into `dc:title`.
    CopiedFromInfo,
}

/// What [`EditSession::set_display_doc_title`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct DocTitleOutcome {
    /// Whether anything was written (one undo entry when true).
    pub changed: bool,
    /// Where the title came from.
    pub source: DocTitleSource,
    /// Whether a new XMP metadata stream was created to hold `dc:title`.
    pub packet_created: bool,
}

impl EditSession {
    /// Make viewers show the document's title rather than its file name:
    /// `/ViewerPreferences /DisplayDocTitle true` (ISO 14289-1 §7.1 ¶10,
    /// ISO 14289-2 §8.11.2; Matterhorn 07-001/07-002), with the title it
    /// shows in the XMP `dc:title` (ISO 14289-1 §7.1 ¶8, Matterhorn 06-003).
    ///
    /// The title, in order: `title` when given (written to `/Info /Title`
    /// and `dc:title`); the packet's existing `dc:title`; the `/Info /Title`,
    /// copied into `dc:title`. With none of them the call is refused: pdfcer
    /// never invents a title. A document with no XMP packet gets a minimal
    /// one when a title has to be written into it. One undo step.
    ///
    /// # Errors
    ///
    /// [`EditError::DocumentTitleMissing`] when there is no title;
    /// [`EditError::XmpPacketUnwritable`] when the packet cannot be updated;
    /// [`EditError::ObjectCreationWouldExposeHiddenObjects`] when a packet
    /// would have to be created; the encryption and certification refusals.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::document::Document;
    /// use pdfcer_core::edit::{DocTitleSource, EditError, EditSession};
    ///
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let doc = Document::from_bytes(
    ///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec(),
    /// )?;
    /// let mut session = EditSession::new(doc);
    /// let outcome = session.set_display_doc_title(Some("Quarterly report"))?;
    /// assert_eq!(outcome.source, DocTitleSource::Supplied);
    /// # Ok(())
    /// # }
    /// ```
    pub fn set_display_doc_title(
        &mut self,
        title: Option<&str>,
    ) -> Result<DocTitleOutcome, EditError> {
        self.check_metadata_edit()?;
        let title = title.map(str::trim).filter(|t| !t.is_empty());
        let packet = crate::font_unembed::catalog_xmp(&self.view());
        let has_dc = packet.as_deref().is_some_and(xmp_has_title);
        let info = self
            .info_text(InfoField::Title)
            .map(|t| t.text)
            .filter(|t| !t.trim().is_empty());
        let (source, write): (DocTitleSource, Option<String>) = match (title, has_dc, info) {
            (Some(t), _, _) => (DocTitleSource::Supplied, Some(t.to_owned())),
            (None, true, _) => (DocTitleSource::Existing, None),
            (None, false, Some(i)) => (DocTitleSource::CopiedFromInfo, Some(i)),
            (None, false, None) => return Err(EditError::DocumentTitleMissing),
        };
        let before = self.undo_top_serial();
        let mut packet_created = false;
        self.grouped(
            CommandKind::SetDisplayDocTitle,
            |s| -> Result<(), EditError> {
                if let Some(t) = &write {
                    if source == DocTitleSource::Supplied {
                        s.set_info_field(InfoField::Title, Some(t))?;
                    }
                    let prop = dc_title();
                    packet_created = s
                        .write_xmp(CommandKind::SetDisplayDocTitle, &[(prop, t.as_str())])?
                        .1;
                }
                s.set_viewer_preference_true(b"DisplayDocTitle")
            },
        )?;
        Ok(DocTitleOutcome {
            changed: self.undo_top_serial() != before,
            source,
            packet_created,
        })
    }

    /// Claim PDF/UA conformance in the XMP metadata: `pdfuaid:part` (and,
    /// for PDF/UA-2, `pdfuaid:rev` `2024`), ISO 14289-1 §5 / ISO 14289-2 §5.
    ///
    /// The claim is written only when [`Self::validate_pdfua`] finds nothing
    /// failing but the identification itself (and, for PDF/UA-2, the file
    /// is PDF 2.0); anything else withholds it, so pdfcer never claims
    /// conformance for a file its checks show does not conform. Passing is
    /// not a full conformance verdict: the needs-a-person findings and the
    /// report's `not_checked` requirements still stand. Returns whether anything was written (one undo
    /// step).
    ///
    /// # Errors
    ///
    /// [`EditError::PdfuaIdentificationWithheld`] listing each failing
    /// finding; [`EditError::XmpPacketUnwritable`]; the encryption and
    /// certification refusals.
    pub fn write_pdfua_identification(&mut self, part: PdfuaPart) -> Result<bool, EditError> {
        self.check_metadata_edit()?;
        let report = self.validate_pdfua(part);
        let mut reasons: Vec<String> = report
            .findings
            .iter()
            .filter(|f| f.severity == Severity::Error)
            .filter(|f| !IDENTIFICATION_ROWS.contains(&f.rule_id.as_str()))
            .map(|f| format!("{}: {}", f.rule_id, f.message))
            .collect();
        let version = self.effective_version();
        if part == PdfuaPart::Ua2 && version < (PdfVersion { major: 2, minor: 0 }) {
            reasons.push(format!(
                "ISO 14289-2 §5: PDF/UA-2 is over PDF 2.0, and this file is PDF {version}"
            ));
        }
        if !reasons.is_empty() {
            return Err(EditError::PdfuaIdentificationWithheld {
                part: part.number(),
                reasons,
            });
        }
        let number = part.number().to_string();
        let mut props = vec![(pdfuaid("part"), number.as_str())];
        if part == PdfuaPart::Ua2 {
            props.push((pdfuaid("rev"), "2024"));
        }
        Ok(self
            .write_xmp(CommandKind::WritePdfuaIdentification, &props)?
            .0)
    }

    /// [`validate_pdfua`] on the session as it stands. The header row
    /// (veraPDF 6.1-1) reads file bytes, which an edited session has none
    /// of; a save keeps the opened file's header, so that row is answered
    /// from the opened file.
    #[must_use]
    pub fn validate_pdfua(&self, part: PdfuaPart) -> PdfuaReport {
        let mut report = validate_pdfua(&self.view(), part);
        let header = self.base.view().bytes().is_some_and(pdfua_header_ok);
        if header {
            report
                .findings
                .retain(|f| !(f.rule_id == "PUI-V-6.1-1" && f.kind == FindingKind::Incomplete));
        }
        report
    }

    /// The gate every catalog-level PDF/UA fixup passes.
    pub(super) fn check_metadata_edit(&self) -> Result<(), EditError> {
        if crate::encryption_gate::forbids(&self.base, &[PermissionBit::ModifyContents]) {
            return Err(EditError::DocumentEncrypted);
        }
        self.check_certification()?;
        Ok(())
    }

    /// The catalog's id and a copy of its dictionary.
    pub(super) fn catalog_entry(&self) -> Result<(ObjId, Dict), EditError> {
        let id = self
            .trailer
            .get(b"Root")
            .and_then(Object::as_reference)
            .ok_or(EditError::PageTree(PageTreeError::NoPageTreeRoot))?;
        match self.value(id) {
            Some(Object::Dict(d)) => Ok((id, d.clone())),
            _ => Err(EditError::NotADictionary {
                id,
                key: "ViewerPreferences",
            }),
        }
    }

    /// Set `/ViewerPreferences /<key> true`, in the preferences object when
    /// it is indirect, else on the catalog. Writes nothing when already so.
    fn set_viewer_preference_true(&mut self, key: &'static [u8]) -> Result<(), EditError> {
        let (catalog_id, mut catalog) = self.catalog_entry()?;
        let held = catalog.get(b"ViewerPreferences").cloned();
        let (target, mut prefs) = match held {
            Some(Object::Reference(id)) => match self.value(id) {
                Some(Object::Dict(d)) => (id, d.clone()),
                _ => (catalog_id, Dict::new()),
            },
            Some(Object::Dict(d)) => (catalog_id, d),
            _ => (catalog_id, Dict::new()),
        };
        if matches!(prefs.get(key), Some(Object::Boolean(true))) {
            return Ok(());
        }
        prefs.insert(Name::from(key), Object::Boolean(true));
        let after = if target == catalog_id {
            catalog.insert(Name::from(b"ViewerPreferences"), Object::Dict(prefs));
            catalog
        } else {
            prefs
        };
        self.commit(Command {
            kind: CommandKind::SetDisplayDocTitle,
            objects: vec![ObjectWrite {
                id: target,
                before: self.state.get(&target).cloned(),
                after: Some(Object::Dict(after)),
            }],
            removals: Vec::new(),
            trailer: None,
        });
        Ok(())
    }

    /// Set each property in the catalog's XMP packet, creating a minimal
    /// packet when there is none, as one command. Returns `(changed,
    /// created)`.
    fn write_xmp(
        &mut self,
        kind: CommandKind,
        props: &[(Property, &str)],
    ) -> Result<(bool, bool), EditError> {
        let unwritable = |reason| EditError::XmpPacketUnwritable { reason };
        let (existing, bytes) = match self.metadata() {
            Metadata::Malformed => {
                return Err(unwritable("the catalog's /Metadata is not a stream"));
            }
            Metadata::Absent => (None, EMPTY_PACKET.as_bytes().to_vec()),
            Metadata::Stream(id, stream) => {
                let decoded = self
                    .view()
                    .slice(stream.data_span)
                    .and_then(|raw| crate::filters::decode_stream(&stream.dict, raw).ok())
                    .ok_or(unwritable("its stream cannot be decoded"))?;
                (Some((id, stream)), decoded)
            }
        };
        let packet = Packet::read(&bytes).map_err(|_| unwritable("it is not UTF-8 XMP"))?;
        let policy = Policy {
            languages: self.xmp_sync.languages,
            authors: self.xmp_sync.authors,
        };
        let mut text = packet.text.to_owned();
        let mut changed = existing.is_none();
        for (prop, value) in props {
            match xmp_rewrite::rewrite(&text, prop, Some(value), policy) {
                Ok(Change::InStep) => {}
                Ok(Change::Rewritten(t)) => {
                    text = t;
                    changed = true;
                }
                Err(_) => {
                    return Err(unwritable(
                        "it is not well-formed XMP, or the value is not XML",
                    ));
                }
            }
        }
        if !changed {
            return Ok((false, false));
        }
        let data = packet.encode(&text);
        match existing {
            Some((id, stream)) => {
                self.commit_xmp_update(kind, id, stream, &data);
                Ok((true, false))
            }
            None => {
                self.commit_new_packet(kind, &data)?;
                Ok((true, true))
            }
        }
    }

    fn commit_xmp_update(&mut self, kind: CommandKind, id: ObjId, stream: Stream, data: &[u8]) {
        let after = Object::Stream(self.unfiltered_stream(stream.dict, data));
        self.commit(Command {
            kind,
            objects: vec![ObjectWrite {
                id,
                before: self.state.get(&id).cloned(),
                after: Some(after),
            }],
            removals: Vec::new(),
            trailer: None,
        });
    }

    /// A new `/Type /Metadata /Subtype /XML` stream (ISO 32000-1 §14.3.2
    /// Table 315), unfiltered, referenced from the catalog.
    fn commit_new_packet(&mut self, kind: CommandKind, data: &[u8]) -> Result<(), EditError> {
        let suppressed = self.base.suppressed_object_count();
        if suppressed > 0 {
            return Err(EditError::ObjectCreationWouldExposeHiddenObjects { count: suppressed });
        }
        let (catalog_id, mut catalog) = self.catalog_entry()?;
        let id = ObjId::new(self.alloc_number()?, 0);
        let mut dict = Dict::new();
        dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"Metadata")));
        dict.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"XML")));
        let stream = self.unfiltered_stream(dict, data);
        catalog.insert(Name::from(b"Metadata"), Object::Reference(id));
        self.commit(Command {
            kind,
            objects: vec![
                ObjectWrite {
                    id,
                    before: None,
                    after: Some(Object::Stream(stream)),
                },
                ObjectWrite {
                    id: catalog_id,
                    before: self.state.get(&catalog_id).cloned(),
                    after: Some(Object::Dict(catalog)),
                },
            ],
            removals: Vec::new(),
            trailer: None,
        });
        Ok(())
    }
}

fn dc_title() -> Property {
    Property {
        ns: DC_NS,
        prefix: "dc",
        local: "title".to_owned(),
        shape: Shape::LangAlt,
    }
}

fn pdfuaid(local: &str) -> Property {
    Property {
        ns: PDFUAID_NS,
        prefix: "pdfuaid",
        local: local.to_owned(),
        shape: Shape::Simple,
    }
}
