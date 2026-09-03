//! # Form-XObject discovery for the text-edit surgery (`Pass 119.0`)
//!
//! The surgery in [`crate::text_edit::edit`] rewrites **one content stream
//! buffer**. Until this module existed, that buffer was always the page's own
//! concatenated `/Contents`, and every glyph drawn from inside a form XObject
//! (§8.10.1) was out of reach — extraction could see it, editing could not.
//! `TextRun::editability` (`Pass 118.0`) published that boundary; this module
//! is the machinery that moves it.
//!
//! ★ **Why this is not a niche case.** Measured on the operator's own
//! benchmark CAD drawing: the page content stream holds 3,007 single-character
//! `Tj` operators spelling the producer's watermark, and **one form XObject
//! holds 1,696 show operators** carrying every label, the title block and every
//! dimension callout (a *pdf dimension* in this project's terminology, rule 15
//! — none of this concerns pdfcer-authored *ce dimensions*). ISO 32000-1
//! §8.10.1 picked exactly this case as its own illustration of the feature:
//! *"a standard component in the output from a computer-aided design system"*.
//!
//! ## What this module answers, and why each question is load-bearing
//!
//! 1. **Which form XObjects does a page reach?** [`scan_page_forms`] — in `Do`
//!    order, transitively through nesting, depth-guarded and cycle-guarded on
//!    the object number (`ARCHITECTURE.md` §10; the same guard extraction
//!    uses, keyed the same way, because one stream can be reached under two
//!    resource names).
//!
//! 2. **★ How many places does editing one of them change?**
//!    [`invocation_set`] — and this is the question that decides the whole
//!    design. A form XObject may legally be painted *"multiple times — either
//!    on several pages or at several locations on the same page"* (§8.10.1,
//!    word-identical in both editions), and **there is no ownership rule
//!    anywhere in either edition** binding a form to a page or to one
//!    invocation. That is a confirmed, permanent negative result in pdfcer's
//!    spec corpus (`iso32000__ref__form_xobject_text_edit.md`, `FX-N1`), argued
//!    from three independent directions: the `/Name` single-identity vestige
//!    was *deprecated*; the standard writes `shall not … more than one`
//!    exclusivity rules four times for other constructs and never here; and
//!    Annex F defines a normative object class for *"shared objects …
//!    referenced from more than one page"*.
//!
//!    **So editing a form's content stream changes every place it appears, and
//!    the standard gives pdfcer no basis to pretend otherwise.** An editor that
//!    does this silently is exactly the failure rule 4 forbids — the operator
//!    would change six sheets while looking at one. Every caller therefore
//!    gets the fan-out count and the page list, whatever it then chooses to do
//!    with them.
//!
//! 3. **Where does a `Tf` name inside a form resolve?** [`FormRef::resources`]
//!    — see [`ResourceTier`] for the three-tier chain and why tier 2 is the
//!    spec's answer even though tier 3 is what most implementations do.
//!
//! ## What this module deliberately does NOT do
//!
//! No mutation, no policy, no refusal. It reports structure; the surgery and
//! its refusal table ([`crate::text_edit::edit`]) decide what to do with it.
//! That split is what lets a shell ask *"what would this edit touch?"* without
//! being anywhere near a write path.
//!
//! ## Cost, stated rather than hidden
//!
//! [`invocation_set`] walks **every page in the document**, because a form
//! reachable from page 3 may also be reachable from page 40 and nothing
//! cheaper can prove otherwise. On a 500-page document that is 500 content
//! streams decoded. It is called once per edit, not per keystroke, and the
//! alternative — assuming a form belongs to the page it was found on — is the
//! silent six-sheet edit above.

use std::collections::{BTreeSet, HashSet};

use crate::content::ContentStream;
use crate::document::Document;
use crate::object::{Dict, ObjId, Object};
use crate::page_tree::{self, Page};
use crate::view::DocumentView;

/// The resource-dictionary tiers a name inside a form XObject may resolve
/// through, in the order [`scan_page_forms`] tries them.
///
/// ## Why there is a chain at all — §7.8.3 bullet 4
///
/// > "PDF files written obeying earlier versions of PDF may have omitted the
/// > `Resources` entry in all form XObjects and Type 3 fonts used on a page.
/// > **All resources that are referenced from those forms and fonts shall be
/// > inherited from the resource dictionary of the page on which they are
/// > used.**"
///
/// That is a **`shall` on the reader**, not a tolerance — and ISO 32000-2
/// *deleted* 1.7's accompanying sentence calling the construct obsolete, while
/// keeping the `shall` verbatim. In PDF 2.0 the fallback is not deprecated at
/// all; 2.0's own §8.10.2 example form XObject omits `/Resources` where 1.7's
/// example carried one.
///
/// ## ★ The part that surprises implementers
///
/// The clause says *"the page on which they are used"* — **the PAGE's resolved
/// resources, not the enclosing form's.** For a nested `A → B` where only `A`
/// carries `/Resources`, the spec's answer for `B` is the **page's**. The
/// commonly-implemented "walk up the invocation chain first" is *not* what the
/// clause says. Registered as `FX-A1` in the spec corpus's ambiguity register.
///
/// pdfcer therefore orders the chain **own → page → enclosing form**, so the
/// spec's answer wins whenever it resolves, and the enclosing-form tier is a
/// disclosed *tolerance beyond the spec* reached only when the spec's answer
/// finds nothing. Which tier actually resolved is recorded and disclosed,
/// because a name that resolved through tier 3 resolved through a rule the
/// standard does not contain.
///
/// ## ★ THE FALLBACK IS WHOLE-DICTIONARY, and that was decided the hard way
///
/// The clause is worded for a form that omitted `/Resources` **entirely**, and
/// this implements exactly that: **a form with its own non-empty `/Resources`
/// uses it and nothing else.** (An empty `<< >>` counts as absent — otherwise
/// every name inside such a form would fail on a file every viewer draws. That
/// is the only liberty taken with the wording.)
///
/// **A per-category merge was built first, and reverted before it shipped.**
/// It let a form carrying `/XObject` but no `/Font` pick the page's fonts up,
/// which looks like pure tolerance and is not, for two reasons found by
/// running it:
///
/// 1. **`pdfcer-render`'s interpreter does not do it.** Its `Do` handler takes
///    the form's own `/Resources` when present and the caller's only when
///    absent. An edit path that resolved a font the renderer does not would
///    compute its advance from `/Widths` nothing else consults, and the edited
///    text would land visibly wrong while every internal check reported
///    success. **`text_extract` agrees with the renderer too** — it reports no
///    run at all for that shape, which a test pins.
/// 2. **It was noisy in the ordinary case.** A page carrying an `/XObject`
///    dictionary and a form that does not is *every* form on *every* file, so
///    the merge reported inheritance almost always, and a disclosure that
///    always fires is one nobody reads.
///
/// So a partially-declared form is **refused by name** rather than guessed at.
/// Three components agreeing on a refusal is worth more than one of them
/// accepting a file the other two cannot draw. Whether that shape occurs in
/// real producer output is an empirical question for `C:\personal_rag\pdf\`,
/// and the answer would change this decision — but it must be **measured**
/// first, on real files, not assumed from the fact that a merge is easy to
/// write.
///
/// ## One divergence from `pdfcer-render`, stated rather than hidden
///
/// At depth 0 — a form invoked straight from the page — "the page's" and "the
/// caller's" are the same dictionary, so pdfcer's three components agree
/// exactly, and that covers essentially every real file. For a **nested**
/// resource-less form they differ: this module follows the clause (the
/// **page's**), the renderer follows the common implementation (the
/// **caller's**). Which tier resolved is recorded in
/// [`FormRef::resource_tier`] so the difference is observable rather than
/// latent, and aligning the renderer is filed work, not a silent to-do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum ResourceTier {
    /// The form's own `/Resources` supplied the category.
    Own,
    /// The **page's** resolved `/Resources` supplied it — §7.8.3 bullet 4,
    /// the spec's answer.
    Page,
    /// The **enclosing form's** effective resources supplied it. A tolerance
    /// beyond the spec (see the type's documentation), disclosed as one.
    EnclosingForm,
}

impl ResourceTier {
    /// A short operator-facing phrase naming the tier, for a disclosure line.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Own => "the form's own /Resources",
            Self::Page => "the page's /Resources (ISO 32000-1 7.8.3 bullet 4)",
            Self::EnclosingForm => "the enclosing form's /Resources (beyond the spec's rule)",
        }
    }
}

/// One form XObject reachable from a page, with everything the surgery needs
/// to plan an edit inside it without re-walking the page.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct FormRef {
    /// The form stream's object identity — the object an edit rewrites.
    pub id: ObjId,
    /// The resource name it was invoked under at its own level (`/Fm0`).
    /// Diagnostics only: a name is not an identity (§8.10.1's cycle guard is
    /// keyed on the object number for exactly this reason).
    pub name: Vec<u8>,
    /// Nesting depth: `0` for a form invoked directly by the page's own
    /// `/Contents`, `1` for one invoked by such a form, and so on.
    pub depth: usize,
    /// The enclosing form, or `None` when the `Do` was in the page's own
    /// content stream. This is what decides whether a copy-on-write is
    /// expressible: rebinding a name in the **page's** `/XObject` subdictionary
    /// is a bounded edit, rebinding one inside another form is not.
    pub parent: Option<ObjId>,
    /// The **effective** resource dictionary for names inside this form: its
    /// own `/Resources` if it has one, else the tier that supplied them. See
    /// [`ResourceTier`].
    pub resources: Dict,
    /// Which tier supplied [`Self::resources`].
    pub resource_tier: ResourceTier,
    /// The form's dictionary, verbatim, so the surgery can inspect `/BBox`,
    /// `/Ref`, `/OPI`, `/OC`, `/Group`, `/StructParent` without re-resolving.
    pub dict: Dict,
}

impl FormRef {
    /// Whether this form's resources came from anywhere but its own
    /// dictionary — i.e. whether it depends on §7.8.3 bullet 4 to render.
    #[must_use]
    pub const fn inherits_resources(&self) -> bool {
        !matches!(self.resource_tier, ResourceTier::Own)
    }

    /// Whether the form's **own** `/Resources` supplies this `/Font` name.
    ///
    /// # Why the edit's disclosure asks this rather than [`Self::inherits_resources`]
    ///
    /// The two answer different questions and only this one is about the edit.
    /// `inherits_resources` is a property of the *form*; this is a property of
    /// **the name the re-encoding actually resolved**. They diverge whenever a
    /// form carries a `/Resources` that happens not to declare the font its own
    /// `Tf` selects — the shape that is refused rather than guessed at (see
    /// [`ResourceTier`]) — and a disclosure worded from the wrong one of them
    /// would be reporting a fact about the dictionary while claiming to report
    /// a fact about the edit.
    #[must_use]
    pub fn owns_font(&self, doc: &Document, name: &[u8]) -> bool {
        doc.resolve(self.dict.get(b"Resources").unwrap_or(&Object::Null))
            .as_dict()
            .and_then(|r| r.get(b"Font"))
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .is_some_and(|fonts| fonts.contains_key(name))
    }
}

/// Everything one page's form scan found.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct FormScan {
    /// The forms reached, in `Do` order, depth-first (a nested form appears
    /// immediately after the form that invokes it).
    pub forms: Vec<FormRef>,
    /// How many `Do` invocations were skipped because the nesting depth guard
    /// fired. Non-zero means content the page draws was **not** scanned — a
    /// pdfcer limit, not a file defect: neither edition states any nesting
    /// limit for form XObjects (`FX-N9`), and 2.0's Annex J.4.2 explicitly
    /// hands recursion protection to the processor.
    pub depth_overflows: u64,
    /// How many `Do` invocations named a form already on the active stack —
    /// a cycle, skipped. A cycle is not conforming content but is seen, and
    /// counting it is what keeps a malformed file from costing a caller the
    /// whole page (§10 fail-clean).
    pub cycles_skipped: u64,
    /// How many `Do` invocations named an XObject that could not be resolved,
    /// decoded or parsed. Counted rather than swallowed, for the same reason
    /// `Page::dangling_contents` is counted.
    pub unresolved: u64,
}

impl FormScan {
    /// The form with this object number, if the scan reached one.
    #[must_use]
    pub fn find(&self, object: u32) -> Option<&FormRef> {
        self.forms.iter().find(|f| f.id.num == object)
    }
}

/// The maximum form-XObject nesting depth this module walks.
///
/// **64, and it is a pdfcer invariant rather than a conformance rule** — the
/// distinction matters to what a refusal is allowed to say. `FX-N9`: neither
/// ISO 32000-1 nor ISO 32000-2 states any nesting limit or cycle prohibition
/// for form XObjects, and the silence is deliberate (2.0 *does* state a
/// 255-deep limit for Type 4 function `{ }` nesting, and Annex J.4.2 hands
/// recursion protection to "the PDF processor"). So a 65-deep document is
/// **conforming** and pdfcer still refuses it; that must be disclosed as
/// pdfcer's limit, never as the file's defect.
///
/// The number matches [`crate::text_extract::ExtractOptions::max_form_depth`]'s
/// default on purpose: if editing walked deeper than extraction, a caret could
/// land on a glyph the edit path could reach and the extract path could not,
/// and the two halves would disagree about what exists.
pub const MAX_FORM_DEPTH: usize = 64;

/// Scan one page for every form XObject it reaches, transitively.
///
/// The walk follows §8.10.1's procedure only in the parts discovery needs:
/// resolve the name in the current resource dictionary, confirm
/// `/Subtype /Form`, recurse with the form's effective resources. Graphics
/// state, `/Matrix` and `/BBox` are irrelevant to *which* streams exist and
/// are deliberately not tracked here — the surgery reads `/BBox` from
/// [`FormRef::dict`] when it needs it.
///
/// `view` is what makes this work inside an [`EditSession`](crate::edit::EditSession):
/// a form whose content this session already rewrote has its payload in the
/// staging half of a split source, and only the view knows that. Pass
/// `doc.view()` for the base document.
///
/// Never fails: an unresolvable, undecodable or unparsable form is counted in
/// [`FormScan::unresolved`] and skipped, because a page that draws one broken
/// form still has every other form editable and refusing the page would cost
/// the operator content that is fine (§10 fail-clean).
#[must_use]
pub fn scan_page_forms(doc: &Document, view: &DocumentView<'_>, page: &Page) -> FormScan {
    let mut scan = FormScan::default();
    let Ok(stream) = ContentStream::from_page(view, page) else {
        scan.unresolved += 1;
        return scan;
    };
    let mut active: Vec<u32> = Vec::new();
    walk_forms(
        doc,
        view,
        &stream,
        &page.resources,
        &page.resources,
        None,
        0,
        &mut active,
        &mut scan,
    );
    scan
}

/// The recursive half of [`scan_page_forms`].
///
/// `page_resources` never changes down the recursion (it is tier 2 for every
/// level, per §7.8.3 bullet 4's *"the page on which they are used"*);
/// `enclosing` is the current level's effective dictionary and becomes tier 3
/// for the level below.
#[allow(clippy::too_many_arguments)]
fn walk_forms(
    doc: &Document,
    view: &DocumentView<'_>,
    stream: &ContentStream,
    page_resources: &Dict,
    enclosing: &Dict,
    parent: Option<ObjId>,
    depth: usize,
    active: &mut Vec<u32>,
    scan: &mut FormScan,
) {
    for op in stream.operations() {
        if op.operator_name(&stream.buf) != Some(b"Do") {
            continue;
        }
        let Some(name) = op
            .operands
            .first()
            .and_then(|t| match &t.kind {
                crate::content::ContentTokenKind::Operand(o) => Some(o),
                _ => None,
            })
            .and_then(Object::as_name)
            .map(|n| n.as_bytes().to_vec())
        else {
            continue;
        };
        let Some(entry) = doc
            .resolve(enclosing.get(b"XObject").unwrap_or(&Object::Null))
            .as_dict()
            .and_then(|d| d.get(&name))
        else {
            scan.unresolved += 1;
            continue;
        };
        // A form XObject is a stream and streams are always indirect
        // (§7.3.8.1), so a direct value here is not an editable target: there
        // is no object number to rewrite.
        let Some(id) = entry.as_reference() else {
            scan.unresolved += 1;
            continue;
        };
        let Some(Object::Stream(form)) = view.graph().value(id) else {
            scan.unresolved += 1;
            continue;
        };
        if doc
            .resolve(form.dict.get(b"Subtype").unwrap_or(&Object::Null))
            .as_name()
            .is_none_or(|n| n.as_bytes() != b"Form")
        {
            continue; // an image XObject: no text, not an error
        }
        if depth >= MAX_FORM_DEPTH {
            scan.depth_overflows += 1;
            continue;
        }
        if active.contains(&id.num) {
            scan.cycles_skipped += 1;
            continue;
        }

        let (resources, resource_tier) =
            effective_resources(doc, &form.dict, page_resources, enclosing);
        scan.forms.push(FormRef {
            id,
            name,
            depth,
            parent,
            resources: resources.clone(),
            resource_tier,
            dict: form.dict.clone(),
        });

        // Recurse. An undecodable or unparsable form still counts as reached
        // (it is in `forms` above and may itself be editable if the failure
        // was ours); only its *children* are lost, which is what `unresolved`
        // records.
        let inner = view
            .slice(form.data_span)
            .and_then(|raw| crate::filters::decode_stream(&form.dict, raw).ok())
            .and_then(|decoded| ContentStream::parse(decoded).ok());
        let Some(inner) = inner else {
            scan.unresolved += 1;
            continue;
        };
        active.push(id.num);
        walk_forms(
            doc,
            view,
            &inner,
            page_resources,
            &resources,
            Some(id),
            depth + 1,
            active,
            scan,
        );
        active.pop();
    }
}

/// Build a form's **effective** resource dictionary by merging the three tiers
/// per category, and record which tier supplied each.
///
/// See [`ResourceTier`] for why the order is own → page → enclosing and why
/// the merge is per-category rather than all-or-nothing.
fn effective_resources(
    doc: &Document,
    form_dict: &Dict,
    page_resources: &Dict,
    enclosing: &Dict,
) -> (Dict, ResourceTier) {
    let own = doc
        .resolve(form_dict.get(b"Resources").unwrap_or(&Object::Null))
        .as_dict();
    // A `/Resources` that is present but EMPTY resolves nothing, so it counts
    // as absent — otherwise a form carrying `<< >>` would be given an empty
    // dictionary and every name inside it would fail, on a file every viewer
    // draws. That is the only liberty taken with the clause's wording.
    if let Some(own) = own
        && !own.is_empty()
    {
        return (own.clone(), ResourceTier::Own);
    }
    if !page_resources.is_empty() {
        return (page_resources.clone(), ResourceTier::Page);
    }
    if !enclosing.is_empty() {
        return (enclosing.clone(), ResourceTier::EnclosingForm);
    }
    (Dict::new(), ResourceTier::Page)
}

/// One place a form XObject is painted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct InvocationSite {
    /// Zero-based index of the page whose content leads to this `Do`.
    pub page_index: usize,
    /// Nesting depth of the `Do`: `0` = the page's own content stream.
    pub depth: usize,
    /// The enclosing form, or `None` when the `Do` is in the page's content.
    pub parent: Option<ObjId>,
}

/// Every place in the document a given form XObject is painted.
///
/// ★ **This is the disclosure that keeps a form edit honest.** See the module
/// documentation: multi-invocation is explicitly sanctioned and there is no
/// ownership rule, so `sites.len() > 1` means an in-place edit changes content
/// the operator is not looking at.
#[derive(Debug, Clone, Default, PartialEq)]
#[non_exhaustive]
pub struct InvocationSet {
    /// The form's object number.
    pub object: u32,
    /// Every `Do` reaching it, in page order then discovery order.
    pub sites: Vec<InvocationSite>,
    /// The distinct page indices it appears on, ascending.
    pub pages: BTreeSet<usize>,
    /// Pages whose scan hit the depth guard or a broken form, so the count
    /// below is a **lower bound** rather than a total. Disclosed as such:
    /// "at least N" is honest, "N" would not be.
    pub incomplete_pages: BTreeSet<usize>,
}

impl InvocationSet {
    /// How many times the form is painted document-wide.
    #[must_use]
    pub fn count(&self) -> usize {
        self.sites.len()
    }

    /// Whether more than one `Do` reaches this form — the condition under
    /// which an in-place edit is visible somewhere the operator is not
    /// looking.
    #[must_use]
    pub fn is_shared(&self) -> bool {
        self.sites.len() > 1
    }

    /// Whether the count is a lower bound rather than a total.
    #[must_use]
    pub fn is_lower_bound(&self) -> bool {
        !self.incomplete_pages.is_empty()
    }

    /// A one-line, operator-facing description of the fan-out, for a
    /// disclosure. Wording is deliberately about *what will change*, not about
    /// PDF structure: "appears in 6 places on 6 pages" is actionable, "form
    /// XObject 23 has 6 invocation sites" is not.
    #[must_use]
    pub fn describe(&self) -> String {
        let at_least = if self.is_lower_bound() {
            "at least "
        } else {
            ""
        };
        let places = self.count();
        let pages = self.pages.len();
        let mut list: Vec<String> = self
            .pages
            .iter()
            .take(8)
            .map(|p| (p + 1).to_string())
            .collect();
        if self.pages.len() > 8 {
            list.push("...".to_owned());
        }
        format!(
            "this text is drawn from shared content that appears in {at_least}{places} place(s) \
             on {at_least}{pages} page(s) (page {})",
            list.join(", ")
        )
    }
}

/// Compute the document-wide invocation set of one form XObject.
///
/// Walks **every page**, because nothing cheaper can prove a form is not also
/// reached from a page the caller did not ask about. See the module docs on
/// cost. `view` is the session-aware source, as for [`scan_page_forms`].
///
/// A page whose scan was incomplete (depth guard, broken form) is recorded in
/// [`InvocationSet::incomplete_pages`] so the count reports as a lower bound
/// rather than as a total — an under-count presented as a total is the same
/// class of defect as a silent edit.
#[must_use]
pub fn invocation_set(doc: &Document, view: &DocumentView<'_>, object: u32) -> InvocationSet {
    invocation_map(doc, view)
        .remove(&object)
        .unwrap_or(InvocationSet {
            object,
            ..InvocationSet::default()
        })
}

/// Every form XObject in the document, keyed by object number, with all of its
/// invocation sites — **one document walk, not one per form.**
///
/// [`invocation_set`] answers the same question for a single object and is
/// implemented on top of this. The map exists because the text-edit planner
/// may consider several forms on a page before one of them yields a match, and
/// asking the per-object function each time would walk every page in the
/// document once per candidate. One walk answers for all of them.
///
/// A page whose scan was incomplete is recorded in **every** set's
/// [`InvocationSet::incomplete_pages`], not just the sets of forms it did
/// find: the whole point of that field is that a form pdfcer failed to see is
/// exactly the one whose absence from a count would be wrong.
#[must_use]
pub fn invocation_map(
    doc: &Document,
    view: &DocumentView<'_>,
) -> std::collections::BTreeMap<u32, InvocationSet> {
    let mut map: std::collections::BTreeMap<u32, InvocationSet> = std::collections::BTreeMap::new();
    let Ok(pages) = page_tree::pages(doc) else {
        return map;
    };
    let mut incomplete: BTreeSet<usize> = BTreeSet::new();
    for (index, page) in pages.iter().enumerate() {
        let scan = scan_page_forms(doc, view, page);
        if scan.depth_overflows > 0 || scan.unresolved > 0 {
            incomplete.insert(index);
        }
        for form in &scan.forms {
            let set = map.entry(form.id.num).or_insert_with(|| InvocationSet {
                object: form.id.num,
                ..InvocationSet::default()
            });
            set.sites.push(InvocationSite {
                page_index: index,
                depth: form.depth,
                parent: form.parent,
            });
            set.pages.insert(index);
        }
    }
    for set in map.values_mut() {
        set.incomplete_pages.clone_from(&incomplete);
    }
    map
}

/// The object numbers of every form reachable from `page`, deduplicated —
/// a cheap pre-filter for a caller that only wants to know *whether* a page
/// has editable form content.
#[must_use]
pub fn form_objects_on_page(doc: &Document, view: &DocumentView<'_>, page: &Page) -> Vec<u32> {
    let scan = scan_page_forms(doc, view, page);
    let mut seen = HashSet::new();
    scan.forms
        .iter()
        .map(|f| f.id.num)
        .filter(|n| seen.insert(*n))
        .collect()
}
