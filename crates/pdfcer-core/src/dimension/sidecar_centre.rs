//! The sidecar encoding of [`CentreStyle`]: one `/Centre` dict holding a
//! `/Line` and a `/Mark` part stroke, each written only when some field is
//! set, the whole dict only when any is.

use crate::object::{Dict, Name, Object};

use super::centre_style::CentreStyle;
use super::sidecar::{put_part_stroke, read_part_stroke};

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
}
