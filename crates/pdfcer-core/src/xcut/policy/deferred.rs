//! The groups not yet driven: every kind, listed, pending one Pass.

use crate::xcut::{Expect, ObjectKind};

/// Every kind pending `pass`. The or-pattern names all of them so that a
/// new kind is a compile error here too, not a silent extra `pass:` cell.
pub(super) fn pending(kind: ObjectKind, pass: &'static str) -> Expect {
    use ObjectKind as K;
    match kind {
        K::Path
        | K::TextObject
        | K::ImageXObject
        | K::InlineImage
        | K::FormXObject
        | K::TextNote
        | K::FreeText
        | K::Callout
        | K::Line
        | K::CeDimension
        | K::Square
        | K::Circle
        | K::Polygon
        | K::PolyLine
        | K::Highlight
        | K::Underline
        | K::StrikeOut
        | K::Squiggly
        | K::Caret
        | K::ReplaceText
        | K::Stamp
        | K::Ink
        | K::Popup
        | K::FileAttachment
        | K::Sound
        | K::Screen
        | K::ThreeD
        | K::Link
        | K::Redact
        | K::Watermark
        | K::UnknownAnnot
        | K::Widget
        | K::Field
        | K::Page
        | K::OutlineItem
        | K::Attachment
        | K::Layer
        | K::NamedDest => Expect::Pending(pass),
    }
}
