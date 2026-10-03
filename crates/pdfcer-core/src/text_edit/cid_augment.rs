//! Decision 173 for a composite font: a `/Type0` font with `/Identity-H`
//! over a `CIDFontType2` subset, extended with glyphs from an installed face.
//!
//! [`begin`] asks the augmenter for the subset with the glyphs appended, in
//! CID mode (the program's `cmap` is not consulted; ISO 32000-2 §9.9,
//! Table 126), and gives each new character a CID: its glyph id under an
//! `/Identity` `/CIDToGIDMap`, else a CID the map leaves at glyph 0 that no
//! `/ToUnicode` entry and no show uses, with the map extended to send it to
//! the new glyph (§9.7.4.2, Table 117). [`finish`] then writes the result
//! copy-on-write: a new `FontFile2`, `FontDescriptor`, map and `/CIDSet`
//! (§9.8, Table 124) under a fresh subset tag, the descendant (with `/W`,
//! §9.7.4.3) and the Type0's `/BaseFont` re-tagged in place, and the
//! `/ToUnicode` extended in place (§9.10.3). Old objects other fonts reach
//! stay as they are.

use std::collections::BTreeSet;

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::text_edit::augment_route::{
    Augmented, FRESH_CIDSET, FRESH_DESCRIPTOR, FRESH_MAP, FontAt, FreshStream, renamed, written,
};
use crate::text_edit::cid_extend::{CFF_CID, CidTarget, STREAM_MAP, unmapped_shown};
use crate::text_edit::font_extend::{
    AddedGlyph, Blocked, FontExtension, NO_OUTLINE, codes_shown, map_private,
};
use crate::text_edit::glyph_find;
use crate::text_edit::program_glyphs::EmbeddedGlyphs;
use crate::text_edit::same_program::{self, CidFontProgram};
use crate::text_edit::subset_augment::{
    AugmentRequest, AugmentedProgram, ProgramAddressing, SubsetAugment,
};
use crate::text_extract::cmap::ToUnicodeCMap;
use crate::view::DocumentView;

/// The refusal of a CFF-based descendant.
const CFF_REFUSED: &str = "the descendant is a CFF-based CIDFontType0, and only a TrueType \
                           program (CIDFontType2) can have glyphs appended";

/// The largest CID an `/Identity-H` code can carry.
const MAX_CID: u32 = 0xFFFF;

/// Whether `blocked`, the CID allocator's refusals, are all ones that
/// appending glyphs could cure.
pub(crate) fn curable(blocked: &[Blocked]) -> bool {
    !blocked.is_empty()
        && blocked
            .iter()
            .all(|b| [NO_OUTLINE, STREAM_MAP, CFF_CID].contains(&b.reason.as_str()))
}

/// The augmented program and the CIDs [`begin`] chose, carried to [`finish`].
#[derive(Debug, Clone)]
pub(crate) struct Begun {
    /// The `/ToUnicode` as it reads with the new CIDs, to encode against.
    pub(crate) cmap: ToUnicodeCMap,
    program: AugmentedProgram,
    /// The extended `/CIDToGIDMap`; `None` under `/Identity`.
    map: Option<Vec<u8>>,
    /// What decision 187 did with the program's `cmap`, for the disclosure.
    cmap_note: Option<&'static str>,
}

/// Augment the subset for `absent` (characters the font's `/ToUnicode`
/// does not produce) and give each a CID.
///
/// # Errors
///
/// Why the font, or one of the characters, cannot be augmented.
pub(crate) fn begin(
    doc: &DocumentView<'_>,
    at: &FontAt<'_>,
    absent: &[char],
    glyphs: &dyn EmbeddedGlyphs,
    settings: &SubsetAugment,
    mode: CidFontProgram,
) -> Result<Begun, String> {
    let t = target(doc, at)?;
    let (mut gids, mut append) = (Vec::new(), Vec::new());
    for &ch in absent {
        match glyph_find::find(glyphs, &t.program, ch, None) {
            Some(f) => gids.push((ch, f.glyph.gid)),
            None => append.push(ch),
        }
    }
    if append.is_empty() {
        return Err("every character already has a glyph in the program".to_owned());
    }
    let shown = codes_shown(doc, t.type0_id)?;
    let shown_chars = shown_chars(&t, &shown);
    let known = known_pairs(&t);
    let base_font = base_font(doc, &t.descendant).ok_or("the descendant has no /BaseFont")?;
    let request = AugmentRequest {
        program: &t.program,
        base_font: &base_font,
        chars: &append,
        shown: &shown_chars,
        outline_check: settings.outline_check,
        hinting_mismatch: settings.hinting_mismatch,
        addressing: ProgramAddressing::CidKeyed(&known),
    };
    let mut program = settings.source.augment(&request).map_err(|r| r.reason)?;
    let cmap_note = same_program::augmented_cmap(doc, glyphs, mode, &mut program.program);
    if program.program.len() > crate::font_embed_missing::MAX_DONOR_BYTES {
        return Err("the new program exceeds the embedded-font size limit".to_owned());
    }
    for &ch in &append {
        let gid = program
            .glyph_ids
            .iter()
            .find(|&&(c, _)| c == ch)
            .map(|&(_, g)| g)
            .ok_or_else(|| format!("the augmenter did not append '{ch}'"))?;
        gids.push((ch, gid));
    }
    let (entries, map) = assign_cids(&t, &gids, &shown)?;
    Ok(Begun {
        cmap: t.to_unicode.with_entries(&entries),
        program,
        map,
        cmap_note,
    })
}

/// The font, read with a `/CIDToGIDMap` stream allowed, refused unless
/// both the descendant and the `/ToUnicode` are its own.
fn target(doc: &DocumentView<'_>, at: &FontAt<'_>) -> Result<CidTarget, String> {
    let t = CidTarget::read_with(doc, (at.resources, at.font_name), at.font_dict, true).map_err(
        |e| {
            if e == CFF_CID {
                CFF_REFUSED.to_owned()
            } else {
                e
            }
        },
    )?;
    t.descendant_private(doc)?;
    map_private(doc, t.type0_id, t.to_unicode.id)?;
    Ok(t)
}

/// The characters the document shows in this font, read through `/ToUnicode`.
fn shown_chars(t: &CidTarget, shown: &BTreeSet<u32>) -> Vec<char> {
    let mut out: Vec<char> = shown
        .iter()
        .filter_map(|&cid| single_char(&t.to_unicode.cmap.lookup(cid)?))
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

fn single_char(s: &str) -> Option<char> {
    let mut it = s.chars();
    let ch = it.next()?;
    it.next().is_none().then_some(ch)
}

/// `(glyph id, character)` for every character `/ToUnicode` gives exactly
/// one CID whose glyph is not `.notdef`.
fn known_pairs(t: &CidTarget) -> Vec<(u32, char)> {
    let Ok(inverse) = t.to_unicode.cmap.partial_inverse() else {
        return Vec::new();
    };
    inverse
        .unambiguous
        .iter()
        .map(|(&ch, &cid)| (t.gid_of(cid), ch))
        .filter(|&(gid, _)| gid != 0)
        .collect()
}

fn base_font(doc: &DocumentView<'_>, d: &Dict) -> Option<String> {
    let n = doc.resolve(d.get(b"BaseFont")?).as_name()?;
    String::from_utf8(n.as_bytes().to_vec()).ok()
}

/// New `(CID, character)` `/ToUnicode` entries, and the extended
/// `/CIDToGIDMap` when the font has a stream one.
type Assigned = (Vec<(u32, char)>, Option<Vec<u8>>);

/// A CID per `(character, glyph id)`.
fn assign_cids(
    t: &CidTarget,
    gids: &[(char, u32)],
    shown: &BTreeSet<u32>,
) -> Result<Assigned, String> {
    let mut map = t.map.clone();
    let mut entries: Vec<(u32, char)> = Vec::new();
    for &(ch, gid) in gids {
        let cid = match &mut map {
            None => identity_cid(t, ch, gid, shown)?,
            Some(map) => {
                let taken = |c: u32| entries.iter().any(|&(e, _)| e == c);
                let cid = free_cid(t, map, gid, shown, taken)
                    .ok_or("no unused CID is left for it under the font's /CIDToGIDMap")?;
                set_gid(map, cid, gid)?;
                cid
            }
        };
        entries.push((cid, ch));
    }
    Ok((entries, map))
}

/// Under `/Identity` a glyph has exactly one CID: its own id.
fn identity_cid(t: &CidTarget, ch: char, gid: u32, shown: &BTreeSet<u32>) -> Result<u32, String> {
    if gid > MAX_CID {
        return Err(format!("glyph {gid} is beyond the largest two-byte CID"));
    }
    if shown.contains(&gid) {
        return Err(unmapped_shown(gid));
    }
    t.to_unicode.needs_entry(ch, gid)?;
    Ok(gid)
}

/// The first CID from `gid` upward, then from 1, that the map leaves at
/// glyph 0 and nothing reads or shows.
fn free_cid(
    t: &CidTarget,
    map: &[u8],
    gid: u32,
    shown: &BTreeSet<u32>,
    taken: impl Fn(u32) -> bool,
) -> Option<u32> {
    let unused = |cid: u32| {
        let at = (cid as usize) * 2;
        let mapped = matches!(map.get(at..at + 2), Some(&[hi, lo]) if hi != 0 || lo != 0);
        !mapped && !shown.contains(&cid) && t.to_unicode.cmap.lookup(cid).is_none() && !taken(cid)
    };
    (gid.clamp(1, MAX_CID)..=MAX_CID)
        .chain(1..gid.min(MAX_CID))
        .find(|&cid| unused(cid))
}

/// Write `gid` as `cid`'s big-endian entry, growing the map with zeros.
fn set_gid(map: &mut Vec<u8>, cid: u32, gid: u32) -> Result<(), String> {
    let gid = u16::try_from(gid).map_err(|_| format!("glyph {gid} is beyond a two-byte map"))?;
    let at = (cid as usize) * 2;
    if map.len() < at + 2 {
        map.resize(at + 2, 0);
    }
    if let Some(slot) = map.get_mut(at..at + 2) {
        slot.copy_from_slice(&gid.to_be_bytes());
    }
    Ok(())
}

/// The extension that writes [`begin`]'s program for `missing` — the
/// `(character, CID)` pairs the encoded replacement uses that no show of
/// this font on the page carries.
///
/// # Errors
///
/// Every character that cannot be added, each with its reason.
pub(crate) fn finish(
    doc: &DocumentView<'_>,
    at: &FontAt<'_>,
    missing: &[(char, u32)],
    glyphs: &dyn EmbeddedGlyphs,
    begun: Begun,
) -> Result<FontExtension, Vec<Blocked>> {
    let first = missing.first().copied().unwrap_or((char::MIN, 0));
    let whole = |reason: String| {
        vec![Blocked {
            ch: first.0,
            code: first.1,
            reason: format!(
                "{NO_OUTLINE}, and it cannot be added from an installed font: {reason}"
            ),
        }]
    };
    let mut t = target(doc, at).map_err(whole)?;
    let original = t.descendant.clone();
    t.program.clone_from(&begun.program.program);
    let stream_map = t.map.is_some();
    t.map.clone_from(&begun.map);
    let cmap_note = begun.cmap_note;
    let added = assessed(doc, &t, missing, glyphs)?;
    let mut ext = extension(doc, &t, added);
    let old_name = base_font(doc, &original).ok_or_else(|| whole("no /BaseFont".to_owned()))?;
    let new_name = renamed(doc, &old_name, &ext);
    let names = (old_name.as_str(), new_name.as_str());
    let mut augmented =
        written(doc, &original, &ext, &begun.program, names, "CID").map_err(&whole)?;
    cid_set(doc, &mut augmented, &ext.added).map_err(&whole)?;
    if let Some(note) = cmap_note {
        augmented.disclosure.push_str(note);
    }
    if stream_map {
        map_stream(&original, &mut augmented, begun.map.unwrap_or_default()).map_err(&whole)?;
        ext.dict
            .insert(name(b"CIDToGIDMap"), Object::Reference(FRESH_MAP));
        augmented
            .disclosure
            .push_str(" The font's /CIDToGIDMap was extended to send each new CID to its glyph.");
    }
    ext.dict.insert(name(b"BaseFont"), name_obj(&new_name));
    ext.dict
        .insert(name(b"FontDescriptor"), Object::Reference(FRESH_DESCRIPTOR));
    let type0 = retagged(doc, &t, names);
    if let Some(type0) = &type0 {
        augmented
            .objects
            .push((t.type0_id, Object::Dict(type0.clone())));
    }
    let mut descendant = ext.dict.clone();
    descendant.insert(
        name(b"FontDescriptor"),
        Object::Dict(augmented.descriptor.clone()),
    );
    let mut view = t.view(descendant);
    if let Some(type0) = type0 {
        view.insert(
            name(b"BaseFont"),
            type0.get(b"BaseFont").cloned().unwrap_or(Object::Null),
        );
    }
    ext.view = Some(view);
    ext.augmented = Some(Box::new(augmented));
    Ok(ext)
}

/// Each of `missing` judged against the augmented program and map.
fn assessed(
    doc: &DocumentView<'_>,
    t: &CidTarget,
    missing: &[(char, u32)],
    glyphs: &dyn EmbeddedGlyphs,
) -> Result<Vec<AddedGlyph>, Vec<Blocked>> {
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
    match t.refuse_shown(doc, &added) {
        Ok(refused) => blocked.extend(refused),
        Err(reason) => {
            blocked.extend(
                missing
                    .first()
                    .map(|&(ch, code)| Blocked { ch, code, reason }),
            )
        }
    }
    if blocked.is_empty() {
        Ok(added)
    } else {
        blocked.sort_by_key(|b| missing.iter().position(|&(_, c)| c == b.code));
        Err(blocked)
    }
}

/// The descendant with `/W` widened and the `/ToUnicode` extended.
fn extension(doc: &DocumentView<'_>, t: &CidTarget, added: Vec<AddedGlyph>) -> FontExtension {
    let entries: Vec<(u32, char)> = added
        .iter()
        .filter(|a| a.mapped)
        .map(|a| (a.code, a.ch))
        .collect();
    let to_unicode = (!entries.is_empty()).then(|| {
        let (d, bytes) = t.to_unicode.extended(&entries);
        (t.to_unicode.id, d, bytes)
    });
    FontExtension {
        font_id: t.descendant_id,
        dict: t.widened(doc, &added),
        added,
        to_unicode,
        reencoded: false,
        view: None,
        augmented: None,
    }
}

fn name(key: &[u8]) -> Name {
    Name(key.to_vec())
}

fn name_obj(s: &str) -> Object {
    Object::Name(Name(s.as_bytes().to_vec()))
}

/// The Type0 dictionary with `/BaseFont` re-tagged, when it begins with the
/// descendant's old name (ISO 32000-2 §9.7.6.1, Table 121: for a
/// `CIDFontType2` descendant it is the descendant's `/BaseFont`, optionally
/// followed by a hyphen and the CMap name).
fn retagged(doc: &DocumentView<'_>, t: &CidTarget, (old, new): (&str, &str)) -> Option<Dict> {
    let current = base_font(doc, &t.type0)?;
    let rest = current.strip_prefix(old)?;
    let mut type0 = t.type0.clone();
    type0.insert(name(b"BaseFont"), name_obj(&format!("{new}{rest}")));
    Some(type0)
}

/// A new `/CIDSet` with each added CID's bit set (§9.8, Table 124: bit
/// `cid`, high bit of byte 0 first), when the old descriptor has one.
fn cid_set(
    doc: &DocumentView<'_>,
    augmented: &mut Augmented,
    added: &[AddedGlyph],
) -> Result<(), String> {
    let Some(old) = augmented.descriptor.get(b"CIDSet").cloned() else {
        return Ok(());
    };
    let unreadable = || "the font's /CIDSet could not be read".to_owned();
    let Object::Stream(s) = doc.resolve(&old) else {
        return Err(unreadable());
    };
    let mut bits = doc
        .slice(s.data_span)
        .and_then(|raw| crate::filters::decode_stream(&s.dict, raw).ok())
        .ok_or_else(unreadable)?;
    for a in added {
        let byte = (a.code >> 3) as usize;
        if bits.len() <= byte {
            bits.resize(byte + 1, 0);
        }
        if let Some(b) = bits.get_mut(byte) {
            *b |= 0x80 >> (a.code & 7);
        }
    }
    augmented
        .fresh_streams
        .push(flate_stream(FRESH_CIDSET, old.as_reference(), &bits)?);
    augmented
        .descriptor
        .insert(name(b"CIDSet"), Object::Reference(FRESH_CIDSET));
    Ok(())
}

/// The extended `/CIDToGIDMap` as a new stream.
fn map_stream(descendant: &Dict, augmented: &mut Augmented, map: Vec<u8>) -> Result<(), String> {
    let old = descendant
        .get(b"CIDToGIDMap")
        .and_then(Object::as_reference);
    augmented
        .fresh_streams
        .push(flate_stream(FRESH_MAP, old, &map)?);
    augmented.map = Some(map);
    Ok(())
}

fn flate_stream(id: ObjId, superseded: Option<ObjId>, data: &[u8]) -> Result<FreshStream, String> {
    let bytes = crate::image_import::flate_encode(data)
        .map_err(|_| "a new stream could not be compressed".to_owned())?;
    let mut dict = Dict::new();
    dict.insert(name(b"Filter"), Object::Name(name(b"FlateDecode")));
    let len = i64::try_from(bytes.len()).unwrap_or(i64::MAX);
    dict.insert(name(b"Length"), Object::Integer(len));
    Ok(FreshStream {
        id,
        superseded,
        dict,
        bytes,
    })
}

/// The characters an edit could add to this font by augmentation, for a
/// typing repertoire: the augmenter's face's characters that `excluded`
/// does not already answer and the program lacks, as the augmenter judges
/// them. The commit can still refuse one (no free CID, a shown CID).
pub(crate) fn augmentable(
    doc: &DocumentView<'_>,
    at: &FontAt<'_>,
    glyphs: &dyn EmbeddedGlyphs,
    settings: &SubsetAugment,
    excluded: impl Fn(char) -> bool,
) -> BTreeSet<char> {
    let Ok(t) = target(doc, at) else {
        return BTreeSet::new();
    };
    let (Ok(shown), Some(base_font)) =
        (codes_shown(doc, t.type0_id), base_font(doc, &t.descendant))
    else {
        return BTreeSet::new();
    };
    let shown_chars = shown_chars(&t, &shown);
    let known = known_pairs(&t);
    let mut request = AugmentRequest {
        program: &t.program,
        base_font: &base_font,
        chars: &[],
        shown: &shown_chars,
        outline_check: settings.outline_check,
        hinting_mismatch: settings.hinting_mismatch,
        addressing: ProgramAddressing::CidKeyed(&known),
    };
    let chars: Vec<char> = settings
        .source
        .candidates(&request)
        .into_iter()
        .filter(|&ch| !excluded(ch) && glyph_find::find(glyphs, &t.program, ch, None).is_none())
        .collect();
    if chars.is_empty() {
        return BTreeSet::new();
    }
    request.chars = &chars;
    settings.source.addable(&request).into_iter().collect()
}
