//! Pass 431.0: set the characters a run's font cannot encode in a fallback
//! face, keeping every other character in the run's own font.
//!
//! The anchor show operator is split around each stretch of fallback
//! characters with a `Tf` naming the fallback resource at the run's own size,
//! and the run's font is restored after it (ISO 32000-1 §9.3.1 Table 103,
//! §9.4.3). `Tc`, `Tw`, `Tz`, `Ts` and the text matrix are untouched, so the
//! fallback glyphs sit on the run's baseline; each advances by its own face's
//! width (§9.4.4).

use std::collections::{BTreeMap, BTreeSet};

use crate::document::Document;
use crate::font_embed::FontEmbedPlan;
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::span::ByteSpan;
use crate::text_edit::addtext;
use crate::text_edit::edit::{
    EditError, EditLayout, EncodedReplacement, MatchRun, OpRec, ShowData, carried_codes,
    classify_font, encode_in, glyph_advance, is_subset_tag,
};
use crate::text_edit::font_extend::FontExtension;
use crate::text_edit::format::{CreatedFace, CreatedFont, embedded_probe, resolve_target_resource};
use crate::text_edit::sibling;
use crate::text_extract::font::ExtractFont;
use crate::view::DocumentView;

/// The face [`EditOptions::fallback`](crate::text_edit::EditOptions::fallback)
/// sets refused characters in.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FallbackFace {
    /// A `/Font` resource key on the page (`F1`), or a `/BaseFont` there
    /// (exact or with its §9.6.4 subset tag stripped). When the page has
    /// none, a standard-14 name (`Helvetica`) adds a non-embedded resource
    /// (§9.6.2.2). Anything else is refused.
    Named(String),
    /// A subset to embed as a new `/Type0` `Identity-H` resource (§9.7.4,
    /// §9.7.6.2). It must carry every character it is to set.
    Embedded(Box<FontEmbedPlan>),
}

/// Where a fallback face's resource came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum FallbackSource {
    /// A font resource already on the page.
    PageResource,
    /// A standard-14 resource the edit adds, no program embedded.
    AddedStandard14,
    /// An embedded subset the edit adds.
    EmbeddedSubset,
}

/// Which characters an edit set in a fallback face, and in which face.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FallbackUse {
    /// The characters set in the fallback face, each once, in the order the
    /// replacement first uses them.
    pub characters: Vec<char>,
    /// The fallback face's `/BaseFont`, subset tag included.
    pub base_font: String,
    /// The `/Font` resource key the content names it by.
    pub font_resource: Vec<u8>,
    /// Whether the resource was already there or the edit added it.
    pub source: FallbackSource,
}

/// The fallback face a [`TextEditPreview`](crate::text_edit::TextEditPreview)
/// draws its fallback glyphs from.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct PreviewFallback {
    /// The `/Font` resource key the commit names it by.
    pub font_resource: Vec<u8>,
    /// Its font dictionary. For an added embedded subset the descendant and
    /// descriptor are inline and the program does not resolve until the
    /// commit; draw from [`Self::font_program`].
    pub font: Dict,
    /// `/BaseFont`, verbatim.
    pub base_font: String,
    /// The decoded program of an added embedded subset; `None` otherwise.
    pub font_program: Option<Vec<u8>>,
}

/// Where the run being edited sits.
pub(crate) struct RunAt<'a> {
    pub(crate) doc: &'a DocumentView<'a>,
    pub(crate) resources: &'a Dict,
    pub(crate) recs: &'a [OpRec],
    pub(crate) own_dict: &'a Dict,
    pub(crate) anchor: &'a ShowData,
}

/// A fallback face resolved against the run's resources.
struct ResolvedFace {
    key: Vec<u8>,
    dict: Dict,
    font: ExtractFont,
    source: FallbackSource,
    created: Option<CreatedFont>,
    program: Option<Vec<u8>>,
    /// An added subset's CIDs; it is set with `Identity-H` 2-byte codes.
    cids: Option<BTreeMap<char, u16>>,
}

/// The replacement split between the run's font and the fallback face.
#[derive(Debug, Clone)]
pub(crate) struct Fallback {
    /// Each glyph of the replacement: its character, whether the fallback
    /// face sets it, and its code in that face.
    glyphs: Vec<(char, bool, u32)>,
    /// The bytes to show, `(in_fallback, bytes)` per stretch.
    segments: Vec<(bool, Vec<u8>)>,
    face_font: ExtractFont,
    preview: PreviewFallback,
    created: Option<CreatedFont>,
    pub(crate) used: FallbackUse,
}

impl Fallback {
    /// The replacement's advance (§9.4.4), each glyph in its own face.
    pub(crate) fn advance(&self, own: &ExtractFont, anchor: &ShowData) -> f64 {
        self.glyphs
            .iter()
            .map(|&(_, fb, code)| glyph_advance(self.font(fb, own), code, anchor))
            .sum()
    }

    fn font<'f>(&'f self, in_fallback: bool, own: &'f ExtractFont) -> &'f ExtractFont {
        if in_fallback { &self.face_font } else { own }
    }

    /// Where the replacement's glyphs land, from `origin_x` along the run's
    /// line; the box spans both faces' ascent and descent.
    pub(crate) fn layout(
        &self,
        anchor: &ShowData,
        own_dict: &Dict,
        own: &ExtractFont,
        origin_x: f64,
    ) -> EditLayout {
        let items = self.glyphs.iter().map(|&(ch, fb, code)| {
            let advance = glyph_advance(self.font(fb, own), code, anchor);
            (Some(ch), code, advance)
        });
        let face = &self.face_font;
        let metrics = (
            f64::from(own.ascent().max(face.ascent())),
            f64::from(own.descent().min(face.descent())),
        );
        let mut layout =
            EditLayout::placed(anchor, own_dict, &own.base_font, items, origin_x, metrics);
        let flags = self.glyphs.iter().map(|g| g.1).collect();
        layout.fallback = Some((self.preview.clone(), flags));
        layout
    }

    /// The anchor operator with its match replaced, switching faces with `Tf`.
    pub(crate) fn emit(&self, anchor: &ShowData, m: &MatchRun, pin_num: Option<f64>) -> Vec<u8> {
        let key = &self.used.font_resource;
        sibling::emit_segmented_operator(anchor, m, &self.segments, pin_num, key)
    }

    /// The resource the commit must create, if any.
    pub(crate) fn into_created(self) -> Option<CreatedFont> {
        self.created
    }
}

/// Split `replace` between the run's font and `face`: each character
/// `encode_own` refuses goes to the face, the rest are encoded together in
/// the run's font. `refused` is the run's refusal of the whole replacement,
/// returned (with the face's reason appended) when the face cannot help.
///
/// # Errors
///
/// `refused`, extended; or the first non-refusal `encode_own` raises.
pub(crate) fn encode<F>(
    at: &RunAt<'_>,
    replace: &str,
    face: &FallbackFace,
    own: &ExtractFont,
    refused: EditError,
    encode_own: F,
) -> Result<
    (
        EncodedReplacement,
        ExtractFont,
        Option<FontExtension>,
        Fallback,
    ),
    EditError,
>
where
    F: Fn(&str) -> Result<(EncodedReplacement, ExtractFont, Option<FontExtension>), EditError>,
{
    let EditError::Refused(mut refusal) = refused else {
        return Err(refused);
    };
    let mut to_face: BTreeSet<char> = BTreeSet::new();
    for ch in replace.chars().collect::<BTreeSet<_>>() {
        match encode_own(ch.encode_utf8(&mut [0; 4])) {
            Ok(_) => {}
            Err(EditError::Refused(_)) => {
                to_face.insert(ch);
            }
            Err(e) => return Err(e),
        }
    }
    if to_face.is_empty() {
        return Err(EditError::Refused(refusal));
    }
    let resolved = match resolve_face(at, face) {
        Ok(r) => r,
        Err(why) => {
            refusal.message = format!("{} The fallback face was not used: {why}.", refusal.message);
            return Err(EditError::Refused(refusal));
        }
    };
    let face_text: String = to_face.iter().collect();
    let face_codes = match face_codes(at, &resolved, &face_text) {
        Ok(codes) => codes,
        Err(ch) => {
            refusal.message = format!(
                "{} The fallback face '{}' cannot set it either: it has no code for {}.",
                refusal.message,
                resolved.font.base_font,
                char_label(ch)
            );
            refusal.character = Some(ch);
            return Err(EditError::Refused(refusal));
        }
    };
    let own_text: String = replace.chars().filter(|c| !to_face.contains(c)).collect();
    let (mut encoded, font, extension) = if own_text.is_empty() {
        (EncodedReplacement::default(), own.clone(), None)
    } else {
        encode_own(&own_text)?
    };
    let fallback = split(
        replace,
        &to_face,
        &encoded,
        &face_codes,
        resolved,
        at.anchor.font_name.as_slice(),
    )?;
    encoded
        .disclosures
        .push(disclosure(&fallback.used, &font.base_font));
    Ok((encoded, font, extension, fallback))
}

/// Interleave the run's codes and the face's codes in replacement order.
fn split(
    replace: &str,
    to_face: &BTreeSet<char>,
    own: &EncodedReplacement,
    face: &BTreeMap<char, (u32, Vec<u8>)>,
    resolved: ResolvedFace,
    own_key: &[u8],
) -> Result<Fallback, EditError> {
    let own_count = replace.chars().filter(|c| !to_face.contains(c)).count();
    let width = own.bytes.len().checked_div(own.codes.len()).unwrap_or(1);
    if own.codes.len() != own_count || own.bytes.len() != own_count * width {
        // One code per character is what lets the two faces interleave.
        return Err(EditError::Unsupported(
            crate::text_edit::cause::UnsupportedCause::EncodingNotInvertible,
        ));
    }
    let mut own_codes = own.codes.iter().zip(own.bytes.chunks(width.max(1)));
    let (mut glyphs, mut segments) = (Vec::new(), Vec::<(bool, Vec<u8>)>::new());
    let mut characters = Vec::new();
    for ch in replace.chars() {
        let in_face = to_face.contains(&ch);
        let (code, bytes) = if in_face {
            if !characters.contains(&ch) {
                characters.push(ch);
            }
            face.get(&ch).map(|(c, b)| (*c, b.as_slice()))
        } else {
            own_codes.next().map(|(c, b)| (*c, b))
        }
        .ok_or(EditError::Unsupported(
            crate::text_edit::cause::UnsupportedCause::EncodingNotInvertible,
        ))?;
        glyphs.push((ch, in_face, code));
        match segments.last_mut() {
            Some((f, b)) if *f == in_face => b.extend_from_slice(bytes),
            _ => segments.push((in_face, bytes.to_vec())),
        }
    }
    debug_assert_ne!(resolved.key.as_slice(), own_key);
    let used = FallbackUse {
        characters,
        base_font: resolved.font.base_font.clone(),
        font_resource: resolved.key.clone(),
        source: resolved.source,
    };
    let preview = PreviewFallback {
        font_resource: resolved.key,
        font: resolved.dict,
        base_font: resolved.font.base_font.clone(),
        font_program: resolved.program,
    };
    Ok(Fallback {
        glyphs,
        segments,
        face_font: resolved.font,
        preview,
        created: resolved.created,
        used,
    })
}

/// `face` as a resource of the run's stream.
fn resolve_face(at: &RunAt<'_>, face: &FallbackFace) -> Result<ResolvedFace, String> {
    let fonts = at
        .resources
        .get(b"Font")
        .map(|o| at.doc.resolve(o))
        .and_then(Object::as_dict)
        .cloned()
        .unwrap_or_default();
    match face {
        FallbackFace::Named(name) => {
            if let Some((key, dict)) = resolve_target_resource(at.doc, at.resources, name) {
                if dict == at.own_dict {
                    return Err(format!("'{name}' names the run's own font"));
                }
                let font = ExtractFont::resolve(at.doc, dict);
                classify_font(at.doc, dict, &font).map_err(|e| e.to_string())?;
                return Ok(ResolvedFace {
                    key,
                    dict: dict.clone(),
                    font,
                    source: FallbackSource::PageResource,
                    created: None,
                    program: None,
                    cids: None,
                });
            }
            let std14 = crate::fontdata::std14_by_base_font(name).ok_or_else(|| {
                format!("'{name}' is neither a font resource here nor a standard-14 face")
            })?;
            let Object::Dict(dict) = addtext::std14_resource_dict(std14) else {
                return Err("the standard-14 font dictionary did not build".to_owned());
            };
            let key = addtext::pick_font_name(&fonts);
            Ok(ResolvedFace {
                font: ExtractFont::resolve(at.doc, &dict),
                created: Some(CreatedFont {
                    key: key.clone(),
                    face: CreatedFace::Simple(dict.clone()),
                }),
                key,
                dict,
                source: FallbackSource::AddedStandard14,
                program: None,
                cids: None,
            })
        }
        FallbackFace::Embedded(plan) => {
            let plan = addtext::with_file_unique_plan_tag(plan, at.doc);
            let (dict, font) = embedded_probe(at.doc, &plan)?;
            let key = addtext::pick_font_name(&fonts);
            let cids = plan.glyphs.iter().map(|g| (g.unicode, g.cid)).collect();
            Ok(ResolvedFace {
                key: key.clone(),
                dict,
                font,
                source: FallbackSource::EmbeddedSubset,
                program: Some(plan.program.clone()),
                cids: Some(cids),
                created: Some(CreatedFont {
                    key,
                    face: CreatedFace::Embedded(Box::new(plan)),
                }),
            })
        }
    }
}

/// Each character of `text` as `(code, bytes)` in the face, or the first it
/// cannot set. An existing subset is held to the codes this stream already
/// shows in it, and an existing simple font with `/Widths` to its range.
fn face_codes(
    at: &RunAt<'_>,
    face: &ResolvedFace,
    text: &str,
) -> Result<BTreeMap<char, (u32, Vec<u8>)>, char> {
    let mut out = BTreeMap::new();
    for ch in text.chars() {
        let coded = match &face.cids {
            Some(cids) => cids
                .get(&ch)
                .map(|&cid| (u32::from(cid), cid.to_be_bytes().to_vec())),
            None => encode_in(&face.font, &BTreeSet::new(), ch.encode_utf8(&mut [0; 4]))
                .ok()
                .filter(|e| e.codes.len() == 1)
                .and_then(|e| Some((*e.codes.first()?, e.bytes)))
                .filter(|(code, _)| existing_has(at, face, *code)),
        };
        out.insert(ch, coded.ok_or(ch)?);
    }
    Ok(out)
}

/// Whether an existing resource can show `code` with a known glyph and width.
fn existing_has(at: &RunAt<'_>, face: &ResolvedFace, code: u32) -> bool {
    if face.source != FallbackSource::PageResource {
        return true;
    }
    if is_subset_tag(&face.font.base_font) {
        return carried_codes(at.recs, &face.key).contains(&code);
    }
    let int = |k: &[u8]| {
        face.dict
            .get(k)
            .map(|o| at.doc.resolve(o))
            .and_then(Object::as_int)
    };
    match (
        face.dict.get(b"Widths"),
        int(b"FirstChar"),
        int(b"LastChar"),
    ) {
        (Some(_), Some(first), Some(last)) => (first..=last).contains(&i64::from(code)),
        (Some(_), _, _) => false,
        (None, _, _) => true,
    }
}

/// Every character the fallback face would set for this run, by the same
/// per-character check [`encode`] applies, for
/// [`run_repertoire`](crate::edit::EditSession::run_repertoire). Empty when
/// the face does not resolve.
pub(crate) fn face_accepts(at: &RunAt<'_>, face: &FallbackFace) -> BTreeSet<char> {
    use crate::text_edit::encoding::{CompositeEncoding, InverseEncoding};
    let Ok(resolved) = resolve_face(at, face) else {
        return BTreeSet::new();
    };
    let candidates: Vec<char> = match (&resolved.cids, resolved.font.glyph_names()) {
        (Some(cids), _) => cids.keys().copied().collect(),
        (None, Some(names)) => {
            InverseEncoding::build(&resolved.font.base_font, names).candidate_chars()
        }
        (None, None) => resolved
            .font
            .to_unicode_cmap()
            .and_then(|cmap| CompositeEncoding::build(&resolved.font.base_font, cmap).ok())
            .map(|c| c.candidate_chars())
            .unwrap_or_default(),
    };
    candidates
        .into_iter()
        .filter(|&ch| face_codes(at, &resolved, ch.encode_utf8(&mut [0; 4])).is_ok())
        .collect()
}

/// Rule 4: which characters went to which face, and where it came from.
fn disclosure(used: &FallbackUse, own: &str) -> String {
    let chars: Vec<String> = used.characters.iter().map(|&c| char_label(c)).collect();
    let source = match used.source {
        FallbackSource::PageResource => "a font already among this stream's resources",
        FallbackSource::AddedStandard14 => {
            "a standard-14 face ADDED to the resources with no program embedded, so a \
             reader draws it with its own substitute (§9.6.2.2)"
        }
        FallbackSource::EmbeddedSubset => {
            "a subset EMBEDDED by this edit as a Type0 Identity-H font (§9.7.6.2)"
        }
    };
    format!(
        "fallback: '{own}' cannot encode {}, so {} set in '{}' (font resource /{}), {source}; \
         the rest of the replacement stays in '{own}'",
        chars.join(", "),
        if chars.len() == 1 {
            "it is"
        } else {
            "they are"
        },
        used.base_font,
        String::from_utf8_lossy(&used.font_resource),
    )
}

fn char_label(c: char) -> String {
    format!("U+{:04X} '{c}'", u32::from(c))
}

/// Disclosed when the added resource lands in a dictionary other owners
/// share.
pub(crate) const SHARED_RESOURCES_NOTE: &str = "fallback: the font resource was added to a /Resources dictionary shared with other \
     pages or forms; it is unreferenced there and changes nothing about how they render";

/// Bind `created` for a one-shot incremental save: numbers from `next`, its
/// streams staged after `staging`'s current end. A write to `owner`'s own
/// dictionary goes into `form_dict` when the owner is a form (a stream).
/// Answers whether the patched resources are shared.
///
/// # Errors
///
/// [`EditError::Unsupported`] when no object number is left or the face
/// does not build.
pub(crate) fn bind_one_shot(
    doc: &Document,
    created: &CreatedFont,
    (owner_id, form_dict): (ObjId, Option<&mut Dict>),
    next: &mut Option<u32>,
    staging: &mut Vec<u8>,
    objects: &mut Vec<(ObjId, Object)>,
) -> Result<bool, EditError> {
    use crate::text_edit::cause::UnsupportedCause;
    let exhausted = || EditError::Unsupported(UnsupportedCause::ObjectNumbersExhausted);
    let first = next.ok_or_else(exhausted)?;
    *next = first.checked_add(created.object_count());
    let base_len = doc.bytes().len();
    let (built, shared) = created
        .objects(
            &doc.view(),
            owner_id,
            form_dict.is_none(),
            ObjId::new(first, 0),
            |bytes| {
                let span = ByteSpan::new(base_len + staging.len(), bytes.len());
                staging.extend_from_slice(bytes);
                span
            },
        )
        .map_err(|detail| EditError::Unsupported(UnsupportedCause::CommitFailed { detail }))?;
    let mut form_dict = form_dict;
    for (id, value) in built {
        match (&mut form_dict, value) {
            (Some(fd), Object::Dict(d)) if id == owner_id => **fd = d,
            (_, value) => objects.push((id, value)),
        }
    }
    Ok(shared)
}
