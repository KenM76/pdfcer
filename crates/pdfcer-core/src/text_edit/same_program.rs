//! Decision 172 route B: characters the run's embedded TrueType program
//! outlines but its font dictionary cannot encode are set through a new
//! `/Type0` + `/CIDFontType2` resource over the same program, each glyph
//! reached by GID (`/Identity-H`, `/CIDToGIDMap /Identity`; ISO 32000-1
//! §9.7.4.2, §9.7.6.2). Decision 187: the program stream is shared only when
//! it has no `cmap`, which §9.9 says "shall not be present" under a CIDFont.

use std::collections::{BTreeMap, BTreeSet};

use crate::font_embed::{DescriptorMetrics, FontEmbedPlan, OutlineKind, SubsetGlyph};
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::text_edit::glyph_find;
use crate::text_edit::program_glyphs::EmbeddedGlyphs;
use crate::view::DocumentView;

/// Which program route B's new `/CIDFontType2` embeds (decision 187), set
/// by [`EditOptions::cid_font_program`](crate::text_edit::EditOptions::cid_font_program).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CidFontProgram {
    /// Share the run's `FontFile2` stream when the program has no `cmap`;
    /// otherwise write a new stream holding the program with only the
    /// `cmap` table removed. Conforms to §9.9 in every case.
    #[default]
    StripCmap,
    /// Always share the run's stream: the smallest file. When the program
    /// has a `cmap` the result breaks §9.9's "shall not be present", and the
    /// edit says so. A document claiming PDF/A gets [`Self::StripCmap`].
    ShareStream,
    /// Route B is not taken: such characters refuse, or go to the fallback
    /// face when one is set.
    Off,
}

/// The program route B's resource embeds, as disclosed in
/// [`FallbackUse::cid_program`](crate::text_edit::FallbackUse::cid_program).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum CidProgramUse {
    /// The run's own `FontFile2` stream; the program has no `cmap`.
    Shared,
    /// The run's own stream, which carries a `cmap` that §9.9 says shall
    /// not be present under a CIDFont: chosen by
    /// [`CidFontProgram::ShareStream`], or forced because the program could
    /// not be copied (its `OS/2.fsType` restricts embedding, or it does not
    /// parse as TrueType).
    SharedWithCmap,
    /// A new `FontFile2` stream: the program with its `cmap` removed.
    StrippedCopy,
}

/// Route B's resource for one edit.
pub(crate) struct Built {
    /// The new font; `program` is the program as embedded or shared.
    pub(crate) plan: FontEmbedPlan,
    /// Set when the run's stream is shared rather than copied.
    pub(crate) shared: Option<ObjId>,
    pub(crate) program_use: CidProgramUse,
    /// Why the requested [`CidFontProgram`] was not what happened.
    pub(crate) override_note: Option<&'static str>,
}

/// Route B over `own_dict`'s program for `chars`, or why not.
pub(crate) fn plan(
    doc: &DocumentView<'_>,
    own_dict: &Dict,
    glyphs: &dyn EmbeddedGlyphs,
    mode: CidFontProgram,
    chars: &BTreeSet<char>,
) -> Result<Built, String> {
    let base_font = own_dict
        .get(b"BaseFont")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_name)
        .map(|n| String::from_utf8_lossy(&n.0).into_owned())
        .ok_or("the run's font has no /BaseFont")?;
    let desc = descriptor(doc, own_dict).ok_or("the run's font has no font descriptor")?;
    let file = desc
        .get(b"FontFile2")
        .and_then(Object::as_reference)
        .ok_or("the run's font embeds no TrueType (/FontFile2) program")?;
    let program = crate::text_edit::font_extend::font_file2(doc, desc)?;
    let glyph_list = glyph_list(glyphs, &program, chars, &base_font)?;
    let (subset_tag, base_name) = split_tag(&base_font, doc);
    let has_cmap = crate::font_embed_missing::sfnt_has_cmap(&program);
    let pdfa = !matches!(
        crate::font_unembed::detect_pdfa(doc),
        crate::font_unembed::PdfaClaim::None
    );
    let mut override_note = None;
    let strip = match mode {
        CidFontProgram::ShareStream if pdfa && has_cmap => {
            override_note = Some(
                "the document claims PDF/A, so the program was copied without its cmap \
                 instead of shared",
            );
            true
        }
        CidFontProgram::ShareStream => false,
        _ => has_cmap,
    };
    let (program, shared, program_use) = if strip {
        match glyphs.program_without_cmap(&program) {
            Some(stripped) => (stripped, None, CidProgramUse::StrippedCopy),
            None => {
                override_note = Some(
                    "the program could not be copied without its cmap (its OS/2 fsType \
                     restricts embedding, or it is not a TrueType sfnt), so it is shared",
                );
                (program, Some(file), CidProgramUse::SharedWithCmap)
            }
        }
    } else if has_cmap {
        (program, Some(file), CidProgramUse::SharedWithCmap)
    } else {
        (program, Some(file), CidProgramUse::Shared)
    };
    let plan = FontEmbedPlan {
        program,
        base_name,
        subset_tag,
        outline_kind: OutlineKind::TrueType,
        glyphs: glyph_list,
        metrics: metrics(doc, desc),
    };
    plan.validate().map_err(|e| e.to_string())?;
    Ok(Built {
        plan,
        shared,
        program_use,
        override_note,
    })
}

/// The descriptor of a simple font, or of a `/Type0`'s descendant.
fn descriptor<'d>(doc: &'d DocumentView<'_>, font: &'d Dict) -> Option<&'d Dict> {
    let dict = match font.get(b"DescendantFonts").map(|o| doc.resolve(o)) {
        Some(Object::Array(kids)) => doc.resolve(kids.first()?).as_dict()?,
        _ => font,
    };
    doc.resolve(dict.get(b"FontDescriptor")?).as_dict()
}

/// One glyph per character, ascending by GID; the first character missing
/// an outline refuses.
fn glyph_list(
    glyphs: &dyn EmbeddedGlyphs,
    program: &[u8],
    chars: &BTreeSet<char>,
    base_font: &str,
) -> Result<Vec<SubsetGlyph>, String> {
    let mut by_gid: BTreeMap<u16, SubsetGlyph> = BTreeMap::new();
    for &ch in chars {
        let found = glyph_find::find(glyphs, program, ch, None)
            .and_then(|f| Some((u16::try_from(f.glyph.gid).ok()?, f.glyph.advance)))
            .filter(|(gid, _)| *gid != 0);
        let Some((gid, advance)) = found else {
            return Err(format!(
                "'{base_font}' embeds no outline for U+{:04X} '{ch}'",
                u32::from(ch)
            ));
        };
        #[allow(clippy::cast_possible_truncation)] // an advance in 1000-unit space
        let width = advance.round() as i32;
        by_gid.entry(gid).or_insert(SubsetGlyph {
            cid: gid,
            width,
            unicode: ch,
        });
    }
    Ok(by_gid.into_values().collect())
}

/// `ABCDEF+Name` as `(tag, name)`; a name with no §9.6.4 tag gets a fresh
/// one no other font in the file uses.
fn split_tag(base_font: &str, doc: &DocumentView<'_>) -> (String, String) {
    if crate::text_edit::edit::is_subset_tag(base_font)
        && let Some((tag, name)) = base_font.split_once('+')
        && !name.is_empty()
    {
        return (tag.to_owned(), name.to_owned());
    }
    let taken: BTreeSet<String> = crate::fontinfo::inventory(doc)
        .fonts
        .into_iter()
        .filter_map(|f| f.subset_tag)
        .collect();
    let tag = crate::text_edit::addtext::unique_subset_tag("AAAAAA", &taken);
    (tag, base_font.to_owned())
}

/// The run's descriptor metrics. The characters route B adds are ones the
/// run's encoding cannot reach, so the new descriptor is `Symbolic`
/// (§9.8.2); `FixedPitch`, `Serif` and `Italic` carry over.
fn metrics(doc: &DocumentView<'_>, desc: &Dict) -> DescriptorMetrics {
    #[allow(clippy::cast_possible_truncation)] // descriptor values are glyph-space integers
    let num = |key: &[u8], default: i32| {
        desc.get(key)
            .map(|o| doc.resolve(o))
            .and_then(Object::as_number)
            .map_or(default, |v| v.round() as i32)
    };
    let mut bbox = [0; 4];
    if let Some(Object::Array(a)) = desc.get(b"FontBBox").map(|o| doc.resolve(o)) {
        for (slot, v) in bbox.iter_mut().zip(a) {
            #[allow(clippy::cast_possible_truncation)] // glyph-space integers
            if let Some(n) = doc.resolve(v).as_number() {
                *slot = n.round() as i32;
            }
        }
    }
    let ascent = num(b"Ascent", 0);
    DescriptorMetrics {
        bbox,
        italic_angle: num(b"ItalicAngle", 0),
        ascent,
        descent: num(b"Descent", 0),
        cap_height: num(b"CapHeight", ascent),
        stem_v: num(b"StemV", 80),
        flags: (num(b"Flags", 0) & !32) | 4,
    }
}

/// [`crate::font_embed::build_objects`]'s objects at `font_id ..= font_id
/// + 3` with the program stream left out: the descriptor's `FontFile2`
/// names the existing `program`, and `/ToUnicode` takes the freed number.
/// Returns the `/Type0` dictionary and the other three objects.
///
/// # Errors
///
/// The plan fails validation, or the numbers do not fit.
pub(crate) fn shared_objects(
    plan: &FontEmbedPlan,
    font_id: ObjId,
    program: ObjId,
    mut stage: impl FnMut(&[u8]) -> crate::span::ByteSpan,
) -> Result<(Dict, Vec<(ObjId, Object)>), String> {
    use crate::object::Name;
    let cmap = plan.to_unicode_cmap();
    let to_unicode = crate::text_edit::edit::make_raw_stream(stage(&cmap), cmap.len());
    let built = crate::font_embed::build_objects(plan, font_id.num, Object::Null, Object::Null)
        .map_err(|e| e.to_string())?;
    let unicode_id = ObjId::new(
        font_id.num.checked_add(3).ok_or("no object numbers left")?,
        0,
    );
    let file_id = ObjId::new(unicode_id.num.saturating_add(1), 0);
    let mut type0 = None;
    let mut rest = Vec::new();
    for (id, obj) in built.objects {
        match obj {
            Object::Dict(mut d) if id == font_id => {
                d.insert(Name::from(b"ToUnicode"), Object::Reference(unicode_id));
                type0 = Some(d);
            }
            Object::Dict(mut d) if d.get(b"FontFile2").is_some() => {
                d.insert(Name::from(b"FontFile2"), Object::Reference(program));
                rest.push((id, Object::Dict(d)));
            }
            _ if id == unicode_id || id == file_id => {}
            obj => rest.push((id, obj)),
        }
    }
    rest.push((unicode_id, to_unicode));
    Ok((type0.ok_or("the font built no /Type0 dictionary")?, rest))
}

/// Every character route B can set over `own_dict`'s program, for
/// [`run_repertoire`](crate::edit::EditSession::run_repertoire). Empty when
/// route B is off or the run's font embeds no usable TrueType program.
pub(crate) fn accepts(
    doc: &DocumentView<'_>,
    own_dict: &Dict,
    glyphs: &dyn EmbeddedGlyphs,
    mode: CidFontProgram,
) -> BTreeSet<char> {
    let Some(program) = descriptor(doc, own_dict)
        .and_then(|d| crate::text_edit::font_extend::font_file2(doc, d).ok())
    else {
        return BTreeSet::new();
    };
    let mut chars: BTreeSet<char> = glyphs.unicode_chars(&program).into_iter().collect();
    chars.extend(glyph_find::named_chars(glyphs, &program));
    chars.retain(|&ch| {
        glyph_find::find(glyphs, &program, ch, None).is_some_and(|f| f.glyph.gid != 0)
    });
    if chars.is_empty() || plan(doc, own_dict, glyphs, mode, &chars).is_err() {
        return BTreeSet::new();
    }
    chars
}
