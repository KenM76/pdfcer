//! The redaction **absence proof**: does a removed string still occur in the
//! bytes about to be written?
//!
//! [`apply_redactions`](crate::redact::apply_redactions) removes content and
//! reports the text it removed ([`RedactionReport::redacted_text`]). This
//! module checks that text against the **output bytes** — the operator's own
//! file, not a fixture — and is the only thing entitled to the word
//! *verified* (ARCHITECTURE §5, Pass 72.0).
//!
//! Each removed string of at least [`MIN_VERIFIABLE_LEN`] characters is looked
//! for in every decoded stream and then in the raw bytes. Where it is found
//! decides the [`AbsenceVerdict`]:
//!
//! | found in | verdict |
//! |---|---|
//! | drawn content (page content, form XObject, tiling pattern, Type 3 glyph procedure) | [`AbsenceVerdict::Refuse`] — the glyphs may still be painted |
//! | a page's extracted text, though in no stream's bytes | [`AbsenceVerdict::Refuse`] — painted, in pieces or through a non-ASCII encoding |
//! | anywhere else (a font program, image samples, an object container, an attachment, metadata, another stream, the raw bytes) | [`AbsenceVerdict::Disclose`] — pdfcer cannot tell a carrier from a coincidence |
//! | nowhere, with every stream decoded | [`AbsenceVerdict::Verified`] |
//!
//! The byte search alone is vacuous for text a page draws in pieces —
//! `[(Con) -20 (fidential)] TJ` holds no `Confidential` — or through codes
//! that are not ASCII (an `Identity-H` font). Each page is therefore also
//! extracted (decoded through its fonts, ISO 32000-1 §9.10) and searched
//! with all whitespace removed from both sides, so a derived word space
//! inside a kerned word cannot hide it either.
//!
//! A drawn-content hit is **often another occurrence** of the same word that
//! was never marked (a title-block label repeated on every sheet); the proof
//! cannot tell it from the removed glyphs. The verdict therefore says
//! *refuse unless the operator says otherwise*: a shell refuses by default
//! and may offer an explicit override naming each string.
//!
//! [`RedactionReport::redacted_text`]: crate::redact::RedactionReport::redacted_text

use crate::document::Document;
use crate::filters::{self, FilterError};
use crate::object::{Dict, ObjId, Object};
use crate::page_tree;
use crate::text_extract::{self, ExtractOptions};

/// The shortest removed string the proof checks.
///
/// Below four characters a match carries no information: a producer that
/// draws one glyph per show operator yields single-character pieces, and
/// "does the file contain `3`?" is always yes. Shorter strings are counted in
/// [`AbsenceVerification::strings_too_short`], never silently dropped. The
/// same floor governs the carrier sweep's evidence, so the two checks agree on
/// what counts.
pub const MIN_VERIFIABLE_LEN: usize = 4;

/// Where a removed string was found again.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ResidualSite {
    /// A stream the document draws: page content, a form XObject (which
    /// includes annotation appearances), a tiling pattern cell or a Type 3
    /// glyph procedure. The only site that makes the verdict
    /// [`AbsenceVerdict::Refuse`].
    DrawnContent,
    /// An embedded font program (`/FontFile*`). A font's `name` table holds
    /// ordinary words by construction.
    FontProgram,
    /// An image XObject's samples, after its byte-stream filters.
    ImageSamples,
    /// A compressed object container (`/Type /ObjStm`). Page content cannot
    /// live in one (ISO 32000-1 §7.5.7), but a dictionary string can.
    ObjectContainer,
    /// An embedded file (`/Type /EmbeddedFile`). Redaction does not reach
    /// into attachments, so a hit here is the likeliest to be real.
    Attachment,
    /// A metadata stream (`/Type /Metadata`).
    Metadata,
    /// Any other decoded stream.
    OtherStream,
    /// A page's extracted text (decoded through its fonts), though no
    /// decoded stream holds the string's bytes: drawn in pieces or through
    /// a non-ASCII encoding. Refused like [`ResidualSite::DrawnContent`].
    ExtractedText,
    /// No decoded stream; the file's raw bytes (a dictionary string, a name,
    /// an undecodable stream's encoded bytes).
    RawBytes,
}

impl ResidualSite {
    /// Whether a hit here means the string may still be painted.
    const fn is_drawn(self) -> bool {
        matches!(self, Self::DrawnContent | Self::ExtractedText)
    }

    /// A stable lower-case token for scripts and logs, e.g. `drawn-content`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DrawnContent => "drawn-content",
            Self::FontProgram => "font-program",
            Self::ImageSamples => "image-samples",
            Self::ObjectContainer => "object-container",
            Self::Attachment => "attachment",
            Self::Metadata => "metadata",
            Self::OtherStream => "other-stream",
            Self::ExtractedText => "extracted-text",
            Self::RawBytes => "raw-bytes",
        }
    }
}

/// One removed string found again, with the first place it was found.
///
/// Drawn content is reported in preference to any other site, so a string
/// that is both drawn and in a font program reads
/// [`ResidualSite::DrawnContent`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Residual {
    /// The removed string.
    pub text: String,
    /// Where it was found.
    pub site: ResidualSite,
}

/// The proof's three-way answer. See the module docs for the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AbsenceVerdict {
    /// Every checked string is absent from every decoded stream and from the
    /// raw bytes, and every stream that could carry text was decoded.
    Verified,
    /// No drawn-content survivor, but something stops the word *verified*: a
    /// residual at a non-drawn site, a stream the proof could not decode, or
    /// output that does not parse. Disclose it; writing is acceptable.
    Disclose,
    /// A removed string still occurs in drawn content. Write nothing unless
    /// the operator explicitly accepts each such string.
    Refuse,
}

/// What the absence proof found. Produced by [`verify_absence`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct AbsenceVerification {
    /// Distinct non-empty strings at or over [`MIN_VERIFIABLE_LEN`] that were
    /// checked.
    pub strings_checked: usize,
    /// Non-empty strings under [`MIN_VERIFIABLE_LEN`], **not** checked. The
    /// proof's own limit, reported so it is not mistaken for coverage.
    pub strings_too_short: usize,
    /// Checked strings found again, in input order.
    pub residuals: Vec<Residual>,
    /// Streams whose filter chain failed to decode (other than an image
    /// codec, whose samples cannot carry text as bytes). Their encoded bytes
    /// are still covered by the raw-byte search; a non-zero count forfeits
    /// [`AbsenceVerdict::Verified`].
    pub streams_undecoded: usize,
    /// The output did not parse as a PDF, so only the raw-byte search ran.
    pub output_unparsed: bool,
    /// Text extraction could not walk the page tree or read at least one
    /// page, so the extracted-text search is incomplete. Forfeits
    /// [`AbsenceVerdict::Verified`].
    pub text_unextracted: bool,
}

impl AbsenceVerification {
    /// The verdict this finding licenses.
    #[must_use]
    pub fn verdict(&self) -> AbsenceVerdict {
        if self.drawn_survivors().next().is_some() {
            AbsenceVerdict::Refuse
        } else if self.is_clean() {
            AbsenceVerdict::Verified
        } else {
            AbsenceVerdict::Disclose
        }
    }

    /// Whether the proof may say *verified*: no residual anywhere, every
    /// stream decoded, and the output parsed.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.residuals.is_empty()
            && self.streams_undecoded == 0
            && !self.output_unparsed
            && !self.text_unextracted
    }

    /// The strings that still occur in drawn content or extracted text —
    /// the ones a [`AbsenceVerdict::Refuse`] is about.
    pub fn drawn_survivors(&self) -> impl Iterator<Item = &str> {
        self.residuals
            .iter()
            .filter(|r| r.site.is_drawn())
            .map(|r| r.text.as_str())
    }
}

/// Run the absence proof over `output` for each string in `redacted`.
///
/// `output` is the bytes about to be written — normally the first element of
/// [`apply_redactions`](crate::redact::apply_redactions)'s result — and
/// `redacted` its report's
/// [`redacted_text`](crate::redact::RedactionReport::redacted_text). Matching
/// on the strings' UTF-8 bytes in every stream and in the raw file, and on
/// each page's extracted text with whitespace ignored, which also sees text
/// drawn in kerned pieces or through a non-ASCII encoding.
///
/// Infallible: unparsable output sets
/// [`AbsenceVerification::output_unparsed`] and still runs the raw search.
///
/// # Examples
///
/// ```
/// use pdfcer_core::redact::{AbsenceVerdict, verify_absence};
///
/// let proof = verify_absence(b"not a pdf", &["SECRET".to_owned()]);
/// assert!(proof.output_unparsed);
/// assert_eq!(proof.verdict(), AbsenceVerdict::Disclose);
/// ```
#[must_use]
pub fn verify_absence(output: &[u8], redacted: &[String]) -> AbsenceVerification {
    let mut out = AbsenceVerification::default();
    let mut needles: Vec<&str> = Vec::new();
    for s in redacted.iter().filter(|s| !s.is_empty()) {
        if s.chars().count() < MIN_VERIFIABLE_LEN {
            out.strings_too_short += 1;
        } else if !needles.contains(&s.as_str()) {
            needles.push(s);
        }
    }
    out.strings_checked = needles.len();
    let mut found: Vec<Option<ResidualSite>> = vec![None; needles.len()];
    match Document::from_bytes(output.to_vec()) {
        Ok(doc) => {
            out.streams_undecoded = sweep_streams(&doc, &needles, &mut found);
            out.text_unextracted = !sweep_text(&doc, &needles, &mut found);
        }
        Err(_) => out.output_unparsed = true,
    }
    for (needle, site) in needles.iter().zip(found) {
        let site =
            site.or_else(|| contains(output, needle.as_bytes()).then_some(ResidualSite::RawBytes));
        if let Some(site) = site {
            out.residuals.push(Residual {
                text: (*needle).to_owned(),
                site,
            });
        }
    }
    out
}

/// Decode every stream once, recording for each needle the best site it was
/// found at (drawn content wins). Returns the count of undecodable streams.
fn sweep_streams(doc: &Document, needles: &[&str], found: &mut [Option<ResidualSite>]) -> usize {
    let view = doc.view();
    let drawn = drawn_stream_ids(doc);
    let mut undecoded = 0;
    for object in doc.objects() {
        let Object::Stream(stream) = &object.value else {
            continue;
        };
        let Some(raw) = view.slice(stream.data_span) else {
            undecoded += 1;
            continue;
        };
        let decoded = match filters::decode_stream(&stream.dict, raw) {
            Ok(d) => d,
            Err(FilterError::ImageCodec(_)) => continue,
            Err(_) => {
                undecoded += 1;
                continue;
            }
        };
        let site = site_of(&stream.dict, object.id, &drawn);
        for (needle, slot) in needles.iter().zip(found.iter_mut()) {
            if *slot == Some(ResidualSite::DrawnContent) || !contains(&decoded, needle.as_bytes()) {
                continue;
            }
            if slot.is_none() || site == ResidualSite::DrawnContent {
                *slot = Some(site);
            }
        }
    }
    undecoded
}

/// Search each page's extracted text for the needles not already found in
/// drawn content, whitespace ignored on both sides. Returns `false` when
/// extraction could not read the page tree or a page.
fn sweep_text(doc: &Document, needles: &[&str], found: &mut [Option<ResidualSite>]) -> bool {
    let Ok(text) = text_extract::extract_document(doc, &ExtractOptions::default()) else {
        return false;
    };
    let squeeze = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let pages: Vec<String> = text
        .pages
        .iter()
        .map(|p| squeeze(&p.runs.iter().map(|r| r.text.as_str()).collect::<String>()))
        .collect();
    for (needle, slot) in needles.iter().zip(found.iter_mut()) {
        let n = squeeze(needle);
        if slot.is_some_and(ResidualSite::is_drawn) || n.is_empty() {
            continue;
        }
        if pages.iter().any(|p| p.contains(&n)) {
            *slot = Some(ResidualSite::ExtractedText);
        }
    }
    text.diagnostics.pages_unreadable == 0
}

/// Streams drawn by reference only: each page's `/Contents` and every Type 3
/// font's `/CharProcs` values (a glyph procedure has no `/Type` or `/Subtype`
/// of its own).
fn drawn_stream_ids(doc: &Document) -> Vec<ObjId> {
    let mut ids: Vec<ObjId> = page_tree::pages(doc)
        .map(|pages| pages.iter().flat_map(|p| p.contents.clone()).collect())
        .unwrap_or_default();
    for object in doc.objects() {
        let Some(dict) = object.value.as_dict() else {
            continue;
        };
        let is_type3 = dict
            .get(b"Subtype")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"Type3");
        if !is_type3 {
            continue;
        }
        if let Some(procs) = dict
            .get(b"CharProcs")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
        {
            ids.extend(procs.iter().filter_map(|(_, v)| v.as_reference()));
        }
    }
    ids
}

/// Classify one stream by what it is (ISO 32000-1 §8.10 form XObjects, §8.7.3
/// tiling patterns, §9.9 font programs, §7.5.7 object streams, §7.11.4
/// embedded files, §14.3.2 metadata).
fn site_of(dict: &Dict, id: ObjId, drawn: &[ObjId]) -> ResidualSite {
    if drawn.contains(&id) {
        return ResidualSite::DrawnContent;
    }
    let name = |key: &[u8]| {
        dict.get(key)
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec())
    };
    match name(b"Subtype").as_deref() {
        Some(b"Form") => return ResidualSite::DrawnContent,
        Some(b"Image") => return ResidualSite::ImageSamples,
        Some(b"Type1C" | b"CIDFontType0C" | b"OpenType") => return ResidualSite::FontProgram,
        _ => {}
    }
    if dict.get(b"PatternType").and_then(Object::as_int) == Some(1) {
        return ResidualSite::DrawnContent;
    }
    if dict.contains_key(b"Length1") {
        return ResidualSite::FontProgram;
    }
    match name(b"Type").as_deref() {
        Some(b"ObjStm") => ResidualSite::ObjectContainer,
        Some(b"EmbeddedFile") => ResidualSite::Attachment,
        Some(b"Metadata") => ResidualSite::Metadata,
        _ => ResidualSite::OtherStream,
    }
}

/// Byte-subsequence search, deliberately independent of the removal code it
/// audits.
fn contains(hay: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && needle.len() <= hay.len()
        && hay.windows(needle.len()).any(|w| w == needle)
}

#[cfg(test)]
#[path = "redact_proof_tests.rs"]
mod tests;
