//! The key-set rule for a `/FreeText` restyle: the redraw writes back only
//! what the verb changed (`markup_rebake_keys`), keeps the callout keys as
//! the source wrote them, and names the keys it keeps but cannot draw.

use super::markup_rebake_keys::{AddedKey, Rewrites};
use super::{EditSession, StyleEdit, TextAnnotStyle};
use crate::annot_author;
use crate::object::{Dict, Name, Object};

/// The callout keys (ISO 32000-2 §12.5.6.6 Table 177). The redraw reads and
/// draws them; the dictionary keeps the source's own values, including an
/// `/LE` style pdfcer does not draw.
const CALLOUT_KEYS: [&[u8]; 4] = [b"IT", b"CL", b"LE", b"RD"];

/// What [`EditSession::keep_free_text_source_keys`] could not keep as it was.
#[derive(Debug, Default)]
pub(super) struct FreeTextKeysKept {
    /// `/BS` was added: the width or dash changed and the source held it in
    /// neither `/BS` nor `/Border`.
    pub(super) border_style_added: bool,
    /// Keys kept in the dictionary that the new appearance does not depict.
    pub(super) keys_not_drawn: Vec<String>,
}

impl EditSession {
    /// Patch a restyled `/FreeText`'s `updated` dictionary back to
    /// `current`'s key set, writing only the properties `style` names.
    pub(super) fn keep_free_text_source_keys(
        &self,
        current: &Dict,
        updated: &mut Dict,
        style: &TextAnnotStyle,
    ) -> FreeTextKeysKept {
        for key in CALLOUT_KEYS {
            match current.get(key) {
                Some(value) => updated.insert(Name::from(key), value.clone()),
                None => {
                    updated.remove(key);
                }
            }
        }
        // A border removed to width 0 writes no `/BS`; record the 0 so the
        // width is patched where the source holds it.
        if let Some(w) = style.border_width
            && !updated.contains_key(b"BS")
        {
            let mut bs = Dict::new();
            bs.insert(Name::from(b"W"), Object::Real(w.max(0.0)));
            updated.insert(Name::from(b"BS"), Object::Dict(bs));
        }
        let rewrites = Rewrites {
            stroke: style.color.is_some() || style.border_width.is_some(),
            interior: style.fill.is_some(),
            dash: style.dash.as_ref().map(|d| match d {
                StyleEdit::Set(d) => Some(d.clone()),
                StyleEdit::Clear => None,
            }),
            ..Rewrites::default()
        };
        let added = self.keep_markup_source_keys(current, updated, &rewrites);
        FreeTextKeysKept {
            border_style_added: added == AddedKey::BorderStyle,
            keys_not_drawn: annot_author::free_text_keys_not_drawn(&self.graph(), current),
        }
    }
}
