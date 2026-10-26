//! The face a font resource names, for formatted copy: a family a word
//! processor recognises, and whether it is italic.

use std::collections::HashMap;

use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};
use crate::page_tree::Page;
use crate::text_extract::ContentStreamRef;
use crate::view::DocumentView;

/// `/FontDescriptor /Flags` bit 7, Italic (ISO 32000-2 §9.8.2, Table 123).
const FLAG_ITALIC: i64 = 64;

/// The face behind one `/Font` resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct FontFace {
    /// The family: `/FontDescriptor /FontFamily` when present (Table 122),
    /// else DERIVED from `/BaseFont` — subset tag and style suffix removed,
    /// `MT`/`PS` dropped, camel case spaced (`TimesNewRomanPSMT` →
    /// `Times New Roman`).
    pub family: String,
    /// Descriptor Italic flag, a non-zero `/ItalicAngle`, or an `Italic` /
    /// `Oblique` style word in `/BaseFont`.
    pub italic: bool,
}

/// Every font resource on a page and in the form XObjects it paints, keyed
/// as [`GlyphProvenance`](crate::text_extract::GlyphProvenance) names them:
/// `(content_stream, font_resource)`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PageFonts {
    faces: HashMap<(ContentStreamRef, Vec<u8>), FontFace>,
}

impl PageFonts {
    /// Read `page`'s font resources and those of every form it paints. A
    /// resource that does not resolve to a font dictionary is skipped.
    #[must_use]
    pub fn read(view: &DocumentView<'_>, page: &Page) -> Self {
        let mut faces = HashMap::new();
        collect(view, &page.resources, ContentStreamRef::Page, &mut faces);
        for form in crate::text_edit::forms::scan_page_forms(view, page).forms {
            let stream = ContentStreamRef::Form {
                object: form.id.num,
            };
            collect(view, &form.resources, stream, &mut faces);
        }
        Self { faces }
    }

    /// The face for a glyph's `(content_stream, font_resource)`.
    #[must_use]
    pub fn face(&self, stream: ContentStreamRef, resource: &[u8]) -> Option<&FontFace> {
        self.faces.get(&(stream, resource.to_vec()))
    }
}

fn collect(
    view: &DocumentView<'_>,
    resources: &Dict,
    stream: ContentStreamRef,
    out: &mut HashMap<(ContentStreamRef, Vec<u8>), FontFace>,
) {
    let Some(fonts) = resources
        .get(b"Font")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict)
    else {
        return;
    };
    for (name, font) in fonts.iter() {
        if let Some(dict) = view.resolve(font).as_dict()
            && let Some(face) = face_of(view, dict)
        {
            out.insert((stream, name.as_bytes().to_vec()), face);
        }
    }
}

fn face_of(view: &DocumentView<'_>, font: &Dict) -> Option<FontFace> {
    let base = font
        .get(b"BaseFont")
        .map(|o| view.resolve(o))
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(n.as_bytes()).into_owned())
        .unwrap_or_default();
    let descriptor = descriptor(view, font);
    let declared = descriptor
        .and_then(|d| d.get(b"FontFamily"))
        .map(|o| view.resolve(o))
        .and_then(|o| match o {
            Object::String(b) => Some(String::from_utf8_lossy(b).trim().to_owned()),
            _ => None,
        })
        .filter(|f| !f.is_empty());
    let (family, style) = split_base_font(&base);
    let family = declared.unwrap_or(family);
    if family.is_empty() {
        return None;
    }
    let num = |key: &[u8]| {
        descriptor
            .and_then(|d| d.get(key))
            .map(|o| view.resolve(o))
            .and_then(Object::as_number)
    };
    #[allow(clippy::cast_possible_truncation)] // a flags word fits in i64
    let flagged = num(b"Flags").is_some_and(|f| (f as i64) & FLAG_ITALIC != 0);
    let angled = num(b"ItalicAngle").is_some_and(|a| a != 0.0);
    let named = style.contains("Italic") || style.contains("Oblique");
    Some(FontFace {
        family,
        italic: flagged || angled || named,
    })
}

/// The descriptor of a simple font, or of a Type 0 font's descendant.
fn descriptor<'a>(view: &'a DocumentView<'a>, font: &'a Dict) -> Option<&'a Dict> {
    let own = font
        .get(b"FontDescriptor")
        .map(|o| view.resolve(o))
        .and_then(Object::as_dict);
    own.or_else(|| {
        let first = font
            .get(b"DescendantFonts")
            .map(|o| view.resolve(o))
            .and_then(Object::as_array)?
            .first()?;
        view.resolve(first)
            .as_dict()?
            .get(b"FontDescriptor")
            .map(|o| view.resolve(o))
            .and_then(Object::as_dict)
    })
}

/// `(family, style)` from a `/BaseFont`: `ABCDEF+Arial-BoldItalicMT` →
/// `("Arial", "BoldItalicMT")`.
pub(super) fn split_base_font(base: &str) -> (String, String) {
    let name = match base.split_once('+') {
        Some((tag, rest)) if tag.len() == 6 && tag.bytes().all(|b| b.is_ascii_uppercase()) => rest,
        _ => base,
    };
    let (family, style) = name.split_once([',', '-']).unwrap_or((name, ""));
    let family = family
        .strip_suffix("PSMT")
        .or_else(|| family.strip_suffix("MT"))
        .or_else(|| family.strip_suffix("PS"))
        .unwrap_or(family);
    let mut spaced = String::with_capacity(family.len() + 4);
    let mut prev_lower = false;
    for c in family.chars() {
        if prev_lower && c.is_ascii_uppercase() {
            spaced.push(' ');
        }
        prev_lower = c.is_ascii_lowercase();
        spaced.push(c);
    }
    (spaced, style.to_owned())
}
