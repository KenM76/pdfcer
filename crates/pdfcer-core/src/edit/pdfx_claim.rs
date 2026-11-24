//! The writes [`EditSession::convert_to_pdfx`] owns: the `/GTS_PDFX`
//! output intent's condition entries, and the claim in Info and XMP.

use super::info_entries::{AltLanguages, AuthorItems, Metadata};
use super::xmp_rewrite::{self, Change, Failure, Packet, Policy, Property, Shape};
use super::{Command, CommandKind, EditError, EditSession, ObjectWrite, encode_text_string};
use crate::object::{Dict, Name, ObjId, Object};
use crate::preflight::PdfxLevel;
use crate::prepress::{PdfxConversion, PdfxConversionReport, PdfxOutputIntent};

/// `pdfxid:` (ISO 15930-7 §6.11.3; spec RAG `pdfx__ref__file_identification.md`).
const PDFXID_NS: &str = "http://www.npes.org/pdfx/ns/id/";
/// `pdfx:`, where custom Info keys are mirrored (XMP Part 3 §2.2.1).
const PDFX_NS: &str = "http://ns.adobe.com/pdfx/1.3/";
/// `pdf:` (Adobe PDF schema).
const PDF_NS: &str = "http://ns.adobe.com/pdf/1.3/";
/// `xmp:` (XMP basic schema).
const XMP_NS: &str = "http://ns.adobe.com/xap/1.0/";
/// `dc:` (Dublin Core).
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
/// A new packet (ISO 16684-1 §7.3; spec RAG `xmp__ref__packet_and_dates.md`).
const SKELETON: &str = "<?xpacket begin=\"\u{FEFF}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF \
xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\"/>\
</rdf:RDF></x:xmpmeta>\n<?xpacket end=\"w\"?>";

/// One XMP property to set or remove.
struct XmpSet {
    prop: Property,
    value: Option<String>,
    /// Not predefined in PDF/A-1 to -3, so not written into such a packet.
    pdfa_foreign: bool,
}

/// What the Info write decided, for the XMP mirror.
struct Claimed {
    version: &'static str,
    conformance: Option<&'static str>,
    trapped: bool,
    title: Option<String>,
    modified: Option<String>,
    created: Option<String>,
}

fn text_object(s: &str) -> Object {
    Object::String(encode_text_string(s))
}

fn name(n: &[u8]) -> Object {
    Object::Name(Name::from(n))
}

/// `D:YYYYMMDDHHmmSSZ` (ISO 32000-1 §7.9.4) and `YYYY-MM-DDTHH:MM:SSZ`.
fn dates(at: i64) -> (String, String) {
    let rfc = crate::civil_time::format_rfc3339_utc(at);
    let digits: String = rfc.chars().filter(char::is_ascii_digit).collect();
    (format!("D:{digits}Z"), rfc)
}

impl EditSession {
    /// Make the one `/GTS_PDFX` output intent carry `intent`'s condition
    /// entries; remove any further one (ISO 15930-7 §6.2.3 allows one).
    pub(super) fn settle_pdfx_intent(
        &mut self,
        intent: &PdfxOutputIntent,
        report: &mut PdfxConversionReport,
    ) -> Result<(), EditError> {
        let Some(catalog_id) = self.trailer.get(b"Root").and_then(Object::as_reference) else {
            report
                .disclosures
                .push("The document has no catalog.".to_owned());
            return Ok(());
        };
        let Some(Object::Dict(mut catalog)) = self.value(catalog_id).cloned() else {
            report
                .disclosures
                .push("The catalog cannot be read.".to_owned());
            return Ok(());
        };
        let Some((holder, mut items, at, mut array_changed)) =
            self.gts_intents(&catalog, &intent.profile, report)
        else {
            return Ok(());
        };
        let mut objects = Vec::new();
        match items.get(at).cloned() {
            Some(Object::Reference(id)) => {
                if let Some(Object::Dict(d)) = self.value(id).cloned()
                    && let Some(d) = patched_intent(d, intent)
                {
                    objects.push(self.write_of(id, Object::Dict(d)));
                }
            }
            Some(Object::Dict(d)) => {
                if let Some(d) = patched_intent(d, intent)
                    && let Some(slot) = items.get_mut(at)
                {
                    *slot = Object::Dict(d);
                    array_changed = true;
                }
            }
            _ => {}
        }
        if array_changed {
            match holder {
                Some(id) => objects.push(self.write_of(id, Object::Array(items))),
                None => {
                    catalog.insert(Name::from(b"OutputIntents"), Object::Array(items));
                    objects.push(self.write_of(catalog_id, Object::Dict(catalog)));
                }
            }
        }
        if let Some(c) = &intent.condition {
            report
                .steps
                .push(format!("Set the GTS_PDFX output condition to \"{c}\"."));
        }
        self.commit_writes(CommandKind::ConvertToPdfx, objects);
        Ok(())
    }

    /// The `/OutputIntents` array (its holder when indirect), with further
    /// `/GTS_PDFX` entries removed and one added when only another
    /// subtype's intent holds `profile`; the index of the `/GTS_PDFX`
    /// entry, and whether the array changed. `None` when there is none.
    fn gts_intents(
        &self,
        catalog: &Dict,
        profile: &[u8],
        report: &mut PdfxConversionReport,
    ) -> Option<(Option<ObjId>, Vec<Object>, usize, bool)> {
        let (holder, mut items) = match catalog.get(b"OutputIntents") {
            Some(Object::Reference(id)) => match self.value(*id) {
                Some(Object::Array(a)) => (Some(*id), a.clone()),
                _ => (None, Vec::new()),
            },
            Some(Object::Array(a)) => (None, a.clone()),
            _ => (None, Vec::new()),
        };
        let gts: Vec<usize> = items
            .iter()
            .enumerate()
            .filter(|(_, item)| self.is_gts_pdfx(item))
            .map(|(i, _)| i)
            .collect();
        let mut array_changed = false;
        for &i in gts.iter().skip(1).rev() {
            items.remove(i);
            array_changed = true;
        }
        if gts.len() > 1 {
            report.disclosures.push(format!(
                "Removed {} further GTS_PDFX output intent(s); PDF/X allows one.",
                gts.len() - 1
            ));
        }
        let at = match gts.first() {
            Some(&i) => i,
            None => {
                let Some(entry) = self.intent_from_same_profile(&items, profile) else {
                    report.disclosures.push(
                        "No GTS_PDFX output intent could be written: the destination profile \
                         is not embedded."
                            .to_owned(),
                    );
                    return None;
                };
                items.push(Object::Dict(entry));
                array_changed = true;
                report.disclosures.push(
                    "Added a GTS_PDFX output intent sharing another output intent's profile."
                        .to_owned(),
                );
                items.len() - 1
            }
        };
        Some((holder, items, at, array_changed))
    }

    /// `item` resolves to an output intent of `/S /GTS_PDFX`.
    fn is_gts_pdfx(&self, item: &Object) -> bool {
        let dict = match item {
            Object::Reference(id) => self.value(*id).and_then(Object::as_dict),
            other => other.as_dict(),
        };
        dict.and_then(|d| d.get(b"S"))
            .and_then(Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"GTS_PDFX")
    }

    /// A `/GTS_PDFX` intent pointing at the `/DestOutputProfile` of an
    /// existing intent whose profile is `profile` (convert colours embeds
    /// nothing when the destination already is the output intent).
    fn intent_from_same_profile(&self, items: &[Object], profile: &[u8]) -> Option<Dict> {
        items.iter().find_map(|item| {
            let dict = match item {
                Object::Reference(id) => self.value(*id).and_then(Object::as_dict),
                other => other.as_dict(),
            }?;
            let reference = dict.get(b"DestOutputProfile")?.as_reference()?;
            let Some(Object::Stream(s)) = self.value(reference) else {
                return None;
            };
            let bytes = self
                .view()
                .slice(s.data_span)
                .and_then(|raw| crate::filters::decode_stream(&s.dict, raw).ok())?;
            if bytes != profile {
                return None;
            }
            let mut d = Dict::new();
            d.insert(Name::from(b"Type"), name(b"OutputIntent"));
            d.insert(Name::from(b"S"), name(b"GTS_PDFX"));
            for key in [&b"OutputConditionIdentifier"[..], b"Info"] {
                if let Some(v) = dict.get(key) {
                    d.insert(Name::from(key), v.clone());
                }
            }
            if d.get(b"OutputConditionIdentifier").is_none() {
                d.insert(
                    Name::from(b"OutputConditionIdentifier"),
                    text_object("Custom"),
                );
            }
            d.insert(
                Name::from(b"DestOutputProfile"),
                Object::Reference(reference),
            );
            Some(d)
        })
    }

    /// The write that makes `id` hold `after`.
    fn write_of(&self, id: ObjId, after: Object) -> ObjectWrite {
        ObjectWrite {
            id,
            before: self.state.get(&id).cloned(),
            after: Some(after),
        }
    }

    /// Step 5 of [`EditSession::convert_to_pdfx`]: the claim, as one
    /// commit.
    pub(super) fn write_pdfx_claim(
        &mut self,
        level: PdfxLevel,
        conversion: &PdfxConversion,
        report: &mut PdfxConversionReport,
    ) -> Result<(), EditError> {
        let (id, mut info, created) = match self.info_id() {
            Some(id) => match self.value(id) {
                Some(Object::Dict(d)) => (id, d.clone(), false),
                _ => {
                    report.disclosures.push(
                        "The Info entry is not a dictionary, so the claim cannot be written."
                            .to_owned(),
                    );
                    return Ok(());
                }
            },
            None => (ObjId::new(self.alloc_number()?, 0), Dict::new(), true),
        };
        let claimed = claim_info(&mut info, level, conversion, report);
        let mut objects = vec![ObjectWrite {
            id,
            before: if created {
                None
            } else {
                self.state.get(&id).cloned()
            },
            after: Some(Object::Dict(info)),
        }];
        let trailer = created.then(|| {
            let mut after = self.trailer.clone();
            after.insert(Name::from(b"Info"), Object::Reference(id));
            (self.trailer.clone(), after)
        });
        self.claim_xmp(level, &claimed, &mut objects, report)?;
        report.steps.push(format!(
            "Wrote the {} claim: Info GTS_PDFXVersion \"{}\"{}, Trapped /{}.",
            level.claim(),
            claimed.version,
            claimed
                .conformance
                .map(|c| format!(", GTS_PDFXConformance \"{c}\""))
                .unwrap_or_default(),
            if claimed.trapped { "True" } else { "False" }
        ));
        self.commit(Command {
            kind: CommandKind::ConvertToPdfx,
            objects,
            removals: Vec::new(),
            trailer,
        });
        Ok(())
    }

    /// Mirror the claim into the XMP packet, creating one for X-4.
    fn claim_xmp(
        &mut self,
        level: PdfxLevel,
        claimed: &Claimed,
        objects: &mut Vec<ObjectWrite>,
        report: &mut PdfxConversionReport,
    ) -> Result<(), EditError> {
        let sets = xmp_sets(level, claimed);
        let (existing, bytes) = match self.metadata() {
            Metadata::Malformed => {
                report.disclosures.push(
                    "The catalog's Metadata is not a stream; the XMP claim was not written."
                        .to_owned(),
                );
                return Ok(());
            }
            Metadata::Absent if level != PdfxLevel::X4 => {
                report.disclosures.push(
                    "The document has no XMP packet; the Info dictionary alone carries the claim."
                        .to_owned(),
                );
                return Ok(());
            }
            Metadata::Absent => (None, SKELETON.as_bytes().to_vec()),
            Metadata::Stream(id, stream) => {
                let decoded = self
                    .view()
                    .slice(stream.data_span)
                    .and_then(|raw| crate::filters::decode_stream(&stream.dict, raw).ok());
                let Some(decoded) = decoded else {
                    report.disclosures.push(
                        "The XMP packet cannot be decoded; the XMP claim was not written."
                            .to_owned(),
                    );
                    return Ok(());
                };
                (Some((id, stream.dict)), decoded)
            }
        };
        let Some(rewritten) = rewrite_all(&bytes, &sets, level, report) else {
            return Ok(());
        };
        let Some(new) = rewritten.or_else(|| existing.is_none().then(|| bytes.clone())) else {
            return Ok(());
        };
        match existing {
            Some((id, dict)) => {
                let stream = self.unfiltered_stream(dict, &new);
                objects.push(self.write_of(id, Object::Stream(stream)));
            }
            None => self.create_packet(&new, objects, report)?,
        }
        Ok(())
    }

    /// A new `/Metadata` stream holding `packet`, and the catalog entry.
    fn create_packet(
        &mut self,
        packet: &[u8],
        objects: &mut Vec<ObjectWrite>,
        report: &mut PdfxConversionReport,
    ) -> Result<(), EditError> {
        let Some(catalog_id) = self.trailer.get(b"Root").and_then(Object::as_reference) else {
            return Ok(());
        };
        let Some(Object::Dict(mut catalog)) = self.value(catalog_id).cloned() else {
            return Ok(());
        };
        let id = ObjId::new(self.alloc_number()?, 0);
        let mut dict = Dict::new();
        dict.insert(Name::from(b"Type"), name(b"Metadata"));
        dict.insert(Name::from(b"Subtype"), name(b"XML"));
        let stream = self.unfiltered_stream(dict, packet);
        objects.push(ObjectWrite {
            id,
            before: None,
            after: Some(Object::Stream(stream)),
        });
        catalog.insert(Name::from(b"Metadata"), Object::Reference(id));
        objects.push(self.write_of(catalog_id, Object::Dict(catalog)));
        report
            .steps
            .push("Created an XMP metadata packet, which PDF/X-4 requires.".to_owned());
        Ok(())
    }
}

/// Set the Info entries of the claim in `info`.
fn claim_info(
    info: &mut Dict,
    level: PdfxLevel,
    conversion: &PdfxConversion,
    report: &mut PdfxConversionReport,
) -> Claimed {
    let (version, conformance) = match level {
        PdfxLevel::X1a2001 => ("PDF/X-1:2001", Some("PDF/X-1a:2001")),
        l => (l.claim(), None),
    };
    info.insert(Name::from(b"GTS_PDFXVersion"), text_object(version));
    match conformance {
        Some(c) => info.insert(Name::from(b"GTS_PDFXConformance"), text_object(c)),
        None => {
            if info.remove(b"GTS_PDFXConformance").is_some() {
                report
                    .disclosures
                    .push("Removed GTS_PDFXConformance, which only PDF/X-1a:2001 uses.".to_owned());
            }
        }
    }
    let had = info
        .get(b"Trapped")
        .and_then(Object::as_name)
        .map(Name::as_bytes);
    let trapped = match (conversion.trapped, had) {
        (Some(t), _) => t,
        (None, Some(b"True")) => true,
        (None, Some(b"False")) => false,
        (None, _) => {
            report
                .disclosures
                .push("Trapped was set to /False: nothing said the file was trapped.".to_owned());
            false
        }
    };
    info.insert(
        Name::from(b"Trapped"),
        name(if trapped { b"True" } else { b"False" }),
    );
    let title = claim_title(info, level, conversion, report);
    let (modified, created) = claim_dates(info, level, conversion, report);
    Claimed {
        version,
        conformance,
        trapped,
        title,
        modified,
        created,
    }
}

/// `/Title` when missing and given; the title written, for `dc:title`.
fn claim_title(
    info: &mut Dict,
    level: PdfxLevel,
    conversion: &PdfxConversion,
    report: &mut PdfxConversionReport,
) -> Option<String> {
    let pre_x4 = level != PdfxLevel::X4;
    let has_title = matches!(info.get(b"Title"), Some(Object::String(_)));
    match (&conversion.title, has_title) {
        (Some(t), false) => {
            info.insert(Name::from(b"Title"), text_object(t));
            report
                .disclosures
                .push(format!("Title was set to \"{t}\": the document had none."));
            Some(t.clone())
        }
        (None, false) if pre_x4 => {
            report.disclosures.push(format!(
                "The document has no Title, which {} requires.",
                level.claim()
            ));
            None
        }
        _ => None,
    }
}

/// `/ModDate`, and `/CreationDate` when missing, from the conversion
/// time; the XMP spellings of what was written.
fn claim_dates(
    info: &mut Dict,
    level: PdfxLevel,
    conversion: &PdfxConversion,
    report: &mut PdfxConversionReport,
) -> (Option<String>, Option<String>) {
    let (mut modified, mut created) = (None, None);
    if let Some(at) = conversion.at {
        let (pdf, rfc) = dates(at);
        if !matches!(info.get(b"CreationDate"), Some(Object::String(_))) {
            info.insert(Name::from(b"CreationDate"), text_object(&pdf));
            report.disclosures.push(
                "CreationDate was set to the conversion time: the document had none.".to_owned(),
            );
            created = Some(rfc.clone());
        }
        info.insert(Name::from(b"ModDate"), text_object(&pdf));
        modified = Some(rfc);
    } else if level != PdfxLevel::X4 {
        report
            .disclosures
            .push("No conversion time was given, so ModDate was not updated.".to_owned());
    }
    (modified, created)
}

/// The XMP properties the claim sets.
fn xmp_sets(level: PdfxLevel, c: &Claimed) -> Vec<XmpSet> {
    let prop = |ns, prefix, local: &str, shape| Property {
        ns,
        prefix,
        local: local.to_owned(),
        shape,
    };
    let set = |prop, value: Option<&str>, pdfa_foreign| XmpSet {
        prop,
        value: value.map(str::to_owned),
        pdfa_foreign,
    };
    let trapped = if c.trapped { "True" } else { "False" };
    let mut sets = vec![
        set(
            prop(PDFXID_NS, "pdfxid", "GTS_PDFXVersion", Shape::Simple),
            Some(c.version),
            level != PdfxLevel::X4,
        ),
        set(
            prop(PDFX_NS, "pdfx", "GTS_PDFXVersion", Shape::Simple),
            Some(c.version),
            true,
        ),
        set(
            prop(PDFX_NS, "pdfx", "GTS_PDFXConformance", Shape::Simple),
            c.conformance,
            true,
        ),
        set(
            prop(PDF_NS, "pdf", "Trapped", Shape::Simple),
            Some(trapped),
            true,
        ),
    ];
    let optional = [(&c.modified, "ModifyDate"), (&c.created, "CreateDate")];
    for (value, local) in optional {
        if let Some(v) = value {
            sets.push(set(
                prop(XMP_NS, "xmp", local, Shape::Simple),
                Some(v),
                false,
            ));
        }
    }
    if let Some(t) = &c.title {
        sets.push(set(
            prop(DC_NS, "dc", "title", Shape::LangAlt),
            Some(t),
            false,
        ));
    }
    sets
}

/// The packet with every set applied: `Some(None)` when it already said
/// all of it, `None` when it cannot be rewritten (disclosed).
fn rewrite_all(
    bytes: &[u8],
    sets: &[XmpSet],
    level: PdfxLevel,
    report: &mut PdfxConversionReport,
) -> Option<Option<Vec<u8>>> {
    let unreadable = |report: &mut PdfxConversionReport| {
        report
            .disclosures
            .push("The XMP packet cannot be read; the XMP claim was not written.".to_owned());
    };
    let Ok(packet) = Packet::read(bytes) else {
        unreadable(report);
        return None;
    };
    let Ok(part) = xmp_rewrite::pdfa_part(packet.text) else {
        unreadable(report);
        return None;
    };
    let pdfa = part.filter(|p| (1..=3).contains(p));
    let policy = Policy {
        languages: AltLanguages::default(),
        authors: AuthorItems::default(),
    };
    let mut text = packet.text.to_owned();
    let mut changed = false;
    for s in sets {
        if let (Some(p), true) = (pdfa, s.pdfa_foreign) {
            report.disclosures.push(format!(
                "{} was not written: the packet claims PDF/A-{p}, which does not predefine it.",
                s.prop.qualified()
            ));
            continue;
        }
        match xmp_rewrite::rewrite(&text, &s.prop, s.value.as_deref(), policy) {
            Ok(Change::InStep) => {}
            Ok(Change::Rewritten(t)) => {
                text = t;
                changed = true;
            }
            Err(Failure::Unrepresentable) => report.disclosures.push(format!(
                "{} was not written: its value holds a character XML cannot carry.",
                s.prop.qualified()
            )),
            Err(Failure::Unreadable) => {
                unreadable(report);
                return None;
            }
        }
    }
    if let Some(p) = pdfa.filter(|_| level == PdfxLevel::X4) {
        report.disclosures.push(format!(
            "pdfxid:GTS_PDFXVersion was written into a packet claiming PDF/A-{p}, which \
             does not predefine it; PDF/A needs an extension schema for it."
        ));
    }
    Some(changed.then(|| packet.encode(&text)))
}

/// `intent` with `wanted`'s condition entries (ISO 32000-2 Table 365), or
/// `None` when it already has them.
fn patched_intent(intent: Dict, wanted: &PdfxOutputIntent) -> Option<Dict> {
    let mut d = intent.clone();
    let entries = [
        (&b"OutputConditionIdentifier"[..], &wanted.condition),
        (b"RegistryName", &wanted.registry_name),
        (b"Info", &wanted.info),
    ];
    for (key, value) in entries {
        if let Some(v) = value {
            d.insert(Name::from(key), text_object(v));
        }
    }
    (d != intent).then_some(d)
}
