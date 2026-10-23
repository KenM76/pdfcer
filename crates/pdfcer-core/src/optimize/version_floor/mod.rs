//! The lowest PDF version a document's content needs: every key, name
//! value, flag and stream construct that ISO 32000-2 marks as introduced in
//! PDF 1.4 or later, found by walking every object with the context each
//! is reached in (the same key can be 1.4 in one dictionary and 2.0 in
//! another).
//!
//! Report-only. The floor starts at 1.3 because nothing older is detected:
//! a document at 1.3 may still use a 1.0-1.2 construct, which every 1.3
//! reader supports. File-structure features a full rewrite does not carry
//! over (object streams, cross-reference streams, the catalog `/Version`)
//! are reported apart in [`VersionFloor::structural`] and do not raise the
//! floor. Lowering a declared version is opt-in: ISO 32000-2 Annex I.2
//! says a writer should never change a document to an older version.

mod checks;
mod content;
mod ctx;
mod icc;
mod keys;
mod scan;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::fmt;

use crate::PdfVersion;
use crate::document::Document;
use crate::object::{ObjId, Object, Provenance};
use crate::xref::SectionShape;
use scan::{Jobs, Notes};

pub use icc::{IccProfile, IccVersionGate};

/// One construct a document uses and the version that introduced it.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Requirement {
    /// The version the construct needs.
    pub version: PdfVersion,
    /// What it is: a key (`/UserUnit`), a key and value (`/Filter
    /// /JPXDecode`), or a short description (`16-bit image samples`).
    pub construct: String,
    /// Objects that use it (the trailer counts as one).
    pub objects: usize,
    /// The lowest-numbered object using it; `None` for the trailer or a
    /// document-level feature.
    pub example: Option<ObjId>,
}

impl fmt::Display for Requirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PDF {}: {} ({} object(s)",
            self.version, self.construct, self.objects
        )?;
        if let Some(id) = self.example {
            write!(f, ", e.g. {} {} R", id.num, id.generation)?;
        }
        f.write_str(")")
    }
}

/// Why a floor is not proven: a stream whose data the scan needs could not
/// be read, so a construct inside it may have been missed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Unprovable {
    /// A content stream that does not decode or tokenize.
    UnreadableContent(ObjId),
    /// A font program that does not decode.
    UnreadableFont(ObjId),
    /// An ICC profile that does not decode, under [`IccVersionGate::Strict`].
    UnreadableProfile(ObjId),
}

/// What [`version_floor`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct VersionFloor {
    /// The highest version any requirement needs, and at least 1.3. It can
    /// exceed the declared version: the document already uses something
    /// newer than it claims.
    pub floor: PdfVersion,
    /// Content requirements, highest version first: the first entries
    /// explain the floor.
    pub requirements: Vec<Requirement>,
    /// File-structure features a full rewrite drops; excluded from
    /// [`floor`](Self::floor).
    pub structural: Vec<Requirement>,
    /// Non-empty when the floor is a lower bound only.
    pub unprovable: Vec<Unprovable>,
    /// Every embedded ICC profile whose header could be read. They raise
    /// [`floor`](Self::floor) only under [`IccVersionGate::Strict`].
    pub icc_profiles: Vec<IccProfile>,
}

/// Settings for [`version_floor_with`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FloorOptions {
    /// Whether ICC profile versions raise the floor.
    pub icc: IccVersionGate,
}

impl FloorOptions {
    /// Set [`icc`](Self::icc).
    #[must_use]
    pub fn with_icc(mut self, icc: IccVersionGate) -> Self {
        self.icc = icc;
        self
    }
}

impl VersionFloor {
    /// Whether the scan read everything it needed, so
    /// [`floor`](Self::floor) is exact.
    #[must_use]
    pub fn proven(&self) -> bool {
        self.unprovable.is_empty()
    }
}

/// The lowest version `doc`'s content needs.
///
/// # Example
///
/// ```
/// # use pdfcer_core::{document::Document, optimize::version_floor};
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let bytes = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/synthetic/hello.pdf"))?;
/// let doc = Document::from_bytes(bytes)?;
/// let found = version_floor(&doc);
/// println!("declared {}, needs {}", doc.version(), found.floor);
/// for r in &found.requirements {
///     println!("  {r}");
/// }
/// # Ok(()) }
/// ```
#[must_use]
pub fn version_floor(doc: &Document) -> VersionFloor {
    version_floor_with(doc, FloorOptions::default())
}

/// [`version_floor`] with explicit settings.
#[must_use]
pub fn version_floor_with(doc: &Document, options: FloorOptions) -> VersionFloor {
    let reached = scan::reached(doc);
    let mut tally = Tally::default();
    let mut unprovable = Vec::new();
    let mut jobs = Jobs::default();
    let mut notes = Notes::new();
    let mut icc_profiles = Vec::new();
    scan::scan_dict(doc.trailer(), ctx::TRAILER, 0, &mut notes, &mut jobs);
    tally.add(&mut notes, None);
    for obj in doc.objects() {
        let bits = reached.get(&obj.id).copied().unwrap_or(0);
        let (dict, stream) = match &obj.value {
            Object::Dict(d) => (d, false),
            Object::Stream(s) => (&s.dict, true),
            _ => continue,
        };
        let stream_bits = if stream { ctx::STREAM } else { 0 };
        scan::scan_dict(dict, bits | stream_bits, 0, &mut notes, &mut jobs);
        if stream && (ctx::own(dict, true) | bits) & ctx::CONTENT != 0 {
            let read =
                content::decoded(doc, obj.id).is_some_and(|d| content::scan_content(d, &mut notes));
            if !read {
                unprovable.push(Unprovable::UnreadableContent(obj.id));
            }
        }
        if stream && bits & ctx::ICC != 0 {
            profile(
                doc,
                obj.id,
                options.icc,
                (&mut icc_profiles, &mut unprovable, &mut notes),
            );
        }
        tally.add(&mut notes, Some(obj.id));
    }
    for id in std::mem::take(&mut jobs.fonts) {
        match content::decoded(doc, id) {
            Some(data) => content::scan_font(&data, &mut notes),
            None => unprovable.push(Unprovable::UnreadableFont(id)),
        }
        tally.add(&mut notes, Some(id));
    }
    let requirements = tally.into_requirements();
    let floor = requirements
        .iter()
        .map(|r| r.version)
        .fold(V13, PdfVersion::max);
    VersionFloor {
        floor,
        requirements,
        structural: structural(doc),
        unprovable,
        icc_profiles,
    }
}

fn profile(
    doc: &Document,
    id: ObjId,
    gate: IccVersionGate,
    (found, unprovable, notes): (&mut Vec<IccProfile>, &mut Vec<Unprovable>, &mut Notes),
) {
    let Some((major, minor)) = content::decoded(doc, id).and_then(|d| icc::header(&d)) else {
        if gate == IccVersionGate::Strict {
            unprovable.push(Unprovable::UnreadableProfile(id));
        }
        return;
    };
    let strict = icc::strict(major, minor);
    if gate == IccVersionGate::Strict
        && let Some(v) = strict
    {
        notes.insert((v, format!("ICC profile version {major}.{minor}")));
    }
    found.push(IccProfile {
        object: id,
        major,
        minor,
        strict: strict.map(version),
    });
}

const V13: PdfVersion = PdfVersion { major: 1, minor: 3 };

const fn version(code: u8) -> PdfVersion {
    PdfVersion {
        major: code / 10,
        minor: code % 10,
    }
}

#[derive(Default)]
struct Tally(BTreeMap<(u8, String), (usize, Option<ObjId>)>);

impl Tally {
    fn add(&mut self, notes: &mut Notes, id: Option<ObjId>) {
        for key in std::mem::take(notes) {
            let e = self.0.entry(key).or_insert((0, None));
            e.0 += 1;
            e.1 = match (e.1, id) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (a, b) => a.or(b),
            };
        }
    }

    fn into_requirements(self) -> Vec<Requirement> {
        let mut out: Vec<Requirement> = self
            .0
            .into_iter()
            .map(|((v, construct), (objects, example))| Requirement {
                version: version(v),
                construct,
                objects,
                example,
            })
            .collect();
        out.sort_by(|a, b| {
            b.version
                .cmp(&a.version)
                .then_with(|| a.construct.cmp(&b.construct))
        });
        out
    }
}

/// Object streams and cross-reference streams (§7.5.7, §7.5.8, PDF 1.5)
/// and the catalog `/Version` (§7.7.2).
fn structural(doc: &Document) -> Vec<Requirement> {
    let mut out = Vec::new();
    let packed: Vec<ObjId> = doc
        .objects()
        .filter(|o| matches!(o.provenance, Provenance::ObjectStream { .. }))
        .map(|o| o.id)
        .collect();
    if !packed.is_empty() {
        out.push(requirement(
            15,
            "object streams",
            packed.len(),
            packed.iter().min().copied(),
        ));
    }
    if matches!(doc.section_shape(), SectionShape::Stream { .. }) || doc.is_hybrid() {
        out.push(requirement(15, "cross-reference stream", 1, None));
    }
    let declared = doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"Version"))
        .and_then(Object::as_name);
    if let Some(v) = declared.and_then(|n| parse_version(n.as_bytes())) {
        out.push(Requirement {
            version: v,
            construct: "catalog /Version".to_owned(),
            objects: 1,
            example: None,
        });
    }
    out
}

fn requirement(v: u8, what: &str, objects: usize, example: Option<ObjId>) -> Requirement {
    Requirement {
        version: version(v),
        construct: what.to_owned(),
        objects,
        example,
    }
}

fn parse_version(b: &[u8]) -> Option<PdfVersion> {
    match b {
        [major @ b'0'..=b'9', b'.', minor @ b'0'..=b'9'] => Some(PdfVersion {
            major: major - b'0',
            minor: minor - b'0',
        }),
        _ => None,
    }
}
