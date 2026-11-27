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

/// What a modelled markup's paste declares: the raw route plus the geometry
/// keys `paste_geometry` translates. Under a rotation or scale its `/AP` is
/// redrawn from that geometry instead, and a scale patches the border width
/// where the source holds it; no other key is added. The runner pastes by
/// translation, so there `/AP` must arrive unchanged.
pub(crate) const MARKUP: &[Change] = &[
    Change::Translated("Rect"),
    Change::Translated("QuadPoints"),
    Change::Translated("CL"),
    Change::Translated("L"),
    Change::Translated("Vertices"),
    Change::Translated("InkList"),
    Change::Rebound("P"),
    Change::Regenerated("NM"),
    Change::Rebaked {
        key: "AP",
        only_when: "a non-translation transform",
    },
    Change::Rebaked {
        key: "Border",
        only_when: "a scale changes the border width (its width element)",
    },
    Change::Rebaked {
        key: "BS",
        only_when: "a scale changes the border width (/W; added, and disclosed, only when no /BS or /Border held it)",
    },
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

/// What a form field's widget declares through `copy_field` and a
/// `NewField` paste: the dictionary travels whole (`formclip`), renamed,
/// rebound and moved. `/Popup` and `/IRT` name source annotations and stay
/// dropped (a widget has no reply thread to keep).
pub(crate) const WIDGET: &[Change] = &[
    Change::Translated("Rect"),
    Change::Rebound("P"),
    Change::Regenerated("NM"),
    Change::Regenerated("T"),
    Change::Dropped {
        key: "Popup",
        disclosed_by: Disclosure::FieldPasteDroppedKeys,
        gap: None,
    },
    Change::Dropped {
        key: "IRT",
        disclosed_by: Disclosure::FieldPasteDroppedKeys,
        gap: None,
    },
    Change::Dropped {
        key: "StructParent",
        disclosed_by: Disclosure::FieldPasteDroppedKeys,
        gap: Some("781.7.k"),
    },
    Change::Unbound {
        key: "OC",
        pass: "781.7.f",
    },
    Change::Renumbered,
];

/// A field with `/Kids` widgets: renamed, its widgets moved and rebound.
/// The driver compares the field and each widget separately, so `/Rect`
/// and `/P` are the widgets' keys.
pub(crate) const FIELD: &[Change] = &[
    Change::Regenerated("T"),
    Change::Translated("Rect"),
    Change::Rebound("P"),
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
        | K::Ink => Expect::Preserve(MARKUP),
        K::CeDimension => Expect::Pending("781.4"),
        // §12.5.6.14: a popup belongs to its parent and travels with it.
        K::Popup => Expect::Refused("popup-travels-with-parent"),
        // A pending redaction pasted into an unreviewed document arms it.
        K::Redact => Expect::Refused("pending-redaction"),
        K::Widget => Expect::Preserve(WIDGET),
        K::Field => Expect::Preserve(FIELD),
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
        | K::Widget
        | K::Field => Expect::Preserve(&[Change::Renumbered]),
        K::Page | K::OutlineItem | K::Attachment | K::Layer | K::NamedDest => {
            Expect::Pending("781.6")
        }
    }
}
