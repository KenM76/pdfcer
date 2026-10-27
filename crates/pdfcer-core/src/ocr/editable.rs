//! The editable-text OCR output: each recognised word is erased from the
//! scan and written as visible text in its measured ink colour; a word that
//! cannot be (a *suspect*) stays in the image under invisible text.
//!
//! The capability is Acrobat Pro's "Editable Text and Images" output, whose
//! documented behaviour is catalogued in the Acrobat parity RAG at
//! `ocr__editable_text_and_images_mode.md`. Its erasure method, codec,
//! colour and threshold are undocumented; the choices here are pdfcer's:
//!
//! - **Erasure** paints a word's ink with the median of a ring around its
//!   box: every pixel differing from that background by more than its own
//!   spread and connected to ink inside the box, so a glyph the engine's
//!   box clips goes whole, plus a one-pixel antialiased edge. Pixels inside
//!   a suspect's box are never painted ([`super::editable_image`]).
//! - **Placement**: engine boxes are padded, so each line's visible words
//!   are refitted to their measured ink: x from the word's own ink, one
//!   baseline (median ink bottom of words without descenders) and one size
//!   (median cap height of words with no descender) per line, unrotated
//!   pages only ([`super::editable_fit`]).
//! - **Colour** is the median of the word's strongest-contrast pixels, in
//!   the image's own device components (`g`, `rg` or `k`, ISO 32000-1
//!   §8.6.8); an ICC-based image's ink is written uncalibrated, disclosed.
//! - **Codec**: a JPEG source stays JPEG, everything else becomes Flate
//!   ([`EditableImageCodec`]).
//! - **Threshold**: [`EditableTextOptions::min_confidence_percent`].

use std::collections::BTreeMap;

use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::Page;
use crate::redact_image::decode;
use crate::vartext::encode_winansi;
use crate::view::DocumentView;

use super::OcrPage;
use super::editable_image::{
    Measure, PixRect, Placement, Raster, encode, erase, filter_name, measure, page_rect,
    placements, unsupported, word_pixels,
};
use super::layer::OcrLayerOptions;
use super::layer_content::{Ink, WordStyle, place_word};

/// What an OCR run writes on the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum OcrOutput {
    /// Invisible text (render mode 3) over the untouched scan: the page
    /// looks exactly as before and becomes searchable.
    #[default]
    SearchableImage,
    /// Recognised words leave the scan and become visible, editable text;
    /// see the [module docs](self).
    EditableText(EditableTextOptions),
}

/// How [`OcrOutput::EditableText`] decides and encodes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct EditableTextOptions {
    /// A scored word below this confidence (percent) is a suspect and stays
    /// in the image. Default 50.
    pub min_confidence_percent: u8,
    /// Whether a word the engine gave no confidence is converted. Default
    /// `true`: an unscored engine would otherwise convert nothing; the count
    /// is disclosed.
    pub unscored_visible: bool,
    /// How a rewritten image is encoded.
    pub image_codec: EditableImageCodec,
}

impl Default for EditableTextOptions {
    fn default() -> Self {
        Self {
            min_confidence_percent: 50,
            unscored_visible: true,
            image_codec: EditableImageCodec::MatchSource,
        }
    }
}

impl EditableTextOptions {
    /// The defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set [`Self::min_confidence_percent`] (values above 100 mean 100).
    #[must_use]
    pub fn with_min_confidence_percent(mut self, percent: u8) -> Self {
        self.min_confidence_percent = percent.min(100);
        self
    }

    /// Set [`Self::unscored_visible`].
    #[must_use]
    pub fn with_unscored_visible(mut self, visible: bool) -> Self {
        self.unscored_visible = visible;
        self
    }

    /// Set [`Self::image_codec`].
    #[must_use]
    pub fn with_image_codec(mut self, codec: EditableImageCodec) -> Self {
        self.image_codec = codec;
        self
    }
}

/// The encoding of an image whose glyphs were erased.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum EditableImageCodec {
    /// A `DCTDecode` source of one or three components is re-encoded as JPEG
    /// (quality 90); any other source becomes `FlateDecode`.
    #[default]
    MatchSource,
    /// Always `FlateDecode` (lossless; larger for photographic scans).
    Flate,
}

/// Why a word stayed in the image as invisible text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum SuspectReason {
    /// Its confidence is below [`EditableTextOptions::min_confidence_percent`].
    LowConfidence,
    /// The engine gave no confidence and [`EditableTextOptions::unscored_visible`] is off.
    Unscored,
    /// A character has no glyph in the Standard-14 face's WinAnsi encoding.
    NoGlyph,
    /// Its box is not inside any image the page's content draws.
    NotOverImage,
    /// Its box is inside more than one drawn image.
    OverlappingImages,
    /// The image under it cannot be rewritten (a mask, `/Decode`, an
    /// indexed or special colour space, a bit depth other than 8 or 1-bit
    /// grey, or samples that could not be decoded).
    ImageUnsupported,
    /// The image under it is drawn more than once on the page.
    ImageDrawnTwice,
    /// Nothing in its box differs enough from the background to be ink.
    NoInk,
    /// The background around it is a picture or pattern, not paper.
    BusyBackground,
}

impl SuspectReason {
    /// A short plain-English description, for a disclosure line.
    #[must_use]
    pub fn describe(self) -> &'static str {
        match self {
            Self::LowConfidence => "below the confidence threshold",
            Self::Unscored => "no confidence reported",
            Self::NoGlyph => "a character the substituted font cannot show",
            Self::NotOverImage => "not over a scanned image",
            Self::OverlappingImages => "over more than one image",
            Self::ImageUnsupported => "in an image that cannot be rewritten",
            Self::ImageDrawnTwice => "in an image drawn twice on the page",
            Self::NoInk => "no ink found in its box",
            Self::BusyBackground => "on a busy background",
        }
    }
}

/// One image the editable output re-encoded.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ImageRewrite {
    /// The object number written: `source` when rewritten in place, a new
    /// object when `cloned`.
    pub image: u32,
    /// The image object the page drew before.
    pub source: u32,
    /// The source is used elsewhere, so the page now draws a copy and every
    /// other use keeps the original.
    pub cloned: bool,
    /// The source's `/Filter` (`/`-joined for a chain, `none` when absent).
    pub filter_before: String,
    /// The written `/Filter`.
    pub filter_after: String,
    /// The source's encoded stream length.
    pub bytes_before: usize,
    /// The written stream length.
    pub bytes_after: usize,
    /// Words whose glyphs were erased from it.
    pub words_erased: usize,
}

impl OcrLayerOptions {
    /// Choose the output ([`OcrOutput::SearchableImage`] by default).
    #[must_use]
    pub fn with_output(mut self, output: OcrOutput) -> Self {
        self.output = output;
        self
    }
}

/// Where a rewritten image goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ImageTarget {
    /// Replace the object (it is drawn only here).
    InPlace(ObjId),
    /// Write a new object and bind the page's name to it.
    Clone,
}

/// A rewritten image, ready for a writer: the dictionary carries the new
/// `/Filter` and no `/Length`.
#[derive(Debug, Clone)]
pub(crate) struct ImageWrite {
    pub(crate) target: ImageTarget,
    /// The page's `/XObject` name for it.
    pub(crate) name: Vec<u8>,
    pub(crate) dict: Dict,
    pub(crate) data: Vec<u8>,
    pub(crate) report: ImageRewrite,
}

impl ImageWrite {
    /// The stream object, its data staged at combined offset `start`.
    pub(crate) fn staged(&self, start: usize) -> Object {
        let mut dict = self.dict.clone();
        let len = i64::try_from(self.data.len()).unwrap_or(i64::MAX);
        dict.insert(crate::object::Name::from(b"Length"), Object::Integer(len));
        Object::Stream(crate::object::Stream {
            dict,
            data_span: crate::span::ByteSpan::new(start, self.data.len()),
        })
    }
}

/// An image under at least one convertible word.
struct Slot {
    placement: Placement,
    dict: Dict,
    raster: Raster,
    codec: Option<crate::image_codec::Codec>,
    filter_before: String,
    bytes_before: usize,
    shared: bool,
    erased: usize,
}

/// The per-word decisions for one page, made before anything is written.
pub(super) struct Analysis {
    /// How each word is written, by index into the page's words.
    pub(super) styles: Vec<WordStyle>,
    reasons: Vec<Option<SuspectReason>>,
    measures: Vec<Option<(usize, Measure)>>,
    /// The first image each word's box falls in, and its pixels there.
    hits: Vec<Option<(ObjId, PixRect)>>,
    slots: Vec<Slot>,
}

impl Analysis {
    /// The page-space bounds of word `w`'s measured ink.
    pub(super) fn ink_rect(&self, w: usize) -> Option<crate::page_tree::Rect> {
        let (slot, m) = self.measures.get(w)?.as_ref()?;
        let s = self.slots.get(*slot)?;
        page_rect(s.placement.ctm, s.raster.width, s.raster.height, m.ink_box)
    }
}

/// The finished editable output for one page.
pub(super) struct Finished {
    pub(super) images: Vec<ImageWrite>,
    pub(super) words_visible: usize,
    pub(super) suspects: Vec<(SuspectReason, usize)>,
}

/// Decide each word of `ocr_page`: visible (with its ink and the image it
/// is erased from) or a suspect. `uses` counts every image's uses across the
/// document, so a shared image is copied rather than changed everywhere.
pub(super) fn analyse(
    view: &DocumentView<'_>,
    page: &Page,
    ocr_page: &OcrPage,
    opts: &OcrLayerOptions,
    eo: &EditableTextOptions,
    uses: &BTreeMap<ObjId, usize>,
) -> Analysis {
    let drawn = placements(view, page);
    let mut a = Analysis {
        styles: Vec::with_capacity(ocr_page.words.len()),
        reasons: Vec::with_capacity(ocr_page.words.len()),
        measures: Vec::with_capacity(ocr_page.words.len()),
        hits: Vec::with_capacity(ocr_page.words.len()),
        slots: Vec::new(),
    };
    let mut cache: BTreeMap<ObjId, Option<usize>> = BTreeMap::new();
    for word in &ocr_page.words {
        let hits = hits_of(view, &drawn, word.rect);
        a.hits.push(hits.first().map(|(p, r)| (p.id, *r)));
        let decided = if place_word(word, opts.font, page.rotate).is_none() {
            Err(None)
        } else {
            let at = (&drawn[..], &hits[..]);
            word_target(view, page, word, eo, uses, at, &mut cache, &mut a.slots).map_err(Some)
        };
        let (style, reason, m) = match decided {
            Ok((slot, m)) => (
                WordStyle::Visible(Ink::from_samples(&m.ink)),
                None,
                Some((slot, m)),
            ),
            Err(reason) => (WordStyle::Invisible, reason, None),
        };
        a.styles.push(style);
        a.reasons.push(reason);
        a.measures.push(m);
    }
    a
}

/// The image slot and measurement for one word, or why it is a suspect.
#[allow(clippy::too_many_arguments)] // the per-page state the word is checked against
fn word_target(
    view: &DocumentView<'_>,
    page: &Page,
    word: &super::RecognizedWord,
    eo: &EditableTextOptions,
    uses: &BTreeMap<ObjId, usize>,
    (drawn, hits): (&[Placement], &[(&Placement, PixRect)]),
    cache: &mut BTreeMap<ObjId, Option<usize>>,
    slots: &mut Vec<Slot>,
) -> Result<(usize, Measure), SuspectReason> {
    match word.confidence {
        Some(c) if c * 100.0 < f32::from(eo.min_confidence_percent) => {
            return Err(SuspectReason::LowConfidence);
        }
        None if !eo.unscored_visible => return Err(SuspectReason::Unscored),
        _ => {}
    }
    if encode_winansi(&word.text).1 > 0 {
        return Err(SuspectReason::NoGlyph);
    }
    let Some(&(placement, rect)) = hits.first() else {
        return Err(SuspectReason::NotOverImage);
    };
    // Checked first: a twice-drawn image also overlaps itself, and this is
    // the more useful reason to disclose.
    if drawn.iter().filter(|p| p.id == placement.id).count() > 1 {
        return Err(SuspectReason::ImageDrawnTwice);
    }
    if hits.len() > 1 {
        return Err(SuspectReason::OverlappingImages);
    }
    let slot = *cache
        .entry(placement.id)
        .or_insert_with(|| open_slot(view, page, placement, uses, slots));
    let slot = slot.ok_or(SuspectReason::ImageUnsupported)?;
    let raster = &slots
        .get(slot)
        .ok_or(SuspectReason::ImageUnsupported)?
        .raster;
    if raster.width == 0 {
        return Err(SuspectReason::ImageUnsupported);
    }
    measure(raster, rect).map(|m| (slot, m))
}

/// Every drawn image whose samples box `rect` falls inside, in paint order,
/// with the pixels it covers.
fn hits_of<'p>(
    view: &DocumentView<'_>,
    drawn: &'p [Placement],
    rect: crate::page_tree::Rect,
) -> Vec<(&'p Placement, PixRect)> {
    drawn
        .iter()
        .filter_map(|p| {
            let Object::Stream(s) = view.resolved(p.id) else {
                return None;
            };
            let dim = |k: &[u8]| {
                s.dict
                    .get(k)
                    .map(|o| view.resolve(o))
                    .and_then(Object::as_int)
                    .and_then(|v| u32::try_from(v).ok())
            };
            word_pixels(p.ctm, dim(b"Width")?, dim(b"Height")?, rect).map(|r| (p, r))
        })
        .collect()
}

/// Decode a drawn image into a new slot, or `None` when it cannot be
/// rewritten.
fn open_slot(
    view: &DocumentView<'_>,
    page: &Page,
    placement: &Placement,
    uses: &BTreeMap<ObjId, usize>,
    slots: &mut Vec<Slot>,
) -> Option<usize> {
    let Object::Stream(s) = view.resolved(placement.id) else {
        return None;
    };
    let raw = view.slice(s.data_span)?;
    let decoded = decode(view, &s.dict, raw, false, &page.resources).ok()?;
    if unsupported(view, &s.dict, &decoded) {
        return None;
    }
    slots.push(Slot {
        placement: placement.clone(),
        dict: s.dict.clone(),
        raster: Raster::from_decoded(&decoded),
        codec: decoded.codec,
        filter_before: filter_name(view, &s.dict),
        bytes_before: raw.len(),
        shared: uses.get(&placement.id).copied().unwrap_or(0) > 1,
        erased: 0,
    });
    Some(slots.len() - 1)
}

/// Erase the words actually written visible (`visible`, word indices) and
/// re-encode every image they touched. Erasure follows all measuring, so
/// one word's erasure never shifts a neighbour's background. `None` when an
/// encoder fails, so the caller refuses rather than write text over glyphs
/// still in the image.
pub(super) fn finish(
    mut a: Analysis,
    visible: &[usize],
    codec: EditableImageCodec,
) -> Option<Finished> {
    for &w in visible {
        if let Some(Some((slot, m))) = a.measures.get(w)
            && let Some(s) = a.slots.get_mut(*slot)
        {
            // A word staying in the image keeps its pixels, even where a
            // converted neighbour's glyph touches it.
            let protect: Vec<PixRect> = a
                .hits
                .iter()
                .enumerate()
                .filter(|(i, _)| !visible.contains(i))
                .filter_map(|(_, h)| h.filter(|(id, _)| *id == s.placement.id))
                .map(|(_, r)| r)
                .collect();
            erase(&mut s.raster, m, &protect);
            s.erased += 1;
        }
    }
    let mut counts: BTreeMap<SuspectReason, usize> = BTreeMap::new();
    for r in a.reasons.iter().flatten() {
        *counts.entry(*r).or_insert(0) += 1;
    }
    let images = a
        .slots
        .iter()
        .filter(|s| s.erased > 0)
        .map(|s| {
            let e = encode(&s.dict, &s.raster, s.codec, codec)?;
            Some(ImageWrite {
                target: if s.shared {
                    ImageTarget::Clone
                } else {
                    ImageTarget::InPlace(s.placement.id)
                },
                name: s.placement.name.clone(),
                report: ImageRewrite {
                    image: s.placement.id.num,
                    source: s.placement.id.num,
                    cloned: s.shared,
                    filter_before: s.filter_before.clone(),
                    filter_after: e.filter.to_owned(),
                    bytes_before: s.bytes_before,
                    bytes_after: e.data.len(),
                    words_erased: s.erased,
                },
                dict: e.dict,
                data: e.data,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    Some(Finished {
        images,
        words_visible: visible.len(),
        suspects: counts.into_iter().collect(),
    })
}

/// The page's effective `/XObject` dictionary, resolved, for a clone's
/// binding to merge into.
pub(super) fn xobject_base(view: &DocumentView<'_>, page: &Page) -> Dict {
    page.resources
        .get(b"XObject")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
        .cloned()
        .unwrap_or_default()
}

/// [`finish`] the analysis and record it on `report`: the visible, suspect
/// and unscored counts and each rewritten image. `None` when an encoder
/// failed.
pub(super) fn apply(
    a: Analysis,
    visible: &[usize],
    ocr_page: &OcrPage,
    codec: EditableImageCodec,
    report: &mut super::layer_report::OcrLayerReport,
) -> Option<Vec<ImageWrite>> {
    let done = finish(a, visible, codec)?;
    report.words_visible = done.words_visible;
    report.words_suspect = done.suspects.iter().map(|(_, n)| n).sum();
    report.words_unscored_visible = visible
        .iter()
        .filter(|&&w| {
            ocr_page
                .words
                .get(w)
                .is_some_and(|w| w.confidence.is_none())
        })
        .count();
    report.images_rewritten = done.images.iter().map(|i| i.report.clone()).collect();
    report.suspects = done.suspects;
    Some(done.images)
}
