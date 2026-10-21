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
//!   GoToE `/SD` points into another file and stays. Marked-content `MCID`s
//!   stay: an MCID without a structure parent is legal (2.0 erratum 308);
//! - print settings: the print entries of the catalog's `/ViewerPreferences`
//!   (§12.2 Table 147), each of which then takes the viewer's default, and
//!   `/Enforce`, whose only defined member is `PrintScaling` (Table 148);
//! - alternate images: an image's `/Alternates` (§8.9.5.4), leaving the base
//!   image, which is the one shown on screen. An image under optional
//!   content is skipped: with its base hidden an alternate may be what shows.
//!
//! An embedded search index is not offered: no published specification
//! names the key Acrobat stores it under.

use std::collections::BTreeSet;
use std::fmt;

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
}

impl DiscardObject {
    /// Every class, in plan order.
    pub const ALL: [Self; 4] = [
        Self::Bookmarks,
        Self::Tags,
        Self::PrintSettings,
        Self::AlternateImages,
    ];

    /// A stable lower-case name: `bookmarks`, `tags`, `print-settings`,
    /// `alternate-images`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Bookmarks => "bookmarks",
            Self::Tags => "tags",
            Self::PrintSettings => "print-settings",
            Self::AlternateImages => "alternate-images",
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct ObjectDiscard {
    /// The class.
    pub object: DiscardObject,
    /// The object holding it: the catalog, or the image for alternates.
    pub owner: ObjId,
    /// Where it is, in words: `catalog`, `image object 12 0`.
    pub location: String,
    /// What leaves the file: the deleted entries plus every object only
    /// they reached. Approximate (written as pdfcer would write it).
    pub bytes: u64,
    /// Entries deleted, each a key path under an object.
    pub(crate) strips: Vec<Strip>,
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
    let mut push = |object, owner, location: String, strips: Vec<Strip>| {
        if !strips.is_empty() && wanted(object) {
            out.push(ObjectDiscard {
                object,
                owner,
                location,
                bytes: 0,
                strips,
            });
        }
    };
    let at = |path: Vec<&'static [u8]>| Strip {
        owner: catalog,
        path,
    };
    push(
        DiscardObject::Bookmarks,
        catalog,
        "catalog".into(),
        bookmark_strips(view, cat).into_iter().map(at).collect(),
    );
    push(
        DiscardObject::Tags,
        catalog,
        "catalog".into(),
        tag_strips(view, catalog, cat, live),
    );
    push(
        DiscardObject::PrintSettings,
        catalog,
        "catalog".into(),
        print_strips(view, cat).into_iter().map(at).collect(),
    );
    for &id in live {
        if let Some(Object::Stream(s)) = view.value(id)
            && is_image(&s.dict)
            && s.dict.get(b"Alternates").is_some()
            && s.dict.get(b"OC").is_none()
        {
            let strips = vec![Strip {
                owner: id,
                path: vec![b"Alternates"],
            }];
            let location = format!("image object {} {}", id.num, id.generation);
            push(DiscardObject::AlternateImages, id, location, strips);
        }
    }
    out
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
        // An action written inline in an annotation or outline item.
        if dict
            .get(b"A")
            .and_then(Object::as_dict)
            .is_some_and(local_sd)
        {
            paths.push(vec![b"A", b"SD"]);
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
