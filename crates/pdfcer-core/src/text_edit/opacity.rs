//! Text fill opacity: an `/ExtGState` whose only entry is `/ca` (ISO 32000-1
//! §8.4.5 Table 58), selected by `gs`. `gs` is General graphics state, which
//! a text object allows (§8.2 Figure 9), so a run inside `BT … ET` can be
//! faded and the ambient alpha put back after it without a `q`/`Q`.

use crate::graph::ObjectGraph;
use crate::object::{Dict, Name, ObjId, Object};
use crate::text_edit::format::FormatError;
use crate::view::DocumentView;
use std::collections::BTreeMap;

/// The resource-name prefix pdfcer binds text alphas under.
const KEY_PREFIX: &str = "pdfcerTA";

/// Add Text's opacity: puts `/key gs` just inside the run's opening `q` in
/// `content` (wrapping it in its own `q … Q` if it has none, so the alpha
/// never leaks into later content) and returns `resources /ExtGState` with
/// the new state merged in.
pub(crate) fn add_text_alpha(
    doc: &DocumentView<'_>,
    resources: &Dict,
    alpha: f64,
    content: &mut Vec<u8>,
) -> Dict {
    let mut ext = resources
        .get(b"ExtGState")
        .and_then(|o| doc.resolve(o).as_dict().cloned())
        .unwrap_or_default();
    let key = free_keys(doc, resources, 1)
        .pop()
        .unwrap_or_else(|| KEY_PREFIX.as_bytes().to_vec());
    let created = CreatedAlpha { key, alpha };
    let mut op = created.op();
    op.push(b'\n');
    match content.windows(3).position(|w| w == b"q\n") {
        Some(at)
            if content
                .get(..at)
                .is_some_and(|lead| lead.iter().all(u8::is_ascii_whitespace)) =>
        {
            content.splice(at + 2..at + 2, op);
        }
        _ => {
            let mut wrapped = b"\nq\n".to_vec();
            wrapped.extend_from_slice(&op);
            wrapped.append(content);
            wrapped.extend_from_slice(b"\nQ\n");
            *content = wrapped;
        }
    }
    ext.insert(Name(created.key.clone()), Object::Dict(created.dict()));
    ext
}

/// The alphas a format change binds, and its `(ambient, emitted)` pair.
pub(crate) type AlphaPlan = (Vec<CreatedAlpha>, Option<(f64, f64)>);

/// The fill alpha (`/ca`) in effect at a content-stream position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FillAlpha {
    /// No `gs` has set `/ca`: Table 52's initial 1.0.
    Initial,
    /// Set by a `gs` whose `/ca` was read.
    Known(f64),
    /// Set by a `gs` whose dictionary or `/ca` could not be read.
    Unknown,
}

impl FillAlpha {
    /// The alpha after `/name gs` under `resources`. A dictionary without
    /// `/ca` leaves it unchanged (§8.4.5: absent entries are not set).
    pub(crate) fn after_gs(self, doc: &DocumentView<'_>, resources: &Dict, name: &[u8]) -> Self {
        let gs = resources
            .get(b"ExtGState")
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict)
            .and_then(|d| d.get(name))
            .map(|o| doc.resolve(o))
            .and_then(Object::as_dict);
        let Some(gs) = gs else {
            return Self::Unknown;
        };
        match gs.get(b"ca").map(|o| doc.resolve(o)) {
            None => self,
            Some(Object::Integer(i)) => Self::Known((*i as f64).clamp(0.0, 1.0)),
            Some(Object::Real(v)) if v.is_finite() => Self::Known(v.clamp(0.0, 1.0)),
            Some(_) => Self::Unknown,
        }
    }

    /// The numeric alpha, or `None` when it cannot be known.
    pub(crate) fn value(self) -> Option<f64> {
        match self {
            Self::Initial => Some(1.0),
            Self::Known(v) => Some(v),
            Self::Unknown => None,
        }
    }
}

/// An `/ExtGState` resource a text edit must create: the key its `gs`
/// names, and the alpha it sets.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CreatedAlpha {
    pub(crate) key: Vec<u8>,
    pub(crate) alpha: f64,
}

impl CreatedAlpha {
    /// The direct `<< /Type /ExtGState /ca a >>` dictionary.
    pub(crate) fn dict(&self) -> Dict {
        let mut d = Dict::new();
        d.insert(Name::from(b"Type"), Object::Name(Name::from(b"ExtGState")));
        d.insert(Name::from(b"ca"), Object::Real(self.alpha));
        d
    }

    /// `/key gs`.
    pub(crate) fn op(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.key.len() + 4);
        out.push(b'/');
        out.extend_from_slice(&self.key);
        out.extend_from_slice(b" gs");
        out
    }
}

/// `n` keys free in `resources /ExtGState`, in the order they are used.
pub(crate) fn free_keys(doc: &DocumentView<'_>, resources: &Dict, n: usize) -> Vec<Vec<u8>> {
    let present = resources
        .get(b"ExtGState")
        .map(|o| doc.resolve(o))
        .and_then(Object::as_dict);
    (1u32..)
        .map(|i| format!("{KEY_PREFIX}{i}").into_bytes())
        .filter(|k| present.is_none_or(|d| d.get(k.as_slice()).is_none()))
        .take(n)
        .collect()
}

/// The writes binding every alpha in `created` into the owner's
/// `/Resources /ExtGState` as direct dictionaries, applied over `pending`
/// (writes the same edit already makes, e.g. a created font) so neither
/// clobbers the other. Returns the writes and whether the patched
/// dictionary may be shared with another owner.
pub(crate) fn bind_alphas<G: ObjectGraph + ?Sized>(
    graph: &G,
    pending: &BTreeMap<ObjId, Object>,
    owner_id: ObjId,
    may_inherit: bool,
    created: &[CreatedAlpha],
) -> (Vec<(ObjId, Object)>, bool) {
    let overlay = Overlay {
        base: graph,
        pending,
    };
    crate::text_edit::addtext::patch_resource(
        &overlay,
        owner_id,
        may_inherit,
        b"ExtGState",
        |entries| {
            for c in created {
                entries.insert(Name(c.key.clone()), Object::Dict(c.dict()));
            }
        },
    )
}

/// A graph with uncommitted writes laid over it.
struct Overlay<'a, G: ?Sized> {
    base: &'a G,
    pending: &'a BTreeMap<ObjId, Object>,
}

impl<G: ObjectGraph + ?Sized> ObjectGraph for Overlay<'_, G> {
    fn value(&self, id: ObjId) -> Option<&Object> {
        self.pending.get(&id).or_else(|| self.base.value(id))
    }

    fn trailer_entry(&self, key: &[u8]) -> Option<&Object> {
        self.base.trailer_entry(key)
    }
}

/// The ops and resources for [`FormatRequest::set_fill_alpha`]: `/k1 gs`
/// appended to `set_ops` and `/k2 gs` (the ambient alpha) to
/// `restore_ops`. Returns the resources to bind and
/// `(ambient, requested)`; asking for the alpha already in effect emits
/// nothing.
///
/// # Errors
///
/// [`FormatError::BadOpacity`] outside 0..=1; [`FormatError::OpacityUnrestorable`]
/// when the ambient alpha is unknown.
///
/// [`FormatRequest::set_fill_alpha`]: crate::text_edit::format::FormatRequest::set_fill_alpha
pub(crate) fn plan_alpha(
    doc: &DocumentView<'_>,
    resources: &Dict,
    ambient: FillAlpha,
    requested: Option<f64>,
    set_ops: &mut Vec<u8>,
    restore_ops: &mut Vec<u8>,
) -> Result<AlphaPlan, FormatError> {
    let Some(alpha) = requested else {
        return Ok((Vec::new(), None));
    };
    if !(0.0..=1.0).contains(&alpha) {
        return Err(FormatError::BadOpacity(alpha));
    }
    let prior = ambient.value().ok_or(FormatError::OpacityUnrestorable)?;
    if (alpha - prior).abs() < ALPHA_EPS {
        return Ok((Vec::new(), Some((prior, alpha))));
    }
    let mut keys = free_keys(doc, resources, 2).into_iter();
    let (Some(set_key), Some(restore_key)) = (keys.next(), keys.next()) else {
        return Err(FormatError::OpacityUnrestorable);
    };
    let set = CreatedAlpha {
        key: set_key,
        alpha,
    };
    let restore = CreatedAlpha {
        key: restore_key,
        alpha: prior,
    };
    for (ops, c) in [(set_ops, &set), (restore_ops, &restore)] {
        if !ops.is_empty() {
            ops.push(b' ');
        }
        ops.extend_from_slice(&c.op());
    }
    Ok((vec![set, restore], Some((prior, alpha))))
}

/// The disclosure when the patched `/ExtGState` dictionary may be shared.
pub(crate) const SHARED_NOTE: &str = "opacity: the graphics-state resource was added to a \
     /Resources dictionary shared with other pages or forms; it is unreferenced there and \
     changes nothing about how they render";

/// Two alphas closer than this are the same opacity.
const ALPHA_EPS: f64 = 1e-6;

/// Bind `created` into the owner's resources over `writes` (the edit's
/// other pending writes), replacing a write for the same object or adding
/// one. Returns whether the patched dictionary may be shared.
pub(crate) fn merge_alphas<G: ObjectGraph + ?Sized>(
    graph: &G,
    writes: &mut Vec<(ObjId, Object)>,
    owner_id: ObjId,
    may_inherit: bool,
    created: &[CreatedAlpha],
) -> bool {
    if created.is_empty() {
        return false;
    }
    let pending: BTreeMap<ObjId, Object> = writes.iter().cloned().collect();
    let (bound, shared) = bind_alphas(graph, &pending, owner_id, may_inherit, created);
    for (id, value) in bound {
        match writes.iter_mut().find(|(w, _)| *w == id) {
            Some(slot) => slot.1 = value,
            None => writes.push((id, value)),
        }
    }
    shared
}
