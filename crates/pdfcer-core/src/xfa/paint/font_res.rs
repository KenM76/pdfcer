//! The `/Font` resource a face is drawn through: a standard-14 dictionary,
//! or a simple font with `/Widths` and a descriptor for a supplied face
//! (ISO 32000-1 §9.6.2 Table 111, §9.8 Table 122).

use super::face::Face;
use super::options::FontEmbedding;
use crate::object::{Dict, Name, Object};
use crate::vartext::standard14_font_dict;
use crate::xfa::{FontProgramKind, XfaFace, XfaFontProgram};

/// The first and last codes a supplied face's `/Widths` covers.
const FIRST_CHAR: u8 = 32;
const LAST_CHAR: u8 = 255;

/// One `/Font` entry of a [`PaintedPage`](super::PaintedPage).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct PaintedFont {
    /// The resource name (`XF0`, ...).
    pub name: String,
    /// The `/BaseFont` written.
    pub base_font: String,
    /// Whether the face is a supplied one, not a standard-14 face.
    pub supplied: bool,
    /// The program a writer embeds under the face's descriptor
    /// (`/FontFile2` for [`FontProgramKind::TrueType`], `/FontFile3
    /// /OpenType` otherwise); `None` under [`FontEmbedding::Reference`],
    /// for a standard-14 face, or when `refusal` is set.
    pub program: Option<XfaFontProgram>,
    /// Why the face's program is not embedded although embedding was asked.
    pub refusal: Option<&'static str>,
}

impl PaintedFont {
    /// The record for a face painted under `name`; a supplied face keeps its
    /// program only when embedding is on and permitted (`refusal` says why not).
    pub(super) fn of(name: String, face: &Face, embedding: FontEmbedding) -> Self {
        let mut out = Self {
            name,
            base_font: base_font(face),
            supplied: false,
            program: None,
            refusal: None,
        };
        if let Face::Supplied(f) = face {
            out.supplied = true;
            if embedding == FontEmbedding::Embed {
                match f.program.as_ref().map(|p| (p, p.embedding_refusal())) {
                    Some((_, Some(why))) => out.refusal = Some(why),
                    Some((p, None)) => out.program = Some(p.clone()),
                    None => out.refusal = Some("no program supplied"),
                }
            }
        }
        out
    }
}

/// The `/BaseFont` `face` is drawn through.
pub(in crate::xfa) fn base_font(face: &Face) -> String {
    match face {
        Face::Std(f) => match standard14_font_dict(*f).get(b"BaseFont") {
            Some(Object::Name(n)) => String::from_utf8_lossy(&n.0).into_owned(),
            _ => String::new(),
        },
        Face::Supplied(f) => f.base_font.clone(),
    }
}

/// The font dictionary for `face`, its descriptor inline.
pub(super) fn dict(face: &Face) -> Dict {
    match face {
        Face::Std(f) => standard14_font_dict(*f),
        Face::Supplied(f) => supplied(f),
    }
}

fn name(n: &str) -> Object {
    Object::Name(Name(n.as_bytes().to_vec()))
}

fn supplied(f: &XfaFace) -> Dict {
    let truetype = f
        .program
        .as_ref()
        .is_none_or(|p| p.kind == FontProgramKind::TrueType);
    let mut d = Dict::new();
    d.insert(Name::from(b"Type"), name("Font"));
    let subtype = if truetype { "TrueType" } else { "Type1" };
    d.insert(Name::from(b"Subtype"), name(subtype));
    d.insert(Name::from(b"BaseFont"), name(&f.base_font));
    d.insert(
        Name::from(b"FirstChar"),
        Object::Integer(i64::from(FIRST_CHAR)),
    );
    d.insert(
        Name::from(b"LastChar"),
        Object::Integer(i64::from(LAST_CHAR)),
    );
    let widths = (FIRST_CHAR..=LAST_CHAR)
        .map(|c| Object::Integer(i64::from(f.width(c).unwrap_or(0))))
        .collect();
    d.insert(Name::from(b"Widths"), Object::Array(widths));
    d.insert(Name::from(b"Encoding"), name("WinAnsiEncoding"));
    d.insert(Name::from(b"FontDescriptor"), Object::Dict(descriptor(f)));
    d
}

/// §9.8.2 flags: FixedPitch 1, Serif 2, Nonsymbolic 32, Italic 64.
fn descriptor(f: &XfaFace) -> Dict {
    let flags = 32 | i64::from(f.fixed_pitch) | i64::from(f.serif) << 1 | i64::from(f.italic) << 6;
    let mut d = Dict::new();
    d.insert(Name::from(b"Type"), name("FontDescriptor"));
    d.insert(Name::from(b"FontName"), name(&f.base_font));
    d.insert(Name::from(b"Flags"), Object::Integer(flags));
    let bbox = f.bbox.iter().map(|v| Object::Real(v.round())).collect();
    d.insert(Name::from(b"FontBBox"), Object::Array(bbox));
    d.insert(Name::from(b"ItalicAngle"), Object::Real(f.italic_angle));
    d.insert(Name::from(b"Ascent"), Object::Real(f.ascent.round()));
    d.insert(Name::from(b"Descent"), Object::Real(f.descent.round()));
    d.insert(Name::from(b"CapHeight"), Object::Real(f.cap_height.round()));
    // StemV is required and rarely known; the common bold/regular estimate.
    let stem = if f.bold { 120 } else { 80 };
    d.insert(Name::from(b"StemV"), Object::Integer(stem));
    d
}
