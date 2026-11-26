//! [`ObjectKind`]: the one list of object kinds, and [`classify_annot`].

use crate::graph::ObjectGraph;
use crate::object::{Dict, Object};

/// The four families an [`ObjectKind`] belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KindGroup {
    /// Page content: operators in a content stream (ISO 32000-1 §8).
    Content,
    /// An annotation dictionary in a page's `/Annots` (§12.5).
    Annotation,
    /// An interactive-form object (§12.7).
    Form,
    /// A document-level structure: a page, an outline item, an attachment,
    /// a layer, a named destination.
    Document,
}

impl KindGroup {
    /// The stable lowercase token used as the prefix of [`ObjectKind::id`].
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Annotation => "annot",
            Self::Form => "form",
            Self::Document => "doc",
        }
    }
}

/// Defines [`ObjectKind`], its `ALL` list, its stable ids and its groups from
/// one table, so the four cannot disagree.
macro_rules! object_kinds {
    ($( $group:ident { $( $(#[$meta:meta])* $variant:ident => $id:literal, )* } )*) => {
        /// Every kind of object a document can hold that an operation must
        /// account for (decision 228).
        ///
        /// **Not `#[non_exhaustive]`, on purpose.** A new variant must break
        /// every exhaustive match over this type — pdfcer's own policy
        /// tables and any consumer's — so that no operation can silently not
        /// know the new kind exists. Each new kind is therefore a breaking
        /// change for consumers, which is intended.
        ///
        /// # Example
        ///
        /// ```
        /// use pdfcer_core::xcut::ObjectKind;
        ///
        /// let kind = ObjectKind::from_id("annot.stamp");
        /// assert_eq!(kind, Some(ObjectKind::Stamp));
        /// assert!(ObjectKind::ALL.iter().all(|k| ObjectKind::from_id(k.id()) == Some(*k)));
        /// ```
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
        pub enum ObjectKind {
            $($( $(#[$meta])* $variant, )*)*
        }

        impl ObjectKind {
            /// Every kind, in declaration order (the matrix's row order).
            pub const ALL: &'static [ObjectKind] = &[$($(ObjectKind::$variant,)*)*];

            /// The stable id, `<group>.<name>`: the matrix row key, and the
            /// key a consumer's own coverage table uses. Never renamed.
            #[must_use]
            pub const fn id(self) -> &'static str {
                match self { $($(Self::$variant => $id,)*)* }
            }

            /// The family this kind belongs to.
            #[must_use]
            pub const fn group(self) -> KindGroup {
                match self { $($(Self::$variant => KindGroup::$group,)*)* }
            }
        }
    };
}

object_kinds! {
    Content {
        /// A path object: construction operators plus one painting operator
        /// (§8.5).
        Path => "content.path",
        /// A text object, `BT` … `ET` (§9.4).
        TextObject => "content.text",
        /// An image XObject painted by `Do` (§8.9.5).
        ImageXObject => "content.image_xobject",
        /// An inline image, `BI` … `EI` (§8.9.7).
        InlineImage => "content.inline_image",
        /// A form XObject painted by `Do` (§8.10).
        FormXObject => "content.form_xobject",
    }
    Annotation {
        /// `/Text`, a sticky note (§12.5.6.4).
        TextNote => "annot.text",
        /// `/FreeText` without a callout line (§12.5.6.6).
        FreeText => "annot.free_text",
        /// `/FreeText` `/IT /FreeTextCallout` (§12.5.6.6 Table 174).
        Callout => "annot.callout",
        /// `/Line` that is not a ce dimension (§12.5.6.7).
        Line => "annot.line",
        /// A ce dimension: a `/Line`, `/Polygon` or `/PolyLine` carrying a
        /// dimension intent (project rule 15).
        CeDimension => "annot.ce_dimension",
        /// `/Square` (§12.5.6.8).
        Square => "annot.square",
        /// `/Circle` (§12.5.6.8).
        Circle => "annot.circle",
        /// `/Polygon` (§12.5.6.9).
        Polygon => "annot.polygon",
        /// `/PolyLine` (§12.5.6.9).
        PolyLine => "annot.polyline",
        /// `/Highlight` (§12.5.6.10).
        Highlight => "annot.highlight",
        /// `/Underline` (§12.5.6.10).
        Underline => "annot.underline",
        /// `/StrikeOut` that is not half of a replace-text pair (§12.5.6.10).
        StrikeOut => "annot.strikeout",
        /// `/Squiggly` (§12.5.6.10).
        Squiggly => "annot.squiggly",
        /// `/Caret` that is not half of a replace-text pair (§12.5.6.11).
        Caret => "annot.caret",
        /// Replace text: a `/Caret` `/IT /Replace` and the `/StrikeOut`
        /// `/IT /StrikeOutTextEdit` grouped to it.
        ReplaceText => "annot.replace_text",
        /// `/Stamp` (§12.5.6.12).
        Stamp => "annot.stamp",
        /// `/Ink` (§12.5.6.13).
        Ink => "annot.ink",
        /// `/Popup` (§12.5.6.14).
        Popup => "annot.popup",
        /// `/FileAttachment` (§12.5.6.15).
        FileAttachment => "annot.file_attachment",
        /// `/Sound` (§12.5.6.16).
        Sound => "annot.sound",
        /// `/Screen` (§12.5.6.18).
        Screen => "annot.screen",
        /// `/3D` (§13.6.2).
        ThreeD => "annot.three_d",
        /// `/Link` (§12.5.6.5).
        Link => "annot.link",
        /// `/Redact` (§12.5.6.23).
        Redact => "annot.redact",
        /// `/Watermark` (§12.5.6.22).
        Watermark => "annot.watermark",
        /// Any other `/Subtype`, carried through the raw route. A decided
        /// kind, not a wildcard: it has its own row and its own fixture.
        UnknownAnnot => "annot.unknown",
    }
    Form {
        /// A `/Widget` annotation (§12.5.6.19).
        Widget => "form.widget",
        /// A field dictionary in the `/AcroForm` field tree (§12.7.3).
        Field => "form.field",
    }
    Document {
        /// A page object (§7.7.3.3).
        Page => "doc.page",
        /// An outline item (§12.3.3).
        OutlineItem => "doc.outline_item",
        /// An embedded file in the `/EmbeddedFiles` name tree (§7.11.4).
        Attachment => "doc.attachment",
        /// An optional-content group (§8.11.2).
        Layer => "doc.layer",
        /// A named destination (§12.3.2.3).
        NamedDest => "doc.named_dest",
    }
}

impl ObjectKind {
    /// The kind whose [`Self::id`] is `id`.
    #[must_use]
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.id() == id)
    }
}

impl std::fmt::Display for ObjectKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.id())
    }
}

/// Classify an annotation dictionary by its `/Subtype` and `/IT` (ISO
/// 32000-1 §12.5.2 Table 164; `/IT` per Tables 174, 175, 178, 181, 183).
///
/// A `/Line`, `/Polygon` or `/PolyLine` whose intent is `LineDimension`,
/// `PolygonDimension` or `PolyLineDimension` is a [`ObjectKind::CeDimension`]
/// — the same test `EditSession` applies before consulting the `/PieceInfo`
/// sidecar. A subtype this list does not name is [`ObjectKind::UnknownAnnot`].
/// Indirect `/Subtype` and `/IT` values are resolved through `graph`.
///
/// # Example
///
/// ```
/// use pdfcer_core::object::{Dict, Name, Object};
/// use pdfcer_core::xcut::{ObjectKind, classify_annot};
///
/// let mut annot = Dict::new();
/// annot.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"FreeText")));
/// annot.insert(Name::from(b"IT"), Object::Name(Name::from(b"FreeTextCallout")));
///
/// // Every value above is direct, so a graph with no objects resolves them.
/// struct Direct;
/// impl pdfcer_core::graph::ObjectGraph for Direct {
///     fn value(&self, _: pdfcer_core::object::ObjId) -> Option<&Object> {
///         None
///     }
///     fn trailer_entry(&self, _: &[u8]) -> Option<&Object> {
///         None
///     }
/// }
/// assert_eq!(classify_annot(&Direct, &annot), ObjectKind::Callout);
/// ```
#[must_use]
pub fn classify_annot<G: ObjectGraph + ?Sized>(graph: &G, dict: &Dict) -> ObjectKind {
    let name = |key: &[u8]| match dict.get(key).map(|o| graph.resolve(o)) {
        Some(Object::Name(n)) => n.as_bytes().to_vec(),
        _ => Vec::new(),
    };
    let intent = name(b"IT");
    let it = intent.as_slice();
    match name(b"Subtype").as_slice() {
        b"Text" => ObjectKind::TextNote,
        b"FreeText" if it == b"FreeTextCallout" => ObjectKind::Callout,
        b"FreeText" => ObjectKind::FreeText,
        b"Line" if it == b"LineDimension" => ObjectKind::CeDimension,
        b"Polygon" if it == b"PolygonDimension" => ObjectKind::CeDimension,
        b"PolyLine" if it == b"PolyLineDimension" => ObjectKind::CeDimension,
        b"Line" => ObjectKind::Line,
        b"Polygon" => ObjectKind::Polygon,
        b"PolyLine" => ObjectKind::PolyLine,
        b"Square" => ObjectKind::Square,
        b"Circle" => ObjectKind::Circle,
        b"Highlight" => ObjectKind::Highlight,
        b"Underline" => ObjectKind::Underline,
        b"StrikeOut" if it == b"StrikeOutTextEdit" => ObjectKind::ReplaceText,
        b"StrikeOut" => ObjectKind::StrikeOut,
        b"Squiggly" => ObjectKind::Squiggly,
        b"Caret" if it == b"Replace" => ObjectKind::ReplaceText,
        b"Caret" => ObjectKind::Caret,
        b"Stamp" => ObjectKind::Stamp,
        b"Ink" => ObjectKind::Ink,
        b"Popup" => ObjectKind::Popup,
        b"FileAttachment" => ObjectKind::FileAttachment,
        b"Sound" => ObjectKind::Sound,
        b"Screen" => ObjectKind::Screen,
        b"3D" => ObjectKind::ThreeD,
        b"Link" => ObjectKind::Link,
        b"Redact" => ObjectKind::Redact,
        b"Watermark" => ObjectKind::Watermark,
        b"Widget" => ObjectKind::Widget,
        _ => ObjectKind::UnknownAnnot,
    }
}
