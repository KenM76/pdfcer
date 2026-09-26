//! Planning for EMF export with text kept as text (G033).
//!
//! Recipe and every field value: `D:\dev\rag\emf\text_records.md` (writer
//! recipe) and `consumers.md` (§ "Text records"). EMF cannot embed a font
//! (`font_embedding_verdict.md`): the consumer draws the characters with an
//! installed face of the recorded name, and the `Dx` array pins each
//! character's origin (GDI, LibreOffice; Inkscape ignores `Dx`).
//!
//! A run becomes one `EMR_EXTTEXTOUTW` only when that can draw it in the
//! right place:
//!
//! - every paint is an opaque solid nonzero fill, `Normal` blend, one
//!   colour and one clip across the run (anything else is rasterised or
//!   drawn as a path by the outline route, so it stays there);
//! - every glyph maps to exactly one BMP, non-control character;
//! - the glyph → device matrix is a uniform scale plus a rotation, not
//!   mirrored (EMF text has no skew, no anisotropic scale, no mirror), and
//!   every glyph sits on the first glyph's baseline at the same matrix;
//! - the face is not a symbol face whose characters a system font of the
//!   same name would draw differently (`Symbol`, `ZapfDingbats`, Wingdings,
//!   Webdings).
//!
//! Any other run is written as its outlines and counted by reason in
//! [`crate::emf::EmfTextOutcome`].

use tiny_skia::{BlendMode, Transform};

use crate::display_list::{ClipId, Op, TextRunInfo};
use crate::svg_text::{family_of, is_sfnt, single_char, uniform_paint};

/// Relative tolerance on the matrix being a similarity, and on each glyph
/// sharing the first glyph's matrix.
const LINEAR_TOL: f32 = 1e-3;
/// Tolerance on baseline offset, in font units.
const BASELINE_TOL: f32 = 0.5;

/// Why a run stays as outlines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EmfFallback {
    Paint,
    Unmapped,
    Geometry,
    SymbolFace,
}

/// A run that can be written as `EMR_EXTTEXTOUTW`.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct EmfTextRun {
    /// UTF-16 units, one per glyph.
    pub chars: Vec<u16>,
    /// The first glyph's origin, device pixels.
    pub origin: (f32, f32),
    /// Each character's distance along the baseline from `origin`, device
    /// pixels, plus one entry for the pen position after the last glyph.
    pub along: Vec<f32>,
    /// The em size, device pixels.
    pub em: f32,
    /// Baseline angle, degrees counterclockwise as seen on the page.
    pub angle: f32,
    pub rgb: [u8; 3],
    pub clip: Option<ClipId>,
    /// The name recorded in `LogFont.Facename`.
    pub face: String,
    /// `LogFont.Weight`, 100..900.
    pub weight: i32,
    pub italic: bool,
}

/// Decide one run.
pub(crate) fn plan_run(run: &TextRunInfo, ops: &[Op]) -> Result<EmfTextRun, EmfFallback> {
    let (rgba, blend, clip) = uniform_paint(ops).ok_or(EmfFallback::Paint)?;
    if rgba[3] != 255 || blend != BlendMode::SourceOver {
        return Err(EmfFallback::Paint);
    }

    let mut chars = Vec::with_capacity(run.glyphs.len());
    for g in &run.glyphs {
        let c = single_char(g.unicode.as_deref()).ok_or(EmfFallback::Unmapped)?;
        chars.push(u16::try_from(u32::from(c)).map_err(|_| EmfFallback::Unmapped)?);
    }

    let first = run.glyphs.first().ok_or(EmfFallback::Paint)?;
    let m = first.to_device;
    // Glyph space is y-up, device y-down: an unmirrored rotation by θ
    // (counterclockwise on the page) at scale s is
    // sx = s·cosθ, kx = −s·sinθ, ky = −s·sinθ, sy = −s·cosθ.
    let s = m.sx.hypot(m.ky);
    if !(s.is_finite() && s > 0.0)
        || (m.sx + m.sy).abs() > LINEAR_TOL * s
        || (m.kx - m.ky).abs() > LINEAR_TOL * s
    {
        return Err(EmfFallback::Geometry);
    }
    let inverse = m.invert().ok_or(EmfFallback::Geometry)?;
    let offset = |t: Transform| -> Option<f32> {
        let p = inverse.pre_concat(t);
        let aligned = (p.sx - 1.0).abs() < LINEAR_TOL
            && (p.sy - 1.0).abs() < LINEAR_TOL
            && p.kx.abs() < LINEAR_TOL
            && p.ky.abs() < LINEAR_TOL
            && p.ty.abs() < BASELINE_TOL
            && p.tx.is_finite();
        aligned.then_some(p.tx * s)
    };
    let mut along = Vec::with_capacity(run.glyphs.len() + 1);
    for g in &run.glyphs {
        along.push(offset(g.to_device).ok_or(EmfFallback::Geometry)?);
    }
    along.push(offset(run.end).ok_or(EmfFallback::Geometry)?);

    let face = face_name(run);
    if is_symbol_face(&face) {
        return Err(EmfFallback::SymbolFace);
    }
    let (weight, italic) = style(run);

    Ok(EmfTextRun {
        chars,
        origin: (m.tx, m.ty),
        along,
        em: s * run.upem,
        angle: (-m.ky).atan2(m.sx).to_degrees(),
        rgb: [rgba[0], rgba[1], rgba[2]],
        clip,
        face,
        weight,
        italic,
    })
}

/// The installed-face name a consumer should look up: the embedded
/// program's own family name when it has one, else one derived from
/// `/BaseFont`. Standard-14 names map to the Windows faces with the same
/// metrics, so no consumer picks the bitmap `Courier`.
fn face_name(run: &TextRunInfo) -> String {
    if run.font.source == crate::font::GlyphSource::Embedded
        && is_sfnt(run.font.data.bytes())
        && let Ok(font) = skrifa::FontRef::new(run.font.data.bytes())
    {
        use skrifa::MetadataProvider as _;
        for id in [
            skrifa::string::StringId::TYPOGRAPHIC_FAMILY_NAME,
            skrifa::string::StringId::FAMILY_NAME,
        ] {
            if let Some(name) = font.localized_strings(id).english_or_first() {
                let name: String = name.chars().filter(|c| !c.is_control()).collect();
                let name = name.trim();
                if !name.is_empty() {
                    return truncate_face(name);
                }
            }
        }
    }
    truncate_face(&face_from_base_font(&run.font.base_font))
}

/// `/BaseFont` → a family name: subset tag and style suffix dropped, a
/// PostScript `PS`/`MT` tail removed, CamelCase split into words.
fn face_from_base_font(base_font: &str) -> String {
    let name = family_of(base_font.split(',').next().unwrap_or(base_font));
    let name = name
        .split(['-', ','])
        .next()
        .unwrap_or(&name)
        .trim()
        .to_owned();
    match name.as_str() {
        "Helvetica" | "Arial" | "ArialMT" => return "Arial".to_owned(),
        "Times" | "TimesRoman" | "TimesNewRoman" | "TimesNewRomanPS" | "TimesNewRomanPSMT" => {
            return "Times New Roman".to_owned();
        }
        "Courier" | "CourierNew" | "CourierNewPS" | "CourierNewPSMT" => {
            return "Courier New".to_owned();
        }
        _ => {}
    }
    let mut stem = name.as_str();
    for tail in ["PSMT", "MT", "PS"] {
        if let Some(s) = stem.strip_suffix(tail)
            && !s.is_empty()
        {
            stem = s;
            break;
        }
    }
    if stem.contains(' ') {
        return stem.to_owned();
    }
    let mut out = String::with_capacity(stem.len() + 4);
    let chars: Vec<char> = stem.chars().collect();
    let mut prev: Option<char> = None;
    for (i, &c) in chars.iter().enumerate() {
        if let Some(prev) = prev
            && c.is_ascii_uppercase()
        {
            let next_lower = chars.get(i + 1).is_some_and(char::is_ascii_lowercase);
            if prev.is_ascii_lowercase() || (prev.is_ascii_uppercase() && next_lower) {
                out.push(' ');
            }
        }
        out.push(c);
        prev = Some(c);
    }
    if out.is_empty() {
        "Arial".to_owned()
    } else {
        out
    }
}

/// `LogFont.Facename` holds 32 UTF-16 units including the terminator.
fn truncate_face(name: &str) -> String {
    let mut out = String::new();
    let mut units = 0;
    for c in name.chars() {
        units += c.len_utf16();
        if units > 31 {
            break;
        }
        out.push(c);
    }
    out
}

fn is_symbol_face(face: &str) -> bool {
    let f = face.to_ascii_lowercase().replace(' ', "");
    f == "symbol" || f.starts_with("zapfdingbats") || f.starts_with("wingdings") || f == "webdings"
}

/// Weight and italic: the program's own `OS/2` when it is an sfnt, else
/// words in `/BaseFont`.
fn style(run: &TextRunInfo) -> (i32, bool) {
    let data = run.font.data.bytes();
    if run.font.source == crate::font::GlyphSource::Embedded
        && is_sfnt(data)
        && let Ok(font) = skrifa::FontRef::new(data)
    {
        use skrifa::MetadataProvider as _;
        let a = font.attributes();
        #[allow(clippy::cast_possible_truncation)]
        let weight = (a.weight.value().round() as i32).clamp(1, 1000);
        return (weight, a.style != skrifa::attribute::Style::Normal);
    }
    let name = run.font.base_font.to_ascii_lowercase();
    let weight = if name.contains("black") || name.contains("heavy") {
        900
    } else if name.contains("extrabold") || name.contains("ultrabold") {
        800
    } else if name.contains("semibold") || name.contains("demi") {
        600
    } else if name.contains("bold") {
        700
    } else if name.contains("medium") {
        500
    } else if name.contains("light") {
        300
    } else {
        400
    };
    (weight, name.contains("italic") || name.contains("oblique"))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::*;

    #[test]
    fn base_font_names_become_installed_face_names() {
        assert_eq!(face_from_base_font("ABCDEF+Arial-BoldMT"), "Arial");
        assert_eq!(face_from_base_font("Helvetica-Oblique"), "Arial");
        assert_eq!(face_from_base_font("Times-Roman"), "Times New Roman");
        assert_eq!(
            face_from_base_font("TimesNewRomanPS-BoldMT"),
            "Times New Roman"
        );
        assert_eq!(face_from_base_font("Courier"), "Courier New");
        assert_eq!(face_from_base_font("CenturyGothic"), "Century Gothic");
        assert_eq!(face_from_base_font("SegoeUI,Bold"), "Segoe UI");
        assert_eq!(face_from_base_font("ISOCPEUR"), "ISOCPEUR");
        assert_eq!(face_from_base_font("OCRAExtended"), "OCRA Extended");
    }

    #[test]
    fn face_names_fit_the_logfont_field() {
        let long = "A".repeat(40);
        assert_eq!(truncate_face(&long).len(), 31);
    }

    #[test]
    fn symbol_faces_are_recognised() {
        for f in ["Symbol", "ZapfDingbats", "Wingdings 3", "Webdings"] {
            assert!(is_symbol_face(f), "{f}");
        }
        assert!(!is_symbol_face("Segoe UI Symbol"));
    }
}
