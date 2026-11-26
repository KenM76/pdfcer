//! The clipboard group: Copy, Cut, Paste, Duplicate and ReloadRoundTrip.

use crate::xcut::{Change, Disclosure, Expect, ObjectKind};

/// What the raw route (`RawAnnotation`: the dictionary and its closure,
/// cloned, minus the source-bound keys) declares it changes.
///
/// `/Popup`, `/IRT` and `/StructParent` name objects in the source document
/// and are dropped and counted; `/OC` is carried verbatim but not yet
/// registered in the destination's `/OCProperties` (ISO 32000-1 §8.11.4).
pub(crate) const RAW: &[Change] = &[
    Change::Translated("Rect"),
    Change::Translated("QuadPoints"),
    Change::Translated("CL"),
    Change::Rebound("P"),
    Change::Regenerated("NM"),
    Change::Dropped {
        key: "Popup",
        disclosed_by: Disclosure::PasteDroppedKeys,
        gap: Some("781.7.e"),
    },
    Change::Dropped {
        key: "IRT",
        disclosed_by: Disclosure::PasteDroppedKeys,
        gap: Some("781.7.e"),
    },
    Change::Dropped {
        key: "StructParent",
        disclosed_by: Disclosure::PasteDroppedKeys,
        gap: Some("781.7.k"),
    },
    Change::Unbound {
        key: "OC",
        pass: "781.7.f",
    },
    Change::Renumbered,
];

/// Copy, Cut, Paste and Duplicate: the same carry, so one table.
pub(super) fn carry(kind: ObjectKind) -> Expect {
    use ObjectKind as K;
    match kind {
        K::Path | K::TextObject | K::ImageXObject | K::InlineImage | K::FormXObject => {
            Expect::Pending("781.5")
        }
        K::TextNote
        | K::FreeText
        | K::Callout
        | K::Caret
        | K::Stamp
        | K::FileAttachment
        | K::Sound
        | K::Screen
        | K::ThreeD
        | K::Link
        | K::Watermark
        | K::UnknownAnnot => Expect::Preserve(RAW),
        K::Line
        | K::Square
        | K::Circle
        | K::Polygon
        | K::PolyLine
        | K::Highlight
        | K::Underline
        | K::StrikeOut
        | K::Squiggly
        | K::ReplaceText
        | K::Ink => Expect::Pending("781.2"),
        K::CeDimension => Expect::Pending("781.4"),
        // §12.5.6.14: a popup belongs to its parent and travels with it.
        K::Popup => Expect::Refused("popup-travels-with-parent"),
        // A pending redaction pasted into an unreviewed document arms it.
        K::Redact => Expect::Refused("pending-redaction"),
        K::Widget | K::Field => Expect::Pending("781.3"),
        K::Page | K::OutlineItem | K::Attachment | K::Layer | K::NamedDest => {
            Expect::Pending("781.6")
        }
    }
}

/// A full rewrite and a reopen: every annotation survives, renumbered.
pub(super) fn reload(kind: ObjectKind) -> Expect {
    use ObjectKind as K;
    match kind {
        K::Path | K::TextObject | K::ImageXObject | K::InlineImage | K::FormXObject => {
            Expect::Pending("781.5")
        }
        K::TextNote
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
        | K::Widget => Expect::Preserve(&[Change::Renumbered]),
        K::Field => Expect::Pending("781.3"),
        K::Page | K::OutlineItem | K::Attachment | K::Layer | K::NamedDest => {
            Expect::Pending("781.6")
        }
    }
}
