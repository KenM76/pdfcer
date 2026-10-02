//! Decision 173 §3: is an installed face the font an embedded subset was cut
//! from? Every check is required; only the compared set (I5) has a setting.

use skrifa::raw::TableProvider;
use skrifa::raw::types::NameId;
use skrifa::{FontRef, MetadataProvider};

use crate::font::program::FontProgram;
use crate::font::sfnt::{Directory, read_u16};
use crate::font::subset::check_embedding_permission;

use super::{AugmentError, Parts, cmap};

/// Which subset glyphs I5 compares (decision 173 §3).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum OutlineCheck<'a> {
    /// Every outlined glyph the subset's cmap reaches whose character the
    /// face also maps.
    #[default]
    AllShared,
    /// Only those glyphs whose character is among the codes in use.
    ShownOnly(&'a [char]),
}

/// I1: the collection member whose name ID 6 equals `font_name` with any
/// `ABCDEF+` subset tag removed; `None` when no member is a candidate.
pub(crate) fn candidate_index(face: &[u8], font_name: &str) -> Option<u32> {
    let wanted = strip_tag(font_name);
    let count = if face.starts_with(b"ttcf") {
        crate::font::sfnt::read_u32(face, 8)?
    } else {
        1
    };
    (0..count).find(|&i| {
        FontRef::from_index(face, i).is_ok_and(|f| {
            f.localized_strings(NameId::POSTSCRIPT_NAME)
                .any(|s| s.chars().eq(wanted.chars()))
        })
    })
}

/// The members of `chars` face member `face_index`'s cmap maps.
pub(crate) fn face_chars(face: &[u8], face_index: u32, chars: &[char]) -> Vec<char> {
    let Ok(f) = Parts::read(face, face_index, "installed font") else {
        return Vec::new();
    };
    chars
        .iter()
        .copied()
        .filter(|&ch| cmap::unicode_glyph(f.get(b"cmap"), ch).is_some())
        .collect()
}

fn strip_tag(name: &str) -> &str {
    match name.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => name,
    }
}

/// I2–I7 for face member `face_index` against `subset`, for the characters
/// to be appended.
pub(crate) fn check(
    subset: &[u8],
    face: &[u8],
    face_index: u32,
    chars: &[char],
    scope: OutlineCheck<'_>,
) -> Result<usize, AugmentError> {
    let dir =
        Directory::parse_face(face, face_index).ok_or(AugmentError::FaceNotTrueTypeOutlines)?;
    if dir.flavor != 0x0001_0000 || dir.table(*b"fvar").is_some() {
        return Err(AugmentError::FaceNotTrueTypeOutlines);
    }
    permitted(face, face_index)?;
    permitted(subset, 0)?;
    let (s, f) = (
        Parts::read(subset, 0, "embedded font")?,
        Parts::read(face, face_index, "installed font")?,
    );
    if read_u16(s.get(b"head"), 18) != read_u16(f.get(b"head"), 18) {
        return Err(AugmentError::UnitsPerEmMismatch);
    }
    for &ch in chars {
        cmap::unicode_glyph(f.get(b"cmap"), ch).ok_or(AugmentError::FaceLacksCharacter { ch })?;
    }
    let compared = compare_shared(subset, face, face_index, &s, &f, scope)?;
    if compared == 0 {
        return Err(AugmentError::IdentityUnproven);
    }
    Ok(compared)
}

/// R109 on one carrier; a program without `OS/2` proceeds, as R109 does.
fn permitted(data: &[u8], index: u32) -> Result<(), AugmentError> {
    let font = FontRef::from_index(data, index).map_err(|e| AugmentError::MalformedFace {
        detail: e.to_string(),
    })?;
    if font.os2().is_err() {
        return Ok(());
    }
    check_embedding_permission(&font).map_err(|e| AugmentError::EmbeddingNotPermitted {
        reason: e.to_string(),
    })
}

/// I5: the number of non-empty outlines compared, or the first mismatch.
fn compare_shared(
    subset: &[u8],
    face: &[u8],
    face_index: u32,
    s: &Parts<'_>,
    f: &Parts<'_>,
    scope: OutlineCheck<'_>,
) -> Result<usize, AugmentError> {
    let malformed = |e: String| AugmentError::MalformedFace { detail: e };
    let sp = FontProgram::parse(subset).map_err(|e| malformed(e.to_string()))?;
    let fp = FontRef::from_index(face, face_index)
        .map(FontProgram::Sfnt)
        .map_err(|e| malformed(e.to_string()))?;
    let mut compared = 0;
    for (c, gid) in cmap::windows_unicode_map(s.get(b"cmap")).unwrap_or_default() {
        let Some(ch) = char::from_u32(c) else {
            continue;
        };
        if let OutlineCheck::ShownOnly(shown) = scope
            && !shown.contains(&ch)
        {
            continue;
        }
        let Some(face_gid) = cmap::unicode_glyph(f.get(b"cmap"), ch) else {
            continue;
        };
        if !s.drawn(gid) {
            continue;
        }
        let mine = sp
            .outline(u32::from(gid))
            .map_err(|e| malformed(e.to_string()))?;
        let theirs = fp
            .outline(u32::from(face_gid))
            .map_err(|e| malformed(e.to_string()))?;
        let same = match (&mine, &theirs) {
            (Some(m), Some(t)) => m.points() == t.points() && m.verbs() == t.verbs(),
            (None, None) => true,
            _ => false,
        };
        if !same {
            return Err(AugmentError::OutlineMismatch { ch, gid });
        }
        if s.metric(gid).0 != f.metric(face_gid).0 {
            return Err(AugmentError::AdvanceMismatch { ch, gid });
        }
        compared += 1; // `drawn` above: never an empty outline
    }
    Ok(compared)
}
