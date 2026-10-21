//! Planning [`super::resubset`]: which `/FontFile2` programs are safe to
//! cut, for which codes, and what the cut saves.
//!
//! A program is cut only when every font drawing from it is one the glyph
//! walk saw drawn: each font dictionary is held only by walked owners
//! (pages, page-tree nodes, walked streams, Type 3 fonts) or by resource
//! dictionaries only they hold, and its descriptor and program are held
//! only by the fonts in the group. Anything else refuses the group.

use std::collections::{BTreeMap, BTreeSet};

use super::fragments::{Referrers, referrers};
use super::glyph_usage::{GlyphUsage, enumerate};
use super::resubset::{
    FontResubsetter, FontUse, ResubsetPlan, ResubsetRefusal, ResubsetRefused, ResubsetRequest,
    ResubsetTarget,
};
use crate::filters::flate;
use crate::font_unembed::{Located, locate};
use crate::fontinfo::{FontSubtype, ProgramKey, Surface, split_subset_tag};
use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::view::DocumentView;

/// Resource dictionaries followed above a font before the holder check
/// gives up.
const MAX_HOLDER_DEPTH: u8 = 4;

/// Plan the cut of every eligible program in `view`, cutting with `source`.
/// `live` is every object reachable from the trailer.
pub(crate) fn plan(
    view: &DocumentView<'_>,
    live: &BTreeSet<ObjId>,
    source: &dyn FontResubsetter,
) -> ResubsetPlan {
    let inventory = crate::fontinfo::inventory(view);
    let located: Vec<Located<'_>> = inventory.fonts.iter().map(|r| locate(view, r)).collect();
    let usage = enumerate(view);
    let refs = referrers(view, live);
    let mut tags: BTreeSet<String> = inventory
        .fonts
        .iter()
        .filter_map(|r| r.subset_tag.clone())
        .collect();
    let mut out = ResubsetPlan::default();
    for (program, group) in groups(&located, &mut out) {
        let check = Group {
            view,
            usage: &usage,
            refs: &refs,
            users: &group,
        };
        match check
            .eligible(program)
            .and_then(|()| check.cut(program, source, &mut tags))
        {
            Ok(target) => out.targets.push(target),
            Err(why) => out
                .refused
                .extend(group.iter().map(|l| refused(l, why.clone()))),
        }
    }
    out
}

/// Embedded fonts grouped by program; a font with no indirect program or
/// dictionary is refused into `out` instead.
fn groups<'l, 'r>(
    located: &'l [Located<'r>],
    out: &mut ResubsetPlan,
) -> BTreeMap<ObjId, Vec<&'l Located<'r>>> {
    let mut by_program: BTreeMap<ObjId, Vec<&Located<'_>>> = BTreeMap::new();
    for l in located.iter().filter(|l| l.record.program.is_embedded()) {
        match (l.record.id, l.program_id) {
            (Some(_), Some(p)) => by_program.entry(p).or_default().push(l),
            (None, _) => out.refused.push(refused(
                l,
                ResubsetRefusal::new(
                    "font-not-indirect",
                    "the font dictionary is not an indirect object",
                ),
            )),
            (Some(_), None) => out.refused.push(refused(
                l,
                ResubsetRefusal::new(
                    "program-direct",
                    "the font program is not an indirect object",
                ),
            )),
        }
    }
    by_program
}

fn refused(l: &Located<'_>, why: ResubsetRefusal) -> ResubsetRefused {
    ResubsetRefused {
        font: l.record.id,
        name: l.record.base_font.clone().unwrap_or_default(),
        why,
    }
}

/// One program's users, checked against the walk.
struct Group<'a, 'v, 'l, 'r> {
    view: &'a DocumentView<'v>,
    usage: &'a GlyphUsage,
    refs: &'a Referrers,
    users: &'a [&'l Located<'r>],
}

impl Group<'_, '_, '_, '_> {
    fn ids(&self) -> Vec<ObjId> {
        self.users.iter().filter_map(|l| l.record.id).collect()
    }

    fn surface(&self, s: &Surface) -> bool {
        self.users.iter().any(|l| l.record.surfaces.contains(s))
    }

    /// `Ok` when nothing but the walk can draw from `program`.
    fn eligible(&self, program: ObjId) -> Result<(), ResubsetRefusal> {
        if let Some(fault) = self.usage.faults.first() {
            return Err(ResubsetRefusal::new(
                "unreadable-content",
                format!("the drawing walk stopped: {fault}"),
            ));
        }
        if self.surface(&Surface::AcroFormDefaultResources) {
            return Err(ResubsetRefusal::new(
                "acroform-dr",
                "the font is in the form's default resources, so a field can type with it",
            ));
        }
        if self.surface(&Surface::UnreferencedObjects) {
            return Err(ResubsetRefusal::new(
                "unreferenced-user",
                "a font sharing the program is drawn by nothing pdfcer walks",
            ));
        }
        if self
            .users
            .iter()
            .any(|l| l.program_key != Some(ProgramKey::FontFile2))
        {
            return Err(ResubsetRefusal::new(
                "format-deferred",
                "only TrueType (/FontFile2) programs are re-subset",
            ));
        }
        if self.users.iter().any(|l| l.cid_set || l.char_set) {
            return Err(ResubsetRefusal::new(
                "cidset-charset-present",
                "the descriptor lists its glyphs in /CIDSet or /CharSet",
            ));
        }
        if self.ids().iter().any(|id| self.usage.unsplit.contains(id)) {
            return Err(ResubsetRefusal::new(
                "non-identity-cmap",
                "a Type 0 font shows text under a CMap other than Identity",
            ));
        }
        if !self.held_by_walk(program) {
            return Err(ResubsetRefusal::new(
                "unreferenced-user",
                "something the drawing walk did not see refers to the font",
            ));
        }
        Ok(())
    }

    /// Every referrer of the fonts, their descendants, descriptors and the
    /// program is one the walk accounts for.
    fn held_by_walk(&self, program: ObjId) -> bool {
        let fonts: BTreeSet<ObjId> = self.ids().into_iter().collect();
        let mut sources = BTreeSet::new();
        let mut descendant_holders = fonts.clone();
        for l in self.users {
            let (Some(id), true) = (l.record.id, l.record.subtype == FontSubtype::Type0) else {
                sources.extend(l.record.id);
                continue;
            };
            let Some(entry) = self
                .view
                .value(id)
                .and_then(Object::as_dict)
                .and_then(|d| d.get(b"DescendantFonts"))
            else {
                return false;
            };
            if let Some(array) = entry.as_reference() {
                if !self.only(array, &fonts) {
                    return false;
                }
                descendant_holders.insert(array);
            }
            let kids = self.view.resolve(entry).as_array().unwrap_or_default();
            sources.extend(kids.iter().filter_map(Object::as_reference));
        }
        let descriptors: BTreeSet<ObjId> =
            self.users.iter().filter_map(|l| l.descriptor_id).collect();
        let program_holders: BTreeSet<ObjId> = descriptors.union(&sources).copied().collect();
        fonts.iter().all(|&f| self.walk_holds(f, 0))
            && sources
                .difference(&fonts)
                .all(|&c| self.only(c, &descendant_holders))
            && descriptors.iter().all(|&d| self.only(d, &sources))
            && self.only(program, &program_holders)
    }

    /// Every referrer of `id` is in `allowed`.
    fn only(&self, id: ObjId, allowed: &BTreeSet<ObjId>) -> bool {
        self.refs
            .get(&id)
            .is_none_or(|rs| rs.iter().all(|r| allowed.contains(r)))
    }

    /// Every referrer of `id` is a walked owner, or a resource dictionary
    /// the walk looked through that only such referrers hold.
    fn walk_holds(&self, id: ObjId, depth: u8) -> bool {
        if depth > MAX_HOLDER_DEPTH {
            return false;
        }
        let Some(rs) = self.refs.get(&id) else {
            return false;
        };
        rs.iter().all(|&r| {
            self.usage.holders.contains(&r) && (self.is_owner(r) || self.walk_holds(r, depth + 1))
        })
    }

    /// A page, page-tree node, stream or Type 3 font: something whose
    /// drawing the walk followed rather than a dictionary it looked through.
    fn is_owner(&self, id: ObjId) -> bool {
        match self.view.value(id) {
            Some(Object::Stream(_)) => true,
            Some(Object::Dict(d)) => {
                let name = |k: &[u8]| {
                    d.get(k)
                        .and_then(Object::as_name)
                        .map(|n| n.as_bytes().to_vec())
                };
                matches!(name(b"Type").as_deref(), Some(b"Page" | b"Pages"))
                    || name(b"Subtype").as_deref() == Some(b"Type3")
            }
            _ => false,
        }
    }

    /// Cut `program` with `source` and measure it.
    fn cut(
        &self,
        program: ObjId,
        source: &dyn FontResubsetter,
        tags: &mut BTreeSet<String>,
    ) -> Result<ResubsetTarget, ResubsetRefusal> {
        let unreadable =
            || ResubsetRefusal::new("malformed-glyf", "the font program cannot be read");
        let Some(Object::Stream(stream)) = self.view.graph().value(program) else {
            return Err(unreadable());
        };
        let raw = self.view.slice(stream.data_span).ok_or_else(unreadable)?;
        let data = crate::filters::decode_stream(&stream.dict, raw).map_err(|_| unreadable())?;
        let empty = BTreeSet::new();
        let uses: Vec<FontUse<'_>> = self
            .ids()
            .into_iter()
            .map(|f| FontUse::new(f, self.usage.codes.get(&f).unwrap_or(&empty)))
            .collect();
        let cut = source.resubset(&ResubsetRequest::new(self.view, &data, &uses))?;
        let after = flate::encode_with_level(&cut.program, 9).len() as u64;
        let before = raw.len() as u64;
        if after >= before {
            return Err(ResubsetRefusal::new(
                "not-smaller",
                format!("the cut program would be {after} bytes, not under {before}"),
            ));
        }
        let name_before = self
            .users
            .first()
            .and_then(|l| l.record.base_font.clone())
            .unwrap_or_default();
        let (_, stem) = split_subset_tag(&name_before);
        let tag = fresh_tag(&cut.program, tags);
        Ok(ResubsetTarget {
            program,
            fonts: self.ids(),
            name_after: format!("{tag}+{stem}"),
            name_before,
            glyphs_kept: cut.glyphs_kept,
            glyphs_total: cut.glyphs_total,
            before_bytes: before,
            after_bytes: after,
            data: cut.program,
        })
    }
}

/// Six uppercase letters derived from `data`, not in `taken`, and added to
/// it (§9.6.4: exactly six uppercase letters, distinct per subset).
fn fresh_tag(data: &[u8], taken: &mut BTreeSet<String>) -> String {
    let mut hash = data.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    loop {
        let mut n = hash;
        let tag: String = (0..6)
            .map(|_| {
                let c = char::from(b'A' + u8::try_from(n % 26).unwrap_or(0));
                n /= 26;
                c
            })
            .collect();
        if taken.insert(tag.clone()) {
            return tag;
        }
        hash = hash.wrapping_mul(0x0100_0000_01b3).wrapping_add(1);
    }
}

#[cfg(test)]
#[path = "resubset_plan_tests.rs"]
mod tests;
