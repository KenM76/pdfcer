//! Several form fields on one clipboard: one shared object closure, so a
//! resource two fields use (a `/DR` font, a shared appearance) travels once
//! and lands once.

use super::{
    FIELD_CLIP_VERSION, FieldClip, FieldPasteOutcome, PasteTooltip, Reader, put_bytes, put_objects,
    put_u32, read_objects,
};
use crate::object::{Name, Object};
use crate::page_tree::Rect;
use crate::span::ByteSpan;
use crate::vector::clip::{ClipError, ClipObject};
use std::collections::BTreeMap;

/// The 12-byte signature a [`FieldClipSet`] payload begins with. Distinct
/// from [`FIELD_CLIP_MAGIC`](super::FIELD_CLIP_MAGIC), so a single-field
/// payload is never misread as a set or the reverse.
pub const FIELD_CLIP_SET_MAGIC: &[u8; 12] = b"PDFCEFST\x00\x00\x00\x01";

/// The most fields one [`FieldClipSet`] carries — the undo-fold limit, since a
/// paste lands every field in one undo entry.
pub const MAX_CLIP_SET_FIELDS: usize = crate::edit::MAX_UNDO_DEPTH;

/// How deep a `/AA` → `/C` → `/JS` reference chain is followed.
const MAX_JS_CHAIN: usize = 16;

/// Several copied form fields and the one object closure they share
/// (`EditSession::copy_fields`).
///
/// Each field keeps its source rectangles, so the selection's layout travels
/// with it; [`Self::bbox`] is the union a paste translates to its origin.
/// [`Self::to_bytes`] carries the set to another process, like
/// [`FieldClip::to_bytes`].
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FieldClipSet {
    /// The fields, in copy order. Their own `objects` are empty; their
    /// clip-local references index [`Self::objects`].
    pub(crate) fields: Vec<FieldClip>,
    /// The shared closure.
    pub(crate) objects: BTreeMap<u32, ClipObject>,
}

impl FieldClipSet {
    /// How many fields the set carries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether the set carries no field.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// The source fields' fully-qualified names, in copy order.
    #[must_use]
    pub fn source_names(&self) -> Vec<&str> {
        self.fields.iter().map(FieldClip::source_name).collect()
    }

    /// How many widgets the set carries across all its fields.
    #[must_use]
    pub fn widget_count(&self) -> usize {
        self.fields.iter().map(FieldClip::widget_count).sum()
    }

    /// How many objects the shared closure holds — a resource two fields
    /// share is counted once.
    #[must_use]
    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    /// The union of every source widget rectangle; `None` for an empty set.
    #[must_use]
    pub fn bbox(&self) -> Option<Rect> {
        self.fields
            .iter()
            .filter_map(FieldClip::bbox)
            .reduce(|a, b| Rect {
                llx: a.llx.min(b.llx),
                lly: a.lly.min(b.lly),
                urx: a.urx.max(b.urx),
                ury: a.ury.max(b.ury),
            })
    }

    /// Serialise the set: [`FIELD_CLIP_SET_MAGIC`], the format version, each
    /// field as a [`FieldClip`] payload with an empty object table, then the
    /// shared table once.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(FIELD_CLIP_SET_MAGIC);
        put_u32(&mut out, FIELD_CLIP_VERSION);
        put_u32(
            &mut out,
            u32::try_from(self.fields.len()).unwrap_or(u32::MAX),
        );
        for field in &self.fields {
            put_bytes(&mut out, &field.to_bytes());
        }
        put_objects(&mut out, &self.objects);
        out
    }

    /// Parse a payload written by [`Self::to_bytes`].
    ///
    /// # Errors
    ///
    /// [`ClipError::NotAClip`] when the magic does not match (checked first);
    /// [`ClipError::NewerFormat`] for a newer build's payload;
    /// [`ClipError::ClipTooLarge`] when a count exceeds
    /// [`MAX_CLIP_SET_FIELDS`] or [`MAX_CLIP_OBJECTS`](super::MAX_CLIP_OBJECTS);
    /// [`ClipError::Truncated`] / [`ClipError::Content`] for a malformed body,
    /// including a member that carries its own objects.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ClipError> {
        let mut r = Reader::new(bytes);
        if r.take(FIELD_CLIP_SET_MAGIC.len())? != FIELD_CLIP_SET_MAGIC.as_slice() {
            return Err(ClipError::NotAClip);
        }
        let version = r.u32()?;
        if version > FIELD_CLIP_VERSION {
            return Err(ClipError::NewerFormat {
                found: version,
                supported: FIELD_CLIP_VERSION,
            });
        }
        let count = r.u32()? as usize;
        if count > MAX_CLIP_SET_FIELDS {
            return Err(ClipError::ClipTooLarge {
                found: count,
                limit: MAX_CLIP_SET_FIELDS,
            });
        }
        let mut fields = Vec::with_capacity(count);
        for _ in 0..count {
            let field = FieldClip::from_bytes(&r.bytes()?)?;
            if !field.objects.is_empty() {
                return Err(ClipError::Content(
                    "a field in a field set carries its own objects".to_owned(),
                ));
            }
            fields.push(field);
        }
        let objects = read_objects(&mut r)?;
        Ok(Self { fields, objects })
    }

    /// Run `f` on member `index` with the shared object table lent to it, so
    /// the single-field paste path sees a self-contained clip. `None` when
    /// `index` is out of range.
    pub(crate) fn with_member<R>(
        &mut self,
        index: usize,
        f: impl FnOnce(&FieldClip) -> R,
    ) -> Option<R> {
        let member = self.fields.get_mut(index)?;
        member.objects = std::mem::take(&mut self.objects);
        let out = f(member);
        self.objects = std::mem::take(&mut member.objects);
        Some(out)
    }

    /// Point every member's calculate script (`/AA /C /JS`) at the renamed
    /// copies: a quoted string literal equal to a key of `renames` becomes its
    /// value. Returns the source names of the members whose script changed.
    ///
    /// Only whole quoted literals are rewritten — `"A"` and `'A'`, never `A`
    /// inside a longer token — so a script that names a field the batch does
    /// not carry is left alone.
    pub(crate) fn repoint_calculations(
        &mut self,
        renames: &BTreeMap<String, String>,
    ) -> Vec<String> {
        let mut changed = Vec::new();
        for field in &mut self.fields {
            let mut value = Object::Dict(std::mem::take(&mut field.field));
            let keys: [&[u8]; 3] = [b"AA", b"C", b"JS"];
            if rewrite_path(&mut value, None, &mut self.objects, &keys, renames, 0) {
                changed.push(field.source_name.clone());
            }
            if let Object::Dict(dict) = value {
                field.field = dict;
            }
        }
        changed
    }
}

/// Follow `keys` from `value` through clip-local references, rewriting the
/// script at the end. `payload` is the stream data when `value` is a whole
/// clip object.
fn rewrite_path(
    value: &mut Object,
    payload: Option<&mut Option<Vec<u8>>>,
    objects: &mut BTreeMap<u32, ClipObject>,
    keys: &[&[u8]],
    renames: &BTreeMap<String, String>,
    depth: usize,
) -> bool {
    if depth > MAX_JS_CHAIN {
        return false;
    }
    if let Object::Reference(id) = value {
        // Taken out while it is walked: a reference cycle then finds nothing.
        let Some(mut entry) = objects.remove(&id.num) else {
            return false;
        };
        let hit = rewrite_path(
            &mut entry.value,
            Some(&mut entry.payload),
            objects,
            keys,
            renames,
            depth + 1,
        );
        objects.insert(id.num, entry);
        return hit;
    }
    let Some((key, rest)) = keys.split_first() else {
        return rewrite_script(value, payload, renames);
    };
    let Object::Dict(dict) = value else {
        return false;
    };
    dict.0
        .iter_mut()
        .find(|(k, _)| k.as_bytes() == *key)
        .is_some_and(|(_, v)| rewrite_path(v, None, objects, rest, renames, depth + 1))
}

/// Rewrite a `/JS` value: a string in place, a stream by decoding it and
/// storing the result unfiltered.
fn rewrite_script(
    value: &mut Object,
    payload: Option<&mut Option<Vec<u8>>>,
    renames: &BTreeMap<String, String>,
) -> bool {
    match value {
        Object::String(script) => match rewrite_js(script, renames) {
            Some(new) => {
                *script = new;
                true
            }
            None => false,
        },
        Object::Stream(stream) => {
            let Some(slot) = payload else {
                return false;
            };
            let Some(raw) = slot.as_ref() else {
                return false;
            };
            let Ok(decoded) = crate::filters::decode_stream(&stream.dict, raw) else {
                return false;
            };
            let Some(new) = rewrite_js(&decoded, renames) else {
                return false;
            };
            stream.dict.remove(b"Filter");
            stream.dict.remove(b"DecodeParms");
            stream.dict.insert(
                Name::from(b"Length"),
                Object::Integer(i64::try_from(new.len()).unwrap_or(i64::MAX)),
            );
            stream.data_span = ByteSpan::new(0, new.len());
            *slot = Some(new);
            true
        }
        _ => false,
    }
}

/// `js` with every quoted literal naming a renamed field replaced, or `None`
/// when nothing matched. A UTF-16BE script (BOM) is decoded and re-encoded.
fn rewrite_js(js: &[u8], renames: &BTreeMap<String, String>) -> Option<Vec<u8>> {
    if js.starts_with(&[0xFE, 0xFF]) {
        let text = crate::edit::decode_text_string(js).text;
        let out = rewrite_literals(text.as_bytes(), renames)?;
        return Some(crate::edit::encode_text_string(&String::from_utf8_lossy(
            &out,
        )));
    }
    rewrite_literals(js, renames)
}

/// The literal scan: every `"…"` / `'…'` run (backslash escapes skipped) whose
/// content is a key of `renames` is replaced, all in one pass, so `A → B`
/// beside `B → C` never chains.
fn rewrite_literals(src: &[u8], renames: &BTreeMap<String, String>) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(src.len());
    let mut changed = false;
    let mut i = 0;
    while let Some(&b) = src.get(i) {
        if b != b'"' && b != b'\'' {
            out.push(b);
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut j = start;
        while let Some(&c) = src.get(j) {
            if c == b'\\' {
                j += 2;
            } else if c == b || c == b'\n' {
                break;
            } else {
                j += 1;
            }
        }
        if src.get(j) != Some(&b) {
            // Unterminated: copy the rest as it is.
            out.extend_from_slice(src.get(i..).unwrap_or_default());
            break;
        }
        let inner = src.get(start..j).unwrap_or_default();
        out.push(b);
        match std::str::from_utf8(inner).ok().and_then(|s| renames.get(s)) {
            Some(new) => {
                out.extend_from_slice(new.as_bytes());
                changed = true;
            }
            None => out.extend_from_slice(inner),
        }
        out.push(b);
        i = j + 1;
    }
    changed.then_some(out)
}

/// What [`FieldClipSet`] paste should do with each field.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum FieldSetPastePolicy {
    /// **Ctrl+V** — every field becomes a new, independent field. A source
    /// name already in use (in the document or earlier in the batch) gets the
    /// first free `_2`, `_3`, … suffix; each such choice is disclosed and
    /// reported in [`FieldSetPasteOutcome::names`]. The other three fields
    /// mean what they mean on
    /// [`FieldPastePolicy::NewField`](super::FieldPastePolicy::NewField), and
    /// apply to every field.
    NewFields {
        /// The accessibility-name decision (R105), for every field.
        tooltip: PasteTooltip,
        /// Carry each source field's `/V`.
        copy_value: bool,
        /// Carry each source field's `/AA` and widget `/A`. Calculate
        /// scripts naming another field of the batch are re-pointed at its
        /// copy.
        copy_actions: bool,
    },
    /// **Ctrl+Shift+V** — every copied widget becomes another widget of the
    /// field bearing its source name here. A radio member whose on-state is
    /// taken takes a fresh one, chosen against the group and the batch.
    AdditionalWidgets,
}

/// What a set paste did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FieldSetPasteOutcome {
    /// One outcome per paste step: per field for
    /// [`FieldSetPastePolicy::NewFields`], per widget for
    /// [`FieldSetPastePolicy::AdditionalWidgets`]. Their own `disclosures`
    /// are empty — every sentence is in [`Self::disclosures`], once.
    pub fields: Vec<FieldPasteOutcome>,
    /// The fully-qualified name each copied field landed under, in copy
    /// order.
    pub names: Vec<String>,
    /// Everything the operator must be told, each sentence once.
    pub disclosures: Vec<String>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn renames(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
            .collect()
    }

    #[test]
    fn literals_are_rewritten_in_one_pass_and_nothing_else_is() {
        let map = renames(&[("A", "B"), ("B", "C")]);
        let out = rewrite_js(
            b"AFSimple_Calculate(\"SUM\", new Array(\"A\", 'B', \"AB\")); var A = 1;",
            &map,
        )
        .unwrap();
        assert_eq!(
            out,
            b"AFSimple_Calculate(\"SUM\", new Array(\"B\", 'C', \"AB\")); var A = 1;".to_vec()
        );
        assert!(rewrite_js(b"event.value = 1;", &map).is_none());
        assert!(rewrite_js(b"\"A", &map).is_none());
    }
}
