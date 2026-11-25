//! The Ink Manager's read side: every colorant a `Separation` or `DeviceN`
//! space names (ISO 32000-2 §8.6.6.4, §8.6.6.5), where it is drawn, and the
//! report types the two ink edits share.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use crate::graph::ObjectGraph;
use crate::object::{Dict, ObjId, Object};
use crate::preflight::walk::{self, Event, MAX_COLORANTS, name_text};
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

/// Every ink a document names, from [`inks`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InkList {
    /// The inks, process first in `Cyan`, `Magenta`, `Yellow`, `Black`
    /// order, then spots by name.
    pub inks: Vec<InkUse>,
    /// False when inks may be missing: a `DeviceN` space names more than 32
    /// colorants, the most read from one space (ISO 32000-1 Annex C,
    /// Table C.1), or a space sits deeper inside one object than the walk
    /// follows. A shell says so beside the list.
    pub complete: bool,
}

/// One ink a document names, from [`inks`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct InkUse {
    /// The colorant name as text that keeps distinct names distinct: UTF-8
    /// as is, `#` and any byte that is not UTF-8 as `#XX` (the escape of
    /// ISO 32000-1 §7.3.5), so `Spot#80` is byte 0x80 and `Spot#2380` the
    /// literal `Spot#80`. [`crate::edit::EditSession::alias_ink`] and
    /// [`InkSelection`] take this spelling; [`ink_name_bytes`] reverses it.
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
    /// These spot inks, spelled as [`InkUse::name`] spells them and
    /// compared exactly; a spelling [`ink_name_bytes`] cannot read selects
    /// nothing.
    Only(Vec<String>),
}

impl InkSelection {
    /// Whether the selection takes the spot `name`.
    #[must_use]
    pub fn selects(&self, name: &[u8]) -> bool {
        match self {
            Self::All => true,
            Self::Only(names) => names
                .iter()
                .any(|n| ink_name_bytes(n).is_some_and(|b| b == name)),
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
    /// The colorant names, in component order; at most [`MAX_COLORANTS`].
    pub(crate) names: Vec<Vec<u8>>,
    /// A `DeviceN` names array longer than [`MAX_COLORANTS`], not all read.
    pub(crate) capped: bool,
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
    let (names, capped) = if separation {
        let name = view.resolve(items.get(1)?).as_name()?;
        (vec![name.as_bytes().to_vec()], false)
    } else {
        let all = view.resolve(items.get(1)?).as_array()?;
        let names = all
            .iter()
            .take(MAX_COLORANTS)
            .filter_map(|o| view.resolve(o).as_name())
            .map(|n| n.as_bytes().to_vec())
            .collect();
        (names, all.len() > MAX_COLORANTS)
    };
    Some(InkSpace {
        separation,
        names,
        capped,
        alternate: items.get(2)?,
        tint: items.get(3)?,
    })
}

/// The family name of a colour space: its name, or its array's head;
/// `None` when neither can be read.
pub(crate) fn family_name(view: &DocumentView<'_>, cs: &Object) -> Option<String> {
    let named = match view.resolve(cs) {
        Object::Name(n) => Some(n),
        Object::Array(a) => a.first().and_then(|o| view.resolve(o).as_name()),
        _ => None,
    }?;
    Some(text(named.as_bytes()))
}

/// A name's bytes as text, spelled as [`InkUse::name`] is.
pub(crate) fn text(bytes: &[u8]) -> String {
    name_text(bytes)
}

/// The bytes of ink name `name`, spelled as [`InkUse::name`] is: `#` and
/// two hex digits is one byte (ISO 32000-1 §7.3.5), anything else is
/// itself. `None` when a `#` is not followed by two hex digits.
///
/// # Examples
///
/// ```
/// use pdfcer_core::prepress::ink_name_bytes;
///
/// assert_eq!(ink_name_bytes("Spot#80"), Some(b"Spot\x80".to_vec()));
/// assert_eq!(ink_name_bytes("No#232"), Some(b"No#2".to_vec()));
/// assert_eq!(ink_name_bytes("Gold#2"), None);
/// ```
#[must_use]
pub fn ink_name_bytes(name: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(name.len());
    let mut rest = name.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        if b == b'#' {
            let hex = tail
                .get(..2)
                .filter(|h| h.iter().all(u8::is_ascii_hexdigit))?;
            let hex = std::str::from_utf8(hex).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            rest = tail.get(2..).unwrap_or_default();
        } else {
            out.push(b);
            rest = tail;
        }
    }
    Some(out)
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
/// following references. False when nesting deeper than [`MAX_NESTING`]
/// was left unvisited.
pub(crate) fn each_array(obj: &Object, depth: usize, f: &mut dyn FnMut(&[Object])) -> bool {
    let is_container = matches!(obj, Object::Array(_) | Object::Dict(_) | Object::Stream(_));
    if !is_container {
        return true;
    }
    if depth > MAX_NESTING {
        return false;
    }
    let deeper = depth + 1;
    match obj {
        Object::Array(items) => {
            f(items);
            items
                .iter()
                .fold(true, |ok, i| each_array(i, deeper, f) & ok)
        }
        Object::Dict(d) => d
            .iter()
            .fold(true, |ok, (_, v)| each_array(v, deeper, f) & ok),
        Object::Stream(s) => s
            .dict
            .iter()
            .fold(true, |ok, (_, v)| each_array(v, deeper, f) & ok),
        _ => true,
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
/// At most 32 colorants are read from one `DeviceN` space (ISO 32000-1
/// Annex C); past that, or past the walk's nesting limit,
/// [`InkList::complete`] is false and the list may be missing inks.
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
/// let list = inks(&doc.view());
/// assert!(list.inks.is_empty() && list.complete);
/// # Ok(())
/// # }
/// ```
#[must_use]
pub fn inks(view: &DocumentView<'_>) -> InkList {
    let mut found = Found::new();
    let complete = defined(view, &mut found);
    drawn(view, &mut found);
    let mut inks: Vec<InkUse> = found
        .into_iter()
        .map(|(name, (alternates, pages, uses))| InkUse {
            kind: kind_of(&name),
            name: text(&name),
            alternates: alternates.into_iter().collect(),
            pages: pages.into_iter().collect(),
            uses,
        })
        .collect();
    inks.sort_by_key(|u| (u.kind, process_rank(&u.name), u.name.clone()));
    InkList { inks, complete }
}

/// Inks by their bytes: alternate families, 1-based pages, uses.
type Found = BTreeMap<Vec<u8>, (BTreeSet<String>, BTreeSet<usize>, usize)>;

/// Every ink a space defines, with its alternate's family when that can be
/// read. False when a space may have been missed or not read in full.
fn defined(view: &DocumentView<'_>, found: &mut Found) -> bool {
    let mut complete = true;
    for id in live_ids(view) {
        let Some(value) = view.graph().value(id) else {
            continue;
        };
        let mut capped = false;
        complete &= each_array(value, 0, &mut |items| {
            let Some(space) = ink_space(view, items) else {
                return;
            };
            capped |= space.capped;
            let alternate = family_name(view, space.alternate);
            for name in space.names.iter().filter(|n| is_ink(n)) {
                let entry = found.entry(name.clone()).or_default();
                entry.0.extend(alternate.clone());
            }
        });
        complete &= !capped;
    }
    complete
}

/// Where pages draw with each ink.
fn drawn(view: &DocumentView<'_>, found: &mut Found) {
    let Ok(pages) = crate::page_tree::pages_in(view.graph()) else {
        return;
    };
    for (i, page) in pages.iter().enumerate() {
        for (_, event) in walk::page_events(view, page) {
            let Event::Colour(u) = event else { continue };
            for name in &u.colorants {
                let bytes = ink_name_bytes(name).unwrap_or_else(|| name.as_bytes().to_vec());
                let entry = found.entry(bytes).or_default();
                entry.1.insert(i + 1);
                entry.2 = entry.2.saturating_add(1);
            }
        }
    }
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
