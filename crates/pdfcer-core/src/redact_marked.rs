//! Replacement text on a marked-content sequence whose glyphs were redacted.
//!
//! A `BDC` property list may carry `/ActualText`, `/Alt` or `/E` (ISO
//! 32000-1 §14.9.3–§14.9.5, Table 323), text that a reader extracts *instead
//! of* the glyphs inside the sequence. Removing the glyphs while leaving that
//! entry leaves the redacted words readable, so when any glyph inside a
//! sequence is removed, the sequence loses all three entries. The kept glyphs
//! then extract through their own encoding.
//!
//! An inline property list is rewritten in place. A named one (a
//! `/Properties` resource, §14.6.2) is replaced by an inline copy without the
//! entries, which leaves the shared resource alone. An inline property list
//! may hold only direct objects, so a named list that references another
//! object is left as it is, and the residual sweep reports it.

use super::{Edit, Surgeon};
use crate::content::{ContentToken, ContentTokenKind};
use crate::object::{Dict, ObjId, Object};

/// The replacement-text keys of Table 323.
const REPLACEMENT_KEYS: [&[u8]; 3] = [b"ActualText", b"Alt", b"E"];

/// A `BDC` whose property list carries replacement text, open until its `EMC`.
pub(super) struct MarkedOpen {
    /// The property-list operand's byte range in the content buffer.
    span: (usize, usize),
    /// The property list with the replacement keys removed.
    stripped: Dict,
    /// `glyphs_removed` when the sequence opened.
    removed_at_open: u64,
}

impl Surgeon<'_> {
    /// `BDC`: open a sequence. Every sequence is pushed (even one without
    /// replacement text) so that `EMC` pops the matching entry.
    pub(super) fn marked_open(&mut self, operands: &[ContentToken]) {
        let entry = operands.get(1).and_then(|t| self.replacement_list(t));
        self.marked.push(entry);
    }

    /// `EMC`: close the innermost sequence.
    pub(super) fn marked_close(&mut self) {
        if let Some(Some(open)) = self.marked.pop() {
            self.strip_if_redacted(open);
        }
    }

    /// End of stream: an unbalanced `BDC` still covers what follows it.
    pub(super) fn marked_close_all(&mut self) {
        while !self.marked.is_empty() {
            self.marked_close();
        }
    }

    fn strip_if_redacted(&mut self, open: MarkedOpen) {
        if self.glyphs_removed == open.removed_at_open {
            return;
        }
        let mut bytes = Vec::new();
        crate::writer::serialize::write_object(
            &mut bytes,
            &Object::Dict(open.stripped),
            ObjId::new(0, 0),
            &[],
            &crate::writer::IdentityEncoder,
        );
        self.edits.push(Edit {
            start: open.span.0,
            end: open.span.1,
            bytes,
        });
        self.replacement_text_stripped += 1;
    }

    /// The property list `token` names or holds, if it carries replacement
    /// text and can be written inline without it.
    fn replacement_list(&self, token: &ContentToken) -> Option<MarkedOpen> {
        let ContentTokenKind::Operand(operand) = &token.kind else {
            return None;
        };
        let dict = match operand {
            Object::Dict(d) => d.clone(),
            Object::Name(n) => self.named_property_list(n.as_bytes())?,
            _ => return None,
        };
        if !REPLACEMENT_KEYS.iter().any(|k| dict.get(k).is_some()) {
            return None;
        }
        let mut stripped = dict;
        for k in REPLACEMENT_KEYS {
            stripped.remove(k);
        }
        let named = matches!(operand, Object::Name(_));
        if named && stripped.iter().any(|(_, v)| has_reference(v)) {
            return None;
        }
        Some(MarkedOpen {
            span: (token.span.start, token.span.end()),
            stripped,
            removed_at_open: self.glyphs_removed,
        })
    }

    fn named_property_list(&self, name: &[u8]) -> Option<Dict> {
        let props = self
            .resources
            .get(b"Properties")
            .map(|o| self.doc.resolve(o))
            .and_then(Object::as_dict)?;
        props
            .get(name)
            .map(|o| self.doc.resolve(o))
            .and_then(Object::as_dict)
            .cloned()
    }
}

fn has_reference(obj: &Object) -> bool {
    match obj {
        Object::Reference(_) => true,
        Object::Array(items) => items.iter().any(has_reference),
        Object::Dict(d) => d.iter().any(|(_, v)| has_reference(v)),
        _ => false,
    }
}
