//! The scale each content stream starts at, for `round`: page content from
//! the page's streams in order, form XObjects and tiling patterns from
//! every stream that draws them.
//!
//! A form or pattern gets a scale only when every reference to it in the
//! document sits in an `/XObject` or `/Pattern` resource dictionary the
//! walk read, and every `Do` of it was under a known matrix. Anything else
//! (an annotation appearance, a soft-mask group, a Type 3 glyph, a stream
//! that did not decode) may draw it at a scale not seen here, so it keeps
//! [`content::Unrounded::UnknownStart`].

use std::collections::{BTreeMap, BTreeSet};

use super::EditSession;
use crate::object::{Dict, ObjId, Object};
use crate::optimize::content::{self, Base, Scale};

/// How many streams one walk reads before it gives up on forms and
/// patterns; a document that nests past it rounds page content only.
const MAX_VISITS: usize = 4096;

/// How deep forms nest before the walk gives up (a cycle, or a file built
/// to be expensive).
const MAX_DEPTH: usize = 24;

/// How far up `/Parent` a page's inherited `/Resources` is looked for.
const MAX_PARENTS: usize = 64;

/// A resource dictionary and the indirect object whose value holds it.
type Held<'a> = (ObjId, &'a Dict);

struct Walk<'s> {
    session: &'s EditSession,
    /// Starts of forms and patterns: the widest over every draw.
    found: BTreeMap<ObjId, Option<Scale>>,
    /// References to each object inside the resource dictionaries read.
    accounted: BTreeMap<ObjId, usize>,
    read: BTreeSet<(ObjId, &'static [u8])>,
    visited: BTreeSet<(ObjId, [u64; 4])>,
    visits: usize,
    gave_up: bool,
}

impl EditSession {
    /// Each content stream's start scale; see the module documentation.
    pub(super) fn content_starts(&self) -> BTreeMap<ObjId, Option<Scale>> {
        let Ok(pages) = self.pages() else {
            return BTreeMap::new();
        };
        let view = self.view();
        let mut out = BTreeMap::new();
        let mut walk = Walk::new(self);
        for page in &pages {
            let streams: Vec<_> = page
                .contents
                .iter()
                .map(|id| self.parsed_content(&view, *id))
                .collect();
            for (id, start) in page.contents.iter().zip(content::starts(&streams)) {
                note(&mut out, *id, start);
            }
            if let Some(res) = walk.page_resources(page.id) {
                let refs: Vec<_> = streams.iter().map(Option::as_ref).collect();
                walk.run(Base::PAGE, &refs, res, 0);
            }
        }
        if !walk.gave_up {
            let total = self.reference_counts();
            for (id, start) in walk.found {
                let all = walk.accounted.get(&id) == total.get(&id);
                note(&mut out, id, start.filter(|_| all));
            }
        }
        out
    }

    /// How many references to each object the live document holds.
    fn reference_counts(&self) -> BTreeMap<ObjId, usize> {
        let mut out = BTreeMap::new();
        for id in self.live_objects() {
            let mut refs = Vec::new();
            if let Some(value) = self.value(id) {
                super::collect_references(value, false, 0, &mut refs);
            }
            for r in refs {
                *out.entry(r).or_default() += 1;
            }
        }
        out
    }
}

/// Record `start` for `id`, keeping the widest; one unknown makes it
/// unknown.
fn note(out: &mut BTreeMap<ObjId, Option<Scale>>, id: ObjId, start: Option<Scale>) {
    out.entry(id)
        .and_modify(|s| *s = widest(*s, start))
        .or_insert(start);
}

/// The larger of two scales; unknown when either is.
fn widest(a: Option<Scale>, b: Option<Scale>) -> Option<Scale> {
    let (a, b) = (a?, b?);
    Some(if b.get() > a.get() { b } else { a })
}

impl<'s> Walk<'s> {
    fn new(session: &'s EditSession) -> Self {
        Self {
            session,
            found: BTreeMap::new(),
            accounted: BTreeMap::new(),
            read: BTreeSet::new(),
            visited: BTreeSet::new(),
            visits: 0,
            gave_up: false,
        }
    }

    fn dict(&self, id: ObjId) -> Option<&'s Dict> {
        match self.session.value(id)? {
            Object::Dict(d) => Some(d),
            Object::Stream(s) => Some(&s.dict),
            _ => None,
        }
    }

    /// `holder`'s entry `key` as a dictionary, with the object holding it.
    fn entry(&self, (holder, dict): Held<'s>, key: &[u8]) -> Option<Held<'s>> {
        match dict.get(key)? {
            Object::Reference(r) => Some((*r, self.dict(*r)?)),
            Object::Dict(d) => Some((holder, d)),
            _ => None,
        }
    }

    /// The page's `/Resources`, own or inherited (§7.7.3.4).
    fn page_resources(&self, page: ObjId) -> Option<Held<'s>> {
        let mut at = page;
        for _ in 0..MAX_PARENTS {
            let dict = self.dict(at)?;
            if dict.get(b"Resources").is_some() {
                return self.entry((at, dict), b"Resources");
            }
            at = dict.get(b"Parent")?.as_reference()?;
        }
        None
    }

    /// Read `res`'s `/XObject` or `/Pattern` dictionary once, counting the
    /// references it holds.
    fn sub(&mut self, res: Held<'s>, kind: &'static [u8]) -> Option<Held<'s>> {
        let held = self.entry(res, kind)?;
        if self.read.insert((held.0, kind)) {
            for (_, v) in held.1.iter() {
                if let Some(r) = v.as_reference() {
                    *self.accounted.entry(r).or_default() += 1;
                }
            }
        }
        Some(held)
    }

    /// Walk streams run from `base` with resources `res`: record each form
    /// they draw and each tiling pattern `res` names, then walk those.
    fn run(
        &mut self,
        base: Base,
        streams: &[Option<&crate::content::ContentStream>],
        res: Held<'s>,
        depth: usize,
    ) {
        self.visits += 1;
        if depth > MAX_DEPTH || self.visits > MAX_VISITS {
            self.gave_up = true;
            return;
        }
        let invoked = content::invoked(base, streams);
        if let Some(xobjects) = self.sub(res, b"XObject") {
            for (name, at) in &invoked.forms {
                if let Some(id) = xobjects
                    .1
                    .get(name.as_slice())
                    .and_then(Object::as_reference)
                {
                    self.form(id, *at, res, depth);
                }
            }
            if !invoked.complete {
                let ids: Vec<ObjId> = xobjects
                    .1
                    .iter()
                    .filter_map(|(_, v)| v.as_reference())
                    .collect();
                for id in ids {
                    self.found.insert(id, None);
                }
            }
        }
        if let Some(patterns) = self.sub(res, b"Pattern") {
            let ids: Vec<ObjId> = patterns
                .1
                .iter()
                .filter_map(|(_, v)| v.as_reference())
                .collect();
            for id in ids {
                self.child(id, Some(base), res, depth);
            }
        }
    }

    fn form(&mut self, id: ObjId, at: Option<Base>, res: Held<'s>, depth: usize) {
        let is_form = self.dict(id).is_some_and(|d| {
            d.get(b"Subtype")
                .and_then(Object::as_name)
                .is_some_and(|n| n.as_bytes() == b"Form")
        });
        if is_form {
            self.child(id, at, res, depth);
        }
    }

    /// A form or tiling pattern drawn from a stream whose own space maps to
    /// default space by `parent` (unknown: `None`).
    fn child(&mut self, id: ObjId, parent: Option<Base>, res: Held<'s>, depth: usize) {
        let Some(Object::Stream(stream)) = self.session.value(id) else {
            return;
        };
        let tiling = matches!(stream.dict.get(b"PatternType"), Some(Object::Integer(1)));
        let form = stream
            .dict
            .get(b"Subtype")
            .and_then(Object::as_name)
            .is_some_and(|n| n.as_bytes() == b"Form");
        if !tiling && !form {
            return;
        }
        let base = parent.and_then(|p| p.under(stream.dict.get(b"Matrix")));
        note(&mut self.found, id, base.and_then(Base::scale));
        let Some(base) = base else {
            return;
        };
        if !self.visited.insert((id, base.key())) {
            return;
        }
        let own = self.entry((id, &stream.dict), b"Resources");
        let content = self.session.parsed_content(&self.session.view(), id);
        self.run(base, &[content.as_ref()], own.unwrap_or(res), depth + 1);
    }
}
