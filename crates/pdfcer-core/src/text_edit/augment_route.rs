//! Decision 173 in core: when route A (decision 172) finds no outline for a
//! character, ask the installed [`SubsetAugmenter`] for the subset with the
//! glyphs appended, and plan route A again against that program.
//!
//! The new program is written as a new `FontFile2` stream under a new,
//! copy-on-write `FontDescriptor`; the font dictionary's `/BaseFont` and the
//! descriptor's `/FontName` take a fresh subset tag together (ISO 32000-2
//! §9.6.4, ST3/ST4). The old descriptor and stream stay as they are for any
//! other dictionary that reaches them.
//!
//! [`SubsetAugmenter`]: crate::text_edit::subset_augment::SubsetAugmenter

use std::collections::BTreeSet;

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::text_edit::font_extend::{
    Blocked, FontExtension, NO_OUTLINE, Target, codes_shown, number, plan_target,
};
use crate::text_edit::glyph_find;
use crate::text_edit::program_glyphs::EmbeddedGlyphs;
use crate::text_edit::subset_augment::{
    AugmentRequest, AugmentedProgram, ProgramAddressing, SubsetAugment,
};
use crate::view::DocumentView;

/// Stands for the new descriptor until the writer assigns it a number.
pub(crate) const FRESH_DESCRIPTOR: ObjId = ObjId::new(u32::MAX, 0);
/// Stands for the new `FontFile2` stream until the writer assigns it a number.
pub(crate) const FRESH_PROGRAM: ObjId = ObjId::new(u32::MAX - 1, 0);
/// Stands for a composite font's new `/CIDToGIDMap` stream.
pub(crate) const FRESH_MAP: ObjId = ObjId::new(u32::MAX - 2, 0);
/// Stands for a composite font's new `/CIDSet` stream.
pub(crate) const FRESH_CIDSET: ObjId = ObjId::new(u32::MAX - 3, 0);

/// The new program and descriptor an augmented extension writes.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Augmented {
    /// The new descriptor; `/FontFile2` refers to [`FRESH_PROGRAM`].
    pub(crate) descriptor: Dict,
    /// The stream dictionary, `/Filter /FlateDecode` with `/Length1`.
    pub(crate) stream_dict: Dict,
    /// The Flate-encoded program.
    pub(crate) encoded: Vec<u8>,
    /// The program, decoded, for a preview to draw from.
    pub(crate) program: Vec<u8>,
    /// The descriptor and program stream this one replaces for the edited
    /// font, when they are separate objects.
    pub(crate) superseded: [Option<ObjId>; 2],
    /// Further placeholder streams (a composite font's map and `/CIDSet`).
    pub(crate) fresh_streams: Vec<FreshStream>,
    /// Further objects revised in place (a composite font's Type0 dictionary).
    pub(crate) objects: Vec<(ObjId, Object)>,
    /// The new `/CIDToGIDMap`, decoded, for a preview to draw from.
    pub(crate) map: Option<Vec<u8>>,
    /// The rule 4 disclosure.
    pub(crate) disclosure: String,
}

/// A new stream written under placeholder `id`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FreshStream {
    pub(crate) id: ObjId,
    /// The stream it replaces for the edited font.
    pub(crate) superseded: Option<ObjId>,
    pub(crate) dict: Dict,
    /// The bytes as stored, filtered per `dict`.
    pub(crate) bytes: Vec<u8>,
}

/// The font an edit's run selects.
#[derive(Clone, Copy)]
pub(crate) struct FontAt<'a> {
    pub(crate) resources: &'a Dict,
    pub(crate) font_name: &'a [u8],
    pub(crate) font_dict: &'a Dict,
}

/// Route A over the augmented subset, for `blocked` — route A's refusals.
/// Returns `blocked` unchanged unless every refusal is a missing outline.
///
/// # Errors
///
/// The refusals, each reason extended with why augmentation failed.
pub(crate) fn plan(
    doc: &DocumentView<'_>,
    font: &FontAt<'_>,
    missing: &[(char, u32)],
    glyphs: &dyn EmbeddedGlyphs,
    settings: &SubsetAugment,
    blocked: Vec<Blocked>,
) -> Result<FontExtension, Vec<Blocked>> {
    let FontAt {
        resources,
        font_name,
        font_dict,
    } = *font;
    if blocked.is_empty() || blocked.iter().any(|b| b.reason != NO_OUTLINE) {
        return Err(blocked);
    }
    if crate::text_edit::cid_extend::is_composite(doc, font_dict) {
        // A composite font reaches here only when /ToUnicode already gives
        // the character a CID whose glyph is empty; a new glyph would need
        // that CID re-pointed, which would change text shown with it.
        return Err(blocked
            .iter()
            .flat_map(|b| {
                let why = format!(
                    "the font's /ToUnicode already gives it CID {}, whose glyph is empty",
                    b.code
                );
                refused(std::slice::from_ref(b), &why)
            })
            .collect());
    }
    let refuse = |why: &str| refused(&blocked, why);
    let mut t = Target::read(doc, resources, font_name, font_dict).map_err(|e| refuse(&e))?;
    let chars: Vec<char> = blocked.iter().map(|b| b.ch).collect();
    let shown = shown_chars(doc, &t).map_err(|e| refuse(&e))?;
    if let Some(&(code, ch)) = shown.iter().find(|(_, ch)| chars.contains(ch)) {
        return Err(refuse(&format!(
            "code {code} already shows '{ch}' as a missing glyph elsewhere, and would change"
        )));
    }
    let base_font = name_of(doc, font_dict, b"BaseFont").ok_or_else(|| refuse("no /BaseFont"))?;
    let shown_list: Vec<char> = shown.iter().map(|&(_, ch)| ch).collect();
    let request = AugmentRequest {
        program: &t.shape.program,
        base_font: &base_font,
        chars: &chars,
        shown: &shown_list,
        outline_check: settings.outline_check,
        hinting_mismatch: settings.hinting_mismatch,
        addressing: ProgramAddressing::Cmap,
    };
    let program = settings
        .source
        .augment(&request)
        .map_err(|r| refuse(&r.reason))?;
    if program.program.len() > crate::font_embed_missing::MAX_DONOR_BYTES {
        return Err(refuse(
            "the new program exceeds the embedded-font size limit",
        ));
    }
    t.shape.program.clone_from(&program.program);
    let mut ext = plan_target(doc, &t, font_dict, missing, glyphs)?;
    let new_name = renamed(doc, &base_font, &ext);
    let names = (base_font.as_str(), new_name.as_str());
    let augmented =
        written(doc, font_dict, &ext, &program, names, "code").map_err(|e| refuse(&e))?;
    attach(&mut ext, augmented, &new_name);
    Ok(ext)
}

/// Point the simple font's dictionary at the new name and descriptor; its
/// view carries the descriptor inline, for a preview to read.
fn attach(ext: &mut FontExtension, augmented: Augmented, new_name: &str) {
    ext.dict.insert(
        Name(b"BaseFont".to_vec()),
        Object::Name(Name(new_name.as_bytes().to_vec())),
    );
    ext.dict.insert(
        Name(b"FontDescriptor".to_vec()),
        Object::Reference(FRESH_DESCRIPTOR),
    );
    let mut view = ext.dict.clone();
    view.insert(
        Name(b"FontDescriptor".to_vec()),
        Object::Dict(augmented.descriptor.clone()),
    );
    ext.view = Some(view);
    ext.augmented = Some(Box::new(augmented));
}

/// `blocked`, each reason extended with why augmentation failed.
fn refused(blocked: &[Blocked], why: &str) -> Vec<Blocked> {
    blocked
        .iter()
        .map(|b| Blocked {
            reason: format!("{NO_OUTLINE}, and it cannot be added from an installed font: {why}"),
            ..b.clone()
        })
        .collect()
}

/// The characters of `candidates` (route A's uncarried `(char, code)`s) an
/// edit could add by augmentation: those the augmenter offers, confirmed by
/// planning the augmented edit, so a typing repertoire and the commit agree.
pub(crate) fn augmentable(
    doc: &DocumentView<'_>,
    font: &FontAt<'_>,
    candidates: &[(char, u32)],
    glyphs: &dyn EmbeddedGlyphs,
    settings: &SubsetAugment,
) -> BTreeSet<char> {
    let offered = offered(doc, font, candidates, settings);
    let missing: Vec<(char, u32)> = candidates
        .iter()
        .copied()
        .filter(|(ch, _)| offered.contains(ch))
        .collect();
    let blocked = missing
        .iter()
        .map(|&(ch, code)| Blocked {
            ch,
            code,
            reason: NO_OUTLINE.to_owned(),
        })
        .collect();
    match plan(doc, font, &missing, glyphs, settings, blocked) {
        Ok(_) => offered,
        Err(_) => BTreeSet::new(),
    }
}

/// What the augmenter says it can append, excluding characters the font
/// already shows (as missing glyphs) elsewhere, which [`plan`] refuses.
fn offered(
    doc: &DocumentView<'_>,
    font: &FontAt<'_>,
    candidates: &[(char, u32)],
    settings: &SubsetAugment,
) -> BTreeSet<char> {
    let Ok(t) = Target::read(doc, font.resources, font.font_name, font.font_dict) else {
        return BTreeSet::new();
    };
    let (Ok(shown), Some(base_font)) = (
        shown_chars(doc, &t),
        name_of(doc, font.font_dict, b"BaseFont"),
    ) else {
        return BTreeSet::new();
    };
    let shown_list: Vec<char> = shown.iter().map(|&(_, ch)| ch).collect();
    let chars: Vec<char> = candidates
        .iter()
        .map(|&(ch, _)| ch)
        .filter(|ch| !shown_list.contains(ch))
        .collect();
    if chars.is_empty() {
        return BTreeSet::new();
    }
    let request = AugmentRequest {
        program: &t.shape.program,
        base_font: &base_font,
        chars: &chars,
        shown: &shown_list,
        outline_check: settings.outline_check,
        hinting_mismatch: settings.hinting_mismatch,
        addressing: ProgramAddressing::Cmap,
    };
    settings.source.addable(&request).into_iter().collect()
}

/// Every `(code, character)` the document shows in the target font.
fn shown_chars(doc: &DocumentView<'_>, t: &Target) -> Result<BTreeSet<(u32, char)>, String> {
    Ok(codes_shown(doc, t.font_id)?
        .into_iter()
        .filter_map(|code| {
            let name = glyph_find::encoded_name(&t.shape.encoding, code)?;
            Some((code, pdfcer_fonts::fontdata::glyph_name_to_unicode(&name)?))
        })
        .collect())
}

fn name_of(doc: &DocumentView<'_>, d: &Dict, key: &[u8]) -> Option<String> {
    let n = doc.resolve(d.get(key)?).as_name()?;
    String::from_utf8(n.as_bytes().to_vec()).ok()
}

/// `base_font` under a new subset tag: deterministic over the stripped name,
/// the old tag and the appended glyphs, and used by no font in the file.
pub(crate) fn renamed(doc: &DocumentView<'_>, base_font: &str, ext: &FontExtension) -> String {
    let (old_tag, stripped) = pdfcer_fonts::fontinfo::split_subset_tag(base_font);
    let mut seed = format!("{stripped}/{}", old_tag.unwrap_or_default());
    for a in &ext.added {
        seed.push_str(&format!("/{}", a.gid));
    }
    let taken: BTreeSet<String> = crate::fontinfo::inventory(doc)
        .fonts
        .into_iter()
        .filter_map(|f| f.subset_tag)
        .collect();
    let tag = crate::text_edit::addtext::unique_subset_tag(&seed, &taken);
    format!("{tag}+{stripped}")
}

/// The new descriptor and stream, and the disclosure, which calls a code
/// `unit` ("code" or "CID").
pub(crate) fn written(
    doc: &DocumentView<'_>,
    font_dict: &Dict,
    ext: &FontExtension,
    program: &AugmentedProgram,
    (old_name, new_name): (&str, &str),
    unit: &str,
) -> Result<Augmented, String> {
    let old_ref = font_dict
        .get(b"FontDescriptor")
        .and_then(Object::as_reference);
    let old = doc
        .resolve(font_dict.get(b"FontDescriptor").unwrap_or(&Object::Null))
        .as_dict()
        .ok_or("the font has no descriptor")?;
    let old_program = old.get(b"FontFile2").and_then(Object::as_reference);
    let mut descriptor = old.clone();
    let scale = 1000.0 / f64::from(program.units_per_em.max(1));
    let bbox = widened_bbox(doc, old, program.bbox, scale);
    descriptor.insert(
        Name(b"FontBBox".to_vec()),
        Object::Array(bbox.iter().map(|&v| Object::Integer(v)).collect()),
    );
    let widest = ext.added.iter().map(|a| a.width).fold(0.0, f64::max);
    let max_width = doc
        .resolve(old.get(b"MaxWidth").unwrap_or(&Object::Null))
        .as_number();
    if max_width.is_some_and(|m| widest > m) {
        descriptor.insert(Name(b"MaxWidth".to_vec()), number(widest));
    }
    descriptor.insert(
        Name(b"FontName".to_vec()),
        Object::Name(Name(new_name.as_bytes().to_vec())),
    );
    descriptor.insert(
        Name(b"FontFile2".to_vec()),
        Object::Reference(FRESH_PROGRAM),
    );
    let encoded = crate::image_import::flate_encode(&program.program)
        .map_err(|_| "the new program could not be compressed".to_owned())?;
    let mut stream_dict = Dict::new();
    let int = |n: usize| Object::Integer(i64::try_from(n).unwrap_or(i64::MAX));
    stream_dict.insert(
        Name(b"Filter".to_vec()),
        Object::Name(Name(b"FlateDecode".to_vec())),
    );
    stream_dict.insert(Name(b"Length".to_vec()), int(encoded.len()));
    stream_dict.insert(Name(b"Length1".to_vec()), int(program.program.len()));
    Ok(Augmented {
        program: program.program.clone(),
        descriptor,
        stream_dict,
        encoded,
        superseded: [old_ref, old_program],
        fresh_streams: Vec::new(),
        objects: Vec::new(),
        map: None,
        disclosure: disclosure(ext, program, (old_name, new_name), unit),
    })
}

/// The old `/FontBBox` grown to cover the program's `head` box, glyph space.
fn widened_bbox(doc: &DocumentView<'_>, old: &Dict, head: [i16; 4], scale: f64) -> [i64; 4] {
    let mut out: Vec<f64> = doc
        .resolve(old.get(b"FontBBox").unwrap_or(&Object::Null))
        .as_array()
        .unwrap_or(&[])
        .iter()
        .filter_map(|o| doc.resolve(o).as_number())
        .collect();
    out.resize(4, 0.0);
    let h: Vec<f64> = head.iter().map(|&v| f64::from(v) * scale).collect();
    let pick = |i: usize, f: fn(f64, f64) -> f64| {
        f(
            out.get(i).copied().unwrap_or(0.0),
            h.get(i).copied().unwrap_or(0.0),
        )
    };
    #[allow(
        clippy::cast_possible_truncation,
        reason = "font-unit boxes scaled to glyph space are far inside i64"
    )]
    let r = |v: f64| v.round() as i64;
    [
        r(pick(0, f64::min).floor()),
        r(pick(1, f64::min).floor()),
        r(pick(2, f64::max).ceil()),
        r(pick(3, f64::max).ceil()),
    ]
}

fn disclosure(
    ext: &FontExtension,
    program: &AugmentedProgram,
    (old_name, new_name): (&str, &str),
    unit: &str,
) -> String {
    let glyphs: Vec<String> = ext
        .added
        .iter()
        .map(|a| {
            format!(
                "'{}' U+{:04X} -> {unit} {}, glyph {}, width {}",
                a.ch,
                u32::from(a.ch),
                a.code,
                a.gid,
                a.width
            )
        })
        .collect();
    let hinting = if program.instructions_stripped {
        "hinting stripped from the added glyphs (the face's hinting programs differ)"
    } else {
        "hinting kept"
    };
    format!(
        "font: added {} glyph(s) to embedded subset '{old_name}' (now '{new_name}') from installed \
         font '{}'. Identified as the same font (inference): {}. {}; {hinting}.",
        ext.added.len(),
        program.face_label,
        program.evidence,
        glyphs.join("; ")
    )
}

/// `writes`' references to the placeholder ids, renumbered by `assign`.
pub(crate) fn renumber(objects: &mut [(ObjId, Object)], assign: &[(ObjId, ObjId)]) {
    let map = |id: ObjId| {
        assign
            .iter()
            .find(|(p, _)| *p == id)
            .map_or(id, |&(_, n)| n)
    };
    for (id, obj) in objects {
        *id = map(*id);
        let dict = match obj {
            Object::Dict(d) => d,
            Object::Stream(s) => &mut s.dict,
            _ => continue,
        };
        let moved: Vec<(Name, ObjId)> = dict
            .iter()
            .filter_map(|(k, v)| v.as_reference().map(|r| (k.clone(), map(r))))
            .collect();
        for (k, r) in moved {
            dict.insert(k, Object::Reference(r));
        }
    }
}
