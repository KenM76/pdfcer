//! The faces an XFA form's text is laid out and drawn in, from the faces a
//! shell registered in a [`FontEnvironment`] (decision 012's seam): each
//! described from its own `name`, `OS/2`, `hhea`, `head`, `post`, `hmtx` and
//! `cmap` tables, so `pdfcer-core` parses no font.

use std::sync::Arc;

use pdfcer_core::fontinfo::FsTypeBits;
use pdfcer_core::xfa::{FontProgramKind, XfaFace, XfaFaceSource, XfaFontProgram, XfaFonts};
use skrifa::instance::{LocationRef, Size};
use skrifa::raw::types::Tag;
use skrifa::raw::{TableProvider, ps};
use skrifa::string::StringId;
use skrifa::{FontRef, MetadataProvider};

use crate::font::installed_faces::{collection_indices, name_string, serif_of};
use crate::font::{FontData, FontEnvironment};

/// The [`XfaFonts`] for `env`'s registered faces: an XFA `typeface` is
/// matched to a face whose family name equals it (case, spaces and hyphens
/// ignored), the one closest to the asked weight and posture, a normal
/// width preferred. With no registered face this is [`XfaFonts::new`], so
/// layout and painting are exactly the standard-14 ones.
///
/// # Examples
///
/// ```
/// use pdfcer_render::FontEnvironment;
/// use pdfcer_render::font::xfa_fonts;
///
/// let fonts = xfa_fonts(&FontEnvironment::bundled());
/// assert!(fonts.face("Myriad Pro", false, false).is_none());
/// ```
#[must_use]
pub fn xfa_fonts(env: &FontEnvironment) -> XfaFonts {
    let faces = EnvFaces::new(env);
    if faces.files.is_empty() {
        XfaFonts::new()
    } else {
        XfaFonts::with_source(faces)
    }
}

/// Each distinct registered font file with the normalised names it is
/// registered under.
struct EnvFaces {
    files: Vec<(Vec<String>, FontData)>,
}

/// Lowercase ASCII alphanumerics: `Myriad Pro`, `MyriadPro` and
/// `myriad-pro` compare equal.
fn norm(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

impl EnvFaces {
    fn new(env: &FontEnvironment) -> Self {
        let mut files: Vec<(Vec<String>, FontData)> = Vec::new();
        for name in env.named_faces() {
            let Some(data) = env.named(name) else {
                continue;
            };
            let ptr = data.bytes().as_ptr();
            match files.iter_mut().find(|(_, d)| d.bytes().as_ptr() == ptr) {
                Some((names, _)) => names.push(norm(name)),
                None => files.push((vec![norm(name)], data.clone())),
            }
        }
        Self { files }
    }
}

/// How far a face is from the asked style: weight, then posture, then a
/// condensed or expanded width. Lower is closer.
fn distance(font: &FontRef<'_>, bold: bool, italic: bool) -> u32 {
    let Ok(os2) = font.os2() else {
        return 8;
    };
    let is_bold = os2.us_weight_class() >= 600;
    let is_italic = os2.fs_selection().bits() & 0x1 != 0;
    u32::from(is_bold != bold) * 4
        + u32::from(is_italic != italic) * 2
        + u32::from(os2.us_width_class() != 5)
}

impl XfaFaceSource for EnvFaces {
    fn face(&self, typeface: &str, bold: bool, italic: bool) -> Option<XfaFace> {
        let want = norm(typeface);
        if want.is_empty() {
            return None;
        }
        let mut best: Option<(u32, &FontData, u32)> = None;
        for (names, data) in &self.files {
            if !names.iter().any(|n| n.starts_with(&want)) {
                continue;
            }
            for index in collection_indices(data.bytes()) {
                let Ok(font) = FontRef::from_index(data.bytes(), index) else {
                    continue;
                };
                let family = [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME]
                    .into_iter()
                    .filter_map(|id| name_string(&font, id))
                    .any(|f| norm(&f) == want);
                let d = distance(&font, bold, italic);
                if family && best.is_none_or(|(b, ..)| d < b) {
                    best = Some((d, data, index));
                }
            }
        }
        let (_, data, index) = best?;
        describe(data, index)
    }
}

/// `data`'s face `index` as an [`XfaFace`], its whole program attached when
/// it can be embedded as a simple font.
fn describe(data: &FontData, index: u32) -> Option<XfaFace> {
    let font = FontRef::from_index(data.bytes(), index).ok()?;
    let upem = f64::from(font.head().ok()?.units_per_em().max(1));
    let em = |v: f64| v * 1000.0 / upem;
    let charmap = font.charmap();
    let metrics = font.glyph_metrics(Size::unscaled(), LocationRef::default());
    let advance = |c: char| {
        let gid = charmap.map(c)?;
        let w = em(f64::from(metrics.advance_width(gid)?)).round();
        // An advance is never negative and fits u16 at 1000 units per em.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        Some(w.clamp(0.0, f64::from(u16::MAX)) as u16)
    };
    let mut face = XfaFace::new(base_font(&font), advance);
    let head = font.head().ok()?;
    let bbox = [head.x_min(), head.y_min(), head.x_max(), head.y_max()].map(|v| em(f64::from(v)));
    let (ascent, descent) = font.hhea().map_or((718.0, -207.0), |h| {
        (
            em(f64::from(h.ascender().to_i16())),
            em(f64::from(h.descender().to_i16())),
        )
    });
    let os2 = font.os2().ok();
    let cap = os2
        .as_ref()
        .and_then(|o| o.s_cap_height())
        .map_or(ascent, |c| em(f64::from(c)));
    face = face.with_metrics(ascent, descent, cap, bbox);
    let fixed = font.post().is_ok_and(|p| p.is_fixed_pitch() != 0);
    let angle = font.post().map_or(0.0, |p| p.italic_angle().to_f64());
    let (bold, italic, serif) = os2.as_ref().map_or((false, false, false), |o| {
        (
            o.us_weight_class() >= 600,
            o.fs_selection().bits() & 0x1 != 0,
            serif_of(o.panose_10(), o.s_family_class()).unwrap_or(false),
        )
    });
    face = face.with_style(bold, italic, serif, fixed, angle);
    if let Some(kind) = program_kind(data.bytes(), &font) {
        let fs_type = os2.map(|o| FsTypeBits::decode(o.fs_type(), o.version()));
        let bytes: Arc<[u8]> = Arc::from(data.bytes());
        face = face.with_program(XfaFontProgram::new(bytes, kind, fs_type));
    }
    Some(face)
}

/// How a whole font file embeds as a simple font (ISO 32000-1 §9.9): `glyf`
/// outlines as TrueType, a name-keyed `CFF ` as OpenType. A collection or a
/// CID-keyed CFF does not.
fn program_kind(bytes: &[u8], font: &FontRef<'_>) -> Option<FontProgramKind> {
    if bytes.starts_with(b"ttcf") {
        return None;
    }
    if font.table_data(Tag::new(b"glyf")).is_some() {
        return Some(FontProgramKind::TrueType);
    }
    let cff = font.table_data(Tag::new(b"CFF "))?;
    let cff = ps::cff::CffFontRef::new_cff(cff.as_bytes(), 0, None).ok()?;
    (!cff.is_cid()).then_some(FontProgramKind::OpenTypeCff)
}

/// The face's PostScript name (name ID 6), else its family's ASCII
/// alphanumerics: a `/BaseFont` has no spaces.
fn base_font(font: &FontRef<'_>) -> String {
    let name = name_string(font, StringId::POSTSCRIPT_NAME)
        .or_else(|| name_string(font, StringId::FAMILY_NAME))
        .unwrap_or_default();
    let name: String = name
        .chars()
        .filter(|c| c.is_ascii_graphic() && !"()<>[]{}/%#".contains(*c))
        .collect();
    if name.is_empty() {
        "XfaFace".to_owned()
    } else {
        name
    }
}

#[cfg(test)]
mod tests;
