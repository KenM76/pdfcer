//! The Ink Manager's read side: every colorant a `Separation` or `DeviceN`
//! space names (ISO 32000-2 §8.6.6.4, §8.6.6.5), where it is drawn, and the
//! report types the two ink edits share.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::preflight::walk::{self, Event};
use crate::view::DocumentView;

/// Nesting followed inside one object when looking for colour spaces.
const MAX_NESTING: usize = 24;

/// Whether an ink prints on a process plate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InkKind {
    /// `Cyan`, `Magenta`, `Yellow` or `Black` (§8.6.6.5).
    Process,
    /// Any other name.
    Spot,
}

/// One ink a document names, from [`inks`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InkUse {
    /// The colorant name, as written.
    pub name: String,
    /// Process or spot.
    pub kind: InkKind,
    /// The alternate space of every space naming the ink, by family
    /// (`DeviceCMYK`, `ICCBased`, `Lab`, ...), sorted. More than one means
    /// the definitions disagree on how the ink looks without its plate.
    pub alternates: Vec<String>,
    /// 1-based pages that draw with it, ascending. Empty when the ink is
    /// defined but nothing drawn uses it.
    pub pages: Vec<usize>,
    /// Distinct uses drawn: each colour space, image or shading naming the
    /// ink counts once per page and once per annotation appearance.
    pub uses: usize,
}

/// Which inks [`crate::edit::EditSession::spot_to_process`] converts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InkSelection {
    /// Every spot ink.
    All,
    /// These spot inks, compared exactly.
    Only(Vec<String>),
}

impl InkSelection {
    /// Whether the selection takes the spot `name`.
    #[must_use]
    pub fn selects(&self, name: &[u8]) -> bool {
        match self {
            Self::All => true,
            Self::Only(names) => names.iter().any(|n| n.as_bytes() == name),
        }
    }
}

/// Something an ink edit left as it was, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InkRefusal {
    /// The object left unchanged, when there is one to point at.
    pub object: Option<ObjId>,
    /// The ink concerned.
    pub ink: String,
    /// One sentence.
    pub reason: String,
}

/// What an ink edit changed, and what it left alone.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct InkEditReport {
    /// Every object written, ascending.
    pub changed: Vec<ObjId>,
    /// Colour spaces renamed (alias) or converted (spot to process).
    pub spaces: usize,
    /// Colour-setting operators rewritten in content streams.
    pub paints: usize,
    /// Images whose samples were converted.
    pub images: usize,
    /// Shadings whose functions were converted.
    pub shadings: usize,
    /// `Indexed` lookup tables converted.
    pub lookups: usize,
    /// What was left as it was.
    pub refused: Vec<InkRefusal>,
    /// Content streams that paint a converted ink under overprint where the
    /// result no longer overprints: overprint mode 0, an image or a shading.
    pub overprint_lost: Vec<ObjId>,
    /// Anything else the operator should know, one sentence each.
    pub disclosures: Vec<String>,
}

impl InkEditReport {
    /// True when nothing was written.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changed.is_empty()
    }

    /// Record a refusal once.
    pub(crate) fn refuse(&mut self, object: Option<ObjId>, ink: &[u8], reason: String) {
        let refusal = InkRefusal {
            object,
            ink: text(ink),
            reason,
        };
        if !self.refused.contains(&refusal) {
            self.refused.push(refusal);
        }
    }

    /// Record a disclosure once.
    pub(crate) fn disclose(&mut self, sentence: String) {
        if !self.disclosures.contains(&sentence) {
            self.disclosures.push(sentence);
        }
    }
}

/// One object an ink edit writes.
#[derive(Debug, Clone)]
pub(crate) enum Planned {
    /// A replacement value; a stream keeps its data.
    Object(Object),
    /// A stream with new data, encoded as `dict`'s `/Filter` says;
    /// `/Length` is set at commit.
    Stream { dict: Dict, data: Vec<u8> },
}

/// An ink edit worked out against a view, before it is committed.
#[derive(Debug, Default)]
pub(crate) struct InkPlan {
    pub(crate) writes: BTreeMap<ObjId, Planned>,
    pub(crate) report: InkEditReport,
}

/// A parsed `Separation` or `DeviceN` array.
pub(crate) struct InkSpace<'a> {
    /// `Separation` rather than `DeviceN`.
    pub(crate) separation: bool,
    /// The colorant names, in component order.
    pub(crate) names: Vec<Vec<u8>>,
    /// The alternate space, unresolved.
    pub(crate) alternate: &'a Object,
    /// The tint transform, unresolved.
    pub(crate) tint: &'a Object,
}

/// Parse `items` as `[/Separation name alt tint]` or
/// `[/DeviceN names alt tint attrs?]`; `None` for any other array.
pub(crate) fn ink_space<'a>(view: &DocumentView<'_>, items: &'a [Object]) -> Option<InkSpace<'a>> {
    let head = items.first().and_then(|o| view.resolve(o).as_name())?;
    let separation = match head.as_bytes() {
        b"Separation" => true,
        b"DeviceN" => false,
        _ => return None,
    };
    let names = if separation {
        vec![view.resolve(items.get(1)?).as_name()?.as_bytes().to_vec()]
    } else {
        view.resolve(items.get(1)?)
            .as_array()?
            .iter()
            .filter_map(|o| view.resolve(o).as_name())
            .map(|n| n.as_bytes().to_vec())
            .collect()
    };
    Some(InkSpace {
        separation,
        names,
        alternate: items.get(2)?,
        tint: items.get(3)?,
    })
}

/// The family name of a colour space: its name, or its array's head.
pub(crate) fn family_name(view: &DocumentView<'_>, cs: &Object) -> String {
    let named = match view.resolve(cs) {
        Object::Name(n) => Some(n),
        Object::Array(a) => a.first().and_then(|o| view.resolve(o).as_name()),
        _ => None,
    };
    named.map_or_else(|| "an unreadable space".to_owned(), |n| text(n.as_bytes()))
}

/// Bytes as text, lossily.
pub(crate) fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Every object reachable from the trailer's `/Root` and `/Info`, ascending.
pub(crate) fn live_ids(view: &DocumentView<'_>) -> Vec<ObjId> {
    let roots: Vec<ObjId> = [b"Root".as_slice(), b"Info"]
        .iter()
        .filter_map(|k| view.trailer_entry(k).and_then(Object::as_reference))
        .collect();
    let mut ids: Vec<ObjId> = crate::edit::reachable(view.graph(), &roots, &HashSet::new())
        .into_iter()
        .collect();
    ids.sort_unstable();
    ids
}

/// Call `f` on every array inside `obj`, itself included, without
/// following references.
pub(crate) fn each_array(obj: &Object, depth: usize, f: &mut dyn FnMut(&[Object])) {
    if depth > MAX_NESTING {
        return;
    }
    match obj {
        Object::Array(items) => {
            f(items);
            for item in items {
                each_array(item, depth + 1, f);
            }
        }
        Object::Dict(d) => d.iter().for_each(|(_, v)| each_array(v, depth + 1, f)),
        Object::Stream(s) => s.dict.iter().for_each(|(_, v)| each_array(v, depth + 1, f)),
        _ => {}
    }
}

/// Every ink named by a `Separation` or `DeviceN` space anywhere in the
/// document, with its alternates and where pages draw with it. `All` and
/// `None` are not inks and are not listed.
///
/// Process inks come first in `Cyan`, `Magenta`, `Yellow`, `Black` order,
/// then spots by name. Usage comes from the same walk preflight uses: page
/// content, forms, patterns, shadings, images and annotations' normal
/// appearances. Type 3 glyph procedures and soft masks are not entered, so
/// an ink drawn only there is listed with no pages.
///
/// # Examples
///
/// ```
/// use pdfcer_core::document::Document;
/// use pdfcer_core::prepress::inks;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let doc = Document::from_bytes(
///     include_bytes!("../../../../fixtures/synthetic/hello.pdf").to_vec(),
/// )?;
/// assert!(inks(&doc.view()).is_empty());
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn inks(view: &DocumentView<'_>) -> Vec<InkUse> {
    let mut found: BTreeMap<Vec<u8>, (BTreeSet<String>, BTreeSet<usize>, usize)> = BTreeMap::new();
    for id in live_ids(view) {
        let Some(value) = view.graph().value(id) else {
            continue;
        };
        each_array(value, 0, &mut |items| {
            if let Some(space) = ink_space(view, items) {
                let alternate = family_name(view, space.alternate);
                for name in space.names.iter().filter(|n| is_ink(n)) {
                    let entry = found.entry(name.clone()).or_default();
                    entry.0.insert(alternate.clone());
                }
            }
        });
    }
    if let Ok(pages) = crate::page_tree::pages_in(view.graph()) {
        for (i, page) in pages.iter().enumerate() {
            for (_, event) in walk::page_events(view, page) {
                let Event::Colour(u) = event else { continue };
                for name in &u.colorants {
                    let entry = found.entry(name.as_bytes().to_vec()).or_default();
                    entry.1.insert(i + 1);
                    entry.2 += 1;
                }
            }
        }
    }
    let mut out: Vec<InkUse> = found
        .into_iter()
        .map(|(name, (alternates, pages, uses))| InkUse {
            kind: kind_of(&name),
            name: text(&name),
            alternates: alternates.into_iter().collect(),
            pages: pages.into_iter().collect(),
            uses,
        })
        .collect();
    out.sort_by_key(|u| (u.kind, process_rank(&u.name), u.name.clone()));
    out
}

/// Not `All` or `None`.
fn is_ink(name: &[u8]) -> bool {
    name != b"All" && name != b"None"
}

/// Process or spot, for a name that is an ink.
fn kind_of(name: &[u8]) -> InkKind {
    if walk::is_spot_colorant(name) {
        InkKind::Spot
    } else {
        InkKind::Process
    }
}

/// CMYK plate order for process names; spots tie.
fn process_rank(name: &str) -> usize {
    ["Cyan", "Magenta", "Yellow", "Black"]
        .iter()
        .position(|p| *p == name)
        .unwrap_or(4)
}
