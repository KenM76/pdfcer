//! Which part of a linearized file each object goes in (F.3.1), decided by
//! who uses it, with qpdf's precedence so its check sees the same parts.

use std::collections::{BTreeMap, BTreeSet};

use super::Graph;
use crate::object::{Name, ObjId, Object};

/// Catalog keys a viewer needs before it can show any page (F.3.5).
const OPEN_DOCUMENT_KEYS: [&[u8]; 5] = [
    b"ViewerPreferences",
    b"PageMode",
    b"Threads",
    b"OpenAction",
    b"AcroForm",
];

/// A reason an object is in the file.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
enum User {
    Root,
    RootKey(Name),
    TrailerKey(Name),
    Page(usize),
    Thumb(usize),
}

/// The objects of each part, in output order.
pub(super) struct Parts {
    /// Part 4: the catalog, then the document-level objects.
    pub part4: Vec<ObjId>,
    /// Part 6: page 1, its private objects, its shared objects, and the
    /// outlines when the document opens on them.
    pub part6: Vec<ObjId>,
    /// Part 7: per later page, the page then its private objects.
    pub part7: Vec<Vec<ObjId>>,
    /// Part 8: objects shared by later pages.
    pub part8: Vec<ObjId>,
    /// Part 9: everything else.
    pub part9: Vec<ObjId>,
    /// Per later page, its shared objects (members of part 6 or 8).
    pub shared: Vec<Vec<ObjId>>,
    /// The outline dictionary and the length of its contiguous run.
    pub outlines: Option<(ObjId, usize)>,
}

#[derive(Default)]
struct Uses {
    by_object: BTreeMap<ObjId, BTreeSet<User>>,
    by_user: BTreeMap<User, BTreeSet<ObjId>>,
}

impl Uses {
    /// What `user` reaches, in number order.
    fn reached(&self, user: &User) -> impl Iterator<Item = ObjId> + '_ {
        self.by_user.get(user).into_iter().flatten().copied()
    }

    fn record(&mut self, id: ObjId, user: &User) -> bool {
        let new = self.by_object.entry(id).or_default().insert(user.clone());
        if new {
            self.by_user.entry(user.clone()).or_default().insert(id);
        }
        new
    }

    /// Everything `user` reaches from `value`. A page reached other than
    /// as `top` is another page's, and is not followed; a page tree node
    /// is followed only from the catalog's `/Pages` (qpdf's rules).
    fn walk(&mut self, graph: &Graph<'_>, user: &User, value: &Object, top: Option<usize>) {
        let tree = matches!(user, User::RootKey(k) if k.as_bytes() == b"Pages");
        let mut pending = vec![(value.clone(), top)];
        while let Some((value, top)) = pending.pop() {
            let value = match value {
                Object::Reference(r) => {
                    let Some(id) = graph.target(r) else { continue };
                    let Some(v) = graph.value(id) else { continue };
                    // A null object (what a dangling reference is written
                    // as) has no user in qpdf's count; it ends part 9.
                    if matches!(v, Object::Null)
                        || top.is_none() && has_type(v, b"Page")
                        || !tree && has_type(v, b"Pages")
                    {
                        continue;
                    }
                    if !self.record(id, user) {
                        continue;
                    }
                    v.clone()
                }
                other => other,
            };
            let dict = match &value {
                Object::Array(items) => {
                    pending.extend(items.iter().map(|i| (i.clone(), None)));
                    continue;
                }
                Object::Dict(d) => d,
                Object::Stream(s) => &s.dict,
                _ => continue,
            };
            for (key, v) in dict.iter() {
                match (top, key.as_bytes()) {
                    (_, b"Length") if matches!(value, Object::Stream(_)) => {}
                    (Some(_), b"Parent") => {}
                    (Some(page), b"Thumb") => self.walk(graph, &User::Thumb(page), v, None),
                    _ => pending.push((v.clone(), None)),
                }
            }
        }
    }
}

fn has_type(value: &Object, ty: &[u8]) -> bool {
    let dict = match value {
        Object::Dict(d) => d,
        Object::Stream(s) => &s.dict,
        _ => return false,
    };
    dict.get(b"Type")
        .and_then(Object::as_name)
        .map(Name::as_bytes)
        == Some(ty)
}

/// Who uses what, from the users qpdf counts.
fn uses(graph: &Graph<'_>, root: ObjId, info: Option<ObjId>, pages: &[ObjId]) -> Uses {
    let mut uses = Uses::default();
    for (i, &page) in pages.iter().enumerate() {
        uses.walk(graph, &User::Page(i), &Object::Reference(page), Some(i));
    }
    if let Some(info) = info {
        let user = User::TrailerKey(Name::from(b"Info"));
        uses.walk(graph, &user, &Object::Reference(info), None);
    }
    if let Some(catalog) = graph.dict(root) {
        for (key, value) in catalog.iter() {
            uses.walk(graph, &User::RootKey(key.clone()), value, None);
        }
    }
    uses.record(root, &User::Root);
    uses
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Category {
    Root,
    Outlines,
    OpenDocument,
    FirstPrivate,
    FirstShared,
    OtherPrivate,
    OtherShared,
    ThumbPrivate,
    ThumbShared,
    Other,
}

fn category(users: &BTreeSet<User>) -> Category {
    let (mut root, mut outlines, mut open, mut first) = (false, false, false, false);
    let (mut other_pages, mut thumbs, mut others) = (0usize, 0usize, 0usize);
    for user in users {
        match user {
            User::Root => root = true,
            User::RootKey(k) if OPEN_DOCUMENT_KEYS.contains(&k.as_bytes()) => open = true,
            User::RootKey(k) if k.as_bytes() == b"Outlines" => outlines = true,
            User::RootKey(_) | User::TrailerKey(_) => others += 1,
            User::Page(0) => first = true,
            User::Page(_) => other_pages += 1,
            User::Thumb(_) => thumbs += 1,
        }
    }
    let alone = others == 0 && thumbs == 0 && !outlines;
    // A page claims what it uses ahead of the outlines, thumbnails and other
    // document-level users: qpdf 12's check counts it that way (probed with
    // two-key fixtures; see `tests::a_page_claims_what_it_shares_with_the_document`).
    match () {
        () if root => Category::Root,
        () if open => Category::OpenDocument,
        () if first && alone && other_pages == 0 => Category::FirstPrivate,
        () if first => Category::FirstShared,
        () if other_pages == 1 => Category::OtherPrivate,
        () if other_pages > 1 => Category::OtherShared,
        () if outlines => Category::Outlines,
        () if thumbs == 1 && others == 0 => Category::ThumbPrivate,
        () if thumbs > 1 => Category::ThumbShared,
        () => Category::Other,
    }
}

/// Every kept object assigned to its part. Objects no user reaches (kept
/// because unreferenced objects were not removed) end part 9.
pub(super) fn parts(graph: &Graph<'_>, root: ObjId, info: Option<ObjId>, pages: &[ObjId]) -> Parts {
    let uses = uses(graph, root, info, pages);
    let mut sets: BTreeMap<u8, BTreeSet<ObjId>> = BTreeMap::new();
    for (&id, users) in &uses.by_object {
        sets.entry(category(users) as u8).or_default().insert(id);
    }
    let mut take = |c: Category| sets.remove(&(c as u8)).unwrap_or_default();
    let (root_set, mut outline_set, open) = (
        take(Category::Root),
        take(Category::Outlines),
        take(Category::OpenDocument),
    );
    let (first_private, first_shared) = (take(Category::FirstPrivate), take(Category::FirstShared));
    let mut other_private = take(Category::OtherPrivate);
    let other_shared = take(Category::OtherShared);
    let mut thumb_private = take(Category::ThumbPrivate);
    let thumb_shared = take(Category::ThumbShared);
    let mut other = take(Category::Other);

    let part4 = root_set.into_iter().chain(open).collect();
    let first_page = pages.first().copied();
    let mut part6: Vec<ObjId> = first_page.into_iter().collect();
    part6.extend(
        first_private
            .into_iter()
            .filter(|&id| Some(id) != first_page),
    );
    part6.extend(first_shared);
    let catalog = graph.dict(root);
    let outline_dict = catalog.and_then(|c| graph.referent(c.get(b"Outlines")));
    let use_outlines = catalog
        .and_then(|c| c.get(b"PageMode"))
        .and_then(Object::as_name)
        .is_some_and(|n| n.as_bytes() == b"UseOutlines");
    let mut outlines = if use_outlines {
        outline_run(&mut part6, outline_dict, &mut outline_set)
    } else {
        None
    };

    let part7 = page_groups(&uses, pages, &mut other_private);

    let part8: Vec<ObjId> = other_shared.into_iter().collect();
    let tree = User::RootKey(Name::from(b"Pages"));
    let mut part9: Vec<ObjId> = uses.reached(&tree).filter(|id| other.remove(id)).collect();
    for i in 0..pages.len() {
        part9.extend(
            uses.reached(&User::Thumb(i))
                .filter(|id| thumb_private.remove(id)),
        );
    }
    part9.extend(thumb_shared);
    if outlines.is_none() {
        outlines = outline_run(&mut part9, outline_dict, &mut outline_set);
    }
    part9.extend(leftovers(other, other_private, thumb_private));
    let placed: BTreeSet<ObjId> = uses.by_object.keys().copied().collect();
    part9.extend(graph.kept.iter().filter(|id| !placed.contains(id)));

    let shared = shared_refs(&uses, pages.len(), &part6, &part8);
    Parts {
        part4,
        part6,
        part7,
        part8,
        part9,
        shared,
        outlines,
    }
}

/// Per page after the first, the page then the objects only it uses.
fn page_groups(uses: &Uses, pages: &[ObjId], private: &mut BTreeSet<ObjId>) -> Vec<Vec<ObjId>> {
    let mut groups = Vec::new();
    for (i, &page) in pages.iter().enumerate().skip(1) {
        let mut group = vec![page];
        private.remove(&page);
        group.extend(uses.reached(&User::Page(i)).filter(|id| private.remove(id)));
        groups.push(group);
    }
    groups
}

/// Per page after the first, the objects it shares that the shared object
/// table lists (F.4.2: the members of parts 6 and 8).
fn shared_refs(uses: &Uses, pages: usize, part6: &[ObjId], part8: &[ObjId]) -> Vec<Vec<ObjId>> {
    let in_table: BTreeSet<ObjId> = part6.iter().chain(part8).copied().collect();
    (1..pages)
        .map(|i| {
            uses.reached(&User::Page(i))
                .filter(|id| in_table.contains(id))
                .filter(|id| uses.by_object.get(id).is_some_and(|u| u.len() > 1))
                .collect()
        })
        .collect()
}

/// Move the outline dictionary, then the rest of `outlines`, to the end of
/// `part`; the dictionary and the run's length.
fn outline_run(
    part: &mut Vec<ObjId>,
    dict: Option<ObjId>,
    outlines: &mut BTreeSet<ObjId>,
) -> Option<(ObjId, usize)> {
    let first = dict.filter(|id| outlines.remove(id))?;
    let start = part.len();
    part.push(first);
    part.extend(std::mem::take(outlines));
    Some((first, part.len() - start))
}

/// The objects still unplaced, in number order. Leftover page- or
/// thumbnail-private objects only exist when a page appears twice in the
/// tree; they still have to be written.
fn leftovers(
    other: BTreeSet<ObjId>,
    other_private: BTreeSet<ObjId>,
    thumb_private: BTreeSet<ObjId>,
) -> BTreeSet<ObjId> {
    other
        .into_iter()
        .chain(other_private)
        .chain(thumb_private)
        .collect()
}
