//! Decision 172 route A for a character the font's encoding cannot address:
//! give it a code no surface shows, through `/Differences`.
//!
//! A nonsymbolic TrueType font reaches a glyph by code → glyph name → Unicode
//! → the program's `(3,1)` `cmap` (ISO 32000-2 §9.6.6.4), so naming the code
//! after the character makes the existing program draw it. Only codes the
//! document never shows are taken, so no existing text changes meaning. The
//! base encoding is kept (decision 172 §5) and the new `/Encoding` dictionary
//! is written inline into the copy-on-write font dictionary, so a shared
//! encoding object is never modified.

use std::collections::BTreeSet;

use crate::object::{Dict, Name, Object};
use crate::text_edit::font_extend::{Blocked, Target, codes_shown, map_private};
use crate::text_edit::glyph_find;
use crate::text_edit::program_glyphs::EmbeddedGlyphs;
use crate::view::DocumentView;
use pdfcer_fonts::fontdata::{BaseEncoding, encoding_glyph_name, unicode_to_glyph_name};

/// Codes never allocated: 0, and the whitespace codes, of which 32 also takes
/// word spacing (§9.3.3) and 9/10/13 are line ends inside a literal string.
const RESERVED: [u32; 4] = [0, 9, 10, 13];

/// Characters given codes, and the font dictionary that names them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Allocation {
    pub(crate) dict: Dict,
    /// `(character, code, glyph name)` in typed order.
    pub(crate) codes: Vec<(char, u32, String)>,
}

impl Allocation {
    /// One disclosure line per allocated code (rule 4).
    pub(crate) fn disclosures(&self, base_font: &str) -> Vec<String> {
        self.codes
            .iter()
            .map(|(ch, code, name)| {
                format!(
                    "font: '{ch}' has no code in the encoding of '{base_font}', so unused code \
                     {code} was named /{name} in the font's /Differences."
                )
            })
            .collect()
    }
}

/// Free codes in preference order: those the base encoding leaves
/// undefined above the control range, then the control codes.
fn free_codes(base: BaseEncoding, taken: &BTreeSet<u32>) -> Vec<u32> {
    let undefined = |c: u8| match encoding_glyph_name(base, c) {
        None => true,
        // WinAnsi's unused codes fall back to `bullet`; 0x95 is the real one.
        Some("bullet") => base == BaseEncoding::WinAnsi && c != 0x95,
        Some(_) => false,
    };
    let mut codes: Vec<u32> = (1..=255u8)
        .filter(|&c| c != b' ' && undefined(c))
        .map(u32::from)
        .filter(|c| !RESERVED.contains(c) && !taken.contains(c))
        .collect();
    codes.sort_by_key(|&c| (c < 32, c));
    codes
}

/// The target font and its free codes, read once.
struct Slots {
    target: Target,
    free: Vec<u32>,
}

impl Slots {
    fn read(
        doc: &DocumentView<'_>,
        resources: &Dict,
        font_name: &[u8],
        font_dict: &Dict,
    ) -> Result<Self, String> {
        let target = Target::read(doc, resources, font_name, font_dict)?;
        let enc = &target.shape.encoding;
        let mut taken = codes_shown(doc, target.font_id)?;
        taken.extend(enc.differed());
        if let Some(map) = &target.shape.to_unicode {
            taken.extend((0..=255).filter(|&c| map.cmap.lookup(c).is_some()));
        }
        let free = free_codes(enc.base, &taken);
        Ok(Self { target, free })
    }

    /// The glyph name `ch` would get, or why it cannot be shown: an AGL name
    /// when the `cmap` reaches the glyph, else the program's own `post` name
    /// for it, the one name a viewer's §9.6.6.4 lookup will find.
    fn name_for(&self, ch: char, glyphs: &dyn EmbeddedGlyphs) -> Result<String, &'static str> {
        let program = &self.target.shape.program;
        if glyphs.unicode_glyph(program, ch).is_some() {
            return unicode_to_glyph_name(ch).ok_or("no glyph name reads back as it");
        }
        glyph_find::find(glyphs, program, ch, None)
            .and_then(|f| f.post_name)
            .ok_or("the embedded program has no outline for it")
    }

    /// The font dictionary with `codes` added to its `/Differences`.
    fn renamed(&self, font_dict: &Dict, codes: &[(char, u32, String)]) -> Dict {
        let enc = &self.target.shape.encoding;
        let mut differences = enc.differences.clone();
        for (_, code, name) in codes {
            differences.push(Object::Integer(i64::from(*code)));
            differences.push(Object::Name(Name(name.as_bytes().to_vec())));
        }
        let mut encoding = enc.dict.clone().unwrap_or_default();
        let base: &[u8] = match enc.base {
            BaseEncoding::MacRoman => b"MacRomanEncoding",
            _ => b"WinAnsiEncoding",
        };
        encoding.insert(
            Name(b"BaseEncoding".to_vec()),
            Object::Name(Name(base.to_vec())),
        );
        encoding.insert(Name(b"Differences".to_vec()), Object::Array(differences));
        let mut dict = font_dict.clone();
        dict.insert(Name(b"Encoding".to_vec()), Object::Dict(encoding));
        dict
    }
}

/// Give each of `chars` an unused code in the font named `font_name`.
///
/// # Errors
///
/// Every character that cannot be given one, with its reason; a reason that
/// belongs to the font as a whole is reported once, against the first.
pub(crate) fn allocate(
    doc: &DocumentView<'_>,
    resources: &Dict,
    font_name: &[u8],
    font_dict: &Dict,
    chars: &[char],
    glyphs: &dyn EmbeddedGlyphs,
) -> Result<Allocation, Vec<Blocked>> {
    let block = |ch: char, reason: &str| Blocked {
        ch,
        code: 0,
        reason: reason.to_owned(),
    };
    let first = chars.first().copied().unwrap_or(char::MIN);
    let slots =
        Slots::read(doc, resources, font_name, font_dict).map_err(|r| vec![block(first, &r)])?;
    let mut free = slots.free.iter().copied();
    let (mut codes, mut blocked) = (Vec::new(), Vec::new());
    for &ch in chars {
        match slots.name_for(ch, glyphs) {
            Err(reason) => blocked.push(block(ch, reason)),
            Ok(name) => match free.next() {
                Some(code) => codes.push((ch, code, name)),
                None => blocked.push(block(ch, "the font has no unused code left to give it")),
            },
        }
    }
    if !blocked.is_empty() {
        return Err(blocked);
    }
    let dict = slots.renamed(font_dict, &codes);
    Ok(Allocation { dict, codes })
}

/// The characters [`allocate`] would give a code, each judged on its own:
/// those the program outlines that `addressable` (the encoding) does not.
pub(crate) fn allocatable(
    doc: &DocumentView<'_>,
    resources: &Dict,
    font_name: &[u8],
    font_dict: &Dict,
    addressable: impl Fn(char) -> bool,
    glyphs: &dyn EmbeddedGlyphs,
) -> BTreeSet<char> {
    let Ok(slots) = Slots::read(doc, resources, font_name, font_dict) else {
        return BTreeSet::new();
    };
    let t = &slots.target;
    let shared = t
        .shape
        .to_unicode
        .as_ref()
        .is_some_and(|m| map_private(doc, t.font_id, m.id).is_err());
    if slots.free.is_empty() || shared {
        return BTreeSet::new();
    }
    let program = &slots.target.shape.program;
    let by_cmap = glyphs
        .unicode_chars(program)
        .into_iter()
        .filter(|&ch| unicode_to_glyph_name(ch).is_some());
    let by_name = glyph_find::named_chars(glyphs, program)
        .into_iter()
        .filter(|&ch| glyph_find::find(glyphs, program, ch, None).is_some());
    by_cmap
        .chain(by_name)
        .filter(|&ch| !addressable(ch) && u32::from(ch) <= 0xFFFF)
        .collect()
}
