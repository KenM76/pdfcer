//! Import an SVG drawing as PDF vector content (decision 186).
//!
//! [`import`](crate::svg_import::import) parses SVG/SVGZ with `usvg` (no text shaping, no system
//! fonts, no file or network access) and converts the simplified tree into
//! a Form XObject whose `/BBox` is the SVG's viewport, in SVG pixels
//! (1 px = 0.75 pt at placement's natural size). The result is a pure value;
//! [`crate::edit::EditSession::add_svg`] writes it into a document.
//!
//! Carried as vector content: paths, fills, strokes (width, caps, joins,
//! dashes, miter limit), linear and radial gradients (shading patterns,
//! ISO 32000-1 §8.7.4.5.3–4), tiling patterns (§8.7.3), clip paths,
//! group and element opacity, blend modes and isolation (transparency
//! groups and `ExtGState`, §11.4, §11.6.4), masks (soft masks, §11.6.5) and
//! embedded PNG/JPEG `data:` images. Everything else is counted by name in
//! [`SvgImportNotes`](crate::svg_import::SvgImportNotes) — never dropped silently.

mod emit;
mod objects;
mod paint;
mod prescan;

use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use usvg::roxmltree;

pub(crate) use objects::SvgObject;

use crate::object::ObjId;

/// Largest SVG or SVGZ file [`import`] reads, in bytes.
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
/// Largest an SVGZ file may decompress to, in bytes.
pub const MAX_DECOMPRESSED_BYTES: usize = 64 * 1024 * 1024;
/// Deepest element nesting accepted, counting `use`, clip, mask, pattern
/// and filter references as nesting.
pub const MAX_ELEMENT_DEPTH: usize = 256;
/// Most PDF content (content streams, functions, images) one import may
/// produce, in bytes.
pub const MAX_CONTENT_BYTES: usize = 64 * 1024 * 1024;
/// Most XML nodes accepted before `use` expansion.
const MAX_XML_NODES: u32 = 500_000;
/// Nested SVG images (`<image href="data:image/svg+xml,…">`) are followed
/// this many levels deep.
const MAX_NESTED_SVG: usize = 1;

/// Something in an SVG that the import skipped or approximated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SvgFeature {
    /// `<text>` — not carried in this version.
    Text,
    /// A `filter` (blur, drop shadow, …): the content is drawn unfiltered.
    Filter,
    /// `<foreignObject>` (embedded HTML).
    ForeignObject,
    /// An `<image>` referencing a file or URL; only `data:` URIs are read.
    ExternalImage,
    /// A GIF `<image>`.
    GifImage,
    /// A WebP `<image>`.
    WebpImage,
    /// An `<image>` whose data could not be decoded.
    UndecodableImage,
    /// An SVG `<image>` that failed to parse, or an image inside an SVG used
    /// as an image (SVG draws none there either).
    NestedSvgImage,
    /// A luminance `mask`: PDF's luminosity weights differ slightly from
    /// SVG's.
    LuminanceMask,
    /// A gradient `spreadMethod` of `repeat`/`reflect` repeated more than
    /// 64 times across the shape, or under a singular transform.
    GradientSpread,
    /// `stroke-linejoin: miter-clip`, drawn as `miter`.
    MiterClipJoin,
    /// A pattern nested inside more than four patterns.
    NestedPattern,
}

impl SvgFeature {
    /// Every variant, in [`Ord`] order.
    pub const ALL: [Self; 12] = [
        Self::Text,
        Self::Filter,
        Self::ForeignObject,
        Self::ExternalImage,
        Self::GifImage,
        Self::WebpImage,
        Self::UndecodableImage,
        Self::NestedSvgImage,
        Self::LuminanceMask,
        Self::GradientSpread,
        Self::MiterClipJoin,
        Self::NestedPattern,
    ];

    /// A short lowercase name for reports: `"text"`, `"filter"`, ….
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Filter => "filter",
            Self::ForeignObject => "foreignObject",
            Self::ExternalImage => "external image reference",
            Self::GifImage => "GIF image",
            Self::WebpImage => "WebP image",
            Self::UndecodableImage => "undecodable image",
            Self::NestedSvgImage => "nested SVG image",
            Self::LuminanceMask => "luminance mask",
            Self::GradientSpread => "gradient spread method",
            Self::MiterClipJoin => "miter-clip line join",
            Self::NestedPattern => "nested pattern",
        }
    }

    const fn index(self) -> usize {
        self as usize
    }
}

impl fmt::Display for SvgFeature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// What an import did not carry exactly, by feature and occurrence count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SvgImportNotes {
    /// Features left out of the PDF.
    pub skipped: BTreeMap<SvgFeature, usize>,
    /// Features drawn, but not exactly as an SVG renderer draws them.
    pub approximated: BTreeMap<SvgFeature, usize>,
}

impl SvgImportNotes {
    /// `true` when everything was carried exactly.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.skipped.is_empty() && self.approximated.is_empty()
    }

    /// One line for an operator, e.g.
    /// `"not carried: filter ×1, text ×2; approximated: luminance mask ×1"`.
    /// Empty when [`Self::is_empty`].
    #[must_use]
    pub fn summary(&self) -> String {
        fn list(m: &BTreeMap<SvgFeature, usize>) -> String {
            m.iter()
                .map(|(f, n)| format!("{f} \u{d7}{n}"))
                .collect::<Vec<_>>()
                .join(", ")
        }
        let mut parts = Vec::new();
        if !self.skipped.is_empty() {
            parts.push(format!("not carried: {}", list(&self.skipped)));
        }
        if !self.approximated.is_empty() {
            parts.push(format!("approximated: {}", list(&self.approximated)));
        }
        parts.join("; ")
    }

    /// Count `n` elements of `f` as not carried; zero records nothing.
    pub(crate) fn skip(&mut self, f: SvgFeature, n: usize) {
        if n > 0 {
            *self.skipped.entry(f).or_insert(0) += n;
        }
    }

    /// Count one use of `f` drawn approximately.
    pub(crate) fn approximate(&mut self, f: SvgFeature) {
        *self.approximated.entry(f).or_insert(0) += 1;
    }
}

/// An SVG converted to PDF objects, ready for
/// [`EditSession::add_svg`](crate::edit::EditSession::add_svg).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportedSvg {
    width: f64,
    height: f64,
    pub(crate) objects: Vec<SvgObject>,
    pub(crate) root: ObjId,
    notes: SvgImportNotes,
}

impl ImportedSvg {
    /// The viewport size in SVG pixels — the Form XObject's `/BBox` is
    /// `[0 0 w h]`.
    #[must_use]
    pub const fn size_px(&self) -> (f64, f64) {
        (self.width, self.height)
    }

    /// The size at 96 px per inch, in points (CSS reference pixel).
    #[must_use]
    pub fn natural_size_pt(&self) -> (f64, f64) {
        (self.width * 0.75, self.height * 0.75)
    }

    /// What the import skipped or approximated.
    #[must_use]
    pub const fn notes(&self) -> &SvgImportNotes {
        &self.notes
    }

    /// How many PDF objects placing it creates (before image soft masks).
    #[must_use]
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }
}

/// Why an SVG could not be imported.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SvgImportError {
    /// The file is over [`MAX_INPUT_BYTES`].
    #[error("the SVG is {len} bytes, over the {limit}-byte import limit")]
    TooLarge {
        /// The file's length.
        len: usize,
        /// [`MAX_INPUT_BYTES`].
        limit: usize,
    },
    /// The SVGZ decompresses past [`MAX_DECOMPRESSED_BYTES`].
    #[error("the compressed SVG expands past the {limit}-byte limit")]
    DecompressedTooLarge {
        /// [`MAX_DECOMPRESSED_BYTES`].
        limit: usize,
    },
    /// The SVGZ's gzip stream is damaged.
    #[error("the compressed SVG is damaged: {0}")]
    Gzip(String),
    /// The SVG is not UTF-8 text.
    #[error("the SVG is not UTF-8 text")]
    NotUtf8,
    /// The SVG is not well-formed XML, or has more than 500,000 nodes.
    #[error("the SVG is not well-formed XML: {0}")]
    Xml(String),
    /// The XML is not a usable SVG document (no root `<svg>`, zero size, …).
    #[error("not a usable SVG document: {0}")]
    Svg(String),
    /// Elements nest deeper than [`MAX_ELEMENT_DEPTH`].
    #[error(
        "elements nest {depth} deep (counting use, clip, mask and pattern references), over the {limit} limit"
    )]
    TooDeep {
        /// The nesting found (an upper bound).
        depth: usize,
        /// [`MAX_ELEMENT_DEPTH`].
        limit: usize,
    },
    /// The drawing needs more than [`MAX_CONTENT_BYTES`] of PDF content.
    #[error("the drawing needs more than {limit} bytes of PDF content")]
    TooComplex {
        /// [`MAX_CONTENT_BYTES`].
        limit: usize,
    },
}

/// Convert an SVG or SVGZ file into PDF vector content.
///
/// # Errors
///
/// [`SvgImportError`] when the input is over a size, depth or output limit,
/// or is not a parseable SVG. Unsupported features are not errors: they are
/// counted in [`ImportedSvg::notes`].
///
/// # Examples
///
/// ```
/// use pdfcer_core::svg_import::{import, SvgFeature};
///
/// let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="100">
///   <rect width="200" height="100" fill="teal"/>
///   <text x="10" y="50">hello</text>
/// </svg>"#;
/// let imported = import(svg)?;
/// assert_eq!(imported.size_px(), (200.0, 100.0));
/// assert_eq!(imported.notes().skipped.get(&SvgFeature::Text), Some(&1));
/// # Ok::<(), pdfcer_core::svg_import::SvgImportError>(())
/// ```
pub fn import(bytes: &[u8]) -> Result<ImportedSvg, SvgImportError> {
    let tally = Tally::default();
    let tree = parse_tree(bytes, 0, &tally)?;
    let mut notes = SvgImportNotes::default();
    for (f, n) in SvgFeature::ALL.into_iter().zip(&tally.0) {
        notes.skip(f, n.load(Ordering::Relaxed));
    }
    emit::emit_document(&tree, notes)
}

/// Counts gathered during parsing, including inside `usvg`'s callbacks.
#[derive(Debug, Default)]
struct Tally([AtomicUsize; SvgFeature::ALL.len()]);

impl Tally {
    fn add(&self, f: SvgFeature, n: usize) {
        if let Some(c) = self.0.get(f.index()) {
            c.fetch_add(n, Ordering::Relaxed);
        }
    }
}

fn parse_tree(bytes: &[u8], level: usize, tally: &Tally) -> Result<usvg::Tree, SvgImportError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(SvgImportError::TooLarge {
            len: bytes.len(),
            limit: MAX_INPUT_BYTES,
        });
    }
    let inflated;
    let bytes = if bytes.starts_with(&[0x1f, 0x8b]) {
        inflated = gunzip(bytes)?;
        inflated.as_slice()
    } else {
        bytes
    };
    let text = std::str::from_utf8(bytes).map_err(|_| SvgImportError::NotUtf8)?;
    let xml = roxmltree::ParsingOptions {
        allow_dtd: true,
        nodes_limit: MAX_XML_NODES,
    };
    let doc = roxmltree::Document::parse_with_options(text, xml)
        .map_err(|e| SvgImportError::Xml(e.to_string()))?;
    prescan::check(&doc, tally)?;

    let image_href_resolver = usvg::ImageHrefResolver {
        resolve_data: Box::new(move |mime, data, _| resolve_data(mime, &data, level, tally)),
        resolve_string: Box::new(move |_, _| {
            let f = if level >= MAX_NESTED_SVG {
                SvgFeature::NestedSvgImage
            } else {
                SvgFeature::ExternalImage
            };
            tally.add(f, 1);
            None
        }),
    };
    let opt = usvg::Options {
        image_href_resolver,
        ..usvg::Options::default()
    };
    usvg::Tree::from_xmltree(&doc, &opt).map_err(|e| SvgImportError::Svg(e.to_string()))
}

/// Decompress an SVGZ, refusing past [`MAX_DECOMPRESSED_BYTES`].
fn gunzip(bytes: &[u8]) -> Result<Vec<u8>, SvgImportError> {
    let limit = MAX_DECOMPRESSED_BYTES as u64;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(bytes)
        .take(limit + 1)
        .read_to_end(&mut out)
        .map_err(|e| SvgImportError::Gzip(e.to_string()))?;
    if out.len() as u64 > limit {
        return Err(SvgImportError::DecompressedTooLarge {
            limit: MAX_DECOMPRESSED_BYTES,
        });
    }
    Ok(out)
}

/// Resolve a `data:` URI image: rasters pass through by magic number, SVG
/// recurses through [`parse_tree`] with the same limits.
fn resolve_data(
    mime: &str,
    data: &Arc<Vec<u8>>,
    level: usize,
    tally: &Tally,
) -> Option<usvg::ImageKind> {
    // SVG 2 §16.2: an SVG used as an image draws no images of its own.
    if level >= MAX_NESTED_SVG {
        tally.add(SvgFeature::NestedSvgImage, 1);
        return None;
    }
    let d = data.as_slice();
    if d.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some(usvg::ImageKind::JPEG(Arc::clone(data)));
    }
    if d.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some(usvg::ImageKind::PNG(Arc::clone(data)));
    }
    if d.starts_with(b"GIF8") {
        return Some(usvg::ImageKind::GIF(Arc::clone(data)));
    }
    if d.starts_with(b"RIFF") && d.get(8..12) == Some(b"WEBP".as_slice()) {
        return Some(usvg::ImageKind::WEBP(Arc::clone(data)));
    }
    if mime == "image/svg+xml" || mime == "text/plain" {
        return match parse_tree(d, level + 1, tally) {
            Ok(tree) => Some(usvg::ImageKind::SVG(tree)),
            Err(_) => {
                tally.add(SvgFeature::NestedSvgImage, 1);
                None
            }
        };
    }
    tally.add(SvgFeature::UndecodableImage, 1);
    None
}
