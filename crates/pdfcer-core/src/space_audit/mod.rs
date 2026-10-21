//! Where a PDF's bytes go (Pass 616.0, decision 191): every byte of the file
//! is attributed to exactly one [`SpaceCategory`], every object gets its
//! stored size, and every image gets its effective resolution on the page.
//!
//! This is the measurement under `optimize`: an operator tunes per-category
//! and per-object settings against it, and the before/after report is
//! [`diff`] of two audits.
//!
//! **Invariant:** the `bytes` of [`SpaceAudit::categories`] sum to exactly
//! [`SpaceAudit::file_bytes`]. Nothing is estimated and nothing is dropped.
//!
//! Attribution rules:
//! - A top-level object owns its whole `N G obj … endobj` span.
//! - An object compressed in an object stream (ISO 32000-2 §7.5.7) owns a
//!   share of its container's span, proportional to its decoded length. The
//!   container's own dictionary and pair table are spread across the same
//!   shares.
//! - A byte outside every current object goes to one of three places.
//!   Superseded `N G obj` bodies from an earlier revision (§7.5.6) go to
//!   [`SpaceCategory::Superseded`]. `xref`/`trailer`/`startxref` sections and
//!   cross-reference streams go to [`SpaceCategory::CrossReference`].
//!   Everything else, such as the header and padding, goes to
//!   [`SpaceCategory::FileOverhead`].
//! - An object reachable from several roles takes the first role found:
//!   page content first, then annotations, then the catalog, then the trailer.

mod bytes;
mod classify;
mod images;
mod uses;

use std::collections::BTreeMap;
use std::fmt;

use crate::document::Document;
use crate::object::{Dict, ObjId, Object};

/// What a span of file bytes is for.
///
/// `#[non_exhaustive]`: categories are added as `optimize` learns to act on
/// finer distinctions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum SpaceCategory {
    /// Page content streams.
    ContentStreams,
    /// Form XObjects drawn by page content.
    FormXObjects,
    /// Image XObjects, including soft masks.
    Images,
    /// Font dictionaries, descriptors, embedded programs, `ToUnicode` maps.
    Fonts,
    /// Colour spaces and ICC profiles.
    ColorSpaces,
    /// Patterns and shadings.
    PatternsAndShadings,
    /// Graphics-state parameter dictionaries.
    GraphicsStates,
    /// Catalog, page tree and page dictionaries.
    PageStructure,
    /// Markup and other annotations, with their appearance streams.
    Annotations,
    /// Link annotations.
    Links,
    /// The interactive form and its widgets.
    Forms,
    /// The outline (bookmarks).
    Bookmarks,
    /// Named destinations.
    NamedDestinations,
    /// Page thumbnails.
    Thumbnails,
    /// The logical structure tree (tags).
    StructureTree,
    /// XMP metadata and the document information dictionary.
    Metadata,
    /// Attached (embedded) files.
    EmbeddedFiles,
    /// Document-level JavaScript.
    JavaScript,
    /// Optional content (layers).
    Layers,
    /// Reachable objects no other category claims.
    OtherReferenced,
    /// Current objects nothing reaches.
    Unreferenced,
    /// Cross-reference tables, trailers and cross-reference streams.
    CrossReference,
    /// Object bodies an incremental update replaced.
    Superseded,
    /// Header, comments, whitespace, and undecodable object-stream containers.
    FileOverhead,
}

impl SpaceCategory {
    /// Every category, in report order.
    pub const ALL: [Self; 24] = [
        Self::ContentStreams,
        Self::FormXObjects,
        Self::Images,
        Self::Fonts,
        Self::ColorSpaces,
        Self::PatternsAndShadings,
        Self::GraphicsStates,
        Self::PageStructure,
        Self::Annotations,
        Self::Links,
        Self::Forms,
        Self::Bookmarks,
        Self::NamedDestinations,
        Self::Thumbnails,
        Self::StructureTree,
        Self::Metadata,
        Self::EmbeddedFiles,
        Self::JavaScript,
        Self::Layers,
        Self::OtherReferenced,
        Self::Unreferenced,
        Self::CrossReference,
        Self::Superseded,
        Self::FileOverhead,
    ];

    /// The stable kebab-case name used in CLI output and JSON.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContentStreams => "content-streams",
            Self::FormXObjects => "form-xobjects",
            Self::Images => "images",
            Self::Fonts => "fonts",
            Self::ColorSpaces => "color-spaces",
            Self::PatternsAndShadings => "patterns-and-shadings",
            Self::GraphicsStates => "graphics-states",
            Self::PageStructure => "page-structure",
            Self::Annotations => "annotations",
            Self::Links => "links",
            Self::Forms => "forms",
            Self::Bookmarks => "bookmarks",
            Self::NamedDestinations => "named-destinations",
            Self::Thumbnails => "thumbnails",
            Self::StructureTree => "structure-tree",
            Self::Metadata => "metadata",
            Self::EmbeddedFiles => "embedded-files",
            Self::JavaScript => "javascript",
            Self::Layers => "layers",
            Self::OtherReferenced => "other-referenced",
            Self::Unreferenced => "unreferenced",
            Self::CrossReference => "cross-reference",
            Self::Superseded => "superseded",
            Self::FileOverhead => "file-overhead",
        }
    }
}

impl fmt::Display for SpaceCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The bytes and object count of one category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CategorySpace {
    /// The category.
    pub category: SpaceCategory,
    /// Bytes of the file attributed to it.
    pub bytes: u64,
    /// Current objects classified into it. Gap categories count 0.
    pub objects: usize,
}

/// One drawing of an image on a page.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub struct ImagePlacement {
    /// 1-based page number.
    pub page: usize,
    /// Drawn width in points (length of the CTM's x axis).
    pub width_pt: f64,
    /// Drawn height in points.
    pub height_pt: f64,
    /// Horizontal pixels per inch at this size.
    pub ppi_x: f64,
    /// Vertical pixels per inch at this size.
    pub ppi_y: f64,
}

impl ImagePlacement {
    /// The lower of the two axes: the resolution a downsample must keep.
    #[must_use]
    pub fn effective_ppi(&self) -> f64 {
        self.ppi_x.min(self.ppi_y)
    }
}

/// Image facts relevant to recompression.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ImageSpace {
    /// `/Width` in pixels.
    pub width: u32,
    /// `/Height` in pixels.
    pub height: u32,
    /// `/BitsPerComponent`; `None` for `/ImageMask` or JPX that omits it.
    pub bits_per_component: Option<u8>,
    /// The colour-space family name (`DeviceRGB`, `ICCBased`, `Indexed`, …).
    pub color_space: Option<String>,
    /// Colour components per sample, `/ICCBased` `/N` resolved; `None`
    /// when the colour space is missing or unreadable.
    pub components: Option<u8>,
    /// `/ImageMask true`: a 1-bit stencil with no colour space.
    pub image_mask: bool,
    /// Where page content draws it. Empty means drawn only by annotation
    /// appearances or not at all; those are not walked yet.
    pub placements: Vec<ImagePlacement>,
}

impl ImageSpace {
    /// The lowest horizontal ppi over all placements: the largest drawn
    /// use, which a downsample along the image's x axis must keep.
    #[must_use]
    pub fn binding_ppi_x(&self) -> Option<f64> {
        self.placements.iter().map(|p| p.ppi_x).reduce(f64::min)
    }

    /// The lowest vertical ppi over all placements.
    #[must_use]
    pub fn binding_ppi_y(&self) -> Option<f64> {
        self.placements.iter().map(|p| p.ppi_y).reduce(f64::min)
    }

    /// The lowest effective ppi over all placements, either axis: the
    /// resolution every use is drawn at or above (decision 192).
    #[must_use]
    pub fn binding_ppi(&self) -> Option<f64> {
        self.binding_placement().map(ImagePlacement::effective_ppi)
    }

    /// The placement drawn at the lowest effective ppi.
    #[must_use]
    pub fn binding_placement(&self) -> Option<&ImagePlacement> {
        self.placements
            .iter()
            .min_by(|a, b| a.effective_ppi().total_cmp(&b.effective_ppi()))
    }

    /// The image's class, as optimize rules select it: an image mask or a
    /// 1-bit image is [`ImageClass::Mono`] unless it is palette-indexed.
    #[must_use]
    pub fn class(&self) -> ImageClass {
        if matches!(self.color_space.as_deref(), Some("Indexed" | "I")) {
            ImageClass::Indexed
        } else if self.image_mask || self.bits_per_component == Some(1) {
            ImageClass::Mono
        } else if self.components == Some(1) {
            ImageClass::Gray
        } else {
            ImageClass::Color
        }
    }
}

/// The image classes optimize settings are grouped by (Acrobat's colour,
/// greyscale and monochrome panels, plus palette images).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum ImageClass {
    /// More than one colour component.
    Color,
    /// One component, more than 1 bit.
    Gray,
    /// 1 bit, or an image mask.
    Mono,
    /// `/Indexed`: samples are palette indices.
    Indexed,
}

impl ImageClass {
    /// Every class, in declaration order.
    pub const ALL: [Self; 4] = [Self::Color, Self::Gray, Self::Mono, Self::Indexed];

    /// The profile and CLI name: `color`, `gray`, `mono` or `indexed`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Color => "color",
            Self::Gray => "gray",
            Self::Mono => "mono",
            Self::Indexed => "indexed",
        }
    }

    /// The class named `name`, if any.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == name)
    }
}

impl fmt::Display for ImageClass {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One current object's size and identity.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ObjectSpace {
    /// The object.
    pub id: ObjId,
    /// Its category.
    pub category: SpaceCategory,
    /// File bytes attributed to it (a share of the container if compressed).
    pub stored_bytes: u64,
    /// Number of the containing object stream, if compressed.
    pub in_object_stream: Option<u32>,
    /// Raw (encoded) stream length, for a stream.
    pub stream_bytes: Option<usize>,
    /// Filter names, outermost first.
    pub filters: Vec<String>,
    /// `/Type`, if present.
    pub type_name: Option<String>,
    /// `/Subtype`, if present.
    pub subtype: Option<String>,
    /// Image facts, for an image XObject.
    pub image: Option<ImageSpace>,
    /// Pages (1-based, ascending) whose dictionary or resources reach it.
    /// Empty for an object no page reaches, such as the catalog or an
    /// outline.
    pub pages: Vec<usize>,
}

/// The result of [`audit`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct SpaceAudit {
    /// Total file size.
    pub file_bytes: u64,
    /// One row per category in [`SpaceCategory::ALL`] order, zero rows included.
    pub categories: Vec<CategorySpace>,
    /// Every current object, largest first.
    pub objects: Vec<ObjectSpace>,
    /// What the audit could not measure, in plain sentences.
    pub notes: Vec<String>,
}

impl SpaceAudit {
    /// Bytes attributed to `category`.
    #[must_use]
    pub fn bytes_in(&self, category: SpaceCategory) -> u64 {
        self.categories
            .iter()
            .find(|c| c.category == category)
            .map_or(0, |c| c.bytes)
    }
}

/// One category's size before and after.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct CategoryChange {
    /// The category.
    pub category: SpaceCategory,
    /// Bytes before.
    pub before: u64,
    /// Bytes after.
    pub after: u64,
}

/// The result of [`diff`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct SpaceDiff {
    /// File size before.
    pub before_bytes: u64,
    /// File size after.
    pub after_bytes: u64,
    /// Every category in [`SpaceCategory::ALL`] order.
    pub categories: Vec<CategoryChange>,
}

/// Attribute every byte of `doc`'s file to a category.
///
/// Read-only and infallible: an unparseable part of the file is still
/// counted, under [`SpaceCategory::FileOverhead`], and named in
/// [`SpaceAudit::notes`].
///
/// # Example
///
/// ```
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use pdfcer_core::document::Document;
/// use pdfcer_core::space_audit::{audit, SpaceCategory};
///
/// let pdf = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n\
/// 2 0 obj\n<< /Type /Pages /Kids [] /Count 0 >>\nendobj\n\
/// xref\n0 3\n0000000000 65535 f\r\n0000000009 00000 n\r\n0000000058 00000 n\r\n\
/// trailer\n<< /Size 3 /Root 1 0 R >>\nstartxref\n110\n%%EOF\n";
/// let doc = Document::from_bytes(pdf.to_vec())?;
/// let report = audit(&doc);
/// let total: u64 = report.categories.iter().map(|c| c.bytes).sum();
/// assert_eq!(total, report.file_bytes);
/// assert!(report.bytes_in(SpaceCategory::PageStructure) > 0);
/// # Ok(()) }
/// ```
#[must_use]
pub fn audit(doc: &Document) -> SpaceAudit {
    let attribution = bytes::attribute(doc);
    let roles = classify::classify(doc);
    let images = images::image_facts(doc);
    let mut uses = uses::page_uses(doc);

    let mut totals: BTreeMap<SpaceCategory, (u64, usize)> = BTreeMap::new();
    for (&cat, &n) in &attribution.gaps {
        totals.entry(cat).or_default().0 += n;
    }
    let mut objects = Vec::new();
    for obj in doc.objects() {
        if attribution.distributed.contains(&obj.id.num) {
            continue;
        }
        let category = roles
            .get(&obj.id)
            .copied()
            .unwrap_or(SpaceCategory::Unreferenced);
        let stored_bytes = attribution.per_object.get(&obj.id).copied().unwrap_or(0);
        let row = totals.entry(category).or_default();
        row.0 += stored_bytes;
        row.1 += 1;
        objects.push(object_space(
            obj.id,
            &obj.value,
            category,
            stored_bytes,
            &attribution,
            &images,
        ));
    }
    for o in &mut objects {
        o.pages = uses.remove(&o.id).unwrap_or_default();
    }
    objects.sort_by(|a, b| {
        b.stored_bytes
            .cmp(&a.stored_bytes)
            .then(a.id.num.cmp(&b.id.num))
    });

    let categories = SpaceCategory::ALL
        .iter()
        .map(|&category| {
            let (bytes, objects) = totals.get(&category).copied().unwrap_or_default();
            CategorySpace {
                category,
                bytes,
                objects,
            }
        })
        .collect();
    let mut notes = attribution.notes;
    if objects.iter().any(|o: &ObjectSpace| o.image.is_some()) {
        notes.push(
            "Image placements count page content only; annotation appearances are not walked."
                .to_owned(),
        );
    }
    SpaceAudit {
        file_bytes: doc.bytes().len() as u64,
        categories,
        objects,
        notes,
    }
}

fn object_space(
    id: ObjId,
    value: &Object,
    category: SpaceCategory,
    stored_bytes: u64,
    attribution: &bytes::Attribution,
    images: &BTreeMap<ObjId, ImageSpace>,
) -> ObjectSpace {
    let dict = dict_of(value);
    ObjectSpace {
        id,
        category,
        stored_bytes,
        in_object_stream: attribution.container_of.get(&id).copied(),
        stream_bytes: match value {
            Object::Stream(s) => Some(s.data_span.len),
            _ => None,
        },
        filters: dict.map(filter_names).unwrap_or_default(),
        type_name: dict.and_then(|d| name_of(d, b"Type")),
        subtype: dict.and_then(|d| name_of(d, b"Subtype")),
        image: images.get(&id).cloned(),
        pages: Vec::new(),
    }
}

/// Compare two audits category by category.
#[must_use]
pub fn diff(before: &SpaceAudit, after: &SpaceAudit) -> SpaceDiff {
    SpaceDiff {
        before_bytes: before.file_bytes,
        after_bytes: after.file_bytes,
        categories: SpaceCategory::ALL
            .iter()
            .map(|&category| CategoryChange {
                category,
                before: before.bytes_in(category),
                after: after.bytes_in(category),
            })
            .collect(),
    }
}

/// The dictionary of a dictionary or stream object.
pub(crate) const fn dict_of(value: &Object) -> Option<&Dict> {
    match value {
        Object::Dict(d) => Some(d),
        Object::Stream(s) => Some(&s.dict),
        _ => None,
    }
}

/// The name stored directly under `key`, as text.
pub(crate) fn name_of(dict: &Dict, key: &[u8]) -> Option<String> {
    dict.get(key)
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
}

fn filter_names(dict: &Dict) -> Vec<String> {
    match dict.get(b"Filter") {
        Some(Object::Name(n)) => vec![String::from_utf8_lossy(n.as_bytes()).into_owned()],
        Some(Object::Array(a)) => a
            .iter()
            .filter_map(Object::as_name)
            .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
pub(crate) mod tests;
