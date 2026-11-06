//! Whether an annotation is printed (ISO 32000-1 §12.5.3 Table 165).

use super::Annotation;

/// The annotation types ISO 32000 Table 171 defines. §12.5.3's Invisible flag
/// applies only to a type outside this set.
const STANDARD_SUBTYPES: [&[u8]; 28] = [
    b"Text",
    b"Link",
    b"FreeText",
    b"Line",
    b"Square",
    b"Circle",
    b"Polygon",
    b"PolyLine",
    b"Highlight",
    b"Underline",
    b"Squiggly",
    b"StrikeOut",
    b"Caret",
    b"Stamp",
    b"Ink",
    b"Popup",
    b"FileAttachment",
    b"Sound",
    b"Movie",
    b"Screen",
    b"Widget",
    b"PrinterMark",
    b"TrapNet",
    b"Watermark",
    b"3D",
    b"Redact",
    b"Projection",
    b"RichMedia",
];

impl Annotation {
    /// Whether a print of the page includes this annotation (§12.5.3 Table
    /// 165): Hidden clear and Print set, whatever NoView says, unless the
    /// Invisible flag applies, which it does only to a type outside Table 171.
    #[must_use]
    pub fn prints(&self) -> bool {
        if self.flags.hidden() || !self.flags.print() {
            return false;
        }
        !self.flags.invisible() || STANDARD_SUBTYPES.contains(&self.subtype.as_slice())
    }
}
