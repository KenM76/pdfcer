//! Compatibility targets: which PDF version a document's features need, so
//! an optimize run can stay at or under a chosen version and say what the
//! result still uses above it.
//!
//! Each feature's version is the one ISO 32000 marks it with: transparency
//! (`SMask`, `CA`/`ca`, `BM` in Table 57, groups in §11.6.6) and
//! `/JBIG2Decode` (§7.4.7) are PDF 1.4; `/JPXDecode` (§7.4.9), optional
//! content (§8.11), object streams (§7.5.7) and cross-reference streams
//! (§7.5.8) are PDF 1.5; `/BrotliDecode` extends PDF 2.0 (EXTN-BROTLI-1).
//! Detection is report-only: nothing here removes or flattens a feature.

use std::collections::BTreeMap;
use std::fmt;

use crate::PdfVersion;
use crate::document::Document;
use crate::object::{Dict, ObjId, Object, Provenance};
use crate::xref::SectionShape;

/// A feature whose PDF version a compatibility target can exclude.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum VersionFeature {
    /// Soft masks, constant alpha below 1, a blend mode other than
    /// `Normal`/`Compatible`, a transparency group, or `/SMaskInData`.
    Transparency,
    /// The `/JBIG2Decode` filter.
    Jbig2,
    /// The `/JPXDecode` filter (JPEG 2000).
    Jpx,
    /// Optional content (layers): the catalog's `/OCProperties`.
    OptionalContent,
    /// Objects compressed in object streams.
    ObjectStreams,
    /// A cross-reference stream in place of a classic table.
    CrossReferenceStreams,
    /// The `/BrotliDecode` filter.
    Brotli,
}

impl VersionFeature {
    /// The version that introduced the feature.
    #[must_use]
    pub const fn introduced(self) -> PdfVersion {
        let (major, minor) = match self {
            Self::Transparency | Self::Jbig2 => (1, 4),
            Self::Jpx
            | Self::OptionalContent
            | Self::ObjectStreams
            | Self::CrossReferenceStreams => (1, 5),
            Self::Brotli => (2, 0),
        };
        PdfVersion { major, minor }
    }

    /// A short operator-facing name.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Transparency => "transparency",
            Self::Jbig2 => "JBIG2 images",
            Self::Jpx => "JPEG 2000 images",
            Self::OptionalContent => "layers",
            Self::ObjectStreams => "object streams",
            Self::CrossReferenceStreams => "cross-reference stream",
            Self::Brotli => "Brotli compression",
        }
    }
}

impl fmt::Display for VersionFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let v = self.introduced();
        write!(f, "{} (PDF {}.{})", self.label(), v.major, v.minor)
    }
}

/// One feature a document uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FeatureUse {
    /// The feature.
    pub feature: VersionFeature,
    /// Objects that use it (1 for a document-level feature such as the
    /// cross-reference stream).
    pub objects: usize,
    /// The lowest-numbered object using it; `None` for a document-level
    /// feature.
    pub example: Option<ObjId>,
}

impl FeatureUse {
    /// Whether the feature needs a version above `target`.
    #[must_use]
    pub fn above(&self, target: PdfVersion) -> bool {
        self.feature.introduced() > target
    }
}

/// Every [`VersionFeature`] `doc` uses, in feature order.
///
/// # Example
///
/// ```
/// # use pdfcer_core::{document::Document, optimize::features_used};
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/synthetic/hello.pdf"))?;
/// let doc = Document::from_bytes(bytes)?;
/// for used in features_used(&doc) {
///     println!("{}: {} object(s)", used.feature, used.objects);
/// }
/// # Ok(()) }
/// ```
#[must_use]
pub fn features_used(doc: &Document) -> Vec<FeatureUse> {
    let mut found: BTreeMap<VersionFeature, (usize, Option<ObjId>)> = BTreeMap::new();
    let mut note = |feature, id: Option<ObjId>| {
        let e = found.entry(feature).or_insert((0, None));
        e.0 += 1;
        e.1 = match (e.1, id) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    };
    for obj in doc.objects() {
        if matches!(obj.provenance, Provenance::ObjectStream { .. }) {
            note(VersionFeature::ObjectStreams, Some(obj.id));
        }
        let dict = match &obj.value {
            Object::Dict(d) => d,
            Object::Stream(s) => &s.dict,
            _ => continue,
        };
        for feature in dict_features(dict) {
            note(feature, Some(obj.id));
        }
    }
    if matches!(doc.section_shape(), SectionShape::Stream { .. }) || doc.is_hybrid() {
        note(VersionFeature::CrossReferenceStreams, None);
    }
    if doc
        .catalog()
        .is_ok_and(|c| c.get(b"OCProperties").is_some())
    {
        note(VersionFeature::OptionalContent, None);
    }
    found
        .into_iter()
        .map(|(feature, (objects, example))| FeatureUse {
            feature,
            objects,
            example,
        })
        .collect()
}

/// How deep [`dict_features`] follows direct dictionaries and arrays; a
/// page's `/Resources /ExtGState /GS0` is three down.
const MAX_DEPTH: usize = 6;

/// The features one object uses: its dictionary and the direct
/// dictionaries and arrays inside it, to [`MAX_DEPTH`]. References are not
/// followed; the object they name is visited on its own.
fn dict_features(dict: &Dict) -> Vec<VersionFeature> {
    let mut out = Vec::new();
    walk_dict(dict, 0, &mut out);
    out.sort_unstable();
    out.dedup();
    out
}

fn walk(value: &Object, depth: usize, out: &mut Vec<VersionFeature>) {
    if depth > MAX_DEPTH {
        return;
    }
    match value {
        Object::Dict(d) => walk_dict(d, depth, out),
        Object::Array(a) => {
            for v in a {
                walk(v, depth + 1, out);
            }
        }
        _ => {}
    }
}

fn walk_dict(d: &Dict, depth: usize, out: &mut Vec<VersionFeature>) {
    if transparent(d) {
        out.push(VersionFeature::Transparency);
    }
    for name in filters(d) {
        match name {
            b"JBIG2Decode" => out.push(VersionFeature::Jbig2),
            b"JPXDecode" => out.push(VersionFeature::Jpx),
            b"BrotliDecode" => out.push(VersionFeature::Brotli),
            _ => {}
        }
    }
    for (_, v) in d.iter() {
        walk(v, depth + 1, out);
    }
}

/// Whether `d` itself carries a transparency entry.
fn transparent(d: &Dict) -> bool {
    let smask = d
        .get(b"SMask")
        .is_some_and(|v| v.as_name().is_none_or(|n| n.as_bytes() != b"None"));
    let alpha = [b"CA".as_slice(), b"ca"].iter().any(|k| {
        d.get(k)
            .and_then(Object::as_number)
            .is_some_and(|a| a < 1.0)
    });
    let blend = d.get(b"BM").is_some_and(|bm| {
        let normal = |o: &Object| {
            o.as_name()
                .is_some_and(|n| matches!(n.as_bytes(), b"Normal" | b"Compatible"))
        };
        match bm {
            Object::Array(a) => !a.iter().all(normal),
            other => !normal(other),
        }
    });
    let group = d
        .get(b"S")
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"Transparency");
    let in_data = d
        .get(b"SMaskInData")
        .and_then(Object::as_int)
        .is_some_and(|n| n != 0);
    smask || alpha || blend || group || in_data
}

/// The filter names on `d`, from a name or an array.
fn filters(d: &Dict) -> Vec<&[u8]> {
    match d.get(b"Filter") {
        Some(Object::Name(n)) => vec![n.as_bytes()],
        Some(Object::Array(a)) => a
            .iter()
            .filter_map(Object::as_name)
            .map(|n| n.as_bytes())
            .collect(),
        _ => Vec::new(),
    }
}
