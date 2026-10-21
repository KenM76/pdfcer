//! Merging image fragments during an optimize run (Pass 616.3): a picture
//! a producer sliced into strips, each its own image XObject placed by
//! `q a 0 0 d e f cm /Name Do Q`, becomes one image.
//!
//! Strips concatenate because samples are stored a row at a time from the
//! top, each row a whole number of bytes (ISO 32000-2 §8.9.3), so strips of
//! one width and sample format stack into one sample array. A run is two
//! or more such placements back to back in a page's content, with nothing
//! between them, and:
//! - the same `Width`, `BitsPerComponent`, `ColorSpace`, `Decode`,
//!   `ImageMask`, `Intent` and `Interpolate` (Table 87);
//! - none of `SMask`, `Mask`, `SMaskInData`, `OC`, `Alternates`, `OPI`,
//!   `StructParent`, `ID` or `Metadata`, which a merged image could not
//!   carry for each strip;
//! - an unrotated, unskewed placement (`b = c = 0`, `a, d > 0`) of one
//!   width and left edge and one row height, each strip touching the last
//!   above or below it;
//! - filters pdfcer decodes itself, so JPEG and other codec strips stay.
//!
//! Only page content is searched; strips inside a form XObject stay. A
//! page is merged only when every drawer of its `/XObject` dictionary is a
//! page whose content pdfcer reads, so the strips' names can leave it: a
//! form or annotation sharing the dictionary, or one inherited from the
//! page tree, could still draw a strip by name.

use std::collections::{BTreeSet, HashMap};

use crate::content::{ContentStream, ContentTokenKind, Operation};
use crate::filters;
use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::span::ByteSpan;
use crate::view::DocumentView;

use super::objects::{DiscardObject, ObjectDiscard};

/// Entries that must agree for strips to share one sample array.
const SAME: &[&[u8]] = &[
    b"Width",
    b"BitsPerComponent",
    b"ColorSpace",
    b"Decode",
    b"ImageMask",
    b"Intent",
    b"Interpolate",
];

/// Entries that bind a strip to something a merged image would lose.
const REFUSED: &[&[u8]] = &[
    b"SMask",
    b"Mask",
    b"SMaskInData",
    b"OC",
    b"Alternates",
    b"OPI",
    b"StructParent",
    b"ID",
    b"Metadata",
];

/// How far a strip may sit from its neighbour, and its row height from the
/// first strip's, as a fraction of that row height: room for producers
/// that round coordinates to two decimals.
const TOLERANCE: f64 = 0.1;

/// One run of strips in a page's content.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Run {
    /// From the first strip's `q` to the last one's `Q`.
    pub(crate) span: ByteSpan,
    /// The strip images, top row first.
    pub(crate) ids: Vec<ObjId>,
    /// The merged image's `Width` and `Height` in samples.
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// The merged placement: `[a 0 0 d e f]` covering every strip.
    pub(crate) matrix: [f64; 6],
}

/// One `q a 0 0 d e f cm /Name Do Q` placement of an eligible image.
struct Strip<'a> {
    span: ByteSpan,
    id: ObjId,
    dict: &'a Dict,
    height: u32,
    a: f64,
    d: f64,
    e: f64,
    f: f64,
}

impl Strip<'_> {
    fn row(&self) -> f64 {
        self.d / f64::from(self.height)
    }

    /// Whether `next` continues the run ending at `self`, which started at
    /// `first`, in direction `down` (or either, for the second strip).
    fn continued_by(&self, first: &Self, next: &Self, down: Option<bool>) -> Option<bool> {
        let row = first.row();
        let near = |x: f64, y: f64, scale: f64| (x - y).abs() <= TOLERANCE * scale;
        let same = SAME.iter().all(|k| first.dict.get(k) == next.dict.get(k));
        if !same
            || !near(next.a, first.a, row)
            || !near(next.e, first.e, row)
            || !near(next.row(), row, row)
        {
            return None;
        }
        let below = near(next.f + next.d, self.f, row);
        let above = near(next.f, self.f + self.d, row);
        match down {
            Some(true) if below => Some(true),
            Some(false) if above => Some(false),
            None if below => Some(true),
            None if above => Some(false),
            _ => None,
        }
    }
}

fn numbers(op: &Operation<'_>) -> Option<Vec<f64>> {
    op.operands
        .iter()
        .map(|t| match &t.kind {
            ContentTokenKind::Operand(o) => o.as_number(),
            _ => None,
        })
        .collect()
}

/// The placement `ops` begins with, when it is a strip of an eligible
/// image.
fn strip<'a>(
    view: &'a DocumentView<'_>,
    buf: &[u8],
    ops: &[Operation<'_>],
    xobjects: &Dict,
) -> Option<Strip<'a>> {
    let [q, cm, draw, end, ..] = ops else {
        return None;
    };
    let is = |op: &Operation<'_>, name: &[u8], operands: usize| {
        op.operator_name(buf) == Some(name) && op.operands.len() == operands
    };
    if !(is(q, b"q", 0) && is(cm, b"cm", 6) && is(draw, b"Do", 1) && is(end, b"Q", 0)) {
        return None;
    }
    let [a, b, c, d, e, f] = numbers(cm)?[..] else {
        return None;
    };
    if b != 0.0 || c != 0.0 || a <= 0.0 || d <= 0.0 {
        return None;
    }
    let ContentTokenKind::Operand(Object::Name(name)) = &draw.operands.first()?.kind else {
        return None;
    };
    let id = xobjects.get(name.as_bytes())?.as_reference()?;
    let Some(Object::Stream(s)) = view.value(id) else {
        return None;
    };
    let dict = &s.dict;
    let subtype = dict.get(b"Subtype").and_then(Object::as_name);
    if subtype.map(Name::as_bytes) != Some(b"Image")
        || REFUSED.iter().any(|k| dict.get(k).is_some())
    {
        return None;
    }
    let height = u32::try_from(view.resolve(dict.get(b"Height")?).as_int()?).ok()?;
    (height > 0).then_some(Strip {
        span: ByteSpan::new(
            q.operator.span.start,
            end.operator.span.end() - q.operator.span.start,
        ),
        id,
        dict,
        height,
        a,
        d,
        e,
        f,
    })
}

/// Every run of two or more strips in `content`, drawn with `resources`.
pub(crate) fn runs(view: &DocumentView<'_>, content: &ContentStream, resources: &Dict) -> Vec<Run> {
    let Some(xobjects) = resources
        .get(b"XObject")
        .map(|x| view.resolve(x))
        .and_then(Object::as_dict)
    else {
        return Vec::new();
    };
    let ops: Vec<Operation<'_>> = content.operations().collect();
    let mut out = Vec::new();
    let mut chain: Vec<Strip<'_>> = Vec::new();
    let mut down = None;
    let mut i = 0;
    while let Some(rest) = ops.get(i..) {
        if rest.is_empty() {
            break;
        }
        let Some(next) = strip(view, &content.buf, rest, xobjects) else {
            out.extend(run_of(&chain, down));
            chain.clear();
            i += 1;
            continue;
        };
        i += 4;
        let joined = match (chain.first(), chain.last()) {
            (Some(first), Some(last)) => last.continued_by(first, &next, down),
            _ => None,
        };
        if joined.is_some() {
            down = joined;
        } else {
            out.extend(run_of(&chain, down));
            chain.clear();
            down = None;
        }
        chain.push(next);
    }
    out.extend(run_of(&chain, down));
    out
}

fn run_of(chain: &[Strip<'_>], down: Option<bool>) -> Option<Run> {
    let (first, last) = (chain.first()?, chain.last()?);
    if chain.len() < 2 {
        return None;
    }
    let mut ids: Vec<ObjId> = chain.iter().map(|s| s.id).collect();
    if down == Some(false) {
        ids.reverse();
    }
    let width = u32::try_from(first.dict.get(b"Width")?.as_int()?).ok()?;
    let height = chain
        .iter()
        .try_fold(0u32, |n, s| n.checked_add(s.height))?;
    let bottom = chain.iter().map(|s| s.f).fold(f64::INFINITY, f64::min);
    let top = chain
        .iter()
        .map(|s| s.f + s.d)
        .fold(f64::NEG_INFINITY, f64::max);
    Some(Run {
        span: ByteSpan::new(first.span.start, last.span.end() - first.span.start),
        ids,
        width,
        height,
        matrix: [first.a, 0.0, 0.0, top - bottom, first.e, bottom],
    })
}

/// A merged image: its dictionary and Flate-encoded samples.
pub(crate) struct Merged {
    pub(crate) dict: Dict,
    pub(crate) data: Vec<u8>,
    /// The encoded size of the strips it replaces, each counted once.
    pub(crate) replaced: u64,
}

/// `run` as one image, or `None` when a strip does not decode, holds too
/// few samples, or the merged image is not smaller than the strips.
pub(crate) fn merge(view: &DocumentView<'_>, run: &Run, resources: &Dict) -> Option<Merged> {
    let first = match view.value(*run.ids.first()?) {
        Some(Object::Stream(s)) => &s.dict,
        _ => return None,
    };
    let mask = first.get(b"ImageMask") == Some(&Object::Boolean(true));
    let (components, bits) = if mask {
        (1, 1)
    } else {
        let bits = u32::try_from(view.resolve(first.get(b"BitsPerComponent")?).as_int()?).ok()?;
        let components = crate::redact_image::colorspace_components(view, first, resources)?;
        (components, bits)
    };
    let stride = crate::image_import::row_bytes(run.width, components, bits);
    let total = stride.checked_mul(usize::try_from(run.height).ok()?)?;
    if total > filters::MAX_DECODED_LEN {
        return None;
    }
    let mut samples = Vec::with_capacity(total);
    let mut seen = BTreeSet::new();
    let mut replaced = 0u64;
    for &id in &run.ids {
        let Some(Object::Stream(s)) = view.value(id) else {
            return None;
        };
        let raw = view.slice(s.data_span)?;
        if seen.insert(id) {
            replaced += raw.len() as u64;
        }
        let rows = usize::try_from(view.resolve(s.dict.get(b"Height")?).as_int()?).ok()?;
        let decoded = filters::decode_stream(&s.dict, raw).ok()?;
        samples.extend_from_slice(decoded.get(..stride.checked_mul(rows)?)?);
    }
    let data = filters::flate::encode(&samples);
    if data.len() as u64 >= replaced {
        return None;
    }
    let mut dict = Dict::default();
    dict.insert(Name::from(b"Type"), Object::Name(Name::from(b"XObject")));
    dict.insert(Name::from(b"Subtype"), Object::Name(Name::from(b"Image")));
    for key in SAME {
        if let Some(v) = first.get(key) {
            dict.insert(Name::from(*key), v.clone());
        }
    }
    dict.insert(
        Name::from(b"Height"),
        Object::Integer(i64::from(run.height)),
    );
    dict.insert(
        Name::from(b"Filter"),
        Object::Name(Name::from(b"FlateDecode")),
    );
    dict.insert(Name::from(b"Length"), Object::Integer(data.len() as i64));
    Some(Merged {
        dict,
        data,
        replaced,
    })
}

/// The objects holding a reference to each object.
pub(crate) type Referrers = HashMap<ObjId, Vec<ObjId>>;

/// Who references each of `live`'s objects, among `live`.
pub(crate) fn referrers(view: &DocumentView<'_>, live: &BTreeSet<ObjId>) -> Referrers {
    let mut out = Referrers::new();
    for &id in live {
        let mut refs = Vec::new();
        if let Some(value) = view.value(id) {
            crate::edit::collect_references(value, false, 0, &mut refs);
        }
        for r in refs {
            out.entry(r).or_default().push(id);
        }
    }
    out
}

fn is_page(d: &Dict) -> bool {
    d.get(b"Type").and_then(Object::as_name).map(Name::as_bytes) == Some(b"Page")
}

/// The pages holding `id` directly, or through resource dictionaries only
/// pages hold; `None` when anything else holds it.
fn users(
    view: &DocumentView<'_>,
    refs: &Referrers,
    id: ObjId,
    depth: u8,
) -> Option<BTreeSet<ObjId>> {
    let mut out = BTreeSet::new();
    for &r in refs.get(&id)? {
        match view.value(r) {
            Some(Object::Dict(d)) if is_page(d) => {
                out.insert(r);
            }
            Some(Object::Dict(d))
                if depth == 0 && d.get(b"XObject") == Some(&Object::Reference(id)) =>
            {
                out.extend(users(view, refs, r, 1)?);
            }
            _ => return None,
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Every page drawing from `page`'s `/XObject` dictionary, when only pages
/// do and none of its forms would look names up in it (a form with no
/// `/Resources` of its own, §7.8.3).
pub(crate) fn sharers(
    view: &DocumentView<'_>,
    refs: &Referrers,
    page: ObjId,
) -> Option<BTreeSet<ObjId>> {
    let res = view.value(page)?.as_dict()?.get(b"Resources")?;
    let xobject = view.resolve(res).as_dict()?.get(b"XObject")?;
    let holder = match (xobject, res) {
        (Object::Reference(x), _) => Some(*x),
        (_, Object::Reference(r)) => Some(*r),
        _ => None,
    };
    let pages = match holder {
        Some(h) => users(view, refs, h, 0)?,
        None => BTreeSet::from([page]),
    };
    let inherits = view.resolve(xobject).as_dict()?.iter().any(|(_, v)| {
        matches!(view.resolve(v), Object::Stream(s)
            if s.dict.get(b"Subtype").and_then(Object::as_name).map(Name::as_bytes) == Some(b"Form")
                && s.dict.get(b"Resources").is_none())
    });
    (!inherits).then_some(pages)
}

/// The `/XObject` names of `page` bound to one of `ids` that no page of
/// `pages` draws any more.
pub(crate) fn unused_names(
    view: &DocumentView<'_>,
    page: &crate::page_tree::Page,
    ids: &BTreeSet<ObjId>,
    pages: &BTreeSet<ObjId>,
) -> Option<Vec<Vec<u8>>> {
    let mut drawn = BTreeSet::new();
    let all = crate::page_tree::pages_in(view).ok()?;
    for p in all.iter().filter(|p| pages.contains(&p.id)) {
        let content = ContentStream::from_page(view, p).ok()?;
        for t in &content.tokens {
            if let ContentTokenKind::Operand(Object::Name(n)) = &t.kind {
                drawn.insert(n.as_bytes().to_vec());
            }
        }
    }
    let xobjects = view.resolve(page.resources.get(b"XObject")?).as_dict()?;
    Some(
        xobjects
            .iter()
            .filter(|(k, v)| {
                v.as_reference().is_some_and(|r| ids.contains(&r)) && !drawn.contains(k.as_bytes())
            })
            .map(|(k, _)| k.as_bytes().to_vec())
            .collect(),
    )
}

/// One item per page holding a run that merges, measured by merging it.
pub(crate) fn fragment_carriers(
    view: &DocumentView<'_>,
    live: &BTreeSet<ObjId>,
) -> Vec<ObjectDiscard> {
    let pages = crate::page_tree::pages_in(view).unwrap_or_default();
    let refs = referrers(view, live);
    let mut out = Vec::new();
    for (index, page) in pages.iter().enumerate() {
        if sharers(view, &refs, page.id).is_none() {
            continue;
        }
        let Ok(content) = ContentStream::from_page(view, page) else {
            continue;
        };
        let (mut strips, mut images, mut bytes) = (0, 0, 0u64);
        let mut size = (0, 0);
        for run in runs(view, &content, &page.resources) {
            if let Some(m) = merge(view, &run, &page.resources) {
                strips += run.ids.len();
                images += 1;
                bytes += m.replaced.saturating_sub(m.data.len() as u64);
                size = (run.width, run.height);
            }
        }
        if images == 0 {
            continue;
        }
        let location = if images == 1 {
            format!(
                "page {}: {strips} strips into 1 image ({}x{})",
                index + 1,
                size.0,
                size.1
            )
        } else {
            format!("page {}: {strips} strips into {images} images", index + 1)
        };
        out.push(ObjectDiscard {
            object: DiscardObject::MergeImageFragments,
            owner: page.id,
            location,
            bytes,
            strips: Vec::new(),
            rewrites: Vec::new(),
            drops: Vec::new(),
        });
    }
    out
}

/// The content that draws the merged image in place of the run.
pub(crate) fn invocation(matrix: [f64; 6], name: &[u8]) -> Vec<u8> {
    let mut out = b"q ".to_vec();
    for v in matrix {
        crate::writer::content::emit_number(&mut out, v);
        out.push(b' ');
    }
    out.extend_from_slice(b"cm /");
    out.extend_from_slice(name);
    out.extend_from_slice(b" Do Q");
    out
}
