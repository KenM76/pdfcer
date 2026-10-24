//! Discarding document objects during an optimize run (Pass 616.3): the
//! classes of Acrobat's Discard Objects list that are plain deletions.
//!
//! Carriers, by clause (ISO 32000-2):
//! - bookmarks: the catalog's `/Outlines` (§12.3.3), and a `/PageMode` or
//!   `/NonFullScreenPageMode` of `/UseOutlines` that would name it (the
//!   spec is silent on that mode with no outline; absent means `/UseNone`);
//! - tags: the catalog's `/StructTreeRoot` and `/MarkInfo` (whose `/Marked
//!   true` would otherwise claim a tagged file, §14.8.1), every
//!   `/StructParent` / `/StructParents` (Table 359), an outline item's `/SE`
//!   and a GoTo action's or destination's `/SD` (2.0 Table 202); a GoToR or
//!   GoToE `/SD` points into another file and stays. Every destination
//!   naming a structure element is first rewritten to name its page: see
//!   [`super::struct_dest`]. Marked-content `MCID`s
//!   stay: an MCID without a structure parent is legal (2.0 erratum 308);
//! - print settings: the print entries of the catalog's `/ViewerPreferences`
//!   (§12.2 Table 147), each of which then takes the viewer's default, and
//!   `/Enforce`, whose only defined member is `PrintScaling` (Table 148);
//! - alternate images: an image's `/Alternates` (§8.9.5.4), leaving the base
//!   image, which is the one shown on screen. An image under optional
//!   content is skipped: with its base hidden an alternate may be what shows.
//!
//! - form actions and external references: see [`actions`];
//! - object data: see [`object_data`];
//! - form fields, multimedia and 3D models: see [`annots`].
//! - flatten layers: each layer visible in the default configuration
//!   (§8.11.4.3) is deleted by `EditSession::delete_layer`, keeping what it
//!   draws; a hidden one is the `hidden-layer` user-data kind's.
//!
//! - search index: the catalog's `/PieceInfo /SearchIndex` (a §14.5 data
//!   dictionary). No ISO or Adobe document names the key; it is the one
//!   PDF4QT and pikepdf remove (`PDF_Spec/adobe_ext/
//!   adobe_ext__embedded_search_index.md`). The other applications' entries
//!   stay, and a `/PieceInfo` left empty goes.
//! - image fragments: see [`super::fragments`].

use std::collections::BTreeSet;
use std::fmt;

use super::actions::{self, Rewrite};
use super::annots::{self, ListDrop};
use super::object_data;
use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::view::DocumentView;

/// One class of document object an optimize run can discard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[non_exhaustive]
pub enum DiscardObject {
    /// The bookmark (outline) tree.
    Bookmarks,
    /// The structure tree: what makes the file tagged, for screen readers,
    /// reflow and PDF/UA.
    Tags,
    /// Embedded print settings: page scaling, duplex, paper tray, page
    /// range and copy count.
    PrintSettings,
    /// The alternate versions of an image (e.g. a high-resolution one for
    /// printing), keeping the one shown on screen.
    AlternateImages,
    /// Form submit, import and reset actions, from fields, links, buttons
    /// and every other action carrier; actions chained after one still run.
    FormActions,
    /// Actions that open another file: go-to-remote, launch, and go-to-
    /// embedded or article targets in another file. Web links stay.
    ExternalReferences,
    /// Object data: the name-value properties a CAD or diagram program
    /// attaches to each drawn object (user properties in the structure
    /// tree). Skipped when the tags themselves are discarded.
    ObjectData,
    /// Form fields and their widgets, removed rather than flattened: what
    /// they showed goes too, signature fields included.
    FormFields,
    /// Sound, movie, screen and rich-media annotations, the actions that
    /// play them, and the document's rendition list.
    Multimedia,
    /// 3D annotations and their models, and the actions that change their
    /// view.
    ThreeDModels,
    /// Layers visible when the document opens, merged into the page: what
    /// they draw stays, now on no layer, so it always shows and prints.
    /// Hidden layers stay layers unless `hidden-layer` is also discarded.
    FlattenLayers,
    /// An embedded full-text search index, which copies the document's text.
    SearchIndex,
    /// A picture stored as many thin strips, each its own image, merged
    /// into one image that draws the same.
    MergeImageFragments,
    /// Runs of short straight segments that trace a curve, as CAD programs
    /// write arcs and splines, redrawn as fitted curves within
    /// [`super::OptimizeSettings::smooth_tolerance`]. Lossy: the drawing
    /// moves by up to the tolerance, and each page's largest move is listed.
    SmoothLines,
}

impl DiscardObject {
    /// Every class, in plan order.
    pub const ALL: [Self; 14] = [
        Self::Bookmarks,
        Self::Tags,
        Self::PrintSettings,
        Self::AlternateImages,
        Self::FormActions,
        Self::ExternalReferences,
        Self::ObjectData,
        Self::FormFields,
        Self::Multimedia,
        Self::ThreeDModels,
        Self::FlattenLayers,
        Self::SearchIndex,
        Self::MergeImageFragments,
        Self::SmoothLines,
    ];

    /// A stable lower-case name: `bookmarks`, `tags`, `print-settings`,
    /// `alternate-images`, `form-actions`, `external-references`,
    /// `object-data`, `form-fields`, `multimedia`, `3d-models`,
    /// `flatten-layers`, `search-index`, `merge-image-fragments`,
    /// `smooth-lines`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bookmarks => "bookmarks",
            Self::Tags => "tags",
            Self::PrintSettings => "print-settings",
            Self::AlternateImages => "alternate-images",
            Self::FormActions => "form-actions",
            Self::ExternalReferences => "external-references",
            Self::ObjectData => "object-data",
            Self::FormFields => "form-fields",
            Self::Multimedia => "multimedia",
            Self::ThreeDModels => "3d-models",
            Self::FlattenLayers => "flatten-layers",
            Self::SearchIndex => "search-index",
            Self::MergeImageFragments => "merge-image-fragments",
            Self::SmoothLines => "smooth-lines",
        }
    }

    /// The class [`Self::as_str`] names.
    ///
    /// ```
    /// use pdfcer_core::optimize::DiscardObject;
    ///
    /// assert_eq!(DiscardObject::from_name("tags"), Some(DiscardObject::Tags));
    /// assert_eq!(DiscardObject::from_name("thumbnail"), None);
    /// ```
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|o| o.as_str() == name)
    }
}

impl fmt::Display for DiscardObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One carrier an optimize run discards.
// Not `Eq`: an action rewrite holds an `Object`, which may be a real.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ObjectDiscard {
    /// The class.
    pub object: DiscardObject,
    /// The object holding it: the catalog, the image for alternates, the
    /// layer (optional content group) for a flatten, or the page for a
    /// fragment merge or a smoothing.
    pub owner: ObjId,
    /// Where it is, in words: `catalog` (for tags, followed by how many
    /// links to a tag now go to its page, as `catalog: 2 link(s) to a tag
    /// now go to its page`), `image object 12 0`, `page 2:
    /// form-fields (3)`, `layer 9 0: Notes`, `page 1: 40 strips into 1 image
    /// (800x1100)`, `page 1: 120 line segments into 6 curves (max deviation
    /// 0.031 pt)`, or for an action `object 12 0: SubmitForm, GoToR`
    /// (the types removed).
    pub location: String,
    /// What leaves the file: the deleted entries plus every object only
    /// they reached. Approximate (written as pdfcer would write it).
    pub bytes: u64,
    /// Entries deleted, each a key path under an object.
    pub(crate) strips: Vec<Strip>,
    /// Action entries set or removed.
    pub(crate) rewrites: Vec<Rewrite>,
    /// References removed from a list.
    pub(crate) drops: Vec<ListDrop>,
}

/// A key path (at most two keys) under an object, as
/// `strip_metadata_key` takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Strip {
    pub(crate) owner: ObjId,
    pub(crate) path: Vec<&'static [u8]>,
}

/// The print entries of a viewer preferences dictionary (§12.2 Table 147;
/// `PrintArea` and `PrintClip` are deprecated in 2.0), and `Enforce`.
const PRINT_KEYS: &[&[u8]] = &[
    b"PrintScaling",
    b"Duplex",
    b"PickTrayByPDFSize",
    b"PrintPageRange",
    b"NumCopies",
    b"PrintArea",
    b"PrintClip",
    b"Enforce",
];

/// The structure-tree back-pointers (Table 359) and an outline item's
/// structure element (Table 153).
const STRUCT_KEYS: &[&[u8]] = &[b"StructParents", b"StructParent", b"SE"];

/// Every carrier of a class `wanted` selects, with `bytes` zero (the session
/// measures it). `live` is every object reachable from the trailer.
pub(crate) fn carriers(
    view: &DocumentView<'_>,
    live: &BTreeSet<ObjId>,
    wanted: impl Fn(DiscardObject) -> bool,
) -> Vec<ObjectDiscard> {
    let Some(catalog) = view.trailer_entry(b"Root").and_then(Object::as_reference) else {
        return Vec::new();
    };
    let Some(cat) = view.value(catalog).and_then(Object::as_dict) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let item = |object, owner, location: String, strips: Vec<Strip>| {
        (!strips.is_empty() && wanted(object)).then_some(ObjectDiscard {
            object,
            owner,
            location,
            bytes: 0,
            strips,
            rewrites: Vec::new(),
            drops: Vec::new(),
        })
    };
    let at = |path: Vec<&'static [u8]>| Strip {
        owner: catalog,
        path,
    };
    out.extend(item(
        DiscardObject::Bookmarks,
        catalog,
        "catalog".into(),
        bookmark_strips(view, cat).into_iter().map(at).collect(),
    ));
    if wanted(DiscardObject::Tags) {
        out.extend(tags_item(view, catalog, cat, live));
    }
    out.extend(item(
        DiscardObject::PrintSettings,
        catalog,
        "catalog".into(),
        print_strips(view, cat).into_iter().map(at).collect(),
    ));
    out.extend(item(
        DiscardObject::SearchIndex,
        catalog,
        "catalog".into(),
        search_index_strip(view, cat).into_iter().map(at).collect(),
    ));
    if wanted(DiscardObject::AlternateImages) {
        out.extend(alternate_carriers(view, live));
    }
    out.extend(object_data::object_data_carriers(
        view, live, catalog, cat, &wanted,
    ));
    out.extend(annots::annotation_carriers(
        view, live, catalog, cat, &wanted,
    ));
    out.extend(actions::action_carriers(view, live, catalog, &wanted));
    if wanted(DiscardObject::FlattenLayers) {
        out.extend(layer_carriers(view));
    }
    if wanted(DiscardObject::MergeImageFragments) {
        out.extend(super::fragments::fragment_carriers(view, live));
    }
    out
}

/// One item per image with `/Alternates` and no `/OC`.
fn alternate_carriers(view: &DocumentView<'_>, live: &BTreeSet<ObjId>) -> Vec<ObjectDiscard> {
    live.iter()
        .filter_map(|&id| match view.value(id) {
            Some(Object::Stream(s))
                if is_image(&s.dict)
                    && s.dict.get(b"Alternates").is_some()
                    && s.dict.get(b"OC").is_none() =>
            {
                Some(ObjectDiscard {
                    object: DiscardObject::AlternateImages,
                    owner: id,
                    location: format!("image object {} {}", id.num, id.generation),
                    bytes: 0,
                    strips: vec![Strip {
                        owner: id,
                        path: vec![b"Alternates"],
                    }],
                    rewrites: Vec::new(),
                    drops: Vec::new(),
                })
            }
            _ => None,
        })
        .collect()
}

/// One item per layer visible by default (§8.11.4.3 `/D`), measured as the
/// group object; the session flattens it with `delete_layer`.
fn layer_carriers(view: &DocumentView<'_>) -> Vec<ObjectDiscard> {
    crate::layers::list_layers(view)
        .into_iter()
        .filter(|l| l.visible_by_default)
        .map(|l| ObjectDiscard {
            object: DiscardObject::FlattenLayers,
            owner: l.id,
            location: format!("layer {} {}: {}", l.id.num, l.id.generation, l.name),
            bytes: view
                .value(l.id)
                .map_or(0, crate::doc_metadata::scan::value_bytes),
            strips: Vec::new(),
            rewrites: Vec::new(),
            drops: Vec::new(),
        })
        .collect()
}

/// The catalog's search index, or the whole `/PieceInfo` when it holds
/// nothing else.
fn search_index_strip(view: &DocumentView<'_>, cat: &Dict) -> Option<Vec<&'static [u8]>> {
    let piece = view.resolve(cat.get(b"PieceInfo")?).as_dict()?;
    piece.get(b"SearchIndex")?;
    Some(if piece.len() == 1 {
        vec![b"PieceInfo"]
    } else {
        vec![b"PieceInfo", b"SearchIndex"]
    })
}

fn bookmark_strips(view: &DocumentView<'_>, cat: &Dict) -> Vec<Vec<&'static [u8]>> {
    if cat.get(b"Outlines").is_none() {
        return Vec::new();
    }
    let outlines = |d: &Dict, key: &[u8]| {
        d.get(key)
            .and_then(Object::as_name)
            .is_some_and(|m| m.as_bytes() == b"UseOutlines")
    };
    let mut out = vec![vec![b"Outlines".as_slice()]];
    if outlines(cat, b"PageMode") {
        out.push(vec![b"PageMode"]);
    }
    if prefs(view, cat).is_some_and(|p| outlines(p, b"NonFullScreenPageMode")) {
        out.push(vec![b"ViewerPreferences", b"NonFullScreenPageMode"]);
    }
    out
}

fn prefs<'a>(view: &'a DocumentView<'_>, cat: &'a Dict) -> Option<&'a Dict> {
    cat.get(b"ViewerPreferences")
        .map(|p| view.resolve(p))
        .and_then(Object::as_dict)
}

/// The structure tree's item: its strips, and the rewrites that turn
/// structure destinations into page destinations first (an inline action's
/// `/SD` goes with them).
fn tags_item(
    view: &DocumentView<'_>,
    catalog: ObjId,
    cat: &Dict,
    live: &BTreeSet<ObjId>,
) -> Option<ObjectDiscard> {
    let strips = tag_strips(view, catalog, cat, live);
    if strips.is_empty() {
        return None;
    }
    let dests = super::struct_dest::rewrites(view, catalog, cat, live);
    let location = match (dests.rewritten, dests.to_first_page) {
        (0, _) => "catalog".to_owned(),
        (n, 0) => format!("catalog: {n} link(s) to a tag now go to its page"),
        (n, f) => format!(
            "catalog: {n} link(s) to a tag now go to its page, {f} of them to the first page (the tag had no page)"
        ),
    };
    Some(ObjectDiscard {
        object: DiscardObject::Tags,
        owner: catalog,
        location,
        bytes: 0,
        strips,
        rewrites: dests.rewrites,
        drops: Vec::new(),
    })
}

fn tag_strips(
    view: &DocumentView<'_>,
    catalog: ObjId,
    cat: &Dict,
    live: &BTreeSet<ObjId>,
) -> Vec<Strip> {
    if cat.get(b"StructTreeRoot").is_none() {
        return Vec::new();
    }
    let mut out = vec![Strip {
        owner: catalog,
        path: vec![b"StructTreeRoot"],
    }];
    if cat.get(b"MarkInfo").is_some() {
        out.push(Strip {
            owner: catalog,
            path: vec![b"MarkInfo"],
        });
    }
    for &id in live {
        let dict = match view.value(id) {
            Some(Object::Dict(d)) => d,
            Some(Object::Stream(s)) => &s.dict,
            _ => continue,
        };
        let keys = STRUCT_KEYS.iter().filter(|k| dict.get(k).is_some());
        let mut paths: Vec<Vec<&'static [u8]>> = keys.map(|&k| vec![k]).collect();
        if local_sd(dict) {
            paths.push(vec![b"SD"]);
        }
        out.extend(paths.into_iter().map(|path| Strip { owner: id, path }));
    }
    out
}

/// An `/SD` naming a structure element of this file: anything but a GoToR
/// or GoToE action, whose `/SD` points into another file.
fn local_sd(dict: &Dict) -> bool {
    let remote = dict
        .get(b"S")
        .and_then(Object::as_name)
        .is_some_and(|n| matches!(n.as_bytes(), b"GoToR" | b"GoToE"));
    dict.get(b"SD").is_some() && !remote
}

fn print_strips(view: &DocumentView<'_>, cat: &Dict) -> Vec<Vec<&'static [u8]>> {
    let Some(prefs) = prefs(view, cat) else {
        return Vec::new();
    };
    PRINT_KEYS
        .iter()
        .filter(|k| prefs.get(k).is_some())
        .map(|&k| vec![b"ViewerPreferences".as_slice(), k])
        .collect()
}

fn is_image(dict: &Dict) -> bool {
    dict.get(b"Subtype")
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"Image")
}
