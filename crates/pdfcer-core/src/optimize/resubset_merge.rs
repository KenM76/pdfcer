//! Merging subsets of one face in a re-subset run: eligible programs whose
//! fonts name one face, cut or not, become one program the cutter shows
//! serves every font ([`super::resubset::FontResubsetter::merge`]).

use std::collections::{BTreeMap, BTreeSet};

use super::resubset::{
    FontResubsetter, ResubsetPlan, ResubsetRefusal, ResubsetTarget, ResubsetUnmerged,
};
use super::resubset_plan::{Prepared, fresh_tag};
use crate::filters::flate;
use crate::fontinfo::split_subset_tag;
use crate::graph::ObjectGraph;
use crate::object::ObjId;
use crate::view::DocumentView;

/// Cut refusals a merge can still start from, using the program as it is.
const MERGEABLE_REFUSALS: [&str; 2] = ["every-glyph-used", "not-smaller"];

/// One run's merge step.
pub(super) struct Merge<'a, 'v> {
    pub(super) view: &'a DocumentView<'v>,
    pub(super) source: &'a dyn FontResubsetter,
    pub(super) tags: &'a mut BTreeSet<String>,
    pub(super) out: &'a mut ResubsetPlan,
}

impl Merge<'_, '_> {
    /// `prepared` less each face that two or more programs hold and that
    /// merged; its merged program joins the plan's targets. A face that
    /// would not merge is recorded in [`ResubsetPlan::unmerged`] and its
    /// programs returned to be cut on their own.
    pub(super) fn run<'l, 'r>(&mut self, prepared: Vec<Prepared<'l, 'r>>) -> Vec<Prepared<'l, 'r>> {
        let mut faces: BTreeMap<String, Vec<Prepared<'l, 'r>>> = BTreeMap::new();
        let mut rest = Vec::new();
        for p in prepared {
            match face(&p).filter(|_| mergeable(&p)) {
                Some(f) => faces.entry(f).or_default().push(p),
                None => rest.push(p),
            }
        }
        for (face, members) in faces {
            if members.len() < 2 {
                rest.extend(members);
                continue;
            }
            match self.merged(&members) {
                Ok(target) => self.out.targets.push(target),
                Err(why) => {
                    self.out.unmerged.push(ResubsetUnmerged {
                        face,
                        programs: members.iter().map(|m| m.program).collect(),
                        why,
                    });
                    rest.extend(members);
                }
            }
        }
        rest
    }

    /// One target replacing every program of `members`: the first is
    /// rewritten, the others are deleted.
    fn merged(&mut self, members: &[Prepared<'_, '_>]) -> Result<ResubsetTarget, ResubsetRefusal> {
        let Some((first, others)) = members.split_first() else {
            return Err(ResubsetRefusal::new(
                "malformed-glyf",
                "no programs to merge",
            ));
        };
        let programs: Vec<&[u8]> = members.iter().map(|m| m.bytes.as_slice()).collect();
        let merged = self.source.merge(&programs)?;
        let encoded = flate::encode_with_level(&merged.program, 9);
        let before: u64 = members.iter().map(|m| m.raw_len).sum();
        let after = encoded.len() as u64;
        if after >= before {
            return Err(ResubsetRefusal::new(
                "not-smaller",
                format!("the merged program would be {after} bytes, not under {before}"),
            ));
        }
        let name_before = first
            .users
            .first()
            .and_then(|l| l.record.base_font.clone())
            .unwrap_or_default();
        let (_, stem) = split_subset_tag(&name_before);
        let tag = fresh_tag(&merged.program, self.tags);
        let fonts: BTreeSet<ObjId> = members
            .iter()
            .flat_map(|m| m.users.iter().filter_map(|l| l.record.id))
            .collect();
        let rewritten: BTreeSet<ObjId> = members
            .iter()
            .flat_map(|m| std::iter::once(m.program).chain(m.renamed.iter().copied()))
            .collect();
        Ok(ResubsetTarget {
            program: first.program,
            merged: others.iter().map(|m| m.program).collect(),
            fonts: fonts.into_iter().collect(),
            name_after: format!("{tag}+{stem}"),
            name_before,
            glyphs_kept: merged.glyphs_kept,
            glyphs_total: merged.glyphs_total,
            before_bytes: before,
            after_bytes: after,
            length1: merged.program.len(),
            data: encoded,
            before: rewritten
                .into_iter()
                .filter_map(|id| Some((id, self.view.value(id)?.clone())))
                .collect(),
        })
    }
}

/// A program a merge can start from: cut, or left whole only because a cut
/// would save nothing.
fn mergeable(p: &Prepared<'_, '_>) -> bool {
    p.outcome
        .as_ref()
        .map_or_else(|why| MERGEABLE_REFUSALS.contains(&why.token), |_| true)
}

/// The face every font of `p` names, without its subset tag or a Type 0
/// font's CMap suffix (ISO 32000-2 Table 121); `None` when they disagree.
fn face(p: &Prepared<'_, '_>) -> Option<String> {
    let mut names = p.users.iter().map(|l| {
        let name = l.record.base_font.as_deref().unwrap_or_default();
        let (_, stem) = split_subset_tag(name);
        let stem = stem
            .strip_suffix("-Identity-H")
            .or_else(|| stem.strip_suffix("-Identity-V"))
            .unwrap_or(stem);
        stem.to_owned()
    });
    let first = names.next().filter(|n| !n.is_empty())?;
    names.all(|n| n == first).then_some(first)
}
