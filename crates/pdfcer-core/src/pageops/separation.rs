//! Preseparated page sets — keeping `/SeparationInfo` truthful across
//! structural page operations (ISO 32000-1 §14.11.4).
//!
//! # What a preseparated file is, and why page operations break it
//!
//! Most PDFs have one page object per page. A **preseparated** file does
//! not. §14.11.4:
//!
//! > "In some workflows … pages are **preseparated** before generating the
//! > PDF file. In a preseparated PDF file, **the separations for a page
//! > shall be described as separate page objects, each painting only a
//! > single colorant** (usually specified in the `DeviceGray` colour
//! > space). … This information shall be contained in a **separation
//! > dictionary (PDF 1.3)** in the `SeparationInfo` entry of each page
//! > object."
//!
//! So a single logical page of a CMYK job is **four page objects** — one
//! per plate — tied together by a `/SeparationInfo` dictionary on each.
//! Table 364 makes the tie explicit, and the wording is unusually strong:
//!
//! > `Pages` — array — *(Required)* "An array of indirect references to
//! > page objects representing separations of the same document page.
//! > **One of the page objects in the array shall be the one with which
//! > this separation dictionary is associated, and all of them shall have
//! > separation dictionaries (`SeparationInfo` entries) containing `Pages`
//! > arrays identical to this one.**"
//!
//! Two obligations follow, and pdfcer honoured neither before this module
//! existed:
//!
//! 1. **`Pages` is Required.** A separation dictionary without it is
//!    non-conforming.
//! 2. **Every member's array must be identical.** Not merely consistent —
//!    identical. So the moment one member of a set is removed, extracted
//!    away, or split into a different file, *every surviving member's
//!    array is wrong*, because each still names a page that is no longer
//!    reachable.
//!
//! That is not a prepress nicety. It makes **page deletion, extraction,
//! splitting and merging** — four operations pdfcer ships — silently
//! produce a corrupt file from a valid one. The operator deletes one page
//! and three *other* pages they never touched are left pointing at it.
//!
//! # What each shipped operation did before this module
//!
//! The two code paths failed in **different** ways, which is why the fix
//! has two halves rather than one.
//!
//! ## The assemble path (extract / split / merge) — a Required key vanished
//!
//! [`crate::pageops::assemble`] deep-copies each selected page behind a
//! **barrier**: the set of pages that are *not* being copied. Its
//! documented propagation rule is that an **array containing a barrier hit
//! refuses as a whole**, and a **dictionary entry whose value hit the
//! barrier is dropped**.
//!
//! Applied to `/SeparationInfo << /Pages [4 0 R 5 0 R 6 0 R] … >>` when
//! objects 5 and 6 are not copied, that rule is followed exactly and the
//! result is wrong: the `/Pages` array refuses, so the `/Pages` **entry is
//! dropped**, and the output keeps a separation dictionary that is missing
//! its Required key. The barrier was not defective — it was doing the
//! right generic thing to a key that needed a specific one. The hit was
//! counted into `dangling_references`, so the file was not silently
//! *unreported*; it was reported as a broken link, which is not what it
//! was.
//!
//! ## The delete path — dangling references nobody counted
//!
//! [`crate::edit::EditSession::delete_pages`] splices the page tree and
//! censuses what the removal breaks via
//! [`crate::pageops::census_dangling`]. That census covers outline items,
//! link annotations, named destinations and page labels. It does **not**
//! walk `/SeparationInfo`. Deleting one plate therefore left the other
//! three with an indirect reference to a freed object — the classic
//! dangling reference, resolving to null, uncounted and unrepaired.
//!
//! This module supplies the missing census class *and* the repair.
//!
//! # The policy, and why the default is `Repair`
//!
//! Three answers are defensible when an operation splits a separation set,
//! so the choice is a [`SeparationPolicy`] rather than a hard-coded
//! behaviour.
//!
//! **This is a product policy, not a spec ambiguity.** §14.11.4 is
//! perfectly clear about the invariant; what it does not say is what a
//! *writer* should do when an edit breaks it, because ISO 32000-1
//! describes files, not editors. The distinction matters for filing: this
//! knob does not belong in the ambiguity register, which exists for cases
//! where two conforming *readers* may legitimately disagree about the same
//! bytes.
//!
//! - [`SeparationPolicy::Repair`] (**default**) rewrites every surviving
//!   member's `/Pages` array to the surviving subset, restoring the
//!   identical-arrays invariant and keeping `/DeviceColorant` intact.
//! - [`SeparationPolicy::Discard`] strips `/SeparationInfo` from the
//!   survivors, demoting them to ordinary pages.
//! - [`SeparationPolicy::Refuse`] declines the operation.
//!
//! `Repair` is the default because **extracting one plate is a real
//! prepress task**, not an error to be prevented, and because it is the
//! answer that discards the least information. Extracting the cyan plate
//! under `Repair` yields a page that still says *"I am the Cyan
//! separation"* — a true statement, in a conforming one-member set (Table
//! 364 requires only that the array contain the page itself; it sets no
//! minimum length). Under `Discard` the same operation yields an anonymous
//! grey page and the fact that it was the cyan plate is gone. `Refuse` is
//! wrong as a default for the same reason: it would block a legitimate
//! operation to protect an invariant that `Repair` simply maintains.
//!
//! When the settings store lands, this is a natural operator setting —
//! shipped default `Repair`, with the reasoning above as the recorded
//! basis rather than a guess.
//!
//! # What is deliberately not done
//!
//! - **No set is auto-completed.** Selecting one member does not silently
//!   drag its three siblings into the output. That would make an
//!   extraction return more pages than were asked for, which is a worse
//!   surprise than the one being fixed.
//! - **An intact set is not rewritten.** If every member survives — the
//!   merge case, and any delete that misses the set entirely — the arrays
//!   are already identical and already correct, so nothing is touched.
//!   Rule 3's minimal-diff obligation is a constraint on the repair, not
//!   an exception to it.
//! - **A malformed input set is not invented into shape.** A
//!   `/SeparationInfo` with no `/Pages`, or a `/Pages` that is not an
//!   array, is already non-conforming on arrival; it is counted into
//!   [`SeparationImpact::malformed`] and left alone. Repairing it would
//!   mean guessing which pages were meant to be in the set, and there is
//!   no evidence in the file to guess from.
//! - **Colorant names are never matched against each other.** Table 364
//!   types `/DeviceColorant` as "name **or** string" with no stated
//!   equivalence rule — is `/Cyan` the same colorant as `(Cyan)`? That is
//!   the open question logged as `OI-A3` in the spec RAG. It does not bite
//!   here, because set membership is decided by **object identity** of the
//!   page references, never by comparing colorant names. The names are
//!   carried for *disclosure only*.

use std::collections::HashSet;

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};

/// What pdfcer does when a structural page operation would split a
/// preseparated page set (§14.11.4).
///
/// See the module docs for why [`Self::Repair`] is the default and why
/// this is a product policy rather than a spec ambiguity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub enum SeparationPolicy {
    /// Rewrite each surviving member's `/Pages` array to the surviving
    /// subset, preserving Table 364's identical-arrays invariant and
    /// keeping `/DeviceColorant`.
    #[default]
    Repair,
    /// Remove `/SeparationInfo` from surviving members entirely; they
    /// become ordinary pages with no record of which plate they were.
    Discard,
    /// Decline the operation rather than change the set.
    Refuse,
}

/// A page's separation dictionary, as read from the file.
///
/// Deliberately not a general-purpose model of Table 364: only the two
/// entries that bear on set membership and disclosure are carried.
/// `/ColorSpace` is preserved by the ordinary copy machinery and never
/// needs to be understood here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeparationDict {
    /// The `/Pages` array, as object ids in file order.
    ///
    /// Empty means the entry was absent or unreadable — see
    /// [`SeparationDict::is_malformed`].
    pub members: Vec<ObjId>,
    /// `/DeviceColorant`, normalized to bytes.
    ///
    /// A `Name` and a `String` both flatten to their bytes because this
    /// value is only ever *displayed*; see the module docs' note on
    /// `OI-A3`.
    pub colorant: Option<Vec<u8>>,
}

impl SeparationDict {
    /// Whether the dictionary arrived without a usable Required `/Pages`.
    ///
    /// Such a page is already non-conforming; pdfcer counts it and leaves
    /// it alone rather than guessing what the set should have been.
    #[must_use]
    pub fn is_malformed(&self) -> bool {
        self.members.is_empty()
    }
}

/// What a page operation did to the document's preseparated page sets.
///
/// # Why the colorants are **named** when [`crate::pageops::DanglingReport`] only counts
///
/// That type states its convention plainly: *"a delete that orphans 300
/// bookmarks should say 300, not list them."* The convention is right
/// there and wrong here, and the difference is worth stating so this does
/// not read as an inconsistency.
///
/// A separation set has as many members as the job has plates — four for
/// process CMYK, a handful more with spot colours. The count is small
/// enough to name, and the name is the entire actionable content:
/// *"3 separations removed"* tells the operator nothing they can act on,
/// while *"removed Magenta, Yellow, Black; kept Cyan"* tells them exactly
/// what they now have. Rule 4's disclosure obligation is satisfied by the
/// second sentence and not by the first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct SeparationImpact {
    /// Preseparated page sets that lost at least one member.
    pub sets_split: usize,
    /// Surviving page objects whose `/SeparationInfo` was rewritten
    /// (under [`SeparationPolicy::Repair`]) or removed (under
    /// [`SeparationPolicy::Discard`]).
    pub pages_changed: usize,
    /// `/DeviceColorant` of each member that left, in encounter order.
    ///
    /// Deduplicated: a colorant appears once however many logical pages
    /// lost that plate. A member whose dictionary named no colorant
    /// contributes nothing rather than a placeholder.
    pub colorants_removed: Vec<Vec<u8>>,
    /// `/DeviceColorant` of each member that survived in a split set,
    /// same encoding and same deduplication.
    pub colorants_kept: Vec<Vec<u8>>,
    /// Separation dictionaries that arrived without a usable Required
    /// `/Pages` array, and were therefore left untouched.
    pub malformed: usize,
    /// Which policy produced this outcome.
    ///
    /// Carried so a front end can describe what actually happened instead
    /// of assuming. Without it the disclosure has to hard-code one
    /// policy's wording, and the first version of this type did exactly
    /// that: under [`SeparationPolicy::Discard`] the CLI announced that
    /// surviving pages "had their `/SeparationInfo` `/Pages` array
    /// rewritten", when `Discard` had in fact removed the dictionary
    /// outright. A true count with a false sentence attached is a worse
    /// disclosure than no sentence, because it is believed.
    ///
    /// Meaningless when [`SeparationImpact::is_empty`] — nothing was
    /// done, so no policy was exercised.
    pub policy: SeparationPolicy,
}

impl SeparationImpact {
    /// Whether any preseparated set was affected at all.
    ///
    /// The common case by far — most documents are not preseparated — so
    /// a front end can skip the whole disclosure on this one call.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.sets_split == 0 && self.pages_changed == 0 && self.malformed == 0
    }

    /// Record one colorant into `colorants_removed`, keeping it unique.
    fn note_removed(&mut self, colorant: Option<&Vec<u8>>) {
        Self::push_unique(&mut self.colorants_removed, colorant);
    }

    /// Record one colorant into `colorants_kept`, keeping it unique.
    fn note_kept(&mut self, colorant: Option<&Vec<u8>>) {
        Self::push_unique(&mut self.colorants_kept, colorant);
    }

    fn push_unique(into: &mut Vec<Vec<u8>>, colorant: Option<&Vec<u8>>) {
        if let Some(name) = colorant
            && !into.iter().any(|seen| seen == name)
        {
            into.push(name.clone());
        }
    }
}

/// Read a page dictionary's `/SeparationInfo` (§14.11.4), if it has one.
///
/// Returns `None` for the overwhelmingly common case of a page that is not
/// part of a preseparated set — one key lookup, which is the whole cost of
/// detection on an ordinary document.
///
/// A `/SeparationInfo` that is present but unreadable as a dictionary is
/// treated as absent: there is nothing to preserve and nothing to repair.
/// One that is a dictionary but lacks a usable `/Pages` comes back with
/// empty [`SeparationDict::members`], which
/// [`SeparationDict::is_malformed`] reports and the callers count.
#[must_use]
pub fn separation_of<G: ObjectGraph + ?Sized>(graph: &G, page: &Dict) -> Option<SeparationDict> {
    let info = page
        .get(b"SeparationInfo")
        .map(|value| graph.resolve(value))
        .and_then(Object::as_dict)?;

    let members = info
        .get(b"Pages")
        .map(|value| graph.resolve(value))
        .and_then(Object::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(Object::as_reference)
                .collect::<Vec<ObjId>>()
        })
        .unwrap_or_default();

    let colorant = info
        .get(b"DeviceColorant")
        .map(|value| graph.resolve(value))
        .and_then(|value| match value {
            Object::Name(name) => Some(name.as_bytes().to_vec()),
            Object::String(bytes) => Some(bytes.clone()),
            _ => None,
        });

    Some(SeparationDict { members, colorant })
}

/// Whether any page in `pages` carries a separation dictionary.
///
/// The cheap pre-flight: a caller that gets `false` can skip every other
/// function here, which is what happens on essentially every document a
/// non-prepress operator will ever open.
#[must_use]
pub fn any_preseparated<G: ObjectGraph + ?Sized>(graph: &G, pages: &[ObjId]) -> bool {
    pages.iter().any(|id| {
        graph
            .resolved(*id)
            .as_dict()
            .is_some_and(|page| page.contains_key(b"SeparationInfo"))
    })
}

/// One surviving page's rewritten dictionary, ready to be written.
#[derive(Debug, Clone, PartialEq)]
pub struct SeparationRewrite {
    /// The page object to overwrite.
    pub page: ObjId,
    /// Its new dictionary — a clone of the original with only
    /// `/SeparationInfo` changed or removed.
    pub dict: Dict,
}

/// The full plan for keeping preseparated sets truthful across a removal.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SeparationPlan {
    /// Pages that must be re-emitted, and what to emit for them.
    ///
    /// Empty under every policy when no set lost a member, so an
    /// unaffected document produces no writes at all (rule 3).
    pub rewrites: Vec<SeparationRewrite>,
    /// What to tell the operator.
    pub impact: SeparationImpact,
}

/// Why a page operation declined to touch a preseparated page set.
///
/// Raised only under [`SeparationPolicy::Refuse`]. Carries the colorant
/// because *"this would split the Cyan/Magenta/Yellow/Black set"* is a
/// sentence an operator can act on, where *"a separation set would be
/// split"* is not.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error(
    "the operation would split a preseparated page set (ISO 32000-1 §14.11.4): \
     {losing} of {total} separations would be removed"
)]
pub struct SeparationSplitRefused {
    /// How many members the set would lose.
    pub losing: usize,
    /// How many members the set has.
    pub total: usize,
}

/// Plan the `/SeparationInfo` repair for a removal-shaped operation.
///
/// This is the **delete** half of the fix: pages are being removed from a
/// document that otherwise stays as it is, so the survivors are edited in
/// place.
///
/// `removed` is the set of page objects going away; `surviving` is every
/// page that stays, in any order. Only surviving pages are ever rewritten
/// — a removed page's own dictionary is about to cease to exist, so
/// repairing it would be work with no reader.
///
/// # The minimal-diff guarantee
///
/// A surviving page is rewritten **only if its own set actually lost a
/// member**. A document with no preseparated pages, or one whose
/// separation sets are entirely inside or entirely outside the removal,
/// yields an empty [`SeparationPlan::rewrites`] and therefore no object
/// writes at all. That is what keeps rule 3 intact: the repair touches
/// exactly the objects whose contents would otherwise have become false.
///
/// # Errors
///
/// [`SeparationSplitRefused`] under [`SeparationPolicy::Refuse`] only, and
/// only when a set would genuinely be split. The other two policies cannot
/// fail: an unreadable page, or one whose separation dictionary is
/// malformed, is counted and skipped rather than raised.
pub fn plan_repair<G: ObjectGraph + ?Sized>(
    graph: &G,
    surviving: &[ObjId],
    removed: &HashSet<ObjId>,
    policy: SeparationPolicy,
) -> Result<SeparationPlan, SeparationSplitRefused> {
    let mut plan = SeparationPlan::default();

    for page_id in surviving {
        let Some(page) = graph.resolved(*page_id).as_dict() else {
            continue;
        };
        let Some(separation) = separation_of(graph, page) else {
            continue;
        };
        if separation.is_malformed() {
            plan.impact.malformed += 1;
            continue;
        }

        // Partition this page's declared set. `lost` drives everything:
        // a set that lost nothing is already correct and is left alone.
        let (lost, kept): (Vec<ObjId>, Vec<ObjId>) = separation
            .members
            .iter()
            .partition(|member| removed.contains(member));
        if lost.is_empty() {
            continue;
        }

        if policy == SeparationPolicy::Refuse {
            return Err(SeparationSplitRefused {
                losing: lost.len(),
                total: separation.members.len(),
            });
        }

        // Disclosure is gathered from the *departing* members' own
        // dictionaries, not from this page's, because this page knows
        // only its own colorant. A member that has already been freed
        // cannot be read, so this must happen against the pre-removal
        // graph — which is exactly the contract of this function.
        for member in &lost {
            let colorant = graph
                .resolved(*member)
                .as_dict()
                .and_then(|dict| separation_of(graph, dict))
                .and_then(|dict| dict.colorant);
            plan.impact.note_removed(colorant.as_ref());
        }
        plan.impact.note_kept(separation.colorant.as_ref());

        let mut rewritten = page.clone();
        match policy {
            SeparationPolicy::Discard => {
                rewritten.remove(b"SeparationInfo");
            }
            SeparationPolicy::Repair | SeparationPolicy::Refuse => {
                let Some(info) = page
                    .get(b"SeparationInfo")
                    .map(|value| graph.resolve(value))
                    .and_then(Object::as_dict)
                else {
                    continue;
                };
                let mut info = info.clone();
                info.insert(Name::from(b"Pages"), references(&kept));
                rewritten.insert(Name::from(b"SeparationInfo"), Object::Dict(info));
            }
        }

        plan.impact.sets_split += 1;
        plan.impact.pages_changed += 1;
        plan.impact.policy = policy;
        plan.rewrites.push(SeparationRewrite {
            page: *page_id,
            dict: rewritten,
        });
    }

    Ok(plan)
}

/// Restore `/SeparationInfo` on a page that has just been deep-copied into
/// a new document.
///
/// This is the **assemble** half of the fix, and it exists because the
/// copier's barrier has already been through this dictionary and done the
/// generically-correct thing: refused the `/Pages` array as a whole
/// (because it names pages that were not copied) and therefore dropped the
/// Required `/Pages` entry. See the module docs.
///
/// `original` is the source page's dictionary, read from the source graph;
/// `copied` is the output dictionary the copier produced, mutated in
/// place. `map` answers *"what object number did this source page get in
/// the output, if it was copied at all?"* — returning `None` for a page
/// that stayed behind the barrier.
///
/// Returns the [`SeparationImpact`] contribution for this one page, which
/// the caller accumulates.
///
/// # Why the whole entry is rebuilt rather than patched
///
/// The copier may have dropped `/Pages` (every set is split) or kept it
/// (no member was barriered — the merge case). Rebuilding from `original`
/// makes both paths produce the same bytes for the same input instead of
/// depending on which branch the barrier happened to take, and costs one
/// dictionary clone on the rare pages that have a separation dictionary at
/// all.
pub fn remap_copied<G, F>(
    graph: &G,
    original: &Dict,
    copied: &mut Dict,
    map: F,
    policy: SeparationPolicy,
) -> Result<SeparationImpact, SeparationSplitRefused>
where
    G: ObjectGraph + ?Sized,
    F: Fn(ObjId) -> Option<u32>,
{
    let mut impact = SeparationImpact::default();

    let Some(separation) = separation_of(graph, original) else {
        return Ok(impact);
    };
    if separation.is_malformed() {
        impact.malformed += 1;
        return Ok(impact);
    }

    let mut kept: Vec<ObjId> = Vec::with_capacity(separation.members.len());
    let mut lost: Vec<ObjId> = Vec::new();
    for member in &separation.members {
        match map(*member) {
            Some(number) => kept.push(ObjId::new(number, 0)),
            None => lost.push(*member),
        }
    }

    if lost.is_empty() {
        // The whole set came across. The copier's own mapping is already
        // correct and complete, so this page is left exactly as copied —
        // no rebuild, no diff, nothing to disclose.
        return Ok(impact);
    }

    if policy == SeparationPolicy::Refuse {
        return Err(SeparationSplitRefused {
            losing: lost.len(),
            total: separation.members.len(),
        });
    }

    for member in &lost {
        let colorant = graph
            .resolved(*member)
            .as_dict()
            .and_then(|dict| separation_of(graph, dict))
            .and_then(|dict| dict.colorant);
        impact.note_removed(colorant.as_ref());
    }
    impact.note_kept(separation.colorant.as_ref());

    match policy {
        SeparationPolicy::Discard => {
            copied.remove(b"SeparationInfo");
        }
        SeparationPolicy::Repair | SeparationPolicy::Refuse => {
            let source = original
                .get(b"SeparationInfo")
                .map(|value| graph.resolve(value))
                .and_then(Object::as_dict);
            let Some(source) = source else {
                return Ok(impact);
            };
            // Rebuild from the source dictionary, carrying every entry the
            // copier already validated except `/Pages`, which is replaced
            // with output-space references. `/ColorSpace` is deliberately
            // taken from the COPIED dictionary when present: it may hold
            // references the copier remapped, and the source's version
            // points into the source document.
            let mut info = Dict::new();
            for (key, value) in source.iter() {
                if key.as_bytes() == b"Pages" {
                    continue;
                }
                if let Some(already) = copied
                    .get(b"SeparationInfo")
                    .and_then(Object::as_dict)
                    .and_then(|copied_info| copied_info.get(key.as_bytes()))
                {
                    info.insert(key.clone(), already.clone());
                } else if !matches!(value, Object::Reference(_)) {
                    // A direct value needs no remapping; an unmapped
                    // reference is dropped rather than carried into a
                    // document where it points at nothing.
                    info.insert(key.clone(), value.clone());
                }
            }
            info.insert(Name::from(b"Pages"), references(&kept));
            copied.insert(Name::from(b"SeparationInfo"), Object::Dict(info));
        }
    }

    impact.sets_split += 1;
    impact.pages_changed += 1;
    impact.policy = policy;
    Ok(impact)
}

/// Merge a per-page [`SeparationImpact`] into a running total.
///
/// Colorant lists are unioned rather than concatenated, so a four-plate
/// job split into four files reports `Cyan, Magenta, Yellow, Black` once
/// each and not sixteen times.
pub fn accumulate(total: &mut SeparationImpact, part: &SeparationImpact) {
    if !part.is_empty() {
        // One policy governs a whole operation, so the last part that did
        // something speaks for all of them.
        total.policy = part.policy;
    }
    total.sets_split += part.sets_split;
    total.pages_changed += part.pages_changed;
    total.malformed += part.malformed;
    for colorant in &part.colorants_removed {
        SeparationImpact::push_unique(&mut total.colorants_removed, Some(colorant));
    }
    for colorant in &part.colorants_kept {
        SeparationImpact::push_unique(&mut total.colorants_kept, Some(colorant));
    }
}

/// Build a `/Pages` array from object ids.
fn references(ids: &[ObjId]) -> Object {
    Object::Array(ids.iter().copied().map(Object::Reference).collect())
}
