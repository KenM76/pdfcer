//! Planning [`super::resubset`]: which `/FontFile2` and `/FontFile3`
//! programs are safe to
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
    FontResubsetter, FontUse, ResubsetFonts, ResubsetPlan, ResubsetRefusal, ResubsetRefused,
    ResubsetRequest, ResubsetTarget,
};
use crate::filters::flate;
use crate::font_unembed::{Located, PdfaClaim, detect_pdfa, locate};
use crate::fontinfo::{FontSubtype, ProgramKey, Surface, split_subset_tag};
use crate::graph::ObjectGraph;
use crate::object::{ObjId, Object};
use crate::view::DocumentView;

/// Resource dictionaries followed above a font before the holder check
/// gives up.
const MAX_HOLDER_DEPTH: u8 = 4;

/// Plan the cut of every eligible program in `view` with `request`,
/// merging subsets of one face when it asks. `live` is every object
/// reachable from the trailer.
pub(crate) fn plan(
    view: &DocumentView<'_>,
    live: &BTreeSet<ObjId>,
    request: ResubsetFonts,
) -> ResubsetPlan {
    plan_with(view, live, request.source, request.merge)
}

/// [`plan`] with `source`, merging when `merge`.
fn plan_with(
    view: &DocumentView<'_>,
    live: &BTreeSet<ObjId>,
    source: &dyn FontResubsetter,
    merge: bool,
) -> ResubsetPlan {
    let inventory = crate::fontinfo::inventory(view);
    let located: Vec<Located<'_>> = inventory.fonts.iter().map(|r| locate(view, r)).collect();
    let usage = enumerate(view);
    let refs = referrers(view, live);
    let pdfa1 = claims_pdfa1(view);
    let mut tags: BTreeSet<String> = inventory
        .fonts
        .iter()
        .filter_map(|r| r.subset_tag.clone())
        .collect();
    let mut out = ResubsetPlan::default();
    let mut prepared = Vec::new();
    for (program, group) in groups(&located, &mut out) {
        let check = Group {
            view,
            usage: &usage,
            refs: &refs,
            users: &group,
            pdfa1,
        };
        match check
            .eligible(program)
            .and_then(|()| check.prepare(program, source, &mut tags))
        {
            Ok(p) => prepared.push(p),
            Err(why) => out
                .refused
                .extend(group.iter().map(|l| refused(l, why.clone()))),
        }
    }
    if merge {
        let mut merge = super::resubset_merge::Merge {
            view,
            source,
            tags: &mut tags,
            out: &mut out,
        };
        prepared = merge.run(prepared);
    }
    for p in prepared {
        match p.outcome {
            Ok(target) => out.targets.push(target),
            Err(why) => out
                .refused
                .extend(p.users.iter().map(|l| refused(l, why.clone()))),
        }
    }
    out.targets.sort_by_key(|t| t.program);
    out
}

/// Whether `view` claims PDF/A in any part but 2, 3 or 4 (part 1, or one
/// pdfcer cannot read), so is held to the strictest rule.
fn claims_pdfa1(view: &DocumentView<'_>) -> bool {
    matches!(
        detect_pdfa(view),
        PdfaClaim::Identified { part, .. }
            if !matches!(part.as_deref(), Some("2" | "3" | "4"))
    )
}

/// Whether `l`, untagged now, would become a PDF/A-1 subset without the
/// list ISO 19005-1 §6.3.5 requires: `/CIDSet` on a CIDFont, `/CharSet` on
/// a Type 1 font. A tagged font keeps whatever it had, which the cut leaves
/// true.
fn owes_a_list(l: &Located<'_>) -> bool {
    if l.record.subset_tag.is_some() {
        return false;
    }
    match l.record.subtype {
        FontSubtype::Type0 | FontSubtype::CidFontType0 | FontSubtype::CidFontType2 => !l.cid_set,
        FontSubtype::Type1 | FontSubtype::MmType1 => !l.char_set,
        _ => false,
    }
}

/// An eligible program, cut or refused, with what a merge needs.
pub(super) struct Prepared<'l, 'r> {
    pub(super) program: ObjId,
    pub(super) users: Vec<&'l Located<'r>>,
    /// The program's stored size.
    pub(super) raw_len: u64,
    /// The program a merge starts from: the cut, or the program as it is.
    pub(super) bytes: Vec<u8>,
    pub(super) outcome: Result<ResubsetTarget, ResubsetRefusal>,
    /// The dictionaries carrying the program's name.
    pub(super) renamed: BTreeSet<ObjId>,
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

/// `l`'s font refused for `why`.
pub(super) fn refused(l: &Located<'_>, why: ResubsetRefusal) -> ResubsetRefused {
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
    /// The document claims PDF/A-1, or a part pdfcer could not read.
    pdfa1: bool,
}

impl<'l, 'r> Group<'_, '_, 'l, 'r> {
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
        if self.users.iter().any(|l| {
            !matches!(
                l.program_key,
                Some(ProgramKey::FontFile2 | ProgramKey::FontFile3)
            )
        }) {
            return Err(ResubsetRefusal::new(
                "format-deferred",
                "only TrueType (/FontFile2) and CFF (/FontFile3) programs are re-subset",
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
        if self.pdfa1 && self.users.iter().any(|l| owes_a_list(l)) {
            return Err(ResubsetRefusal::new(
                "pdfa1-list-owed",
                "the document claims PDF/A-1, which requires a /CIDSet or /CharSet on a subset font; this font has none, and pdfcer does not yet write one",
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

    /// Decode `program` and cut it with `source`. `Err` only when it
    /// cannot be read; a refused cut is kept in [`Prepared::outcome`].
    fn prepare(
        &self,
        program: ObjId,
        source: &dyn FontResubsetter,
        tags: &mut BTreeSet<String>,
    ) -> Result<Prepared<'l, 'r>, ResubsetRefusal> {
        let (raw_len, data) = decoded(self.view, program)?;
        let (bytes, outcome) = match self.cut(program, raw_len, &data, source, tags) {
            Ok((target, cut)) => (cut, Ok(target)),
            Err(why) => (data, Err(why)),
        };
        Ok(Prepared {
            program,
            users: self.users.to_vec(),
            raw_len,
            bytes,
            outcome,
            renamed: self.renamed(),
        })
    }

    /// Cut `data`, the decoded `program` of `before` stored bytes, with
    /// `source` and measure it; the target and the cut program.
    fn cut(
        &self,
        program: ObjId,
        before: u64,
        data: &[u8],
        source: &dyn FontResubsetter,
        tags: &mut BTreeSet<String>,
    ) -> Result<(ResubsetTarget, Vec<u8>), ResubsetRefusal> {
        let empty = BTreeSet::new();
        let uses: Vec<FontUse<'_>> = self
            .ids()
            .into_iter()
            .map(|f| FontUse::new(f, self.usage.codes.get(&f).unwrap_or(&empty)))
            .collect();
        let cut = source.resubset(&ResubsetRequest::new(self.view, data, &uses))?;
        if cut.glyphs_kept >= cut.glyphs_total {
            return Err(ResubsetRefusal::new(
                "every-glyph-used",
                "the document can draw every glyph the program holds, so there is nothing to cut",
            ));
        }
        let encoded = flate::encode_with_level(&cut.program, 9);
        let after = encoded.len() as u64;
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
        let snapshot = std::iter::once(program)
            .chain(self.renamed())
            .filter_map(|id| Some((id, self.view.value(id)?.clone())))
            .collect();
        let target = ResubsetTarget {
            program,
            merged: Vec::new(),
            fonts: self.ids(),
            name_after: format!("{tag}+{stem}"),
            name_before,
            glyphs_kept: cut.glyphs_kept,
            glyphs_total: cut.glyphs_total,
            before_bytes: before,
            after_bytes: after,
            length1: cut.program.len(),
            data: encoded,
            before: snapshot,
        };
        Ok((target, cut.program))
    }

    /// The dictionaries carrying the program's name: the fonts, each Type 0
    /// font's descendants, and the descriptors (§9.6.4).
    fn renamed(&self) -> BTreeSet<ObjId> {
        let mut out: BTreeSet<ObjId> = self.ids().into_iter().collect();
        for l in self.users {
            out.extend(l.descriptor_id);
            let kids = l
                .record
                .id
                .and_then(|id| self.view.value(id))
                .and_then(Object::as_dict)
                .and_then(|d| d.get(b"DescendantFonts"))
                .map(|e| self.view.resolve(e))
                .and_then(Object::as_array)
                .unwrap_or_default();
            out.extend(kids.iter().filter_map(Object::as_reference));
        }
        out
    }
}

/// The stored size of stream `program`, and its data decoded.
pub(super) fn decoded(
    view: &DocumentView<'_>,
    program: ObjId,
) -> Result<(u64, Vec<u8>), ResubsetRefusal> {
    let unreadable = || ResubsetRefusal::new("malformed-glyf", "the font program cannot be read");
    let Some(Object::Stream(stream)) = view.graph().value(program) else {
        return Err(unreadable());
    };
    let raw = view.slice(stream.data_span).ok_or_else(unreadable)?;
    let data = crate::filters::decode_stream(&stream.dict, raw).map_err(|_| unreadable())?;
    Ok((raw.len() as u64, data))
}

/// Six uppercase letters derived from `data`, not in `taken`, and added to
/// it (§9.6.4: exactly six uppercase letters, distinct per subset).
pub(super) fn fresh_tag(data: &[u8], taken: &mut BTreeSet<String>) -> String {
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
