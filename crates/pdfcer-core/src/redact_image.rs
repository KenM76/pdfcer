//! # Redaction of raster images — destroy the covered samples (ISO 32000-1 §12.5.6.23)
//!
//! The image half of [`crate::redact`], split out because it is a different
//! kind of surgery from the glyph removal that module performs: glyphs are
//! removed from a content stream, image samples are removed from a sample
//! grid, and the two share nothing but the region geometry.
//!
//! ## The clause this enacts
//!
//! §12.5.6.23, `shall`-strength: *"If a portion of an image is contained in
//! a redaction region, **that portion of the image data shall be destroyed**;
//! clipping or image masks shall not be used to hide that data."* The
//! derived mechanics are the spec RAG's `iso32000__ref__redaction_removal.md`
//! §4, and this module is their enactment, item for item:
//!
//! 1. **Detect** — an image XObject painted by `Do`, or an inline image
//!    (`BI … ID … EI`), whose placement (the unit square × CTM, §8.9.4)
//!    intersects a region. The content interpreter in `redact` records every
//!    such placement as an [`ImageHit`].
//! 2. **Decode** — through [`crate::image_codec`], so every codec pdfcer can
//!    read (Flate/LZW/RunLength raw samples, DCT, CCITT, JBIG2, JPX) is an
//!    image pdfcer can redact. A codec pdfcer cannot decode is a placement it
//!    cannot destroy, and that is reported **by name**, per placement — never
//!    masked, never silently skipped (see "What happens to a placement pdfcer
//!    cannot destroy", below).
//! 3. **Clear the in-region samples** — the region is mapped back through
//!    the inverse placement matrix into image space, and every sample cell
//!    that cell-intersects it is overwritten. Over-coverage is the
//!    deliberate bias (a cell on the region's edge is cleared, not kept),
//!    the same bias the glyph surgery applies.
//! 4. **Re-encode** — always `FlateDecode` over the raw samples. A lossy
//!    codec is never re-run: the requirement is that the original in-region
//!    samples are gone, not that the survivors are re-compressed the way the
//!    producer compressed them, and Flate is lossless for the survivors.
//! 5. **Copy-on-write when shared** — one image XObject may be painted by
//!    several `Do`s, on this page or others. Clearing the shared object would
//!    destroy every placement, including ones the operator never marked. So a
//!    *partially* covered placement always receives its **own clone** with
//!    its own cleared cells, bound under a fresh resource name on the page
//!    that draws it; the original is then **tombstoned** (replaced in place
//!    by a 1×1 paper-sample image) if every use of it in the document was a
//!    marked placement, and left untouched — and disclosed — if some other,
//!    unmarked placement still needs it.
//! 6. **Inline images** are content-stream bytes, so their re-encode is a
//!    content edit: the `BI … EI` span is replaced with a Flate-encoded
//!    inline image over the cleared samples (or removed outright).
//!
//! ## A wholly covered placement is REMOVED, not cleared
//!
//! When a single region contains the whole placement, there is no partial
//! grid to clear: the `Do` (or the whole `BI … EI`) is deleted from the
//! content stream. That is a true removal — nothing paints the image any
//! more — and the object's fate follows rule 5: tombstoned if this was its
//! last use, left for its other placements otherwise. On a CAD sheet
//! *"redact this logo"* and *"redact this scanned signature"* are exactly
//! this shape, and it is the common case the pdfcer-gui request of 2026-09-03
//! asked for by name.
//!
//! ## Why a tombstone rather than a deletion
//!
//! The full rewrite `apply_redactions` forces re-emits every object the
//! dirty set does not name. A deleted object would leave dangling references
//! in every resource dictionary that still names it (a shared `/Resources`
//! between pages is common), which is legal (§7.3.10: an unresolvable
//! reference is `null`) but ugly and trips validators. A 1×1 paper-sample
//! image under the same object number resolves everywhere the original did,
//! carries none of its samples, and costs a few dozen bytes.
//!
//! ## What happens to a placement pdfcer cannot destroy
//!
//! A placement is undestroyable when its image cannot be decoded (a codec
//! feature pdfcer has not implemented, a corrupt codestream, a bit depth
//! Flate cannot carry, a colour model with no PDF mapping) or when the
//! XObject is not an indirect object and so has no object number to rewrite.
//! Every such case carries a reason string. The caller — `redact` — uses it
//! to **retain the mark**: the `/Redact` annotation whose region touches the
//! placement is left in the document, unapplied, with no text removed under
//! it and no overlay drawn over it, and the report names the placement and
//! the reason. The other marks on the page and in the document are applied
//! normally. That is the "refuse per REGION, not per document" the request
//! asked for, and it keeps the cardinal rule intact: a retained mark is
//! visibly not redacted, where a burnt box over live pixels would be the
//! exact false redaction §12.5.6.23 names.
//!
//! ## What a destroyed cell becomes: paper
//!
//! Overwritten cells take the colour space's **no-ink value** — all-ones
//! for a `DeviceGray`/`DeviceRGB`/`Cal*`/`ICCBased` (1 or 3) sample
//! (white), all-zeros for `DeviceCMYK`/`Separation`/`DeviceN`/`ICCBased`
//! (4) (no ink), all-ones for an `/ImageMask` (unpainted), and a `/Decode`
//! whose first pair is inverted flips the choice. (An `/Indexed` image gets
//! entry 0, which is whatever the palette says; there is no colour-space
//! answer for "paper" in a palette.) The point is consistency with Table
//! 192: a mark with no `/IC` leaves its region **transparent**, so the
//! destroyed part of an image must look like the page behind it, not like a
//! black block the operator did not ask for. When the mark does carry an
//! `/IC`, the burnt-in box covers the region anyway.
//!
//! ## Masks are destroyed with the image
//!
//! An `/SMask` (§11.6.5.3) is a second sample grid over the same placement,
//! and its alpha is a *shape* — a signature on a transparent background is
//! recognisable from its soft mask alone. So the soft mask's in-region cells
//! are overwritten too — to **transparent** (zero), so the region shows the
//! page behind, matching the paper rule above — and a stencil `/Mask`
//! stream (§8.9.6.4) likewise, to "masked out" (one). A colour-key `/Mask`
//! array is left alone: it names sample values, not positions, and carries
//! no shape.
//!
//! ## Rotated placements over-cover
//!
//! The region is axis-aligned in user space. Under a placement matrix that
//! is a multiple of 90° its image-space pre-image is still a rectangle and
//! the cleared cells are exact. Under any other rotation or a skew the
//! pre-image is a quadrilateral and this module clears its **bounding
//! rectangle** in image space — more than the region, never less. The
//! placement is counted ([`ImageOutcome::rotated_overcovered`]) and disclosed.
//!
//! ## Spec sources
//!
//! - `iso32000__s__12.5.6.23.md` — the destroy-not-mask clause.
//! - `iso32000__ref__redaction_removal.md` §4 — the derived mechanics above.
//! - `iso32000__s__8.9.md` / `8.9.7` — image dictionary, sample layout
//!   (§8.9.3: rows padded to a byte), the unit-square mapping (§8.9.4),
//!   inline image entries (Table 93) and the full-name/abbreviation rule.

use std::collections::BTreeMap;

use crate::content::{ContentStream, ContentTokenKind};
use crate::document::Document;
use crate::image_codec::{self, Codec, CodecColorModel, CodedImage};
use crate::image_import::row_bytes;
use crate::object::{Dict, Name, ObjId, Object, Stream};
use crate::page_tree::Page;
use crate::redact::{Mat, RegionBox, aabb};
use crate::span::ByteSpan;
use crate::view::DocumentView;

/// Form-recursion ceiling for the use census (ARCHITECTURE.md §10). Deeper
/// nesting is counted as unresolved, which biases the census toward
/// "shared" — the safe direction, since a shared image is cloned rather
/// than cleared in place.
const MAX_FORM_DEPTH: usize = 32;

/// One image placement the content interpreter found intersecting a
/// redaction region.
#[derive(Debug, Clone)]
pub(crate) struct ImageHit {
    /// The byte span of the painting operation in the decoded content
    /// buffer — the whole `name Do` operation, or `BI` through `EI`.
    pub span: (usize, usize),
    /// The CTM in force when the image was painted (unit square × `ctm` is
    /// the placement, §8.9.4).
    pub ctm: Mat,
    /// Where the samples live.
    pub source: ImageSource,
}

/// Where an intersecting image's samples come from.
#[derive(Debug, Clone)]
pub(crate) enum ImageSource {
    /// A `/XObject` resource painted by `Do`, resolved to its object.
    XObject {
        /// The resource name the `Do` used.
        name: Vec<u8>,
        /// The image object, when the resource entry was an indirect
        /// reference (§7.3.8.1 requires streams to be indirect; `None` is the
        /// malformed direct case, which cannot be rewritten by number).
        id: Option<ObjId>,
    },
    /// An inline image; `params` is the normalized `BI` dictionary and
    /// `data` the still-encoded bytes between `ID` and `EI`.
    Inline {
        /// Normalized parameter dictionary (full key names).
        params: Dict,
        /// Span of the encoded sample bytes in the decoded content buffer.
        data: ByteSpan,
    },
}

impl ImageHit {
    /// The placement's bounding box in user space, for the report.
    pub(crate) fn bbox(&self) -> (f64, f64, f64, f64) {
        placement_bbox(self.ctm)
    }
}

/// The AABB of the unit square under `ctm`.
fn placement_bbox(ctm: Mat) -> (f64, f64, f64, f64) {
    aabb(&[
        ctm.apply(0.0, 0.0),
        ctm.apply(1.0, 0.0),
        ctm.apply(0.0, 1.0),
        ctm.apply(1.0, 1.0),
    ])
}

/// Is the whole placement inside ONE region? (The union of several regions
/// is not tested — a placement covered only by the union is cleared cell
/// by cell instead, which reaches the same samples.)
pub(crate) fn wholly_covered(ctm: Mat, regions: &[RegionBox]) -> bool {
    const EPS: f64 = 1e-6;
    let (min_x, min_y, max_x, max_y) = placement_bbox(ctm);
    regions.iter().any(|r| {
        r.min_x - EPS <= min_x
            && max_x <= r.max_x + EPS
            && r.min_y - EPS <= min_y
            && max_y <= r.max_y + EPS
    })
}

/// A rectangle of sample cells `[col0, col1) × [row0, row1)`, row 0 at the
/// top of the image (§8.9.4: image space's origin is the top-left sample).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cells {
    col0: u32,
    col1: u32,
    row0: u32,
    row1: u32,
}

impl Cells {
    fn is_empty(self) -> bool {
        self.col0 >= self.col1 || self.row0 >= self.row1
    }

    fn all(width: u32, height: u32) -> Self {
        Self {
            col0: 0,
            col1: width,
            row0: 0,
            row1: height,
        }
    }
}

/// The cells of a `width × height` grid placed by `ctm` that intersect
/// `region`, or `None` when none do.
///
/// The region's corners are mapped back through the inverse of `ctm` into
/// the unit square, their bounding box is taken (exact for axis-aligned and
/// 90°-rotated placements, an over-cover otherwise — see the module docs),
/// and it is snapped OUTWARD to cell boundaries: `floor` on the near edge,
/// `ceil` on the far edge, so a cell the region touches at all is cleared.
///
/// A singular `ctm` (zero-area placement) returns every cell: the image is
/// invisible, so nothing is lost by clearing it, and a matrix pdfcer cannot
/// invert is not a reason to leave samples behind.
fn covered_cells(ctm: Mat, region: RegionBox, width: u32, height: u32) -> Option<Cells> {
    let det = ctm.a * ctm.d - ctm.b * ctm.c;
    if det.abs() < 1e-12 || !det.is_finite() {
        return Some(Cells::all(width, height));
    }
    let inv = Mat {
        a: ctm.d / det,
        b: -ctm.b / det,
        c: -ctm.c / det,
        d: ctm.a / det,
        e: (ctm.c * ctm.f - ctm.d * ctm.e) / det,
        f: (ctm.b * ctm.e - ctm.a * ctm.f) / det,
    };
    let corners = [
        inv.apply(region.min_x, region.min_y),
        inv.apply(region.max_x, region.min_y),
        inv.apply(region.min_x, region.max_y),
        inv.apply(region.max_x, region.max_y),
    ];
    let (u0, v0, u1, v1) = aabb(&corners);
    // Clip to the unit square.
    let u0 = u0.max(0.0);
    let u1 = u1.min(1.0);
    let v0 = v0.max(0.0);
    let v1 = v1.min(1.0);
    if u0 >= u1 || v0 >= v1 {
        return None;
    }
    let w = f64::from(width);
    let h = f64::from(height);
    // Rows count from the TOP: v = 1 is row 0.
    let to_u32 = |x: f64| -> u32 { u32::try_from(x.max(0.0) as i64).unwrap_or(u32::MAX) };
    let cells = Cells {
        col0: to_u32((u0 * w).floor()).min(width),
        col1: to_u32((u1 * w).ceil()).min(width),
        row0: to_u32(((1.0 - v1) * h).floor()).min(height),
        row1: to_u32(((1.0 - v0) * h).ceil()).min(height),
    };
    if cells.is_empty() { None } else { Some(cells) }
}

/// Whether `ctm` is anything other than an axis-aligned or 90°-rotated
/// placement — the case where [`covered_cells`] over-covers.
fn is_skewed(ctm: Mat) -> bool {
    let axis = ctm.b.abs() < 1e-9 && ctm.c.abs() < 1e-9;
    let quarter = ctm.a.abs() < 1e-9 && ctm.d.abs() < 1e-9;
    !(axis || quarter)
}

/// Overwrite every component of every sample in `cells` with all-zero
/// (`set == false`) or all-one (`set == true`) bits.
///
/// `samples` is the §8.9.3 layout: `bpc`-bit samples, `components` per
/// pixel, rows padded to a byte. Bit depths 1, 2, 4, 8 and 16 are handled;
/// the caller has already refused anything else.
fn clear_cells(samples: &mut [u8], width: u32, components: u32, bpc: u32, cells: Cells, set: bool) {
    let stride = row_bytes(width, components, bpc);
    let fill: u8 = if set { 0xFF } else { 0x00 };
    for row in cells.row0..cells.row1 {
        let row_start = (row as usize).saturating_mul(stride);
        match bpc {
            8 | 16 => {
                let bytes_per_px = (components * bpc / 8) as usize;
                let from = row_start + cells.col0 as usize * bytes_per_px;
                let to = row_start + cells.col1 as usize * bytes_per_px;
                if let Some(slice) = samples.get_mut(from..to.min(samples.len())) {
                    slice.fill(fill);
                }
            }
            _ => {
                // Sub-byte samples: walk bit by bit. Rare (bilevel scans are
                // the one common case, and those are 1 component) so the
                // per-bit cost is acceptable.
                let first_bit = u64::from(cells.col0) * u64::from(components) * u64::from(bpc);
                let end_bit = u64::from(cells.col1) * u64::from(components) * u64::from(bpc);
                let mut bit = first_bit;
                while bit < end_bit {
                    let byte = row_start + (bit / 8) as usize;
                    let Some(b) = samples.get_mut(byte) else {
                        break;
                    };
                    let mask = 0x80u8 >> (bit % 8);
                    if set {
                        *b |= mask;
                    } else {
                        *b &= !mask;
                    }
                    bit += 1;
                }
            }
        }
    }
}

/// A decoded image in the shape the re-encoder needs: raw §8.9.3 samples
/// plus the geometry that describes them, already reconciled between the
/// dictionary and the codestream.
#[derive(Debug, Clone)]
struct Decoded {
    samples: Vec<u8>,
    width: u32,
    height: u32,
    components: u32,
    bpc: u32,
    /// Which codec produced the samples (drives the dictionary rewrite).
    codec: Option<Codec>,
    color_model: CodecColorModel,
    icc_profile: Option<Vec<u8>>,
    embedded_alpha: Option<Vec<u8>>,
    /// The dictionary's colour space disagreed with the codestream's
    /// component count; the rewrite substitutes a device space.
    colorspace_substituted: bool,
    /// JPX `/SMaskInData 2`: samples are preblended; the alpha is not
    /// recoverable, so the rewrite drops the transparency.
    preblended_alpha_dropped: bool,
    /// The bit value a destroyed cell takes: `true` = all ones. The colour
    /// space's no-ink ("paper") sample, `/Decode`-aware (module docs).
    paper: bool,
}

/// Why a placement cannot be destroyed. A plain string because every
/// reason ends up in one report note and nothing branches on it.
type Blocker = String;

/// Decode an image stream into [`Decoded`], or say why it cannot be.
fn decode(
    view: &DocumentView<'_>,
    dict: &Dict,
    raw: &[u8],
    inline: bool,
    resources: &Dict,
) -> Result<Decoded, Blocker> {
    let coded: CodedImage = image_codec::decode_image_view(view, dict, raw, inline)
        .map_err(|e| format!("its samples could not be decoded ({e})"))?;
    let is_mask = dict
        .get(b"ImageMask")
        .map(|o| view.graph().resolve(o))
        .and_then(|o| match o {
            Object::Boolean(b) => Some(*b),
            _ => None,
        })
        .unwrap_or(false);
    let declared = if is_mask {
        Some(1)
    } else {
        colorspace_components(view, dict, resources)
    };
    let paper = paper_is_ones(view, dict, is_mask, resources);
    let (components, colorspace_substituted) = match (coded.components, declared) {
        (0, Some(n)) => (n, false),
        (n, Some(m)) if u32::from(n) == m => (m, false),
        (n, Some(_)) if n > 0 => (u32::from(n), true),
        (n, None) if n > 0 => (u32::from(n), false),
        _ => {
            return Err("its colour space's component count could not be determined".to_string());
        }
    };
    let bpc = if coded.codec.is_some() && coded.bits_per_component > 0 {
        u32::from(coded.bits_per_component)
    } else {
        dict.get(b"BitsPerComponent")
            .map(|o| view.graph().resolve(o))
            .and_then(Object::as_int)
            .and_then(|v| u32::try_from(v).ok())
            .unwrap_or(if is_mask { 1 } else { 0 })
    };
    if !matches!(bpc, 1 | 2 | 4 | 8 | 16) {
        return Err(format!(
            "its bit depth ({bpc}) is not one FlateDecode can carry (1, 2, 4, 8 or 16)"
        ));
    }
    if matches!(coded.color_model, CodecColorModel::Unknown { .. }) {
        return Err("its codestream declares a component count with no PDF colour mapping".into());
    }
    let (width, height) = if coded.codec.is_some() && coded.width > 0 && coded.height > 0 {
        (coded.width, coded.height)
    } else {
        let int = |k: &[u8]| {
            dict.get(k)
                .map(|o| view.graph().resolve(o))
                .and_then(Object::as_int)
                .and_then(|v| u32::try_from(v).ok())
                .unwrap_or(0)
        };
        (int(b"Width"), int(b"Height"))
    };
    if width == 0 || height == 0 {
        return Err("its dimensions are zero".into());
    }
    let expected = row_bytes(width, components, bpc).saturating_mul(height as usize);
    let mut samples = coded.samples;
    // Short data is padded with zeros (which is what a viewer paints for the
    // missing tail, and is already "cleared"); long data is truncated. Both
    // only ever remove information.
    samples.resize(expected, 0);
    Ok(Decoded {
        samples,
        width,
        height,
        components,
        bpc,
        codec: coded.codec,
        color_model: coded.color_model,
        icc_profile: coded.icc_profile,
        embedded_alpha: coded.embedded_alpha,
        colorspace_substituted,
        preblended_alpha_dropped: coded.notes.jpx_smask_in_data_preblended,
        paper,
    })
}

/// Whether "paper" for this image is the all-ones sample (module docs).
///
/// Subtractive spaces (CMYK, Separation, DeviceN, a 4-component ICC) have
/// no ink at zero; additive and grey spaces are white at their maximum; an
/// image mask is unpainted at one. A `/Decode` array whose first pair is
/// `[1 0]` inverts the sample's meaning, so it inverts the answer.
fn paper_is_ones(view: &DocumentView<'_>, dict: &Dict, is_mask: bool, resources: &Dict) -> bool {
    let g = view.graph();
    let inverted = dict
        .get(b"Decode")
        .map(|o| g.resolve(o))
        .and_then(Object::as_array)
        .and_then(|a| {
            let d0 = g.resolve(a.first()?).as_number()?;
            let d1 = g.resolve(a.get(1)?).as_number()?;
            Some(d0 > d1)
        })
        .unwrap_or(false);
    let ones = if is_mask {
        true
    } else {
        !space_is_subtractive(
            view,
            dict.get(b"ColorSpace").unwrap_or(&Object::Null),
            resources,
            0,
        )
    };
    ones != inverted
}

/// Is a colour space one whose zero sample means "no ink"? (`Indexed` is
/// answered `true` so entry 0 is chosen — see the module docs.)
fn space_is_subtractive(
    view: &DocumentView<'_>,
    cs: &Object,
    resources: &Dict,
    depth: usize,
) -> bool {
    if depth > 4 {
        return false;
    }
    let g = view.graph();
    match g.resolve(cs) {
        Object::Name(name) => match name.as_bytes() {
            b"DeviceCMYK" | b"CMYK" => true,
            b"DeviceGray" | b"G" | b"CalGray" | b"DeviceRGB" | b"RGB" | b"CalRGB" | b"Pattern" => {
                false
            }
            other => g
                .resolve(resources.get(b"ColorSpace").unwrap_or(&Object::Null))
                .as_dict()
                .and_then(|d| d.get(other))
                .is_some_and(|named| space_is_subtractive(view, named, resources, depth + 1)),
        },
        Object::Array(items) => {
            let family = items
                .first()
                .map(|o| g.resolve(o))
                .and_then(Object::as_name)
                .map(|n| n.as_bytes().to_vec())
                .unwrap_or_default();
            match family.as_slice() {
                b"DeviceCMYK" | b"CMYK" | b"Separation" | b"DeviceN" | b"Indexed" | b"I" => true,
                b"ICCBased" => {
                    let n = match items.get(1).map(|o| g.resolve(o)) {
                        Some(Object::Stream(st)) => st.dict.get(b"N"),
                        Some(Object::Dict(d)) => d.get(b"N"),
                        _ => None,
                    };
                    n.map(|o| g.resolve(o)).and_then(Object::as_int) == Some(4)
                }
                _ => false,
            }
        }
        _ => false,
    }
}

/// The component count implied by an image dictionary's `/ColorSpace`,
/// resolved through the page's `/ColorSpace` resources for a named space.
fn colorspace_components(view: &DocumentView<'_>, dict: &Dict, resources: &Dict) -> Option<u32> {
    let cs = dict.get(b"ColorSpace")?;
    space_components(view, cs, resources, 0)
}

fn space_components(
    view: &DocumentView<'_>,
    cs: &Object,
    resources: &Dict,
    depth: usize,
) -> Option<u32> {
    if depth > 4 {
        return None;
    }
    let g = view.graph();
    match g.resolve(cs) {
        Object::Name(name) => match name.as_bytes() {
            b"DeviceGray" | b"G" | b"CalGray" => Some(1),
            b"DeviceRGB" | b"RGB" | b"CalRGB" => Some(3),
            b"DeviceCMYK" | b"CMYK" => Some(4),
            b"Pattern" => None,
            other => {
                let named = g
                    .resolve(resources.get(b"ColorSpace").unwrap_or(&Object::Null))
                    .as_dict()?
                    .get(other)?;
                space_components(view, named, resources, depth + 1)
            }
        },
        Object::Array(items) => {
            let family = g.resolve(items.first()?).as_name()?.as_bytes().to_vec();
            match family.as_slice() {
                b"DeviceGray" | b"G" | b"CalGray" | b"Indexed" | b"I" | b"Separation" => Some(1),
                b"DeviceRGB" | b"RGB" | b"CalRGB" | b"Lab" => Some(3),
                b"DeviceCMYK" | b"CMYK" => Some(4),
                b"ICCBased" => {
                    let stream = g.resolve(items.get(1)?);
                    let n = match stream {
                        Object::Stream(s) => s.dict.get(b"N"),
                        Object::Dict(d) => d.get(b"N"),
                        _ => None,
                    }?;
                    g.resolve(n).as_int().and_then(|v| u32::try_from(v).ok())
                }
                b"DeviceN" => {
                    let names = g.resolve(items.get(1)?).as_array()?;
                    u32::try_from(names.len()).ok()
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// The device colour-space name for a codec's own colour model, used when a
/// JPX dictionary carries no `/ColorSpace` (Table 89: the codestream wins)
/// or when the dictionary's space disagreed with the codestream.
fn device_space_for(model: CodecColorModel, components: u32) -> Option<&'static [u8]> {
    match (model, components) {
        (CodecColorModel::Gray | CodecColorModel::Bilevel, _) | (_, 1) => Some(b"DeviceGray"),
        (CodecColorModel::Rgb | CodecColorModel::Untransformed3, _) | (_, 3) => Some(b"DeviceRGB"),
        (CodecColorModel::Cmyk, _) | (_, 4) => Some(b"DeviceCMYK"),
        _ => None,
    }
}

/// Object numbers and the staging buffer new streams are written into.
///
/// Mirrors `redact`'s own allocation: a created stream's bytes go into the
/// staging buffer past the base file and its span is expressed in the
/// combined coordinate system the writer resolves (R45).
pub(crate) struct Allocator<'a> {
    pub staging: &'a mut Vec<u8>,
    pub base_len: usize,
    pub next_num: &'a mut u32,
}

impl Allocator<'_> {
    fn alloc(&mut self) -> ObjId {
        let n = *self.next_num;
        *self.next_num = self.next_num.saturating_add(1);
        ObjId::new(n, 0)
    }

    fn stage(&mut self, bytes: &[u8]) -> ByteSpan {
        let start = self.base_len + self.staging.len();
        self.staging.extend_from_slice(bytes);
        ByteSpan::new(start, bytes.len())
    }

    /// A `FlateDecode` stream object over `data` with `dict`'s entries.
    fn flate_stream(&mut self, mut dict: Dict, data: &[u8]) -> Object {
        let encoded = crate::filters::flate::encode(data);
        let span = self.stage(&encoded);
        dict.insert(
            Name::from(b"Length"),
            Object::Integer(i64::try_from(encoded.len()).unwrap_or(i64::MAX)),
        );
        dict.insert(
            Name::from(b"Filter"),
            Object::Name(Name::from(b"FlateDecode")),
        );
        Object::Stream(Stream {
            dict,
            data_span: span,
        })
    }
}

/// What the image surgery produced for one page, to be merged by `redact`.
#[derive(Debug, Default)]
pub(crate) struct ImageOutcome {
    /// Content edits: `(start, end, replacement)` in the decoded buffer.
    pub edits: Vec<(usize, usize, Vec<u8>)>,
    /// Objects to write: fresh clones, tombstones, masks, ICC streams.
    pub objects: Vec<(ObjId, Object)>,
    /// `/XObject` resource bindings the page must gain (fresh names).
    pub bindings: Vec<(Vec<u8>, ObjId)>,
    /// Placements whose in-region samples were destroyed and re-encoded.
    pub cleared: u64,
    /// Placements removed from the page outright (wholly covered).
    pub removed: u64,
    /// Placements that received a copy-on-write clone because the original
    /// is still painted elsewhere.
    pub cloned_shared: u64,
    /// Placements whose skewed matrix made the cleared cells an over-cover.
    pub rotated_overcovered: u64,
    /// Human-readable disclosures, one per placement.
    pub notes: Vec<String>,
}

/// A census of how many times each image XObject is painted anywhere in
/// the document — page content, form XObjects (recursively) and annotation
/// appearance streams.
///
/// The count is what decides copy-on-write versus tombstone (module docs,
/// rule 5). A use the census cannot see (an undecodable form, nesting past
/// [`MAX_FORM_DEPTH`]) is recorded as an extra use of *every* image that
/// form's resources name, and every tiling pattern a page's resources name
/// is walked as if painted, so the misses bias toward "shared", which is
/// the safe direction: a needless clone costs bytes, a needless tombstone
/// blanks an unmarked placement.
pub(crate) fn image_use_census(doc: &Document, pages: &[Page]) -> BTreeMap<ObjId, usize> {
    let view = doc.view();
    let mut uses: BTreeMap<ObjId, usize> = BTreeMap::new();
    for page in pages {
        if let Ok(stream) = ContentStream::from_page(&view, page) {
            let mut active = Vec::new();
            count_uses(
                doc,
                &view,
                &stream,
                &page.resources,
                &page.resources,
                0,
                &mut active,
                &mut uses,
            );
        } else {
            count_all_named(doc, &page.resources, &mut uses);
        }
        // Tiling patterns (§8.7.3) are content streams with their own
        // resources, painted by `scn`/`SCN` rather than `Do`. Every pattern
        // the page's resources name is walked as if painted once — an
        // over-count, which is the safe direction (see the doc comment).
        if let Some(patterns) = page
            .resources
            .get(b"Pattern")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
        {
            for (_n, v) in patterns.iter() {
                if let Object::Reference(id) = v {
                    let mut active = Vec::new();
                    walk_form(
                        doc,
                        &view,
                        *id,
                        &page.resources,
                        &page.resources,
                        0,
                        &mut active,
                        &mut uses,
                    );
                }
            }
        }
        // Annotation appearance streams are forms with their own resources.
        let Some(annots) = doc
            .get(page.id)
            .and_then(|io| io.value.as_dict())
            .and_then(|d| d.get(b"Annots"))
            .map(|o| doc.resolve(o))
            .and_then(Object::as_array)
        else {
            continue;
        };
        for entry in annots {
            let Some(dict) = doc.resolve(entry).as_dict() else {
                continue;
            };
            let Some(ap) = dict
                .get(b"AP")
                .map(|o| doc.resolve(o))
                .and_then(Object::as_dict)
            else {
                continue;
            };
            let mut streams: Vec<ObjId> = Vec::new();
            for (_k, v) in ap.iter() {
                match v {
                    Object::Reference(id) => streams.push(*id),
                    Object::Dict(states) => {
                        for (_s, sv) in states.iter() {
                            if let Object::Reference(id) = sv {
                                streams.push(*id);
                            }
                        }
                    }
                    _ => {}
                }
            }
            for id in streams {
                let mut active = Vec::new();
                walk_form(
                    doc,
                    &view,
                    id,
                    &page.resources,
                    &page.resources,
                    0,
                    &mut active,
                    &mut uses,
                );
            }
        }
    }
    uses
}

/// Count every image the `/XObject` resources of `resources` name, once
/// each — the fallback when a content stream cannot be read.
fn count_all_named(doc: &Document, resources: &Dict, uses: &mut BTreeMap<ObjId, usize>) {
    let Some(xobjects) = resources
        .get(b"XObject")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict)
    else {
        return;
    };
    for (_n, v) in xobjects.iter() {
        if let Object::Reference(id) = v {
            *uses.entry(*id).or_insert(0) += 1;
        }
    }
}

/// Recurse into a form XObject by id.
#[allow(clippy::too_many_arguments)]
fn walk_form(
    doc: &Document,
    view: &DocumentView<'_>,
    id: ObjId,
    page_resources: &Dict,
    enclosing: &Dict,
    depth: usize,
    active: &mut Vec<u32>,
    uses: &mut BTreeMap<ObjId, usize>,
) {
    let Some(Object::Stream(form)) = view.graph().value(id) else {
        return;
    };
    let own = doc
        .resolve(form.dict.get(b"Resources").unwrap_or(&Object::Null))
        .as_dict()
        .filter(|d| !d.is_empty())
        .cloned();
    let resources = own.unwrap_or_else(|| {
        if page_resources.is_empty() {
            enclosing.clone()
        } else {
            page_resources.clone()
        }
    });
    if depth >= MAX_FORM_DEPTH || active.contains(&id.num) {
        count_all_named(doc, &resources, uses);
        return;
    }
    let inner = view
        .slice(form.data_span)
        .and_then(|raw| crate::filters::decode_stream(&form.dict, raw).ok())
        .and_then(|decoded| ContentStream::parse(decoded).ok());
    let Some(inner) = inner else {
        count_all_named(doc, &resources, uses);
        return;
    };
    active.push(id.num);
    count_uses(
        doc,
        view,
        &inner,
        page_resources,
        &resources,
        depth + 1,
        active,
        uses,
    );
    active.pop();
}

/// Count the `Do` uses in one content stream, recursing into forms.
#[allow(clippy::too_many_arguments)]
fn count_uses(
    doc: &Document,
    view: &DocumentView<'_>,
    stream: &ContentStream,
    page_resources: &Dict,
    enclosing: &Dict,
    depth: usize,
    active: &mut Vec<u32>,
    uses: &mut BTreeMap<ObjId, usize>,
) {
    for op in stream.operations() {
        if op.operator_name(&stream.buf) != Some(b"Do") {
            continue;
        }
        let Some(name) = op
            .operands
            .first()
            .and_then(|t| match &t.kind {
                ContentTokenKind::Operand(o) => o.as_name(),
                _ => None,
            })
            .map(|n| n.as_bytes().to_vec())
        else {
            continue;
        };
        let Some(entry) = doc
            .resolve(enclosing.get(b"XObject").unwrap_or(&Object::Null))
            .as_dict()
            .and_then(|d| d.get(&name))
        else {
            continue;
        };
        let Some(id) = entry.as_reference() else {
            continue;
        };
        let Some(Object::Stream(xobj)) = view.graph().value(id) else {
            continue;
        };
        let subtype = doc
            .resolve(xobj.dict.get(b"Subtype").unwrap_or(&Object::Null))
            .as_name()
            .map(|n| n.as_bytes().to_vec())
            .unwrap_or_default();
        match subtype.as_slice() {
            b"Image" => *uses.entry(id).or_insert(0) += 1,
            b"Form" => walk_form(
                doc,
                view,
                id,
                page_resources,
                enclosing,
                depth,
                active,
                uses,
            ),
            _ => {}
        }
    }
}

/// Decoded images by object id, so an image painted twice is decoded once.
#[derive(Default)]
pub(crate) struct DecodeCache {
    entries: BTreeMap<ObjId, Result<Decoded, Blocker>>,
}

impl DecodeCache {
    fn get(
        &mut self,
        doc: &Document,
        view: &DocumentView<'_>,
        id: ObjId,
        resources: &Dict,
    ) -> Result<Decoded, Blocker> {
        if let Some(hit) = self.entries.get(&id) {
            return hit.clone();
        }
        let result = match view.graph().value(id) {
            Some(Object::Stream(stream)) => match view.slice(stream.data_span) {
                Some(raw) => decode(view, &stream.dict, raw, false, resources),
                None => Err("its stream bytes are outside the file".to_string()),
            },
            _ => Err("its object is not a stream".to_string()),
        };
        let _ = doc;
        self.entries.insert(id, result.clone());
        result
    }
}

/// The reason `hit` cannot be destroyed, or `None` when it can.
///
/// Called for every hit BEFORE any surgery, so the caller can retain the
/// marks that touch an undestroyable placement and apply the rest.
pub(crate) fn blocker(
    doc: &Document,
    view: &DocumentView<'_>,
    resources: &Dict,
    content: &[u8],
    hit: &ImageHit,
    cache: &mut DecodeCache,
) -> Option<Blocker> {
    match &hit.source {
        ImageSource::XObject { id: None, .. } => {
            Some("its XObject is a direct object with no object number to rewrite".into())
        }
        ImageSource::XObject { id: Some(id), .. } => {
            if let Err(why) = cache.get(doc, view, *id, resources) {
                return Some(why);
            }
            // The masks must be destroyable too, or the shape survives.
            let dict = match view.graph().value(*id) {
                Some(Object::Stream(s)) => s.dict.clone(),
                _ => return Some("its object is not a stream".into()),
            };
            for key in [&b"SMask"[..], &b"Mask"[..]] {
                if let Some(Object::Reference(mid)) = dict.get(key)
                    && let Err(why) = cache.get(doc, view, *mid, resources)
                {
                    return Some(format!(
                        "its /{} could not be decoded: {why}",
                        String::from_utf8_lossy(key)
                    ));
                }
            }
            None
        }
        ImageSource::Inline { params, data } => {
            let raw = data.slice(content)?;
            decode(view, params, raw, true, resources).err()
        }
    }
}

/// Plan and produce the image surgery for one page.
///
/// `regions` are the SURVIVING regions (marks retained because of a blocker
/// have already been removed by the caller). `uses` is the document-wide
/// census and `covered` the number of marked placements per image id across
/// the whole document, so the tombstone decision sees every page.
#[allow(clippy::too_many_arguments)]
pub(crate) fn plan_page(
    doc: &Document,
    view: &DocumentView<'_>,
    page_number: usize,
    resources: &Dict,
    content: &[u8],
    hits: &[ImageHit],
    regions: &[RegionBox],
    uses: &BTreeMap<ObjId, usize>,
    covered: &BTreeMap<ObjId, usize>,
    tombstoned: &mut BTreeMap<ObjId, ()>,
    cache: &mut DecodeCache,
    alloc: &mut Allocator<'_>,
    out: &mut ImageOutcome,
) {
    let mut fresh_names = 0u32;
    for hit in hits {
        let (bx0, by0, bx1, by1) = hit.bbox();
        let placement = format!(
            "page {page_number} image at ({bx0:.1}, {by0:.1}) {:.1}×{:.1} pt",
            bx1 - bx0,
            by1 - by0
        );
        let whole = wholly_covered(hit.ctm, regions);
        match &hit.source {
            ImageSource::Inline { params, data } => {
                if whole {
                    out.edits.push((hit.span.0, hit.span.1, Vec::new()));
                    out.removed += 1;
                    out.notes.push(format!(
                        "redaction: {placement} (inline) was REMOVED ENTIRELY — the region contained the whole image"
                    ));
                    continue;
                }
                let Some(raw) = data.slice(content) else {
                    continue;
                };
                let Ok(mut decoded) = decode(view, params, raw, true, resources) else {
                    continue; // the blocker pass already retained this mark
                };
                let Some(cells) = clear_regions(&mut decoded, hit.ctm, regions, None) else {
                    continue; // touches the bounding box, covers no cell
                };
                if is_skewed(hit.ctm) {
                    out.rotated_overcovered += 1;
                }
                let bytes = inline_bytes(params, &decoded);
                out.edits.push((hit.span.0, hit.span.1, bytes));
                out.cleared += 1;
                out.notes.push(format!(
                    "redaction: {placement} (inline) had {} sample cell(s) destroyed and was re-encoded",
                    cells
                ));
            }
            ImageSource::XObject { name, id: Some(id) } => {
                let id = *id;
                let total_uses = uses.get(&id).copied().unwrap_or(1).max(1);
                let marked = covered.get(&id).copied().unwrap_or(1);
                let exclusive = marked >= total_uses;
                let Some(Object::Stream(original)) = view.graph().value(id) else {
                    continue;
                };
                let original_dict = original.dict.clone();
                if whole {
                    out.edits.push((hit.span.0, hit.span.1, Vec::new()));
                    out.removed += 1;
                    if exclusive {
                        tombstone(
                            doc,
                            view,
                            id,
                            &original_dict,
                            resources,
                            cache,
                            alloc,
                            out,
                            tombstoned,
                        );
                        out.notes.push(format!(
                            "redaction: {placement} (/{}) was REMOVED ENTIRELY — the region contained the whole image; its object {} 0 R now holds a 1×1 blank",
                            String::from_utf8_lossy(name),
                            id.num
                        ));
                    } else {
                        out.notes.push(format!(
                            "redaction: {placement} (/{}) was REMOVED from this page; its object {} 0 R is SHARED with {} other placement(s) that were not marked, and its samples survive there",
                            String::from_utf8_lossy(name),
                            id.num,
                            total_uses - marked
                        ));
                    }
                    continue;
                }
                // Partial: a clone with this placement's cells cleared.
                let Ok(mut decoded) = cache.get(doc, view, id, resources) else {
                    continue;
                };
                let Some(cells) = clear_regions(&mut decoded, hit.ctm, regions, None) else {
                    continue;
                };
                if is_skewed(hit.ctm) {
                    out.rotated_overcovered += 1;
                }
                let mut dict = rewrite_dict(&original_dict, &decoded, resources, alloc, out);
                // Masks travel with the clone, cleared over the same placement:
                // the soft mask to transparent, the stencil to masked-out.
                for (key, set) in [(&b"SMask"[..], false), (&b"Mask"[..], true)] {
                    let Some(Object::Reference(mid)) = original_dict.get(key) else {
                        continue;
                    };
                    let Ok(mut mask) = cache.get(doc, view, *mid, resources) else {
                        continue;
                    };
                    let Some(Object::Stream(mask_stream)) = view.graph().value(*mid) else {
                        continue;
                    };
                    clear_regions(&mut mask, hit.ctm, regions, Some(set));
                    let mask_dict =
                        rewrite_dict(&mask_stream.dict.clone(), &mask, resources, alloc, out);
                    let mask_obj = alloc.flate_stream(mask_dict, &mask.samples);
                    let mask_id = alloc.alloc();
                    out.objects.push((mask_id, mask_obj));
                    dict.insert(Name::from(key), Object::Reference(mask_id));
                }
                let clone_id = alloc.alloc();
                let clone = alloc.flate_stream(dict, &decoded.samples);
                out.objects.push((clone_id, clone));
                fresh_names += 1;
                let fresh = format!("pdfceRd{}_{}", id.num, fresh_names).into_bytes();
                let mut replacement = Vec::with_capacity(fresh.len() + 5);
                replacement.push(b'/');
                replacement.extend_from_slice(&fresh);
                replacement.extend_from_slice(b" Do");
                out.edits.push((hit.span.0, hit.span.1, replacement));
                out.bindings.push((fresh, clone_id));
                out.cleared += 1;
                if exclusive {
                    tombstone(
                        doc,
                        view,
                        id,
                        &original_dict,
                        resources,
                        cache,
                        alloc,
                        out,
                        tombstoned,
                    );
                    out.notes.push(format!(
                        "redaction: {placement} (/{}) had {cells} sample cell(s) destroyed and was re-encoded as {} 0 R; the original {} 0 R now holds a 1×1 blank",
                        String::from_utf8_lossy(name),
                        clone_id.num,
                        id.num
                    ));
                } else {
                    out.cloned_shared += 1;
                    out.notes.push(format!(
                        "redaction: {placement} (/{}) had {cells} sample cell(s) destroyed in a COPY ({} 0 R); the original {} 0 R is SHARED with {} other placement(s) that were not marked, and its samples survive there",
                        String::from_utf8_lossy(name),
                        clone_id.num,
                        id.num,
                        total_uses - marked
                    ));
                }
            }
            ImageSource::XObject { id: None, .. } => {}
        }
    }
}

/// Clear every region's cells in `decoded` to `set` (`None` = the image's
/// own paper value); returns the number of cells cleared, or `None` when no
/// region covered a cell. Embedded JPX alpha over the same cells becomes
/// transparent.
fn clear_regions(
    decoded: &mut Decoded,
    ctm: Mat,
    regions: &[RegionBox],
    set: Option<bool>,
) -> Option<u64> {
    let set = set.unwrap_or(decoded.paper);
    let mut count = 0u64;
    for region in regions {
        let Some(cells) = covered_cells(ctm, *region, decoded.width, decoded.height) else {
            continue;
        };
        count += u64::from(cells.col1 - cells.col0) * u64::from(cells.row1 - cells.row0);
        clear_cells(
            &mut decoded.samples,
            decoded.width,
            decoded.components,
            decoded.bpc,
            cells,
            set,
        );
        if let Some(alpha) = decoded.embedded_alpha.as_mut() {
            clear_cells(alpha, decoded.width, 1, 8, cells, false);
        }
    }
    (count > 0).then_some(count)
}

/// Replace `id` in place with a 1×1 paper-sample image of the same colour
/// space, and do the same to its `/SMask` / stencil `/Mask`. Idempotent per
/// id (a second covered placement of the same image finds it done).
#[allow(clippy::too_many_arguments)]
fn tombstone(
    doc: &Document,
    view: &DocumentView<'_>,
    id: ObjId,
    original_dict: &Dict,
    resources: &Dict,
    cache: &mut DecodeCache,
    alloc: &mut Allocator<'_>,
    out: &mut ImageOutcome,
    tombstoned: &mut BTreeMap<ObjId, ()>,
) {
    if tombstoned.contains_key(&id) {
        return;
    }
    tombstoned.insert(id, ());
    let Ok(decoded) = cache.get(doc, view, id, resources) else {
        return;
    };
    let mut dict = rewrite_dict(original_dict, &decoded, resources, alloc, out);
    dict.insert(Name::from(b"Width"), Object::Integer(1));
    dict.insert(Name::from(b"Height"), Object::Integer(1));
    dict.remove(b"SMask");
    dict.remove(b"Mask");
    let fill = if decoded.paper { 0xFF } else { 0x00 };
    let one = vec![fill; row_bytes(1, decoded.components, decoded.bpc)];
    out.objects.push((id, alloc.flate_stream(dict, &one)));
    for key in [&b"SMask"[..], &b"Mask"[..]] {
        let Some(Object::Reference(mid)) = original_dict.get(key) else {
            continue;
        };
        if tombstoned.contains_key(mid) {
            continue;
        }
        tombstoned.insert(*mid, ());
        let Ok(mask) = cache.get(doc, view, *mid, resources) else {
            continue;
        };
        let Some(Object::Stream(mask_stream)) = view.graph().value(*mid) else {
            continue;
        };
        let mut mask_dict = rewrite_dict(&mask_stream.dict.clone(), &mask, resources, alloc, out);
        mask_dict.insert(Name::from(b"Width"), Object::Integer(1));
        mask_dict.insert(Name::from(b"Height"), Object::Integer(1));
        // A soft mask tombstone is transparent; a stencil's is masked out.
        let fill = if key == b"SMask" { 0x00 } else { 0xFF };
        let one = vec![fill; row_bytes(1, mask.components, mask.bpc)];
        out.objects
            .push((*mid, alloc.flate_stream(mask_dict, &one)));
    }
}

/// The image dictionary for a re-encoded copy of `original`: the filter
/// chain and its parameters gone (the caller adds `/FlateDecode`), geometry
/// from the decoded samples, and the codec-specific reconciliations the
/// module docs describe.
fn rewrite_dict(
    original: &Dict,
    decoded: &Decoded,
    resources: &Dict,
    alloc: &mut Allocator<'_>,
    out: &mut ImageOutcome,
) -> Dict {
    let mut dict = original.clone();
    for key in [
        &b"Filter"[..],
        b"F",
        b"DecodeParms",
        b"DP",
        b"Length",
        b"SMaskInData",
    ] {
        dict.remove(key);
    }
    dict.insert(
        Name::from(b"Width"),
        Object::Integer(i64::from(decoded.width)),
    );
    dict.insert(
        Name::from(b"Height"),
        Object::Integer(i64::from(decoded.height)),
    );
    let is_mask = matches!(dict.get(b"ImageMask"), Some(Object::Boolean(true)));
    if !is_mask {
        dict.insert(
            Name::from(b"BitsPerComponent"),
            Object::Integer(i64::from(decoded.bpc)),
        );
    }
    let needs_space =
        !is_mask && (dict.get(b"ColorSpace").is_none() || decoded.colorspace_substituted);
    if needs_space {
        if let (Some(icc), Some(Codec::Jpx)) = (&decoded.icc_profile, decoded.codec) {
            // The codestream's own profile becomes an /ICCBased space, so the
            // colour that Table 89 said came from the codestream still does.
            let mut icc_dict = Dict::new();
            icc_dict.insert(
                Name::from(b"N"),
                Object::Integer(i64::from(decoded.components)),
            );
            let icc_obj = alloc.flate_stream(icc_dict, icc);
            let icc_id = alloc.alloc();
            out.objects.push((icc_id, icc_obj));
            dict.insert(
                Name::from(b"ColorSpace"),
                Object::Array(vec![
                    Object::Name(Name::from(b"ICCBased")),
                    Object::Reference(icc_id),
                ]),
            );
        } else if let Some(space) = device_space_for(decoded.color_model, decoded.components) {
            dict.insert(Name::from(b"ColorSpace"), Object::Name(Name::from(space)));
        }
        if decoded.colorspace_substituted {
            out.notes.push(format!(
                "redaction: an image's /ColorSpace disagreed with its codestream's {} component(s); the re-encoded copy carries the matching device space",
                decoded.components
            ));
        }
    }
    // Table 89: /Decode is ignored for JPX unless /ImageMask. Under Flate it
    // would be applied, so it must not travel.
    if decoded.codec == Some(Codec::Jpx) && !is_mask {
        dict.remove(b"Decode");
    }
    if decoded.preblended_alpha_dropped {
        out.notes.push(
            "redaction: a JPX image carried preblended alpha (/SMaskInData 2), which the re-encoded copy cannot keep — it is drawn opaque".to_string(),
        );
    }
    if let Some(alpha) = &decoded.embedded_alpha {
        let mut sm = Dict::new();
        sm.insert(Name::from(b"Type"), Object::Name(Name::from(b"XObject")));
        sm.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Image")));
        sm.insert(
            Name::from(b"Width"),
            Object::Integer(i64::from(decoded.width)),
        );
        sm.insert(
            Name::from(b"Height"),
            Object::Integer(i64::from(decoded.height)),
        );
        sm.insert(
            Name::from(b"ColorSpace"),
            Object::Name(Name::from(b"DeviceGray")),
        );
        sm.insert(Name::from(b"BitsPerComponent"), Object::Integer(8));
        let sm_obj = alloc.flate_stream(sm, alpha);
        let sm_id = alloc.alloc();
        out.objects.push((sm_id, sm_obj));
        dict.insert(Name::from(b"SMask"), Object::Reference(sm_id));
    }
    let _ = resources;
    dict
}

/// The `BI … ID … EI` bytes for a re-encoded inline image over `decoded`.
///
/// Full key names are used (Table 93 lists the abbreviations as *permitted*
/// alternatives; the full names are the primary spelling) and the colour
/// space is carried over as the parser normalized it. `/Filter`,
/// `/DecodeParms` and any length entry are dropped and replaced by
/// `/FlateDecode`; `/Decode`, `/ImageMask`, `/Interpolate` and `/ColorSpace`
/// survive because they describe the samples, which are unchanged in
/// meaning.
fn inline_bytes(params: &Dict, decoded: &Decoded) -> Vec<u8> {
    use crate::writer::encoder::IdentityEncoder;
    use crate::writer::serialize::write_object;
    let encoded = crate::filters::flate::encode(&decoded.samples);
    let mut dict = params.clone();
    for key in [&b"Filter"[..], b"F", b"DecodeParms", b"DP", b"Length", b"L"] {
        dict.remove(key);
    }
    dict.insert(
        Name::from(b"Width"),
        Object::Integer(i64::from(decoded.width)),
    );
    dict.insert(
        Name::from(b"Height"),
        Object::Integer(i64::from(decoded.height)),
    );
    if !matches!(dict.get(b"ImageMask"), Some(Object::Boolean(true))) {
        dict.insert(
            Name::from(b"BitsPerComponent"),
            Object::Integer(i64::from(decoded.bpc)),
        );
    }
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"FlateDecode")),
    );
    let mut out = Vec::with_capacity(encoded.len() + 64);
    out.extend_from_slice(b"BI");
    for (k, v) in dict.iter() {
        out.push(b' ');
        out.push(b'/');
        out.extend_from_slice(k.as_bytes());
        out.push(b' ');
        // An inline image's dictionary is direct by construction, so the
        // owner id and source buffer are never consulted.
        write_object(&mut out, v, ObjId::new(0, 0), &[], &IdentityEncoder);
    }
    out.extend_from_slice(b" ID ");
    out.extend_from_slice(&encoded);
    out.extend_from_slice(b" EI");
    out
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn scale(w: f64, h: f64, x: f64, y: f64) -> Mat {
        Mat {
            a: w,
            b: 0.0,
            c: 0.0,
            d: h,
            e: x,
            f: y,
        }
    }

    fn region(x0: f64, y0: f64, x1: f64, y1: f64) -> RegionBox {
        RegionBox {
            min_x: x0,
            min_y: y0,
            max_x: x1,
            max_y: y1,
        }
    }

    #[test]
    fn covered_cells_maps_a_region_into_top_down_rows() {
        // A 10×10 image placed at (100,100)-(200,200). A region over the
        // TOP-LEFT quarter of the placement must hit rows 0..5, cols 0..5.
        let ctm = scale(100.0, 100.0, 100.0, 100.0);
        let cells = covered_cells(ctm, region(100.0, 150.0, 150.0, 200.0), 10, 10).unwrap();
        assert_eq!(
            cells,
            Cells {
                col0: 0,
                col1: 5,
                row0: 0,
                row1: 5
            }
        );
    }

    #[test]
    fn covered_cells_snaps_outward_and_a_touch_covers_nothing() {
        let ctm = scale(100.0, 100.0, 0.0, 0.0);
        // Region from 0.31 to 0.69 of the width: cells 3..7 (outward snap).
        let c = covered_cells(ctm, region(31.0, 0.0, 69.0, 100.0), 10, 10).unwrap();
        assert_eq!((c.col0, c.col1), (3, 7));
        // A region that only touches the right edge covers no cell.
        assert!(covered_cells(ctm, region(100.0, 0.0, 150.0, 100.0), 10, 10).is_none());
        // Entirely outside: none.
        assert!(covered_cells(ctm, region(200.0, 0.0, 250.0, 100.0), 10, 10).is_none());
    }

    #[test]
    fn a_quarter_turn_is_exact_and_a_skew_is_flagged() {
        // 90° rotation: unit square → x in [-h, 0], y in [0, w].
        let rot = Mat {
            a: 0.0,
            b: 100.0,
            c: -50.0,
            d: 0.0,
            e: 50.0,
            f: 0.0,
        };
        assert!(!is_skewed(rot));
        assert!(!is_skewed(scale(1.0, 1.0, 0.0, 0.0)));
        assert!(is_skewed(Mat {
            a: 70.0,
            b: 70.0,
            c: -70.0,
            d: 70.0,
            e: 0.0,
            f: 0.0
        }));
        // Under the rotation, image column 0 is at the placement's BOTTOM
        // (v axis → +x… ) — just assert some cells are found and clipped.
        let c = covered_cells(rot, region(0.0, 0.0, 25.0, 50.0), 10, 10).unwrap();
        assert!(!c.is_empty());
        assert!(c.col1 <= 10 && c.row1 <= 10);
    }

    #[test]
    fn wholly_covered_needs_one_region_to_contain_the_placement() {
        let ctm = scale(100.0, 50.0, 50.0, 100.0);
        assert!(wholly_covered(ctm, &[region(0.0, 0.0, 2000.0, 2000.0)]));
        assert!(!wholly_covered(ctm, &[region(60.0, 110.0, 120.0, 140.0)]));
        // Two regions that together cover it do not count.
        assert!(!wholly_covered(
            ctm,
            &[
                region(0.0, 0.0, 100.0, 200.0),
                region(100.0, 0.0, 200.0, 200.0)
            ]
        ));
    }

    #[test]
    fn clear_cells_handles_every_bit_depth() {
        // 8-bit RGB, 4×2.
        let mut s = vec![0xAB; 4 * 3 * 2];
        clear_cells(
            &mut s,
            4,
            3,
            8,
            Cells {
                col0: 1,
                col1: 3,
                row0: 1,
                row1: 2,
            },
            false,
        );
        assert!(s[..12].iter().all(|&b| b == 0xAB));
        assert_eq!(&s[12..15], &[0xAB; 3]);
        assert_eq!(&s[15..21], &[0; 6]);
        assert_eq!(&s[21..24], &[0xAB; 3]);

        // 1-bit gray, 12 wide (2-byte rows): clear cols 4..10 of row 0.
        let mut b = vec![0xFF, 0xFF, 0xFF, 0xFF];
        clear_cells(
            &mut b,
            12,
            1,
            1,
            Cells {
                col0: 4,
                col1: 10,
                row0: 0,
                row1: 1,
            },
            false,
        );
        assert_eq!(b, vec![0xF0, 0x3F, 0xFF, 0xFF]);

        // 16-bit gray, 2 wide: set cell (1, 0).
        let mut w = vec![0u8; 4];
        clear_cells(
            &mut w,
            2,
            1,
            16,
            Cells {
                col0: 1,
                col1: 2,
                row0: 0,
                row1: 1,
            },
            true,
        );
        assert_eq!(w, vec![0, 0, 0xFF, 0xFF]);

        // 4-bit, 3 wide: set cell 1 → the low nibble of byte 0.
        let mut n = vec![0u8; 2];
        clear_cells(
            &mut n,
            3,
            1,
            4,
            Cells {
                col0: 1,
                col1: 2,
                row0: 0,
                row1: 1,
            },
            true,
        );
        assert_eq!(n, vec![0x0F, 0x00]);
    }

    #[test]
    fn device_space_follows_the_component_count() {
        assert_eq!(
            device_space_for(CodecColorModel::Unspecified, 3),
            Some(&b"DeviceRGB"[..])
        );
        assert_eq!(
            device_space_for(CodecColorModel::Cmyk, 4),
            Some(&b"DeviceCMYK"[..])
        );
        assert_eq!(
            device_space_for(CodecColorModel::Bilevel, 1),
            Some(&b"DeviceGray"[..])
        );
        assert_eq!(device_space_for(CodecColorModel::Unspecified, 2), None);
    }
}
