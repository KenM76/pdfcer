//! A whole block's replacement text encoded in its first run's font, for
//! [`crate::text_edit::block_text`]: the same encode, code allocation,
//! subset extension and same-face sibling search an `edit_text` of that run
//! performs (§9.4.3, §9.7.6.2, §9.10.3).

use super::{
    EditError, EditOptions, EditPlanTarget, EditRequest, FontExtension, FontWrites, OpRec, Rec,
    ShowData, anchor_font, encode_with_sibling, find_anchor,
};
use crate::object::Dict;
use crate::span::ByteSpan;
use crate::text_extract::font::ExtractFont;
use crate::view::DocumentView;

/// The block text as codes in the font the commit sets it in.
pub(crate) struct BlockEncoding {
    /// One code per character of the encoded text, in order.
    pub(crate) codes: Vec<u32>,
    /// The `Tf` resource name: the run's own, or a same-face sibling's.
    pub(crate) font_resource: Vec<u8>,
    pub(crate) font: ExtractFont,
    /// The font dictionary as the commit leaves it.
    pub(crate) font_dict: Dict,
    pub(crate) writes: FontWrites,
    /// Decision 173's new program, decoded, when the commit replaces it.
    pub(crate) program: Option<Vec<u8>>,
    /// Characters the font gained a glyph, code or `/ToUnicode` entry for.
    pub(crate) glyphs_added: Vec<char>,
    /// `/BaseFont` of the run's own font when a sibling carries the text.
    pub(crate) substituted_from: Option<String>,
    /// The anchor run's recorded state (colours, text state, matrices).
    pub(crate) anchor: ShowData,
    pub(crate) disclosures: Vec<String>,
}

/// Encode `text` in the font of the show operator `span` names.
///
/// # Errors
///
/// The [`EditError`] an `edit_text` replacing that operator's text with
/// `text` would raise: a refused character, an unresolvable or unsupported
/// font, or [`EditError::PinnedSpanNotFound`]. A refusal names every refused
/// character, not only the first: the rest are found by encoding each
/// distinct character alone.
pub(crate) fn encode_block<'a>(
    doc: &'a DocumentView<'a>,
    target: &'a EditPlanTarget,
    recs: &[OpRec],
    page_index: usize,
    span: ByteSpan,
    text: &str,
    opts: &EditOptions,
) -> Result<BlockEncoding, EditError> {
    match encode_once(doc, target, recs, page_index, span, text, opts) {
        Err(EditError::Refused(mut r)) if !r.message.contains("Also refused") => {
            let mut seen: Vec<char> = r.character.into_iter().collect();
            let mut rest = Vec::new();
            for c in text.chars().filter(|c| !c.is_whitespace()) {
                if seen.contains(&c) {
                    continue;
                }
                seen.push(c);
                let one = c.to_string();
                if let Err(EditError::Refused(o)) =
                    encode_once(doc, target, recs, page_index, span, &one, opts)
                {
                    rest.push(format!("U+{:04X} '{c}' ({})", c as u32, o.trigger.id()));
                }
            }
            if !rest.is_empty() {
                r.message
                    .push_str(&format!(" Also refused: {}.", rest.join("; ")));
            }
            Err(EditError::Refused(r))
        }
        other => other,
    }
}

fn encode_once<'a>(
    doc: &'a DocumentView<'a>,
    target: &'a EditPlanTarget,
    recs: &[OpRec],
    page_index: usize,
    span: ByteSpan,
    text: &str,
    opts: &EditOptions,
) -> Result<BlockEncoding, EditError> {
    let req = EditRequest::whole_operator(page_index, span, text);
    let index = find_anchor(recs, &req)?;
    let Some(OpRec {
        rec: Rec::Show(anchor),
        ..
    }) = recs.get(index)
    else {
        return Err(EditError::PinnedSpanNotFound {
            start: span.start,
            end: span.start.saturating_add(span.len),
        });
    };
    let (font_dict, font, class) = anchor_font(doc, target, anchor)?;
    let enc = encode_with_sibling(
        doc, target, recs, font, &class, font_dict, anchor, &req, opts, true,
    )?;
    let font_dict = enc
        .extension
        .as_ref()
        .map_or_else(|| enc.dict.clone(), |e| e.view().clone());
    let glyphs_added = enc
        .extension
        .as_ref()
        .map(|e| e.added.iter().map(|g| g.ch).collect())
        .unwrap_or_default();
    let (font_resource, substituted_from) = match enc.sibling {
        Some((name, own)) => (name, Some(own.base_font)),
        None => (anchor.font_name.clone(), None),
    };
    Ok(BlockEncoding {
        codes: enc.encoded.codes,
        font_resource,
        font: enc.font,
        font_dict,
        program: FontExtension::program_of(enc.extension.as_ref()),
        writes: FontWrites::of(enc.extension),
        glyphs_added,
        substituted_from,
        anchor: (**anchor).clone(),
        disclosures: distinct(enc.encoded.disclosures),
    })
}

/// The encoder speaks once per character occurrence; a block repeats them.
fn distinct(all: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(all.len());
    for d in all {
        if !out.contains(&d) {
            out.push(d);
        }
    }
    out
}
