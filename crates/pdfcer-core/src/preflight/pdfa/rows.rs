//! The PDF/A requirement inventory (`PAI-<axis>-n`): one row per failable
//! or advisory requirement of levels 1b, 2b and 3b, with the ISO 19005
//! clause each part states it in. Rows, levels and clauses are those of the
//! spec RAG's `pdfa__req__inventory_index.md` and its per-axis files;
//! titles are paraphrases. Definition-only (`X`) rows have no rule.

use super::PdfaLevel;

/// How pdfcer answers a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum How {
    /// Checked once per document.
    Document,
    /// Checked on every page.
    Page,
    /// Answered by another row; no rule of its own.
    CoveredBy(&'static str),
    /// Checkable, but not checked yet: reported as `Incomplete`.
    NotYet,
    /// The requirement text is NOT SOURCED: reported as `Incomplete`.
    NotSourced,
    /// A reader-side rule: nothing to test in a file.
    NoTest,
}

/// One requirement.
#[derive(Debug, Clone, Copy)]
pub(super) struct Row {
    /// `PAI-FS-1`.
    pub(super) id: &'static str,
    /// Advisory ("should", or derived rather than stated): warns, never fails.
    pub(super) advisory: bool,
    /// Part digits the row applies to (`123`, `1`, `23`, `3`).
    parts: &'static str,
    /// The ISO 19005-1 clause.
    clause1: &'static str,
    /// The ISO 19005-2 and -3 clause (the same in both parts).
    clause23: &'static str,
    pub(super) how: How,
    /// What is required, in a few words.
    pub(super) title: &'static str,
    /// What a checked row does not see, reported once at info severity.
    pub(super) note: &'static str,
}

/// Where a row stands for one level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum State {
    /// Checked by pdfcer.
    Checked,
    /// NOT SOURCED for this level.
    NotSourced,
    /// Sourced but not checked yet.
    NotYet,
}

impl Row {
    fn parse(line: &'static str) -> Option<Self> {
        let mut f = line.split('|');
        let id = f.next()?;
        let advisory = f.next()? == "R";
        let parts = f.next()?;
        let clause1 = f.next()?;
        let clause23 = f.next()?;
        let how = match f.next()? {
            "D" => How::Document,
            "P" => How::Page,
            "Y" => How::NotYet,
            "N" => How::NotSourced,
            other => other.strip_prefix("C:").map_or(How::NoTest, How::CoveredBy),
        };
        Some(Self {
            id,
            advisory,
            parts,
            clause1,
            clause23,
            how,
            title: f.next()?,
            note: f.next().unwrap_or(""),
        })
    }

    /// Whether the row applies to `level`.
    pub(super) fn applies(&self, level: PdfaLevel) -> bool {
        self.parts.contains(char::from(b'0' + level.part()))
    }

    /// How the row stands for `level`; `None` when it yields no rule.
    pub(super) fn state(&self, level: PdfaLevel) -> Option<State> {
        if !self.applies(level) {
            return None;
        }
        match self.how {
            How::CoveredBy(_) | How::NoTest => None,
            How::NotSourced => Some(State::NotSourced),
            How::NotYet => Some(State::NotYet),
            How::Document | How::Page => Some(State::Checked),
        }
    }

    /// The clause `level`'s part states the row in, as text
    /// (`ISO 19005-2:2011 §6.1.2`).
    pub(super) fn clause(&self, level: PdfaLevel) -> String {
        let clause = if level.part() == 1 {
            self.clause1
        } else {
            self.clause23
        };
        format!("{} {clause}", level.standard())
    }
}

/// Every row, in inventory order.
pub(super) fn all() -> impl Iterator<Item = Row> {
    TABLE.lines().filter_map(Row::parse)
}

/// The row with `id`.
pub(super) fn find(id: &str) -> Option<Row> {
    all().find(|r| r.id == id)
}

/// `id|F or R|parts|-1 clause|-2/-3 clause|how|title|note`; how is `D`
/// document, `P` page, `Y` not yet, `N` NOT SOURCED, `-` no test, `C:x`
/// covered by x.
const TABLE: &str = "\
PAI-FS-1|F|123|§6.1.2|§6.1.2|D|the header is %PDF-n.n at byte 0 (-2/-3: 1.0 to 1.7, then an end of line)|\n\
PAI-FS-2|F|123|§6.1.2|§6.1.2|D|the header is followed by a comment of at least four bytes above 127|\n\
PAI-FS-3|F|123|§6.1.3|§6.1.3|D|the trailer has a non-empty /ID|the first-page trailer of a linearized file is not read\n\
PAI-FS-4|F|1|§6.1.3||Y|a linearized file's two /ID values are identical|\n\
PAI-FS-5|F|123|§6.1.3|§6.1.3|D|the trailer has no /Encrypt|\n\
PAI-FS-6|F|123|§6.1.3|§6.1.3 NOTE 1|D|nothing follows the last %%EOF but one end of line|\n\
PAI-FS-7|F|1|§6.1.4||Y|xref subsection numbers are separated by one space|\n\
PAI-FS-8|F|123|§6.1.4|§6.1.4|Y|the xref keyword and subsection header are separated by one end of line|\n\
PAI-FS-9|F|1|§6.1.4||D|no cross-reference streams (and so, DERIVED, no object streams)|only the newest cross-reference section is inspected\n\
PAI-FS-10|F|123|§6.1.6|§6.1.6|Y|hexadecimal strings have an even number of digits|\n\
PAI-FS-11|F|123|§6.1.6|§6.1.6|Y|hexadecimal strings hold only hexadecimal digits and white space|\n\
PAI-FS-12|F|123|§6.1.7|§6.1.7.1|Y|stream /Length matches the data|\n\
PAI-FS-13|F|123|§6.1.7|§6.1.7.1|Y|stream and endstream keywords are delimited by end-of-line markers|\n\
PAI-FS-14|F|123|§6.1.7|§6.1.7.1|D|no stream has /F, /FFilter or /FDecodeParms|\n\
PAI-FS-15|F|123|§6.1.10|§6.1.7.2|D|stream filters are the permitted standard ones (never LZWDecode; JXLDecode is in no ISO 32000 part)|\n\
PAI-FS-16|F|23||§6.1.7.2|D|a Crypt filter uses only the Identity crypt filter|\n\
PAI-FS-17|F|123|§6.1.10|§6.1.10|Y|inline image filters are permitted ones|\n\
PAI-FS-18|F|23||§6.1.8|Y|font, colourant and structure-type names are valid UTF-8|\n\
PAI-FS-19|F|123|§6.1.8|§6.1.9|Y|object headers and endobj are delimited as required|\n\
PAI-FS-20|F|23||§6.1.12|D|/Perms holds only UR3 and DocMDP, and DocMDP signatures carry no digest keys|\n\
PAI-FS-21|R|23||§6.1.11|-|readers ignore linearization|\n\
PAI-FS-30|F|123|§6.1.12|§6.1.13|D|integers lie in the 32-bit range|operands inside content streams are not checked\n\
PAI-FS-31|F|1|§6.1.12||D|reals lie within ±32767|operands inside content streams are not checked\n\
PAI-FS-32|F|23||§6.1.13|D|reals lie within ±3.403e38|operands inside content streams are not checked\n\
PAI-FS-33|F|23||§6.1.13|D|non-zero reals are no closer to 0 than ±1.175e-38|operands inside content streams are not checked\n\
PAI-FS-34|F|1|§6.1.12||D|strings are shorter than 65536 bytes|operands inside content streams are not checked\n\
PAI-FS-35|F|23||§6.1.13|D|strings are at most 32767 bytes|operands inside content streams are not checked\n\
PAI-FS-36|F|123|§6.1.12|§6.1.13|D|names are at most 127 bytes|operands inside content streams are not checked\n\
PAI-FS-37|F|1|§6.1.12||D|arrays hold at most 8191 elements|operands inside content streams are not checked\n\
PAI-FS-38|F|1|§6.1.12||D|dictionaries hold at most 4095 entries|operands inside content streams are not checked\n\
PAI-FS-39|F|123|§6.1.12|§6.1.13|D|at most 8388607 indirect objects|only objects reachable from /Root and /Info are counted\n\
PAI-FS-40|F|123|§6.1.12|§6.1.13|Y|q/Q nesting is at most 28 deep|\n\
PAI-FS-41|F|1|§6.1.12||D|a DeviceN space has at most 8 colourants|\n\
PAI-FS-42|F|23||§6.1.13|D|a DeviceN space has at most 32 colourants|\n\
PAI-FS-43|F|123|§6.1.12|§6.1.13|Y|CID values are at most 65535|\n\
PAI-FS-44|F|23||§6.1.13|P|every page boundary is 3 to 14400 units in each direction|/UserUnit is not applied\n\
PAI-MD-1|F|123|§6.7.2|§6.6.2.1|D|the catalog has a /Metadata XMP stream|\n\
PAI-MD-2|F|1|§6.7.2||D|the metadata stream has no /Filter|\n\
PAI-MD-3|F|123|§6.7.5|§6.6.2.1|D|the xpacket header has no bytes attribute|\n\
PAI-MD-4|F|123|§6.7.5|§6.6.2.1|D|the xpacket header has no encoding attribute|\n\
PAI-MD-5|F|123|§6.7.9|§6.6.2.1|Y|the XMP is well-formed RDF/XML|\n\
PAI-MD-6|F|23||§6.6.2.1|D|the XMP packet is UTF-8|\n\
PAI-MD-7|F|123|§6.7.9|§6.6.2.3.1|Y|every XMP property is predefined or described by an extension schema|\n\
PAI-MD-8|F|123|§6.7.9|§6.6.2.3.1|Y|every XMP property value has its schema's type|\n\
PAI-MD-9|F|123|§6.7.8|§6.6.2.3.2|Y|extension-schema structures have only their defined fields|\n\
PAI-MD-10|F|123|§6.7.8|§6.6.2.3.3|Y|the extension-schema container is a pdfaExtension:schemas Bag|\n\
PAI-MD-11|F|123|§6.7.8|§6.6.2.3.3|Y|extension schema entries are complete and typed|\n\
PAI-MD-12|F|123|§6.7.8|§6.6.2.3.3|Y|extension property entries are complete and typed|\n\
PAI-MD-13|F|123|§6.7.8|§6.6.2.3.3|Y|extension value-type entries are complete and typed|\n\
PAI-MD-14|F|123|§6.7.8|§6.6.2.3.3|Y|extension field entries are complete and typed|\n\
PAI-MD-15|F|123|§6.7.11|§6.6.4|D|the catalog XMP carries the PDF/A identification schema|\n\
PAI-MD-16|F|123|§6.7.11|§6.6.4|D|pdfaid:part names this part|\n\
PAI-MD-17|F|123|§6.7.11|§6.6.4|D|pdfaid:conformance is a level of this part|\n\
PAI-MD-18|F|123|§6.7.11|§6.6.4|D|the identification properties use the prefix pdfaid|\n\
PAI-MD-20|F|1|§6.1.5, §6.7.3||D|Info CreationDate matches xmp:CreateDate|\n\
PAI-MD-21|F|1|§6.7.3||D|Info Title matches dc:title|\n\
PAI-MD-22|F|1|§6.7.3||D|Info Author matches a one-item dc:creator|\n\
PAI-MD-23|F|1|§6.7.3||D|Info Subject matches dc:description|\n\
PAI-MD-24|F|1|§6.7.3||D|Info Keywords matches pdf:Keywords|\n\
PAI-MD-25|F|1|§6.7.3||D|Info Creator matches xmp:CreatorTool|\n\
PAI-MD-26|F|1|§6.7.3||D|Info Producer matches pdf:Producer|\n\
PAI-MD-27|F|1|§6.7.3||D|Info ModDate matches xmp:ModifyDate|\n\
PAI-MD-28|R|23||§6.1.5|-|readers ignore the Info dictionary|\n\
PAI-MD-29|F|1|§6.1.5||C:PAI-MD-20|Info entries are consistent with the XMP|\n\
PAI-OI-3|F|1|§6.2.2||D|the PDF/A output profile is a v2 prtr or mntr Gray, RGB or CMYK profile|\n\
PAI-OI-4|F|23||§6.2.3|D|the PDF/A output profile is a v2 or v4 prtr or mntr Gray, RGB or CMYK profile|\n\
PAI-OI-5|F|123|§6.2.2|§6.2.3|D|every output intent with a profile uses the same profile object|\n\
PAI-OI-6|F|23||§6.2.3|D|no output intent has /DestOutputProfileRef|\n\
PAI-OI-7|R|3||§6.2.3|-|readers ignore a profile's /Alternate|\n\
PAI-CS-1|F|1|§6.2.3.2||D|ICCBased profiles are v2 prtr, mntr, scnr or spac Gray, RGB, CMYK or Lab|\n\
PAI-CS-2|F|23||§6.2.4.2|D|ICCBased profiles are v2 or v4 prtr, mntr, scnr or spac Gray, RGB, CMYK or Lab|\n\
PAI-CS-3|F|1|§6.2.3.2||D|an ICCBased /N matches its profile's channel count|\n\
PAI-CS-4|F|123|§6.2.3.3|§6.2.4.3|P|DeviceRGB needs an RGB PDF/A output intent (-2/-3: or a DefaultRGB)|alternate spaces of Separation and DeviceN are not walked, nor Type 3 glyphs and soft masks; a form's own Default* is not consulted\n\
PAI-CS-5|F|123|§6.2.3.3|§6.2.4.3|P|DeviceCMYK needs a CMYK PDF/A output intent (-2/-3: or a DefaultCMYK)|alternate spaces of Separation and DeviceN are not walked, nor Type 3 glyphs and soft masks; a form's own Default* is not consulted\n\
PAI-CS-6|F|123|§6.2.3.3|§6.2.4.3|P|DeviceGray needs a PDF/A output intent (-2/-3: or a DefaultGray)|alternate spaces of Separation and DeviceN are not walked, nor Type 3 glyphs and soft masks; a form's own Default* is not consulted\n\
PAI-CS-7|F|23||§6.2.4.2|Y|overprinted ICCBased CMYK uses OPM 0|\n\
PAI-CS-8|F|23||§6.2.4.4|D|a DeviceN space with spot colourants has a /Colorants dictionary naming each|\n\
PAI-CS-9|F|23||§6.2.4.4|D|Separation spaces of one colourant share alternate and tint transform|\n\
PAI-CS-10|F|1|§6.5.3||P|annotation /C and /IC colours need an RGB PDF/A output intent|\n\
PAI-FT-1|F|123|§6.3.2|§6.2.11.2|D|a font dictionary has /Type /Font|\n\
PAI-FT-2|F|123|§6.3.2|§6.2.11.2|D|a font /Subtype is a standard one|\n\
PAI-FT-3|F|123|§6.3.2|§6.2.11.2|D|a font has /BaseFont (Type 3 exempt)|\n\
PAI-FT-4|F|123|§6.3.2|§6.2.11.2|D|a simple font has FirstChar, LastChar and matching Widths (standard 14 exempt)|\n\
PAI-FT-5|F|123|§6.3.2|§6.2.11.2|D|a FontFile3 /Subtype is Type1C or CIDFontType0C (-2/-3: or OpenType)|\n\
PAI-FT-6|F|123|§6.3.3.1|§6.2.11.3.1|Y|a CIDFont's CIDSystemInfo agrees with its CMap|\n\
PAI-FT-7|F|123|§6.3.3.2|§6.2.11.3.2|D|a CIDFontType2 has /CIDToGIDMap (-2/-3: when embedded)|text drawn only in rendering mode 3 is not exempted\n\
PAI-FT-8|F|123|§6.3.3.3|§6.2.11.3.3|Y|a non-predefined CMap is embedded|\n\
PAI-FT-9|F|123|§6.3.3.3|§6.2.11.3.3|Y|an embedded CMap's WMode agrees with its dictionary|\n\
PAI-FT-10|F|23||§6.2.11.3.3|Y|an embedded CMap uses only predefined CMaps|\n\
PAI-FT-11|F|123|§6.3.4|§6.2.11.4.1|D|every font program is embedded|text drawn only in rendering mode 3 is not exempted\n\
PAI-FT-12|F|123|§6.3.5|§6.2.11.4.1|Y|every glyph shown is in the embedded program|\n\
PAI-FT-13|F|123|§6.3.5|§6.2.11.4.2|D|a Type 1 subset has /CharSet (-1; -2/-3: if present, complete)|a /CharSet is not compared with the program's glyphs\n\
PAI-FT-14|F|123|§6.3.5|§6.2.11.4.2|D|a CIDFont subset has /CIDSet (-1; -2/-3: if present, complete)|a /CIDSet is not compared with the program's glyphs\n\
PAI-FT-15|F|123|§6.3.6|§6.2.11.5|Y|dictionary widths match the program's advances|\n\
PAI-FT-16|F|1|§6.3.7||D|a non-symbolic TrueType font uses MacRoman or WinAnsi with no /Differences|\n\
PAI-FT-17|F|23||§6.2.11.6|Y|a non-symbolic TrueType program has a usable cmap and encoding|\n\
PAI-FT-18|F|123|§6.3.7|§6.2.11.6|D|a symbolic TrueType font has no /Encoding|\n\
PAI-FT-19|F|123|§6.3.7|§6.2.11.6|Y|a symbolic TrueType program's cmap has the required subtables|\n\
PAI-FT-20|F|23||§6.2.11.8|Y|no glyph shown is .notdef|\n\
PAI-TG-1|F|1|§6.4||D|an ExtGState /SMask is absent or /None|\n\
PAI-TG-2|F|1|§6.4||D|no XObject has /SMask|\n\
PAI-TG-3|F|1|§6.4||D|no page or form has a transparency group|\n\
PAI-TG-4|F|1|§6.4||D|an ExtGState /BM is Normal or Compatible|\n\
PAI-TG-5|F|1|§6.4||D|an ExtGState /CA is 1.0|\n\
PAI-TG-6|F|1|§6.4||D|an ExtGState /ca is 1.0|\n\
PAI-TG-7|F|23||§6.2.10|D|blend modes are ISO 32000-1 standard ones|\n\
PAI-TG-8|F|23||§6.2.10|P|a page that uses transparency, without a PDF/A output intent, has a /Group /CS|ISO 19005-2 Annex A (what uses transparency) is NOT SOURCED; pdfcer's transparency scan stands in\n\
PAI-TG-10|F|123|§6.2.8|§6.2.5|D|no ExtGState has /TR|\n\
PAI-TG-11|F|123|§6.2.8|§6.2.5|D|an ExtGState /TR2 is /Default|\n\
PAI-TG-12|F|23||§6.2.5|D|no ExtGState has /HTP|\n\
PAI-TG-13|F|23||§6.2.5|D|halftones are type 1 or 5 and have no /HalftoneName|\n\
PAI-TG-14|F|23||§6.2.5|Y|halftone transfer functions are present exactly for non-primary colourants|\n\
PAI-TG-15|F|123|§6.2.9|§6.2.6|D|rendering intents are the four standard ones|the ri operator inside content streams is not checked\n\
PAI-TG-16|R|23||§6.2.7|N|flatness constraints|\n\
PAI-TG-17|F|123|§6.2.10|§6.2.2|Y|no operator outside the base PDF version, even inside BX/EX|\n\
PAI-TG-18|F|23||§6.2.2|Y|content streams that need resources carry their own /Resources|\n\
PAI-TG-20|F|123|§6.2.4|§6.2.8|D|no image has /Alternates|\n\
PAI-TG-21|F|123|§6.2.4|§6.2.8, §6.2.9|D|no XObject has /OPI|\n\
PAI-TG-22|F|123|§6.2.4|§6.2.8|D|no image has /Interpolate true|inline images are not checked\n\
PAI-TG-23|F|1|§6.2.4||D|image BitsPerComponent is 1, 2, 4 or 8|\n\
PAI-TG-24|F|23||§6.2.8|D|image BitsPerComponent is 1, 2, 4, 8 or 16|\n\
PAI-TG-25|F|123|§6.2.4|§6.2.8|D|an image mask has BitsPerComponent 1|\n\
PAI-TG-26|F|23||§6.2.8.3|Y|a JPEG 2000 image has 1, 3 or 4 channels|\n\
PAI-TG-27|F|23||§6.2.8.3|Y|a JPEG 2000 image without /ColorSpace has a usable colour specification|\n\
PAI-TG-28|F|23||§6.2.8.3|Y|a JPEG 2000 image has one bit depth of 1 to 38|\n\
PAI-TG-29|R|1|§5.4||D|JPEG 2000 post-dates the PDF 1.4 base (status NEEDS VERIFICATION)|\n\
PAI-TG-30|F|123|§6.2.5|§6.2.9|D|no form XObject has /Subtype2 /PS or /PS|\n\
PAI-TG-31|F|123|§6.2.6|§6.2.9|D|no reference XObjects|\n\
PAI-TG-32|F|123|§6.2.7|§6.2.9|D|no PostScript XObjects|\n\
PAI-IA-1|F|1|§6.5.2||P|annotation subtypes are permitted ones|\n\
PAI-IA-2|F|23||§6.3.1|P|annotation subtypes are permitted ones|\n\
PAI-IA-3|F|1|§6.5.3||P|an annotation /CA is 1.0|\n\
PAI-IA-4|F|1|§6.5.3||P|an annotation has /F with Print set and Hidden, Invisible and NoView clear|\n\
PAI-IA-5|F|23||§6.3.2|P|an annotation other than Popup has /F|\n\
PAI-IA-6|F|23||§6.3.2|P|an annotation's /F sets Print and clears Hidden, Invisible, NoView and ToggleNoView|\n\
PAI-IA-7|F|23||§6.3.3|P|an annotation has /AP (zero-size, Popup and Link exempt)|\n\
PAI-IA-8|F|123|§6.5.3|§6.3.3|P|an appearance dictionary has only /N|\n\
PAI-IA-9|F|123|§6.5.3|§6.3.3|P|a button widget's /N is a dictionary of state streams|\n\
PAI-IA-10|F|123|§6.5.3|§6.3.3|P|any other annotation's /N is a stream|\n\
PAI-IA-11|F|123|§6.9|§6.4.1|D|AcroForm /NeedAppearances is absent or false|\n\
PAI-IA-12|F|1|§6.9||P|a widget has /AP|\n\
PAI-IA-13|F|23||§6.4.2|D|the AcroForm has no /XFA|\n\
PAI-IA-14|F|23||§6.4.2|D|the catalog has no /NeedsRendering true|\n\
PAI-IA-15|F|23||§6.4.3|Y|a signature /ByteRange covers the file except /Contents|\n\
PAI-IA-16|F|23||§6.4.3|Y|a signature's PKCS#7 includes the signing certificate|\n\
PAI-IA-17|F|23||§6.4.3|Y|a signature's PKCS#7 has one SignerInfo|\n\
PAI-IA-20|F|1|§6.6.1||D|actions are permitted types|\n\
PAI-IA-21|F|23||§6.5.1|D|actions are permitted types|\n\
PAI-IA-22|F|123|§6.6.1|§6.5.1|D|named actions are NextPage, PrevPage, FirstPage or LastPage|\n\
PAI-IA-23|F|1|§6.6.1||P|a widget has no /A|\n\
PAI-IA-24|F|123|§6.6.2|§6.4.1|P|a widget has no /AA (-2/-3: and no /A)|\n\
PAI-IA-25|F|123|§6.6.2|§6.4.1|D|a form field has no /AA|\n\
PAI-IA-26|F|123|§6.6.2|§6.5.2|D|the catalog has no /AA|\n\
PAI-IA-27|F|23||§6.5.2|P|a page has no /AA|\n\
PAI-IA-28|R|123|§6.6.1 (DERIVED)|§6.5.1 (DERIVED)|D|no document-level JavaScript in the name dictionary|\n\
PAI-IA-29|R|123|§6.6.1 (DERIVED)|§6.5.1 (DERIVED)|C:PAI-IA-20|outline, link, OpenAction and /Next actions follow the action rules|\n\
PAI-EO-1|F|1|§6.1.11||D|no file specification has /EF|\n\
PAI-EO-2|F|1|§6.1.11||D|the name dictionary has no /EmbeddedFiles|\n\
PAI-EO-3|F|23||§6.8|D|a file specification with /EF has /F and /UF|\n\
PAI-EO-4|F|2||§6.8|D|every embedded file is PDF/A-1 or PDF/A-2|an embedded file's claim is read, the file itself is not validated\n\
PAI-EO-5|F|3||§6.8|D|an embedded file stream's /Subtype is a MIME type|\n\
PAI-EO-6|F|3||§6.8|D|a file specification with /EF has /AFRelationship|the value list is NOT SOURCED from ISO 19005-3; only presence is checked\n\
PAI-EO-7|F|3||§6.8|D|every embedded file is an associated file (named by an /AF array)|a file specification written directly inside another object is not checked\n\
PAI-EO-10|F|1|§6.1.13||D|the catalog has no /OCProperties|\n\
PAI-EO-11|F|23||§6.9|D|each optional-content configuration has a non-empty /Name|\n\
PAI-EO-12|F|23||§6.9|D|optional-content configuration names are unique|\n\
PAI-EO-13|F|23||§6.9|D|a configuration's /Order names every optional-content group|\n\
PAI-EO-14|F|23||§6.9|D|no optional-content configuration has /AS|\n\
PAI-EO-20|F|23||§6.10|D|the name dictionary has no /AlternatePresentations|\n\
PAI-EO-21|F|23||§6.10|P|a page has no /PresSteps|\n\
PAI-EO-22|F|23||§6.11|D|the catalog has no /Requirements|";
