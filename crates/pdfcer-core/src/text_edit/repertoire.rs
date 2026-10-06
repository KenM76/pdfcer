//! Which characters one located text run will accept, asked before the first
//! keystroke instead of refused after the last (`Pass 280.0`).

use std::collections::{BTreeMap, BTreeSet};

use crate::content::ContentStream;
use crate::object::Dict;
use crate::span::ByteSpan;
use crate::text_edit::cause::UnsupportedCause;
use crate::text_edit::edit::{
    EditError, EditOptions, EditRequest, EditTarget, OpRec, Rec, Walk, carried_codes,
    classify_font, find_anchor, resolve_font_dict, writes_vertically,
};
use crate::text_edit::encoding::{CharEncoding, CompositeEncoding, InverseEncoding};
use crate::text_edit::format::{FormatError, RunRepertoire};
use crate::text_edit::{EmbeddedGlyphs, SubsetAugment, sibling};
use crate::text_extract::font::ExtractFont;
use crate::view::DocumentView;

/// The run's repertoire, computed once per pinned run and kept by the caller
/// for the life of the edit (a per-keystroke face scan would walk the whole
/// page per character).
///
/// # Contract
///
/// A character the query accepts, `encode_str` must not refuse for that run.
/// True by construction: acceptance calls the accepting code itself
/// ([`InverseEncoding::encode_char`] / [`CompositeEncoding::encode_str`]) and
/// the same `carried_codes` subset floor, in the same order, seeded with the
/// same `prefer` set (`R221`).
///
/// The repertoire comes from the run's own font resource, so an embedded
/// subset inherits the `R-INV-1` floor unless `opts.embedded_glyphs` reads
/// the program and the decision 172/173 extensions can add the character.
/// With `opts.sibling_fonts`, characters a same-face sibling resource accepts
/// are added (decision 174); the edit sets a replacement in one font, so a
/// replacement mixing characters only the run's font carries with characters
/// only a sibling carries still refuses. With `opts.fallback`, the characters
/// its face would set are added too, and also listed in
/// [`RunRepertoire::via_fallback`] (Pass 431.0).
///
/// A run with no usable encoding answers an empty [`RunRepertoire`] with
/// [`RunRepertoire::reason`] set, not an error; a run that cannot be located
/// is an `Err`.
pub(crate) fn run_repertoire(
    doc: &DocumentView<'_>,
    page: &crate::page_tree::Page,
    stream: &ContentStream,
    find: &str,
    pinned_span: Option<ByteSpan>,
    opts: &EditOptions,
) -> Result<RunRepertoire, FormatError> {
    let mut walk = Walk::new(doc, &page.resources);
    for op in stream.operations() {
        walk.operation(&op, &stream.buf);
    }
    let recs = walk.recs;
    // The edit path's own location, so "the run" means the same run in both.
    let locate = EditRequest {
        page_index: 0,
        find: find.to_owned(),
        replace: String::new(),
        pinned_span,
        target: EditTarget::Auto,
        span_from_pin: false,
    };
    let anchor_index = find_anchor(&recs, &locate).map_err(FormatError::from_edit)?;
    let Some(OpRec {
        rec: Rec::Show(anchor),
        ..
    }) = recs.get(anchor_index)
    else {
        return Err(FormatError::NoMatch(find.to_owned()));
    };
    let orig_dict =
        resolve_font_dict(doc, &page.resources, &anchor.font_name).ok_or_else(|| {
            FormatError::Unsupported(
                "the run's font resource is unresolvable (outlined/vector art has no font to \
                 format)"
                    .to_owned(),
            )
        })?;
    let font = ExtractFont::resolve(doc, orig_dict);
    let mut out = empty_repertoire(&font, anchor, find, pinned_span);
    // The R-INV-5 tie-break seed: it decides WHICH code, never WHETHER.
    let prefer: BTreeSet<u8> = anchor
        .slots
        .iter()
        .filter_map(|s| u8::try_from(s.code).ok())
        .collect();
    let query = FontQuery {
        doc,
        resources: &page.resources,
        recs: &recs,
        name: &anchor.font_name,
        dict: orig_dict,
        prefer: &prefer,
        glyphs: opts.embedded_glyphs,
        augment: opts.subset_augment.as_ref(),
    };
    let own = match font_accepts(&query) {
        Ok(a) => a,
        Err((reason, cause)) => {
            out.reason = Some(reason);
            out.cause = cause;
            return Ok(out);
        }
    };
    out.accepted = own.accepted;
    out.ambiguous = own.ambiguous;
    out.embedded_subset = own.embedded_subset;
    out.candidates_tested = own.candidates_tested;
    add_other_fonts(&mut out, &query, anchor, &font.base_font, opts);
    if out.accepted.is_empty() {
        out.reason = Some(empty_reason(out.candidates_tested));
    }
    Ok(out)
}

/// What `opts.sibling_fonts` and `opts.fallback` add: characters set in a
/// font other than the run's, only for a match inside one show operator.
fn add_other_fonts(
    out: &mut RunRepertoire,
    query: &FontQuery<'_>,
    anchor: &crate::text_edit::edit::ShowData,
    base_font: &str,
    opts: &EditOptions,
) {
    if !sibling::splittable(anchor, true) {
        return;
    }
    if opts.sibling_fonts {
        out.accepted.extend(sibling_accepts(query, base_font));
    }
    if let Some(face) = opts.fallback {
        let at = crate::text_edit::fallback::RunAt {
            doc: query.doc,
            resources: query.resources,
            recs: query.recs,
            own_dict: query.dict,
            anchor,
        };
        out.via_fallback = crate::text_edit::fallback::face_accepts(&at, face)
            .into_iter()
            .filter(|ch| !out.accepted.contains(ch))
            .collect();
        out.accepted.extend(out.via_fallback.iter().copied());
    }
    if let Some(glyphs) = query.glyphs
        && opts.cid_font_program != crate::text_edit::CidFontProgram::Off
    {
        let route_b = crate::text_edit::same_program::accepts(
            query.doc,
            query.dict,
            glyphs,
            opts.cid_font_program,
        );
        out.accepted.extend(route_b);
    }
}

/// The run's identity, with nothing accepted yet.
fn empty_repertoire(
    font: &ExtractFont,
    anchor: &crate::text_edit::edit::ShowData,
    find: &str,
    pinned_span: Option<ByteSpan>,
) -> RunRepertoire {
    RunRepertoire {
        base_font: font.base_font.clone(),
        resource: String::from_utf8_lossy(&anchor.font_name).into_owned(),
        // Resolved: an empty pinned `find` means the whole operator.
        text: crate::text_edit::edit::effective_find(anchor, find, pinned_span).to_owned(),
        accepted: BTreeSet::new(),
        via_fallback: BTreeSet::new(),
        ambiguous: BTreeMap::new(),
        embedded_subset: false,
        candidates_tested: 0,
        reason: None,
        cause: None,
    }
}

/// One font resource's repertoire inputs.
struct FontQuery<'a> {
    doc: &'a DocumentView<'a>,
    resources: &'a Dict,
    recs: &'a [OpRec],
    name: &'a [u8],
    dict: &'a Dict,
    prefer: &'a BTreeSet<u8>,
    glyphs: Option<&'static dyn EmbeddedGlyphs>,
    augment: Option<&'a SubsetAugment>,
}

/// One font resource's answer.
struct Accepts {
    accepted: BTreeSet<char>,
    ambiguous: BTreeMap<char, Vec<u32>>,
    candidates_tested: usize,
    embedded_subset: bool,
}

/// A refusal: the reason, and its structured cause when it has one.
type Refused = (String, Option<UnsupportedCause>);

/// What the same-face siblings of `q`'s font accept (decision 174).
fn sibling_accepts(q: &FontQuery<'_>, base_font: &str) -> BTreeSet<char> {
    let vertical = writes_vertically(q.doc, q.dict);
    let mut accepted = BTreeSet::new();
    for (name, dict) in sibling::candidates(q.doc, q.resources, q.dict, base_font) {
        if writes_vertically(q.doc, dict) != vertical {
            continue;
        }
        let sib = FontQuery {
            name: &name,
            dict,
            ..*q
        };
        if let Ok(a) = font_accepts(&sib) {
            accepted.extend(a.accepted);
        }
    }
    accepted
}

fn font_accepts(q: &FontQuery<'_>) -> Result<Accepts, Refused> {
    let font = ExtractFont::resolve(q.doc, q.dict);
    // classify_font's refusals are the "this editor should not open" cases,
    // answered empty rather than as an error.
    let class = match classify_font(q.doc, q.dict, &font) {
        Ok(c) => c,
        Err(EditError::Unsupported(cause)) => return Err((cause.to_string(), Some(cause))),
        Err(e) => return Err((e.to_string(), None)),
    };
    let embedded_subset = class.embedded && class.subset;
    // The subset floor is a fact about the PAGE, not the face.
    let carried = if embedded_subset {
        carried_codes(q.recs, q.name)
    } else {
        BTreeSet::new()
    };
    let ((accepted, candidates_tested), ambiguous) = if font.is_simple() {
        (
            simple_accepts(q, &font, embedded_subset, &carried)?,
            BTreeMap::new(),
        )
    } else {
        composite_accepts(q, &font, embedded_subset, &carried)?
    };
    Ok(Accepts {
        accepted,
        ambiguous,
        candidates_tested,
        embedded_subset,
    })
}

/// A simple font. `encode_char` cannot refuse a candidate here except under
/// R-INV-8, so the floor does the discriminating; it is called anyway because
/// it is the accepting code and where the code comes from (R221).
fn simple_accepts(
    q: &FontQuery<'_>,
    font: &ExtractFont,
    embedded_subset: bool,
    carried: &BTreeSet<u32>,
) -> Result<(BTreeSet<char>, usize), Refused> {
    let Some(glyph_names) = font.glyph_names() else {
        let cause = UnsupportedCause::EncodingNotInvertible;
        return Err((cause.to_string(), Some(cause)));
    };
    let inverse = InverseEncoding::build(&font.base_font, glyph_names);
    let mut accepted = BTreeSet::new();
    let mut tested = 0usize;
    let mut uncarried: Vec<(char, u32)> = Vec::new();
    for ch in inverse.candidate_chars() {
        tested += 1;
        let code = match inverse.encode_char(ch, q.prefer) {
            CharEncoding::Code(c) | CharEncoding::Chosen { code: c, .. } => c,
            CharEncoding::Refuse(_) => continue,
        };
        if embedded_subset && !carried.contains(&u32::from(code)) {
            uncarried.push((ch, u32::from(code)));
            continue;
        }
        accepted.insert(ch);
    }
    if let Some(g) = q.glyphs.filter(|_| !uncarried.is_empty()) {
        accepted.extend(crate::text_edit::font_extend::addable(
            q.doc,
            q.resources,
            q.name,
            q.dict,
            &uncarried,
            g,
        ));
        if let Some(settings) = q.augment {
            uncarried.retain(|(ch, _)| !accepted.contains(ch));
            let at = crate::text_edit::augment_route::FontAt {
                resources: q.resources,
                font_name: q.name,
                font_dict: q.dict,
            };
            accepted.extend(crate::text_edit::augment_route::augmentable(
                q.doc, &at, &uncarried, g, settings,
            ));
        }
    }
    if let Some(g) = q.glyphs.filter(|_| embedded_subset) {
        accepted.extend(crate::text_edit::code_alloc::allocatable(
            q.doc,
            q.resources,
            q.name,
            q.dict,
            |ch| inverse.has_char(ch),
            g,
        ));
    }
    Ok((accepted, tested))
}

/// A composite font: the floor is the same question asked of CIDs.
fn composite_accepts(
    q: &FontQuery<'_>,
    font: &ExtractFont,
    embedded_subset: bool,
    carried: &BTreeSet<u32>,
) -> Result<((BTreeSet<char>, usize), BTreeMap<char, Vec<u32>>), Refused> {
    let Some(cmap) = font.to_unicode_cmap() else {
        let cause = UnsupportedCause::CompositeWithoutToUnicode;
        return Err((cause.to_string(), Some(cause)));
    };
    let Ok(composite) = CompositeEncoding::build(&font.base_font, cmap) else {
        let cause = UnsupportedCause::FontMapNotInvertible {
            detail: "the composite font's map".to_owned(),
        };
        return Err((cause.to_string(), Some(cause)));
    };
    let mut accepted = BTreeSet::new();
    let mut tested = 0usize;
    let mut uncarried: Vec<(char, u32)> = Vec::new();
    for ch in composite.candidate_chars() {
        tested += 1;
        let Ok(enc) = composite.encode_str(&ch.to_string()) else {
            continue;
        };
        let first = enc.cids.first().map(|&c| u32::from(c));
        if embedded_subset && first.is_some_and(|cid| !carried.contains(&cid)) {
            uncarried.extend(first.map(|cid| (ch, cid)));
            continue;
        }
        accepted.insert(ch);
    }
    if let Some(g) = q.glyphs.filter(|_| embedded_subset) {
        if !uncarried.is_empty() {
            accepted.extend(crate::text_edit::font_extend::addable(
                q.doc,
                q.resources,
                q.name,
                q.dict,
                &uncarried,
                g,
            ));
        }
        accepted.extend(
            crate::text_edit::cid_extend::allocatable(q.doc, q.resources, q.name, q.dict, g)
                .into_iter()
                .filter(|ch| !composite.covers(*ch))
                .filter(|ch| !composite.ambiguous_chars().contains_key(ch)),
        );
        if let Some(settings) = q.augment {
            let at = crate::text_edit::augment_route::FontAt {
                resources: q.resources,
                font_name: q.name,
                font_dict: q.dict,
            };
            let answered = accepted.clone();
            let excluded = |ch: char| {
                answered.contains(&ch)
                    || composite.covers(ch)
                    || composite.ambiguous_chars().contains_key(&ch)
            };
            accepted.extend(crate::text_edit::cid_augment::augmentable(
                q.doc, &at, g, settings, excluded,
            ));
        }
    }
    Ok(((accepted, tested), composite.ambiguous_chars().clone()))
}

/// An empty answer still owes a reason: a font whose every character is
/// produced by more than one code (R-INV-4) is not a font that addresses
/// nothing.
fn empty_reason(candidates_tested: usize) -> String {
    if candidates_tested == 0 {
        "this font addresses no character unambiguously — every entry in its map is produced \
         by more than one code (R-INV-4), so pdfcer cannot tell which code means which \
         character"
            .to_owned()
    } else {
        format!(
            "none of the {candidates_tested} character(s) this font addresses can be shown by \
             this run — an embedded subset carries only the codes already drawn on this page \
             (R-INV-1)"
        )
    }
}
