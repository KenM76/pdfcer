//! Decision 172 route A: make a character the page never shows typeable
//! through an embedded subset's existing font dictionary, by addition only.
//!
//! This module covers a simple, nonsymbolic `/TrueType` font with a
//! WinAnsi or MacRoman base encoding; `cid_extend` covers the composite
//! shape. The code is the one the encoding assigns the character (ISO
//! 32000-2 §9.6.6.4: code → glyph name → Unicode → the program's `(3,1)`
//! cmap), or an unused one `code_alloc` names in `/Differences`. The writes
//! are `/FirstChar`, `/LastChar` and `/Widths` (§9.6.2), any gap filled by
//! `/MissingWidth` (Table 122), and `/ToUnicode` entries. The program bytes
//! are never touched.

use std::collections::BTreeSet;

use crate::content::ContentStream;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::text_edit::edit::{carried_codes, walk_records};
use crate::text_edit::forms::scan_page_forms;
use crate::text_edit::glyph_find;
use crate::text_edit::program_glyphs::EmbeddedGlyphs;
pub(crate) use crate::text_edit::unicode_map::UnicodeMap;
use crate::view::DocumentView;
use pdfcer_fonts::fontdata::BaseEncoding;

/// Upper bound on objects visited proving a resource graph does not reach the font.
const REACH_BUDGET: usize = 20_000;

/// A character added to the font dictionary.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AddedGlyph {
    pub(crate) ch: char,
    pub(crate) code: u32,
    pub(crate) gid: u32,
    /// The width now in `/Widths`, glyph space.
    pub(crate) width: f64,
    /// Whether `/Widths` had to change for it.
    pub(crate) widened: bool,
    /// Whether `/ToUnicode` gains an entry for it.
    pub(crate) mapped: bool,
    /// The `post` name the glyph was found by, when the `cmap` missed it.
    pub(crate) post_name: Option<String>,
}

/// The new revision of the font dictionary.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FontExtension {
    pub(crate) font_id: ObjId,
    pub(crate) dict: Dict,
    pub(crate) added: Vec<AddedGlyph>,
    /// The rewritten `/ToUnicode` stream: id, dictionary, unfiltered bytes.
    pub(crate) to_unicode: Option<(ObjId, Dict, Vec<u8>)>,
    /// Whether `dict` carries codes newly named in `/Differences`.
    pub(crate) reencoded: bool,
    /// The font dictionary the edit resolves when `dict` is not it: a Type0
    /// font with its rewritten descendant inline.
    pub(crate) view: Option<Dict>,
}

impl FontExtension {
    /// The font dictionary as the edit sees it.
    pub(crate) fn view(&self) -> &Dict {
        self.view.as_ref().unwrap_or(&self.dict)
    }

    /// The object write, or nothing when the dictionary is unchanged.
    pub(crate) fn write(&self) -> Option<(ObjId, Object)> {
        (self.reencoded || self.added.iter().any(|a| a.widened))
            .then(|| (self.font_id, Object::Dict(self.dict.clone())))
    }

    /// One disclosure line per added character (rule 4).
    pub(crate) fn disclosures(&self, base_font: &str) -> Vec<String> {
        self.added
            .iter()
            .map(|a| {
                let how = if a.widened {
                    format!(
                        "the font dictionary gained width {} for it, read from the program's \
                         metrics",
                        fmt_width(a.width)
                    )
                } else {
                    "the font dictionary already gave it a width".to_owned()
                };
                let how = match &a.post_name {
                    Some(n) => format!("{}; {how}", glyph_find::inferred_clause(a.ch, n)),
                    None => how,
                };
                if self.view.is_some() {
                    let found = match (a.mapped, a.post_name.is_some()) {
                        (true, true) => "/ToUnicode gained an entry for it",
                        (true, false) => {
                            "found through the program's cmap; /ToUnicode gained an entry for it"
                        }
                        _ => "the CID /ToUnicode already gave it",
                    };
                    return format!(
                        "font: '{}' was typed as CID {}, glyph {} that the embedded subset '{}' \
                         already contains but the document never showed ({found}); {how}. The \
                         font program is unchanged.",
                        a.ch, a.code, a.gid, base_font
                    );
                }
                format!(
                    "font: '{}' was typed with glyph {} that the embedded subset '{}' already \
                     contains but the document never showed, as code {}; {how}. The font \
                     program is unchanged.",
                    a.ch, a.gid, base_font, a.code
                )
            })
            .collect()
    }
}

/// Why route A cannot add a character.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Blocked {
    pub(crate) ch: char,
    pub(crate) code: u32,
    pub(crate) reason: String,
}

fn fmt_width(w: f64) -> String {
    if w.fract() == 0.0 {
        format!("{w:.0}")
    } else {
        format!("{w}")
    }
}

/// The font a plan extends, read once.
pub(crate) struct Target {
    pub(crate) font_id: ObjId,
    pub(crate) shape: Shape,
}

impl Target {
    /// The font `font_name` selects in `resources`, which must be an indirect
    /// object so the copy-on-write rewrite has an id to replace.
    pub(crate) fn read(
        doc: &DocumentView<'_>,
        resources: &Dict,
        font_name: &[u8],
        font_dict: &Dict,
    ) -> Result<Self, String> {
        let font_id = font_object(doc, resources, font_name)?;
        let shape = Shape::read(doc, font_dict)?;
        Ok(Self { font_id, shape })
    }

    /// The per-character rule. Codes are distinct per character, so judging
    /// each against the unextended `/Widths` is the same as judging them in
    /// sequence.
    fn assess(
        &self,
        ch: char,
        code: u32,
        glyphs: &dyn EmbeddedGlyphs,
    ) -> Result<AddedGlyph, String> {
        let s = &self.shape;
        let encoded = glyph_find::encoded_name(&s.encoding, code);
        let found = glyph_find::find(glyphs, &s.program, ch, encoded.as_deref())
            .ok_or_else(|| "the embedded program has no outline for it".to_owned())?;
        let glyph = found.glyph;
        let width = glyph.advance.round();
        let current = width_at(&s.widths, s.first_char, code).unwrap_or(s.missing_width);
        let mapped = match &s.to_unicode {
            Some(map) => map.needs_entry(ch, code)?,
            None => false,
        };
        Ok(AddedGlyph {
            ch,
            code,
            gid: glyph.gid,
            width,
            widened: (current - width).abs() > 0.5,
            mapped,
            post_name: found.post_name,
        })
    }

    /// Writes `/FirstChar`, `/LastChar` and `/Widths` covering every widened
    /// code into `dict`.
    fn widen(&self, dict: &mut Dict, added: &[AddedGlyph]) {
        let s = &self.shape;
        let (mut widths, mut first_char) = (s.widths.clone(), s.first_char);
        for a in added.iter().filter(|a| a.widened) {
            set_width(
                &mut widths,
                &mut first_char,
                a.code,
                a.width,
                s.missing_width,
            );
        }
        let last = first_char + widths.len() as u32 - 1;
        dict.insert(
            Name(b"FirstChar".to_vec()),
            Object::Integer(first_char.into()),
        );
        dict.insert(Name(b"LastChar".to_vec()), Object::Integer(last.into()));
        dict.insert(
            Name(b"Widths".to_vec()),
            Object::Array(widths.iter().map(|&w| number(w)).collect()),
        );
    }

    /// The rewritten `/ToUnicode` stream, when any added code needs an entry.
    fn extended_map(
        &self,
        doc: &DocumentView<'_>,
        added: &[AddedGlyph],
    ) -> Result<Option<(ObjId, Dict, Vec<u8>)>, String> {
        let Some(map) = &self.shape.to_unicode else {
            return Ok(None);
        };
        let entries: Vec<(u32, char)> = added
            .iter()
            .filter(|a| a.mapped)
            .map(|a| (a.code, a.ch))
            .collect();
        if entries.is_empty() {
            return Ok(None);
        }
        map_private(doc, self.font_id, map.id)?;
        let (d, bytes) = map.extended(&entries);
        Ok(Some((map.id, d, bytes)))
    }
}

/// The refusal for a code whose new width would restyle text drawn elsewhere.
pub(crate) fn shown_elsewhere(code: u32) -> String {
    format!("code {code} is already shown elsewhere in the document with a different width")
}

/// Plan the extension for `missing` — `(character, code)` pairs the encoding
/// assigns but no show of this font on the page uses.
///
/// # Errors
///
/// Every character that cannot be added, each with its reason; a reason that
/// belongs to the font as a whole is reported once, against the first.
pub(crate) fn plan(
    doc: &DocumentView<'_>,
    resources: &Dict,
    font_name: &[u8],
    font_dict: &Dict,
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
    if crate::text_edit::cid_extend::is_composite(doc, font_dict) {
        return crate::text_edit::cid_extend::plan(
            doc, resources, font_name, font_dict, missing, glyphs,
        );
    }
    let t = Target::read(doc, resources, font_name, font_dict).map_err(whole)?;
    let (mut added, mut blocked) = (Vec::new(), Vec::new());
    for &(ch, code) in missing {
        match t.assess(ch, code, glyphs) {
            Ok(a) => added.push(a),
            Err(reason) => blocked.push(Blocked { ch, code, reason }),
        }
    }

    let mut dict = font_dict.clone();
    if added.iter().any(|a| a.widened) {
        let shown = codes_shown(doc, t.font_id).map_err(whole)?;
        blocked.extend(
            added
                .iter()
                .filter(|a| a.widened && shown.contains(&a.code))
                .map(|a| Blocked {
                    ch: a.ch,
                    code: a.code,
                    reason: shown_elsewhere(a.code),
                }),
        );
    }
    if !blocked.is_empty() {
        blocked.sort_by_key(|b| missing.iter().position(|&(_, c)| c == b.code));
        return Err(blocked);
    }
    if added.iter().any(|a| a.widened) {
        t.widen(&mut dict, &added);
    }
    let to_unicode = t.extended_map(doc, &added).map_err(whole)?;
    Ok(FontExtension {
        font_id: t.font_id,
        dict,
        added,
        to_unicode,
        reencoded: false,
        view: None,
    })
}

/// Which of `candidates` [`plan`] would add, each judged on its own — the
/// repertoire query's half of the one planner. One document scan at most.
pub(crate) fn addable(
    doc: &DocumentView<'_>,
    resources: &Dict,
    font_name: &[u8],
    font_dict: &Dict,
    candidates: &[(char, u32)],
    glyphs: &dyn EmbeddedGlyphs,
) -> BTreeSet<char> {
    if crate::text_edit::cid_extend::is_composite(doc, font_dict) {
        return crate::text_edit::cid_extend::addable(
            doc, resources, font_name, font_dict, candidates, glyphs,
        );
    }
    let Ok(t) = Target::read(doc, resources, font_name, font_dict) else {
        return BTreeSet::new();
    };
    let added: Vec<AddedGlyph> = candidates
        .iter()
        .filter_map(|&(ch, code)| t.assess(ch, code, glyphs).ok())
        .collect();
    let mut added = added;
    if let Some(map) = &t.shape.to_unicode
        && added.iter().any(|a| a.mapped)
        && map_private(doc, t.font_id, map.id).is_err()
    {
        added.retain(|a| !a.mapped);
    }
    let shown = if added.iter().any(|a| a.widened) {
        match codes_shown(doc, t.font_id) {
            Ok(s) => Some(s),
            Err(_) => return added.iter().filter(|a| !a.widened).map(|a| a.ch).collect(),
        }
    } else {
        None
    };
    added
        .iter()
        .filter(|a| !a.widened || shown.as_ref().is_some_and(|s| !s.contains(&a.code)))
        .map(|a| a.ch)
        .collect()
}

/// A width as an integer when whole, so `/Widths` and `/W` stay compact.
pub(crate) fn number(w: f64) -> Object {
    if w.fract() == 0.0 && w.abs() < 1e15 {
        Object::Integer(w as i64)
    } else {
        Object::Real(w)
    }
}

fn width_at(widths: &[f64], first_char: u32, code: u32) -> Option<f64> {
    let i = code.checked_sub(first_char)? as usize;
    widths.get(i).copied().filter(|&w| w != 0.0)
}

/// Grow `widths` to cover `code`, filling the gap with `missing`.
fn set_width(widths: &mut Vec<f64>, first_char: &mut u32, code: u32, w: f64, missing: f64) {
    if widths.is_empty() {
        *first_char = code;
        widths.push(w);
        return;
    }
    if code < *first_char {
        let gap = (*first_char - code) as usize;
        widths.splice(0..0, std::iter::repeat_n(missing, gap));
        *first_char = code;
    }
    let i = (code - *first_char) as usize;
    if i >= widths.len() {
        widths.resize(i + 1, missing);
    }
    if let Some(slot) = widths.get_mut(i) {
        *slot = w;
    }
}

/// The font's `/Encoding`: a WinAnsi or MacRoman base, named or as a
/// dictionary's `/BaseEncoding`, plus any `/Differences` (§9.6.5.1).
pub(crate) struct Encoding {
    pub(crate) base: BaseEncoding,
    /// The `/Differences` array, references resolved.
    pub(crate) differences: Vec<Object>,
    /// The encoding dictionary's own entries, when it is one.
    pub(crate) dict: Option<Dict>,
}

impl Encoding {
    fn read(doc: &DocumentView<'_>, font: &Dict) -> Result<Self, &'static str> {
        const UNSUPPORTED: &str =
            "only a WinAnsi or MacRoman based encoding can be extended so far";
        let base = |n: Option<&Name>| match n.map(Name::as_bytes) {
            Some(b"WinAnsiEncoding") => Ok(BaseEncoding::WinAnsi),
            Some(b"MacRomanEncoding") => Ok(BaseEncoding::MacRoman),
            _ => Err(UNSUPPORTED),
        };
        match doc.resolve(font.get(b"Encoding").unwrap_or(&Object::Null)) {
            Object::Name(n) => Ok(Self {
                base: base(Some(n))?,
                differences: Vec::new(),
                dict: None,
            }),
            Object::Dict(d) => Ok(Self {
                base: base(
                    doc.resolve(d.get(b"BaseEncoding").unwrap_or(&Object::Null))
                        .as_name(),
                )?,
                differences: doc
                    .resolve(d.get(b"Differences").unwrap_or(&Object::Null))
                    .as_array()
                    .unwrap_or(&[])
                    .iter()
                    .map(|o| doc.resolve(o).clone())
                    .collect(),
                dict: Some(d.clone()),
            }),
            _ => Err(UNSUPPORTED),
        }
    }

    /// Codes `/Differences` assigns.
    pub(crate) fn differed(&self) -> BTreeSet<u32> {
        let mut out = BTreeSet::new();
        let mut code: Option<i64> = None;
        for o in &self.differences {
            match o {
                Object::Integer(n) => code = Some(*n),
                Object::Name(_) => {
                    if let Some(c) = code.and_then(|c| u32::try_from(c).ok()) {
                        out.insert(c);
                    }
                    code = code.map(|c| c + 1);
                }
                _ => {}
            }
        }
        out
    }
}

/// What route A needs from the font dictionary.
pub(crate) struct Shape {
    first_char: u32,
    widths: Vec<f64>,
    missing_width: f64,
    pub(crate) program: Vec<u8>,
    pub(crate) to_unicode: Option<UnicodeMap>,
    pub(crate) encoding: Encoding,
}

impl Shape {
    fn read(doc: &DocumentView<'_>, font: &Dict) -> Result<Self, &'static str> {
        let name = |d: &Dict, k: &[u8]| {
            doc.resolve(d.get(k).unwrap_or(&Object::Null))
                .as_name()
                .map(|n| n.as_bytes().to_vec())
        };
        if name(font, b"Subtype").as_deref() != Some(b"TrueType".as_slice()) {
            return Err("only a simple TrueType font can be extended so far");
        }
        let encoding = Encoding::read(doc, font)?;
        let to_unicode = UnicodeMap::read(doc, font, 1)?;
        let descriptor = doc
            .resolve(font.get(b"FontDescriptor").unwrap_or(&Object::Null))
            .as_dict()
            .ok_or("the font has no descriptor")?;
        let flags = doc
            .resolve(descriptor.get(b"Flags").unwrap_or(&Object::Null))
            .as_int()
            .unwrap_or(0);
        if flags & 4 != 0 || flags & 32 == 0 {
            return Err("the font is symbolic, so its codes do not name characters");
        }
        let program = font_file2(doc, descriptor)?;
        let num = |d: &Dict, k: &[u8]| doc.resolve(d.get(k).unwrap_or(&Object::Null)).as_number();
        let widths: Vec<f64> = doc
            .resolve(font.get(b"Widths").unwrap_or(&Object::Null))
            .as_array()
            .unwrap_or(&[])
            .iter()
            .map(|o| doc.resolve(o).as_number().unwrap_or(0.0))
            .collect();
        let first_char = num(font, b"FirstChar")
            .filter(|&f| (0.0..=255.0).contains(&f))
            .map_or(0, |f| f as u32);
        Ok(Self {
            first_char,
            widths,
            missing_width: num(descriptor, b"MissingWidth").unwrap_or(0.0),
            program,
            to_unicode,
            encoding,
        })
    }
}

/// The object id of the font `font_name` selects in `resources`; the
/// copy-on-write rewrite needs one to replace.
pub(crate) fn font_object(
    doc: &DocumentView<'_>,
    resources: &Dict,
    font_name: &[u8],
) -> Result<ObjId, String> {
    doc.resolve(resources.get(b"Font").unwrap_or(&Object::Null))
        .as_dict()
        .and_then(|f| f.get(font_name))
        .and_then(Object::as_reference)
        .ok_or_else(|| "the font dictionary is not a separate object".to_owned())
}

/// The decoded `/FontFile2` program `descriptor` embeds.
pub(crate) fn font_file2(
    doc: &DocumentView<'_>,
    descriptor: &Dict,
) -> Result<Vec<u8>, &'static str> {
    match doc.resolve(descriptor.get(b"FontFile2").unwrap_or(&Object::Null)) {
        Object::Stream(s) => doc
            .slice(s.data_span)
            .and_then(|raw| crate::filters::decode_stream(&s.dict, raw).ok()),
        _ => None,
    }
    .ok_or("the embedded program could not be read")
}

/// Every code shown by font object `font` on any page, any form a page
/// invokes, or any annotation appearance.
///
/// `Err` when some surface that could show it cannot be read — then "unused"
/// cannot be proven, which is a decision 172 guard.
pub(crate) fn codes_shown(doc: &DocumentView<'_>, font: ObjId) -> Result<BTreeSet<u32>, String> {
    let unproven = |what: &str| format!("{what}, so the new code cannot be proven unused");
    let pages =
        crate::page_tree::pages_in(doc).map_err(|_| unproven("the page tree is unreadable"))?;
    let mut shown = BTreeSet::new();
    for page in &pages {
        ensure_plain(doc, &page.resources, font).map_err(&unproven)?;
        let stream = ContentStream::from_page(doc, page)
            .map_err(|_| unproven("a page's content cannot be parsed"))?;
        collect(doc, &page.resources, &stream, font, &mut shown);
        let scan = scan_page_forms(doc, page);
        if scan.unresolved > 0 || scan.depth_overflows > 0 {
            return Err(unproven("a form XObject cannot be read"));
        }
        for form in &scan.forms {
            ensure_plain(doc, &form.resources, font).map_err(&unproven)?;
            if font_names(doc, &form.resources, font).is_empty() {
                continue;
            }
            let stream = decode_form(doc, form.id)
                .ok_or_else(|| unproven("a form XObject's content cannot be parsed"))?;
            collect(doc, &form.resources, &stream, font, &mut shown);
        }
        appearances(doc, page.id, font, &mut shown).map_err(&unproven)?;
    }
    Ok(shown)
}

/// `Ok` when no surface reaches `/ToUnicode` stream `map` except through font
/// `font`: page resources (forms included, transitively), annotation
/// appearances and the interactive form's `/DR`. Rewriting a shared map would
/// change another font's text.
pub(crate) fn map_private(doc: &DocumentView<'_>, font: ObjId, map: ObjId) -> Result<(), String> {
    let shared = || "the /ToUnicode map may be shared with another font".to_owned();
    let pages = crate::page_tree::pages_in(doc).map_err(|_| shared())?;
    let reached = |o: &Object| reaches_avoiding(doc, o, map, Some(font));
    for page in &pages {
        if reached(&Object::Dict(page.resources.clone())) {
            return Err(shared());
        }
        let annots = doc
            .resolved(page.id)
            .as_dict()
            .and_then(|p| p.get(b"Annots"))
            .map(|a| doc.resolve(a))
            .and_then(Object::as_array)
            .unwrap_or(&[]);
        let ap = |a: &Object| doc.resolve(a).as_dict().and_then(|d| d.get(b"AP")).cloned();
        if annots.iter().filter_map(ap).any(|a| reached(&a)) {
            return Err(shared());
        }
    }
    let dr = doc
        .catalog_dict()
        .and_then(|c| c.get(b"AcroForm"))
        .map(|a| doc.resolve(a))
        .and_then(Object::as_dict)
        .and_then(|a| a.get(b"DR"));
    match dr {
        Some(dr) if reached(dr) => Err(shared()),
        _ => Ok(()),
    }
}

fn collect(
    doc: &DocumentView<'_>,
    resources: &Dict,
    stream: &ContentStream,
    font: ObjId,
    shown: &mut BTreeSet<u32>,
) {
    let names = font_names(doc, resources, font);
    if names.is_empty() {
        return;
    }
    let recs = walk_records(doc, resources, stream);
    for name in names {
        shown.extend(carried_codes(&recs, &name));
    }
}

/// The `/Font` resource names in `resources` that reference `font`.
fn font_names(doc: &DocumentView<'_>, resources: &Dict, font: ObjId) -> Vec<Vec<u8>> {
    doc.resolve(resources.get(b"Font").unwrap_or(&Object::Null))
        .as_dict()
        .map(|fonts| {
            fonts
                .iter()
                .filter(|(_, v)| v.as_reference() == Some(font))
                .map(|(k, _)| k.as_bytes().to_vec())
                .collect()
        })
        .unwrap_or_default()
}

/// Refuse a surface whose patterns, graphics states or Type 3 fonts reach
/// `font`: their content is not walked here.
fn ensure_plain(doc: &DocumentView<'_>, resources: &Dict, font: ObjId) -> Result<(), &'static str> {
    for key in [b"Pattern".as_slice(), b"ExtGState"] {
        if let Some(v) = resources.get(key)
            && reaches(doc, v, font)
        {
            return Err("a pattern or soft mask may show the font");
        }
    }
    let fonts = doc.resolve(resources.get(b"Font").unwrap_or(&Object::Null));
    for (_, f) in fonts.as_dict().map(Dict::iter).into_iter().flatten() {
        if f.as_reference() == Some(font) {
            continue;
        }
        if let Some(res) = doc.resolve(f).as_dict().and_then(|d| d.get(b"Resources"))
            && reaches(doc, res, font)
        {
            return Err("a Type 3 font may show the font");
        }
    }
    Ok(())
}

/// Annotation appearance streams on `page` that name `font` directly are
/// walked; one that reaches it any other way is unproven.
fn appearances(
    doc: &DocumentView<'_>,
    page: ObjId,
    font: ObjId,
    shown: &mut BTreeSet<u32>,
) -> Result<(), &'static str> {
    let Some(annots) = doc
        .resolved(page)
        .as_dict()
        .and_then(|p| p.get(b"Annots"))
        .map(|a| doc.resolve(a))
        .and_then(Object::as_array)
    else {
        return Ok(());
    };
    for annot in annots {
        let Some(ap) = doc
            .resolve(annot)
            .as_dict()
            .and_then(|a| a.get(b"AP"))
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
        else {
            continue;
        };
        for (_, entry) in ap.iter() {
            let streams: Vec<&Object> = match doc.resolve(entry) {
                Object::Dict(states) => states.iter().map(|(_, s)| s).collect(),
                _ => vec![entry],
            };
            for s in streams {
                let Object::Stream(st) = doc.resolve(s) else {
                    continue;
                };
                let Some(res) = st.dict.get(b"Resources") else {
                    continue;
                };
                if !reaches(doc, res, font) {
                    continue;
                }
                let Some(res) = doc.resolve(res).as_dict() else {
                    return Err("an annotation appearance may show the font");
                };
                let plain = ["XObject", "Pattern", "ExtGState"]
                    .iter()
                    .all(|k| res.get(k.as_bytes()).is_none_or(|v| !reaches(doc, v, font)));
                let stream = doc
                    .slice(st.data_span)
                    .and_then(|raw| crate::filters::decode_stream(&st.dict, raw).ok())
                    .and_then(|d| ContentStream::parse(d).ok());
                match (plain, stream) {
                    (true, Some(stream)) => collect(doc, res, &stream, font, shown),
                    _ => return Err("an annotation appearance may show the font"),
                }
            }
        }
    }
    Ok(())
}

fn decode_form(doc: &DocumentView<'_>, id: ObjId) -> Option<ContentStream> {
    let Some(Object::Stream(form)) = doc.graph().value(id) else {
        return None;
    };
    let raw = doc.slice(form.data_span)?;
    let decoded = crate::filters::decode_stream(&form.dict, raw).ok()?;
    ContentStream::parse(decoded).ok()
}

/// Whether the object graph under `start` references `font`. Exhausting the
/// budget answers `true`: an unproven "no" is a "maybe".
fn reaches(doc: &DocumentView<'_>, start: &Object, font: ObjId) -> bool {
    reaches_avoiding(doc, start, font, None)
}

/// [`reaches`], not looking through object `skip`.
fn reaches_avoiding(
    doc: &DocumentView<'_>,
    start: &Object,
    font: ObjId,
    skip: Option<ObjId>,
) -> bool {
    let mut seen: BTreeSet<ObjId> = skip.into_iter().collect();
    let mut stack: Vec<&Object> = vec![start];
    let mut budget = REACH_BUDGET;
    while let Some(o) = stack.pop() {
        budget = match budget.checked_sub(1) {
            Some(b) => b,
            None => return true,
        };
        match o {
            Object::Reference(id) => {
                if *id == font {
                    return true;
                }
                if seen.insert(*id)
                    && let Some(v) = doc.graph().value(*id)
                {
                    stack.push(v);
                }
            }
            Object::Array(a) => stack.extend(a.iter()),
            Object::Dict(d) => stack.extend(d.iter().map(|(_, v)| v)),
            Object::Stream(s) => stack.extend(s.dict.iter().map(|(_, v)| v)),
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_width_fills_gaps_with_missing_width() {
        let mut w = vec![667.0, 600.0, 722.0];
        let mut first = 65;
        set_width(&mut w, &mut first, 68, 656.0, 0.0);
        assert_eq!((first, w.clone()), (65, vec![667.0, 600.0, 722.0, 656.0]));
        set_width(&mut w, &mut first, 32, 278.0, 5.0);
        assert_eq!(first, 32);
        assert_eq!(w.len(), 37);
        assert_eq!(
            (w.first(), w.get(1), w.get(33)),
            (Some(&278.0), Some(&5.0), Some(&667.0))
        );
    }

    #[test]
    fn a_zero_width_counts_as_absent() {
        assert_eq!(width_at(&[600.0, 0.0], 65, 66), None);
        assert_eq!(width_at(&[600.0, 0.0], 65, 65), Some(600.0));
        assert_eq!(width_at(&[600.0], 65, 64), None);
    }
}
