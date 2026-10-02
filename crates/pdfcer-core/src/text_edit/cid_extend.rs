//! Decision 172 route A for a composite font: a `/Type0` font with
//! `/Identity-H` over a `CIDFontType2` subset whose `/CIDToGIDMap` is
//! `/Identity` or absent, so a CID names its glyph directly (ISO 32000-2
//! §9.7.4.2, §9.7.5.2).
//!
//! A character gets the CID the font's `/ToUnicode` already gives it, else
//! [`allocate`] gives it the glyph id the program's cmap reaches, as a new
//! two-byte `bfchar` entry (§9.10.3). A width that differs from `/DW` is
//! appended to the descendant's `/W` (§9.7.4.3), which is rewritten
//! copy-on-write. The Type0 dictionary and the program are never touched.

use std::collections::BTreeSet;

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::text_edit::font_extend::{
    AddedGlyph, Blocked, FontExtension, UnicodeMap, codes_shown, font_file2, font_object,
    map_private, number, shown_elsewhere,
};
use crate::text_edit::glyph_find;
use crate::text_edit::program_glyphs::EmbeddedGlyphs;
use crate::text_extract::cmap::ToUnicodeCMap;
use crate::view::DocumentView;

/// [`CidTarget::read`]'s refusal of a CFF-based descendant.
pub(crate) const CFF_CID: &str = "only a TrueType-based CIDFont can be extended so far";
/// [`CidTarget::read`]'s refusal of a `/CIDToGIDMap` stream.
pub(crate) const STREAM_MAP: &str =
    "the font's /CIDToGIDMap is a stream, so a CID does not name its glyph directly";
/// A `/CIDToGIDMap` stream covers at most CIDs 0..=0xFFFF (two bytes each).
const MAX_MAP_BYTES: usize = 2 * 0x1_0000;

/// The composite font a plan extends, read once.
pub(crate) struct CidTarget {
    pub(crate) type0_id: ObjId,
    pub(crate) type0: Dict,
    pub(crate) descendant_id: ObjId,
    pub(crate) descendant: Dict,
    pub(crate) program: Vec<u8>,
    pub(crate) to_unicode: UnicodeMap,
    default_width: f64,
    /// `/W` flattened to `(first, last, width)`.
    ranges: Vec<(u32, u32, f64)>,
    /// The decoded `/CIDToGIDMap` stream; `None` for `/Identity`.
    pub(crate) map: Option<Vec<u8>>,
}

/// Whether `font` is a `/Type0` dictionary, the shape this module handles.
pub(crate) fn is_composite(doc: &DocumentView<'_>, font: &Dict) -> bool {
    name(doc, font, b"Subtype").as_deref() == Some(b"Type0".as_slice())
}

fn name(doc: &DocumentView<'_>, d: &Dict, key: &[u8]) -> Option<Vec<u8>> {
    doc.resolve(d.get(key).unwrap_or(&Object::Null))
        .as_name()
        .map(|n| n.as_bytes().to_vec())
}

impl CidTarget {
    /// The Type0 font `font_name` selects in `resources`, refusing a
    /// `/CIDToGIDMap` stream.
    pub(crate) fn read(
        doc: &DocumentView<'_>,
        resources: &Dict,
        font_name: &[u8],
        type0: &Dict,
    ) -> Result<Self, String> {
        Self::read_with(doc, (resources, font_name), type0, false)
    }

    /// [`Self::read`], accepting a `/CIDToGIDMap` stream when `stream_map`.
    pub(crate) fn read_with(
        doc: &DocumentView<'_>,
        (resources, font_name): (&Dict, &[u8]),
        type0: &Dict,
        stream_map: bool,
    ) -> Result<Self, String> {
        let type0_id = font_object(doc, resources, font_name)?;
        if name(doc, type0, b"Encoding").as_deref() != Some(b"Identity-H".as_slice()) {
            return Err("only an /Identity-H composite font can be extended so far".to_owned());
        }
        let descendant_id = doc
            .resolve(type0.get(b"DescendantFonts").unwrap_or(&Object::Null))
            .as_array()
            .and_then(|a| a.first())
            .and_then(Object::as_reference)
            .ok_or("the descendant font is not a separate object")?;
        let descendant = doc
            .resolved(descendant_id)
            .as_dict()
            .cloned()
            .ok_or("the descendant font could not be read")?;
        if name(doc, &descendant, b"Subtype").as_deref() != Some(b"CIDFontType2".as_slice()) {
            return Err(CFF_CID.to_owned());
        }
        let map = cid_to_gid(doc, &descendant, stream_map)?;
        let descriptor = doc
            .resolve(descendant.get(b"FontDescriptor").unwrap_or(&Object::Null))
            .as_dict()
            .ok_or("the font has no descriptor")?;
        let program = font_file2(doc, descriptor)?;
        let to_unicode = UnicodeMap::read(doc, type0, 2)?
            .ok_or("the font has no /ToUnicode map, so a new CID could not be read back")?;
        let default_width = doc
            .resolve(descendant.get(b"DW").unwrap_or(&Object::Null))
            .as_number()
            .unwrap_or(1000.0);
        let ranges = w_ranges(&w_items(doc, &descendant));
        Ok(Self {
            type0_id,
            type0: type0.clone(),
            descendant_id,
            descendant,
            program,
            to_unicode,
            default_width,
            ranges,
            map,
        })
    }

    /// The glyph `cid` selects (ISO 32000-2 §9.7.4.2 Table 117): the
    /// big-endian pair at `2·cid` of a map stream, 0 beyond its end; `cid`
    /// itself under `/Identity`.
    pub(crate) fn gid_of(&self, cid: u32) -> u32 {
        let Some(map) = &self.map else {
            return cid;
        };
        let at = usize::try_from(cid).unwrap_or(usize::MAX).saturating_mul(2);
        match map.get(at..at.saturating_add(2)) {
            Some(&[hi, lo]) => u32::from(u16::from_be_bytes([hi, lo])),
            _ => 0,
        }
    }

    /// `ch` typed as `cid`: its glyph, the width `/W` must give it, and
    /// whether `/ToUnicode` needs an entry; refused when the glyph is empty
    /// or the width or map conflicts.
    pub(crate) fn assess(
        &self,
        ch: char,
        cid: u32,
        glyphs: &dyn EmbeddedGlyphs,
    ) -> Result<AddedGlyph, String> {
        let gid = self.gid_of(cid);
        let glyph = glyphs
            .glyph_by_id(&self.program, gid, ch)
            .ok_or_else(|| "the embedded program has no outline for it".to_owned())?;
        let width = glyph.advance.round();
        let listed = self
            .ranges
            .iter()
            .find(|&&(lo, hi, _)| (lo..=hi).contains(&cid))
            .map(|&(_, _, w)| w);
        let widened = match listed {
            Some(w) if (w - width).abs() > 0.5 => {
                return Err(format!(
                    "the font's /W already gives CID {cid} width {w}, not the program's {width}"
                ));
            }
            Some(_) => false,
            None => (self.default_width - width).abs() > 0.5,
        };
        let mapped = self.to_unicode.needs_entry(ch, cid)?;
        let by_cmap = glyphs.unicode_glyph(&self.program, ch).map(|g| g.gid);
        let post_name = (mapped && by_cmap != Some(gid))
            .then(|| glyph_find::post_name_for(glyphs, &self.program, ch))
            .flatten();
        Ok(AddedGlyph {
            ch,
            code: cid,
            gid,
            width,
            widened,
            mapped,
            post_name,
        })
    }

    /// The descendant with a `cid [w]` entry appended to `/W` per widened CID.
    pub(crate) fn widened(&self, doc: &DocumentView<'_>, added: &[AddedGlyph]) -> Dict {
        let mut w = w_items(doc, &self.descendant);
        for a in added.iter().filter(|a| a.widened) {
            w.push(Object::Integer(i64::from(a.code)));
            w.push(Object::Array(vec![number(a.width)]));
        }
        let mut d = self.descendant.clone();
        d.insert(Name(b"W".to_vec()), Object::Array(w));
        d
    }

    /// The Type0 dictionary as the edit sees it: `descendant` inline.
    pub(crate) fn view(&self, descendant: Dict) -> Dict {
        let mut t = self.type0.clone();
        t.insert(
            Name(b"DescendantFonts".to_vec()),
            Object::Array(vec![Object::Dict(descendant)]),
        );
        t
    }

    /// Refuse what would change text shown elsewhere: a new width for a
    /// shown CID, or a new map entry for a CID shown without one.
    pub(crate) fn refuse_shown(
        &self,
        doc: &DocumentView<'_>,
        added: &[AddedGlyph],
    ) -> Result<Vec<Blocked>, String> {
        if !added.iter().any(|a| a.widened || a.mapped) {
            return Ok(Vec::new());
        }
        let shown = codes_shown(doc, self.type0_id)?;
        Ok(added
            .iter()
            .filter(|a| shown.contains(&a.code))
            .filter_map(|a| {
                let reason = if a.mapped {
                    unmapped_shown(a.code)
                } else if a.widened {
                    shown_elsewhere(a.code)
                } else {
                    return None;
                };
                Some(Blocked {
                    ch: a.ch,
                    code: a.code,
                    reason,
                })
            })
            .collect())
    }

    /// `Ok` when only this Type0 font reaches the descendant.
    pub(crate) fn descendant_private(&self, doc: &DocumentView<'_>) -> Result<(), String> {
        map_private(doc, self.type0_id, self.descendant_id)
            .map_err(|_| "the descendant CIDFont may be shared with another font".to_owned())
    }
}

/// `/CIDToGIDMap`: `None` for `/Identity` or absent (the default), the
/// decoded stream when `stream_map` allows one (§9.7.4.2 Table 117).
fn cid_to_gid(
    doc: &DocumentView<'_>,
    descendant: &Dict,
    stream_map: bool,
) -> Result<Option<Vec<u8>>, String> {
    match descendant.get(b"CIDToGIDMap").map(|o| doc.resolve(o)) {
        None => Ok(None),
        Some(o) if o.as_name().is_some_and(|n| n.as_bytes() == b"Identity") => Ok(None),
        Some(Object::Stream(s)) if stream_map => {
            let map = doc
                .slice(s.data_span)
                .and_then(|raw| crate::filters::decode_stream(&s.dict, raw).ok())
                .ok_or("the font's /CIDToGIDMap stream could not be read")?;
            if map.len() > MAX_MAP_BYTES {
                return Err("the font's /CIDToGIDMap is longer than 65536 CIDs".to_owned());
            }
            Ok(Some(map))
        }
        Some(_) => Err(STREAM_MAP.to_owned()),
    }
}

/// The refusal of a CID a show already uses with no `/ToUnicode` entry.
pub(crate) fn unmapped_shown(cid: u32) -> String {
    format!(
        "CID {cid} is already shown elsewhere without a /ToUnicode entry, so mapping it would \
         change that text"
    )
}

/// `/W`'s items, references resolved.
pub(crate) fn w_items(doc: &DocumentView<'_>, descendant: &Dict) -> Vec<Object> {
    doc.resolve(descendant.get(b"W").unwrap_or(&Object::Null))
        .as_array()
        .unwrap_or(&[])
        .iter()
        .map(|o| doc.resolve(o).clone())
        .collect()
}

/// §9.7.4.3's two forms, `c [w…]` and `c_first c_last w`, flattened.
fn w_ranges(items: &[Object]) -> Vec<(u32, u32, f64)> {
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(first) = items.get(i).and_then(Object::as_int) {
        let Ok(first) = u32::try_from(first) else {
            break;
        };
        match items.get(i + 1) {
            Some(Object::Array(list)) => {
                for (k, w) in (first..).zip(list) {
                    if let Some(w) = w.as_number() {
                        out.push((k, k, w));
                    }
                }
                i += 2;
            }
            Some(o) => {
                let last = o.as_int().and_then(|v| u32::try_from(v).ok());
                if let (Some(last), Some(w)) = (last, items.get(i + 2).and_then(Object::as_number))
                {
                    out.push((first, last, w));
                }
                i += 3;
            }
            None => break,
        }
    }
    out
}

/// Plan the extension for `missing` — `(character, CID)` pairs no show of
/// this font on the page uses.
///
/// # Errors
///
/// Every character that cannot be added, each with its reason; a reason that
/// belongs to the font as a whole is reported once, against the first.
pub(crate) fn plan(
    doc: &DocumentView<'_>,
    resources: &Dict,
    font_name: &[u8],
    type0: &Dict,
    missing: &[(char, u32)],
    glyphs: &dyn EmbeddedGlyphs,
) -> Result<FontExtension, Vec<Blocked>> {
    let first = missing.first().copied().unwrap_or((char::MIN, 0));
    let whole = |reason: String| {
        vec![Blocked {
            ch: first.0,
            code: first.1,
            reason,
        }]
    };
    let t = CidTarget::read(doc, resources, font_name, type0).map_err(whole)?;
    let (mut added, mut blocked) = (Vec::new(), Vec::new());
    for &(ch, cid) in missing {
        match t.assess(ch, cid, glyphs) {
            Ok(a) => added.push(a),
            Err(reason) => blocked.push(Blocked {
                ch,
                code: cid,
                reason,
            }),
        }
    }
    blocked.extend(t.refuse_shown(doc, &added).map_err(whole)?);
    if !blocked.is_empty() {
        blocked.sort_by_key(|b| missing.iter().position(|&(_, c)| c == b.code));
        return Err(blocked);
    }
    let widened = added.iter().any(|a| a.widened);
    if widened {
        t.descendant_private(doc).map_err(whole)?;
    }
    let descendant = if widened {
        t.widened(doc, &added)
    } else {
        t.descendant.clone()
    };
    let entries: Vec<(u32, char)> = added
        .iter()
        .filter(|a| a.mapped)
        .map(|a| (a.code, a.ch))
        .collect();
    let to_unicode = if entries.is_empty() {
        None
    } else {
        map_private(doc, t.type0_id, t.to_unicode.id).map_err(whole)?;
        let (d, bytes) = t.to_unicode.extended(&entries);
        Some((t.to_unicode.id, d, bytes))
    };
    Ok(FontExtension {
        font_id: t.descendant_id,
        view: Some(t.view(descendant.clone())),
        dict: descendant,
        added,
        to_unicode,
        reencoded: false,
        augmented: None,
    })
}

/// Which of `candidates` [`plan`] would add, each judged on its own.
pub(crate) fn addable(
    doc: &DocumentView<'_>,
    resources: &Dict,
    font_name: &[u8],
    type0: &Dict,
    candidates: &[(char, u32)],
    glyphs: &dyn EmbeddedGlyphs,
) -> BTreeSet<char> {
    let Ok(t) = CidTarget::read(doc, resources, font_name, type0) else {
        return BTreeSet::new();
    };
    let mut added: Vec<AddedGlyph> = candidates
        .iter()
        .filter_map(|&(ch, cid)| t.assess(ch, cid, glyphs).ok())
        .collect();
    if added.iter().any(|a| a.mapped) && map_private(doc, t.type0_id, t.to_unicode.id).is_err() {
        added.retain(|a| !a.mapped);
    }
    if added.iter().any(|a| a.widened) && t.descendant_private(doc).is_err() {
        added.retain(|a| !a.widened);
    }
    let Ok(refused) = t.refuse_shown(doc, &added) else {
        return added
            .iter()
            .filter(|a| !a.widened && !a.mapped)
            .map(|a| a.ch)
            .collect();
    };
    added
        .iter()
        .filter(|a| !refused.iter().any(|b| b.code == a.code))
        .map(|a| a.ch)
        .collect()
}

/// The font's `/ToUnicode` as it reads once each of `absent` is given, as
/// its CID, the glyph id the program's cmap reaches for it. `/Identity`
/// makes the two the same number.
///
/// # Errors
///
/// Every character that cannot be given a CID, with its reason.
pub(crate) fn allocate(
    doc: &DocumentView<'_>,
    resources: &Dict,
    font_name: &[u8],
    type0: &Dict,
    absent: &[char],
    glyphs: &dyn EmbeddedGlyphs,
) -> Result<ToUnicodeCMap, Vec<Blocked>> {
    let first = absent.first().copied().unwrap_or(char::MIN);
    let whole = |reason: String| {
        vec![Blocked {
            ch: first,
            code: 0,
            reason,
        }]
    };
    let t = CidTarget::read(doc, resources, font_name, type0).map_err(whole)?;
    let (mut cids, mut blocked) = (Vec::new(), Vec::new());
    for &ch in absent {
        let found = glyph_find::find(glyphs, &t.program, ch, None)
            .ok_or_else(|| "the embedded program has no outline for it".to_owned())
            .and_then(|f| {
                t.to_unicode
                    .needs_entry(ch, f.glyph.gid)
                    .map(|_| f.glyph.gid)
            });
        match found {
            Ok(cid) => cids.push((ch, cid)),
            Err(reason) => blocked.push(Blocked {
                ch,
                code: 0,
                reason,
            }),
        }
    }
    if !cids.is_empty() {
        let shown = codes_shown(doc, t.type0_id).map_err(whole)?;
        cids.retain(|&(ch, cid)| {
            let keep = !shown.contains(&cid);
            if !keep {
                blocked.push(Blocked {
                    ch,
                    code: cid,
                    reason: unmapped_shown(cid),
                });
            }
            keep
        });
    }
    if !blocked.is_empty() {
        blocked.sort_by_key(|b| absent.iter().position(|&c| c == b.ch));
        return Err(blocked);
    }
    let entries: Vec<(u32, char)> = cids.iter().map(|&(ch, cid)| (cid, ch)).collect();
    Ok(t.to_unicode.with_entries(&entries))
}

/// The characters [`allocate`] would give a CID that [`plan`] then adds.
pub(crate) fn allocatable(
    doc: &DocumentView<'_>,
    resources: &Dict,
    font_name: &[u8],
    type0: &Dict,
    glyphs: &dyn EmbeddedGlyphs,
) -> BTreeSet<char> {
    let Ok(t) = CidTarget::read(doc, resources, font_name, type0) else {
        return BTreeSet::new();
    };
    let mut chars = glyphs.unicode_chars(&t.program);
    chars.extend(glyph_find::named_chars(glyphs, &t.program));
    chars.sort_unstable();
    chars.dedup();
    let candidates: Vec<(char, u32)> = chars
        .into_iter()
        .filter_map(|ch| {
            Some((
                ch,
                glyph_find::find(glyphs, &t.program, ch, None)?.glyph.gid,
            ))
        })
        .filter(|&(ch, cid)| t.to_unicode.needs_entry(ch, cid) == Ok(true))
        .collect();
    addable(doc, resources, font_name, type0, &candidates, glyphs)
}
