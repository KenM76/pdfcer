//! Embedded 3D artwork: find it, and get its bytes out unchanged.
//!
//! Reads the two routes a PDF carries a 3D model by:
//!
//! - a `/Subtype /3D` annotation whose `/3DD` is a 3D stream or a
//!   `/3DRef` dictionary naming a shared one (ISO 32000-1 §13.6.2 Table 298,
//!   §13.6.3 Table 300, §13.6.3.3 Table 303);
//! - a `/Subtype /RichMedia` annotation whose `/RichMediaContent`
//!   `/Configurations` hold a `/3D` instance naming an embedded-file asset
//!   (ISO 32000-2 §13.7, Tables 341–343).
//!
//! Nothing here decodes a model. [`extract_3d`] undoes the stream's `/Filter`
//! chain (bounded by [`crate::filters::MAX_DECODED_LEN`]) and returns the
//! U3D / PRC / STEP bytes as the file carried them. The returned bytes are
//! untrusted.
//!
//! Every walk is bounded: annotations per page by
//! [`crate::annot::MAX_ANNOTS_PER_PAGE`], configurations and instances by
//! [`MAX_RICH_MEDIA_ENTRIES`], the whole listing by [`MAX_3D_ARTWORKS`].
//! Hitting a bound sets [`ThreeDNotes::truncated`].

use std::collections::HashSet;

use crate::annot::MAX_ANNOTS_PER_PAGE;
use crate::filters::{self, FilterError};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::textstring::decode_text_string;
use crate::view::DocumentView;

/// Ceiling on how many artworks one listing reports (a pdfcer guard; the
/// spec sets none).
pub const MAX_3D_ARTWORKS: usize = 65_536;

/// Ceiling on `/Configurations` entries, and on `/Instances` per
/// configuration, walked in one RichMedia annotation (a pdfcer guard).
pub const MAX_RICH_MEDIA_ENTRIES: usize = 1_024;

/// A 3D data format, as declared by the file or recognised from the bytes.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ThreeDFormat {
    /// ECMA-363 Universal 3D.
    U3d,
    /// ISO 14739-1 Product Representation Compact.
    Prc,
    /// ISO 10303-21 STEP. Only legal as a RichMedia asset (ISO 32000-2
    /// §13.7); a `/3D` stream may not carry it.
    Step,
    /// Anything else, carrying the declared name or MIME type bytes.
    Other(Vec<u8>),
}

impl ThreeDFormat {
    /// The conventional file extension, without the dot.
    ///
    /// # Examples
    ///
    /// ```
    /// use pdfcer_core::threed::ThreeDFormat;
    /// assert_eq!(ThreeDFormat::Prc.extension(), "prc");
    /// assert_eq!(ThreeDFormat::Other(b"X".to_vec()).extension(), "bin");
    /// ```
    #[must_use]
    pub fn extension(&self) -> &'static str {
        match self {
            Self::U3d => "u3d",
            Self::Prc => "prc",
            Self::Step => "stp",
            Self::Other(_) => "bin",
        }
    }

    /// A short label for listings: `U3D`, `PRC`, `STEP`, or the declared
    /// bytes lossily decoded.
    #[must_use]
    pub fn label(&self) -> String {
        match self {
            Self::U3d => "U3D".to_owned(),
            Self::Prc => "PRC".to_owned(),
            Self::Step => "STEP".to_owned(),
            Self::Other(bytes) => String::from_utf8_lossy(bytes).into_owned(),
        }
    }
}

/// Recognise a 3D format from the first bytes of decoded data.
///
/// U3D opens with the file-header block type `0x00443355` stored
/// little-endian (`U3D\0`, ECMA-363 §9.4.1); PRC with the ASCII `PRC`
/// (ISO 14739-1 §7); STEP with `ISO-10303-21;` (ISO 10303-21 §5).
///
/// # Examples
///
/// ```
/// use pdfcer_core::threed::{sniff_3d_format, ThreeDFormat};
/// assert_eq!(sniff_3d_format(b"U3D\0\x18\0\0\0"), Some(ThreeDFormat::U3d));
/// assert_eq!(sniff_3d_format(b"PRC\x08"), Some(ThreeDFormat::Prc));
/// assert_eq!(sniff_3d_format(b"ISO-10303-21;\n"), Some(ThreeDFormat::Step));
/// assert_eq!(sniff_3d_format(b"%PDF"), None);
/// ```
#[must_use]
pub fn sniff_3d_format(bytes: &[u8]) -> Option<ThreeDFormat> {
    if bytes.starts_with(b"U3D\0") {
        Some(ThreeDFormat::U3d)
    } else if bytes.starts_with(b"PRC") {
        Some(ThreeDFormat::Prc)
    } else if bytes.starts_with(b"ISO-10303-21;") {
        Some(ThreeDFormat::Step)
    } else {
        None
    }
}

/// Where an artwork's bytes live.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ThreeDSource {
    /// A `/3D` annotation's 3D stream.
    Stream {
        /// `true` when reached through a `/3DRef` dictionary (Table 303),
        /// i.e. the stream may be shared by several annotations.
        shared: bool,
    },
    /// A RichMedia `/3D` instance's `/Asset` file specification.
    RichMediaAsset {
        /// The asset's `/UF` (else `/F`) filename, decoded as a §7.9.2
        /// text string. Untrusted: pass it through
        /// [`crate::attachments::sanitize_attachment_name`] before using
        /// it as a path.
        name: Option<String>,
        /// The file specification's object id, when indirect.
        filespec_id: Option<ObjId>,
    },
}

/// One piece of 3D artwork found on a page.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ThreeDArtwork {
    /// Zero-based page index.
    pub page_index: usize,
    /// The annotation's object id, when indirect.
    pub annot_id: Option<ObjId>,
    /// Which route carries the bytes.
    pub source: ThreeDSource,
    /// The stream holding the data. `None` when the annotation names no
    /// resolvable stream; such an artwork cannot be extracted.
    pub stream_id: Option<ObjId>,
    /// The format the file declares: a 3D stream's `/Subtype` (Table 300),
    /// else an asset's MIME `/Subtype` or filename extension. `None` when
    /// nothing declares one.
    pub declared: Option<ThreeDFormat>,
    /// Entries in the 3D stream's `/VA` views array (Table 300); 0 for a
    /// RichMedia asset.
    pub view_count: usize,
    /// The annotation has an `/AP /N` poster appearance (required for a
    /// `/3D` annotation by §13.6.2).
    pub has_poster: bool,
}

/// What a listing could not do.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct ThreeDNotes {
    /// A bound was hit; the listing is incomplete.
    pub truncated: bool,
    /// The page tree could not be walked, so nothing was listed.
    pub page_tree_unwalkable: bool,
    /// `/3D` or RichMedia annotations whose data named no resolvable
    /// stream. Counted so a visible 3D poster with no listing row is
    /// explained.
    pub annotations_without_stream: usize,
}

/// Why [`extract_3d`] produced no bytes.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
pub enum ThreeDError {
    /// The artwork names no stream.
    #[error("3D artwork has no data stream")]
    NoStream,
    /// The stream id is missing from this view or is not a stream.
    #[error("3D stream {0} is missing or is not a stream")]
    StreamUnresolvable(ObjId),
    /// The view cannot serve the stream's byte span.
    #[error("3D stream {0} has a byte span this view cannot serve")]
    SpanUnservable(ObjId),
    /// The `/Filter` chain failed, is unsupported, or exceeded the decode
    /// ceiling.
    #[error("3D stream could not be decoded: {0}")]
    Decode(#[from] FilterError),
}

/// Extracted 3D data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Extracted3D {
    /// The decoded bytes, exactly as the model file.
    pub data: Vec<u8>,
    /// The format the bytes' magic identifies, if any.
    pub sniffed: Option<ThreeDFormat>,
}

impl Extracted3D {
    /// `true` when the bytes' magic names a format other than the one the
    /// file declared. A caller should disclose this; it is either a
    /// mislabelled file or a hostile one.
    #[must_use]
    pub fn contradicts(&self, declared: Option<&ThreeDFormat>) -> bool {
        match (declared, &self.sniffed) {
            (Some(d), Some(s)) => d != s,
            _ => false,
        }
    }
}

/// Every piece of 3D artwork in the document, in page then `/Annots` order.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::threed::list_3d;
/// let doc = Document::from_bytes(
///     include_bytes!("../../../fixtures/synthetic/minimal.pdf").to_vec(),
/// )?;
/// assert!(list_3d(&doc).is_empty());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
pub fn list_3d<G: ObjectGraph + ?Sized>(graph: &G) -> Vec<ThreeDArtwork> {
    list_3d_with_notes(graph).0
}

/// [`list_3d`] plus what the listing could not do.
#[must_use]
pub fn list_3d_with_notes<G: ObjectGraph + ?Sized>(graph: &G) -> (Vec<ThreeDArtwork>, ThreeDNotes) {
    let mut out = Vec::new();
    let mut notes = ThreeDNotes::default();
    let Ok(pages) = crate::page_tree::pages_in(graph) else {
        notes.page_tree_unwalkable = true;
        return (out, notes);
    };
    for (page_index, page) in pages.iter().enumerate() {
        let Some(annots) = graph
            .resolved(page.id)
            .as_dict()
            .and_then(|d| d.get(b"Annots"))
            .map(|o| graph.resolve(o))
            .and_then(Object::as_array)
        else {
            continue;
        };
        for (seen, entry) in annots.iter().enumerate() {
            if seen >= MAX_ANNOTS_PER_PAGE {
                notes.truncated = true;
                break;
            }
            let Some(annot) = graph.resolve(entry).as_dict() else {
                continue;
            };
            let base = ThreeDArtwork {
                page_index,
                annot_id: entry.as_reference(),
                source: ThreeDSource::Stream { shared: false },
                stream_id: None,
                declared: None,
                view_count: 0,
                has_poster: has_poster(graph, annot),
            };
            let before = out.len();
            match name_of(graph, annot, b"Subtype") {
                Some(b"3D") => three_d_annotation(graph, annot, base, &mut out),
                Some(b"RichMedia") => rich_media(graph, annot, &base, &mut out, &mut notes),
                _ => continue,
            }
            if out.len() == before {
                notes.annotations_without_stream += 1;
            }
            if out.len() >= MAX_3D_ARTWORKS {
                out.truncate(MAX_3D_ARTWORKS);
                notes.truncated = true;
                return (out, notes);
            }
        }
    }
    (out, notes)
}

fn name_of<'a, G: ObjectGraph + ?Sized>(
    graph: &'a G,
    dict: &'a Dict,
    key: &[u8],
) -> Option<&'a [u8]> {
    dict.get(key)
        .map(|o| graph.resolve(o))
        .and_then(Object::as_name)
        .map(|n| n.as_bytes())
}

fn has_poster<G: ObjectGraph + ?Sized>(graph: &G, annot: &Dict) -> bool {
    annot
        .get(b"AP")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
        .and_then(|ap| ap.get(b"N"))
        .is_some_and(|n| !matches!(graph.resolve(n), Object::Null))
}

/// Follow `obj` by reference to a stream, returning its id and dictionary.
fn stream_at<'a, G: ObjectGraph + ?Sized>(graph: &'a G, obj: &Object) -> Option<(ObjId, &'a Dict)> {
    let id = obj.as_reference()?;
    match graph.resolved(id) {
        Object::Stream(s) => Some((id, &s.dict)),
        _ => None,
    }
}

/// §13.6.2 Table 298 `/3DD`: a 3D stream, or a `/3DRef` dictionary whose
/// `/3D` entry is the (shared) stream (Table 303).
fn three_d_annotation<G: ObjectGraph + ?Sized>(
    graph: &G,
    annot: &Dict,
    mut art: ThreeDArtwork,
    out: &mut Vec<ThreeDArtwork>,
) {
    let Some(dd) = annot.get(b"3DD") else {
        return;
    };
    let found = stream_at(graph, dd).map(|s| (s, false)).or_else(|| {
        let r = graph.resolve(dd).as_dict()?;
        (name_of(graph, r, b"Type") == Some(b"3DRef"))
            .then(|| r.get(b"3D"))
            .flatten()
            .and_then(|o| stream_at(graph, o))
            .map(|s| (s, true))
    });
    let Some(((id, dict), shared)) = found else {
        return;
    };
    art.source = ThreeDSource::Stream { shared };
    art.stream_id = Some(id);
    art.declared = name_of(graph, dict, b"Subtype").map(|n| match n {
        b"U3D" => ThreeDFormat::U3d,
        b"PRC" => ThreeDFormat::Prc,
        other => ThreeDFormat::Other(other.to_vec()),
    });
    art.view_count = dict
        .get(b"VA")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
        .map_or(0, <[Object]>::len);
    out.push(art);
}

/// ISO 32000-2 §13.7: `/RichMediaContent` → `/Configurations` →
/// `/Instances` with `/Subtype /3D` → `/Asset` file specification →
/// `/EF` stream. Each asset stream is listed once per annotation.
fn rich_media<G: ObjectGraph + ?Sized>(
    graph: &G,
    annot: &Dict,
    base: &ThreeDArtwork,
    out: &mut Vec<ThreeDArtwork>,
    notes: &mut ThreeDNotes,
) {
    let Some(content) = annot
        .get(b"RichMediaContent")
        .map(|o| graph.resolve(o))
        .and_then(Object::as_dict)
    else {
        return;
    };
    let mut seen_streams: HashSet<ObjId> = HashSet::new();
    let configs = array_of(graph, content, b"Configurations");
    if configs.len() > MAX_RICH_MEDIA_ENTRIES {
        notes.truncated = true;
    }
    for config in configs.iter().take(MAX_RICH_MEDIA_ENTRIES) {
        let Some(config) = graph.resolve(config).as_dict() else {
            continue;
        };
        let instances = array_of(graph, config, b"Instances");
        if instances.len() > MAX_RICH_MEDIA_ENTRIES {
            notes.truncated = true;
        }
        for instance in instances.iter().take(MAX_RICH_MEDIA_ENTRIES) {
            let Some(instance) = graph.resolve(instance).as_dict() else {
                continue;
            };
            if name_of(graph, instance, b"Subtype") != Some(b"3D") {
                continue;
            }
            let Some(asset_obj) = instance.get(b"Asset") else {
                continue;
            };
            let Some(filespec) = graph.resolve(asset_obj).as_dict() else {
                continue;
            };
            let Some((id, dict)) = filespec
                .get(b"EF")
                .map(|o| graph.resolve(o))
                .and_then(Object::as_dict)
                .and_then(|ef| ef.get(b"F").or_else(|| ef.get(b"UF")))
                .and_then(|o| stream_at(graph, o))
            else {
                continue;
            };
            if !seen_streams.insert(id) {
                continue;
            }
            let name = [b"UF".as_slice(), b"F"].iter().find_map(|k| {
                match graph.resolve(filespec.get(k)?) {
                    Object::String(bytes) => Some(decode_text_string(bytes).text),
                    _ => None,
                }
            });
            let declared = name_of(graph, dict, b"Subtype")
                .map(format_from_mime)
                .or_else(|| name.as_deref().and_then(format_from_extension));
            let mut art = base.clone();
            art.source = ThreeDSource::RichMediaAsset {
                name,
                filespec_id: asset_obj.as_reference(),
            };
            art.stream_id = Some(id);
            art.declared = declared;
            out.push(art);
        }
    }
}

fn array_of<'a, G: ObjectGraph + ?Sized>(graph: &'a G, d: &'a Dict, key: &[u8]) -> &'a [Object] {
    d.get(key)
        .map(|o| graph.resolve(o))
        .and_then(Object::as_array)
        .unwrap_or_default()
}

/// §13.7 names the 3D MIME types `model/u3d`, `model/prc` and
/// `model/step*` (`#2F`-escaped in a name, §7.3.5).
fn format_from_mime(mime: &[u8]) -> ThreeDFormat {
    let lower = mime.to_ascii_lowercase();
    match lower.as_slice() {
        b"model/u3d" => ThreeDFormat::U3d,
        b"model/prc" => ThreeDFormat::Prc,
        m if m.starts_with(b"model/step") => ThreeDFormat::Step,
        _ => ThreeDFormat::Other(mime.to_vec()),
    }
}

fn format_from_extension(name: &str) -> Option<ThreeDFormat> {
    let (_, ext) = name.rsplit_once('.')?;
    match ext.to_ascii_lowercase().as_str() {
        "u3d" => Some(ThreeDFormat::U3d),
        "prc" => Some(ThreeDFormat::Prc),
        "stp" | "step" => Some(ThreeDFormat::Step),
        _ => None,
    }
}

/// Decode `artwork`'s stream and identify its format from the bytes.
///
/// The view must be of the document `artwork` was listed from; an id is
/// meaningless in another.
///
/// # Errors
///
/// [`ThreeDError::NoStream`], [`ThreeDError::StreamUnresolvable`],
/// [`ThreeDError::SpanUnservable`], or [`ThreeDError::Decode`] (which
/// includes the decompression ceiling).
pub fn extract_3d(
    view: &DocumentView<'_>,
    artwork: &ThreeDArtwork,
) -> Result<Extracted3D, ThreeDError> {
    let id = artwork.stream_id.ok_or(ThreeDError::NoStream)?;
    let Object::Stream(stream) = view.resolved(id) else {
        return Err(ThreeDError::StreamUnresolvable(id));
    };
    let raw = view
        .slice(stream.data_span)
        .ok_or(ThreeDError::SpanUnservable(id))?;
    let data = filters::decode_stream(&stream.dict, raw)?;
    let sniffed = sniff_3d_format(&data);
    Ok(Extracted3D { data, sniffed })
}
