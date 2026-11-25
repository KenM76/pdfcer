//! The sidecar encoding of [`CentreStyle`]: one `/Centre` dict holding a
//! `/Line` and a `/Mark` part stroke and the mark's `/MarkGap`,
//! `/MarkExtension` and `/MarkLineFont`, each written only when set, the
//! whole dict only when any is.

use crate::object::{Dict, Name, Object};

use super::centre::CentreMarkSize;
use super::centre_style::CentreStyle;
use super::sidecar::{positive_of, put_part_stroke, read_part_stroke};

/// Write a [`CentreMarkSize`] as `/<key>` (the number) and `/<key>Kind`
/// (`/text` or `/fixed`).
pub(super) fn put_mark_size(d: &mut Dict, key: &[u8], size: CentreMarkSize) {
    let (kind, v): (&[u8], f64) = match size {
        CentreMarkSize::TextHeight(v) => (b"text", v),
        CentreMarkSize::Fixed(v) => (b"fixed", v),
    };
    d.insert(
        Name([key, b"Kind"].concat()),
        Object::Name(Name::from(kind)),
    );
    d.insert(Name::from(key), Object::Real(v));
}

/// Read what [`put_mark_size`] wrote; absent, non-positive or an unknown
/// kind reads as unset.
pub(super) fn read_mark_size(d: &Dict, key: &[u8]) -> Option<CentreMarkSize> {
    let v = positive_of(d.get(key))?;
    match d.get(&[key, b"Kind"].concat()).and_then(Object::as_name) {
        Some(n) if n.as_bytes() == b"text" => Some(CentreMarkSize::TextHeight(v)),
        Some(n) if n.as_bytes() == b"fixed" => Some(CentreMarkSize::Fixed(v)),
        _ => None,
    }
}

/// Write `/Centre` into `d` when `c` sets anything.
pub(super) fn put_centre(d: &mut Dict, c: CentreStyle) {
    if c.is_inherit() {
        return;
    }
    let mut inner = Dict::new();
    if !c.line.is_follow() {
        inner.insert(Name::from(b"Line"), put_part_stroke(c.line));
    }
    if !c.mark.is_follow() {
        inner.insert(Name::from(b"Mark"), put_part_stroke(c.mark));
    }
    if let Some(g) = c.mark_gap {
        put_mark_size(&mut inner, b"MarkGap", g);
    }
    if let Some(e) = c.mark_extension {
        put_mark_size(&mut inner, b"MarkExtension", e);
    }
    if let Some(on) = c.mark_line_font {
        inner.insert(Name::from(b"MarkLineFont"), Object::Boolean(on));
    }
    d.insert(Name::from(b"Centre"), Object::Dict(inner));
}

/// Read `/Centre` back; absent or malformed fields inherit.
pub(super) fn read_centre(d: &Dict) -> CentreStyle {
    let Some(c) = d.get(b"Centre").and_then(Object::as_dict) else {
        return CentreStyle::INHERIT;
    };
    CentreStyle::INHERIT
        .with_line(read_part_stroke(c, b"Line"))
        .with_mark(read_part_stroke(c, b"Mark"))
        .with_mark_gap(read_mark_size(c, b"MarkGap"))
        .with_mark_extension(read_mark_size(c, b"MarkExtension"))
        .with_mark_line_font(match c.get(b"MarkLineFont") {
            Some(Object::Boolean(on)) => Some(*on),
            _ => None,
        })
}
