//! Grouping page objects into one form XObject, and inlining a form back
//! into the content that paints it (pdfcer-gui request G280).
//!
//! A group is a form XObject (ISO 32000-1 §8.10) holding its members, each
//! wrapped as [`super::restack`] wraps a moved object, painted by one `Do`
//! where the front-most member was. The form carries no `/Group`, so it
//! composites exactly as the loose objects did (§11.6.6 applies only to
//! transparency group XObjects). Ungrouping is the §8.10.1 `Do` procedure
//! written out: `q <Matrix> cm <BBox> re W n <content> Q`.

use std::collections::BTreeSet;

use super::decompose::VectorObject;
use super::edit::VectorEditError;
use super::geometry::{Bounds, Matrix, Point};
use super::restack::{GsResources, RestackLimitReason, gather};
use crate::content::ContentStream;
use crate::object::{Dict, Name, Object};
use crate::pageops::region::{ResourceUse, rewrite_with};
use crate::writer::content::emit_number;

/// The `/PieceInfo` key a group's marker is written under (§14.5: the
/// application's name).
pub(crate) const MARKER_KEY: &[u8] = b"pdfcer";
/// The §14.5 Table 319 `/LastModified` a marker carries: fixed, so the same
/// group always serialises to the same bytes.
const MARKER_DATE: &[u8] = b"D:20260801000000Z";

/// Why objects could not be grouped, or a form could not be ungrouped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GroupError {
    /// No object was named.
    #[error("no objects were given to group")]
    NoObjects,
    /// One member cannot be gathered with the others without rendering
    /// differently; nothing was grouped.
    #[error("object {object} cannot join the group: {}", reason_text(*.reason))]
    MemberCannotJoin {
        /// The member's index.
        object: usize,
        /// [`RestackLimitReason::Scope`]: it is under a different clip or
        /// marked-content section than the front-most member, or in state
        /// that cannot be rebuilt. [`RestackLimitReason::Entangled`]: its
        /// own bytes set a clip, marked content or an unbalanced `q`/`Q`.
        reason: RestackLimitReason,
    },
    /// The transform at the group's position cannot be inverted, so the
    /// group has no bounding box.
    #[error("the transform at the group's position is not invertible")]
    NotInvertible,
    /// The object is not a form XObject with an object of its own.
    #[error("object {index} is not a form XObject")]
    NotAForm {
        /// The object's index.
        index: usize,
    },
    /// The form is a transparency group (§11.6.6), which composites as a
    /// unit; its content inlined would blend differently.
    #[error("the form is a transparency group, which composites as a unit")]
    TransparencyGroup,
    /// The form carries `/OC` (§8.11.3.2): its visibility is a layer's, and
    /// inlined content would always show.
    #[error("the form belongs to an optional-content layer")]
    OptionalContent,
    /// The objects are already nested as deep as pdfcer reads; one more
    /// level would paint them but leave them unreachable for editing.
    #[error("the objects are already {limit} forms deep, the deepest nesting pdfcer edits")]
    TooDeep {
        /// [`crate::content::MAX_FORM_DEPTH`].
        limit: usize,
    },
    /// The content cannot stand in another stream.
    #[error("the content cannot be moved between streams: {reason}")]
    Unportable {
        /// What is wrong with it.
        reason: &'static str,
    },
    /// A bad index, or objects whose bytes overlap.
    #[error(transparent)]
    Edit(#[from] VectorEditError),
}

const fn reason_text(reason: RestackLimitReason) -> &'static str {
    match reason {
        RestackLimitReason::Scope => {
            "it is under a different clip, layer or marked-content section, or in graphics \
             state that cannot be rebuilt"
        }
        RestackLimitReason::Entangled => {
            "its own bytes set a clip, marked content or unbalanced q/Q"
        }
    }
}

/// A resource's category (`ExtGState`, `Font`, …) and its name in it.
pub(crate) type ResourceKey = (&'static [u8], Vec<u8>);

/// A planned group.
pub(crate) struct GroupPlan {
    /// The stream's content with the members replaced by one `Do`.
    pub content: Vec<u8>,
    /// The new form's content.
    pub form_content: Vec<u8>,
    /// The form's `/BBox`, `[llx lly urx ury]`.
    pub bbox: [f64; 4],
    /// Maps the space `initial` lands in to the form's own space.
    pub to_form: Matrix,
    /// `/ExtGState`s the form content names beyond the stream's own.
    pub bind: Vec<(Vec<u8>, Dict)>,
    /// The group's object index once the edit is in.
    pub index: usize,
    /// How many objects went in.
    pub members: usize,
}

/// Group `selected` objects of a stream (its decomposition `objects` under
/// `initial`, over `content`) into a form painted as `/xobject_name Do`.
///
/// # Errors
///
/// [`GroupError`]; nothing is planned unless every member can join.
pub(crate) fn plan_group(
    content: &ContentStream,
    objects: &[VectorObject],
    initial: Matrix,
    selected: &[usize],
    res: &GsResources<'_>,
    xobject_name: &[u8],
) -> Result<GroupPlan, GroupError> {
    let mut members = selected.to_vec();
    members.sort_unstable();
    members.dedup();
    let Some(&top) = members.last() else {
        return Err(GroupError::NoObjects);
    };
    let g = gather(content, objects, &members, res)?.map_err(|l| GroupError::MemberCannotJoin {
        object: l.object,
        reason: l.reason,
    })?;
    let to_form = g
        .dest_ctm
        .post_concat(initial)
        .inverse()
        .ok_or(GroupError::NotInvertible)?;
    let (mut extent, mut outset) = (Bounds::EMPTY, 1.0_f64);
    for o in members.iter().filter_map(|&m| objects.get(m)) {
        extent = extent.union(o.page_bbox());
        outset = outset.max(paint_outset(o));
    }
    let bbox = form_box(extent.inflate(outset), to_form).ok_or(GroupError::NotInvertible)?;

    let buf = &content.buf;
    let mut out = Vec::with_capacity(buf.len());
    let mut cursor = 0;
    for ((start, end), removal) in &g.removals {
        out.extend_from_slice(buf.get(cursor..*start).unwrap_or_default());
        out.extend_from_slice(removal);
        cursor = *end;
    }
    out.extend_from_slice(buf.get(cursor..g.dest).unwrap_or_default());
    out.extend_from_slice(b"\n/");
    out.extend_from_slice(xobject_name);
    out.extend_from_slice(b" Do\n");
    out.extend_from_slice(buf.get(g.dest..).unwrap_or_default());
    Ok(GroupPlan {
        content: out,
        form_content: g.wraps.concat(),
        bbox,
        to_form,
        bind: g.bind,
        index: top + 1 - members.len(),
        members: members.len(),
    })
}

/// How far an object's paint can reach past its `page_bbox`: a stroke's
/// half width times the default miter limit of 10 (§8.4.3.5), and half a
/// text object's height for glyphs overhanging their advance box.
pub(crate) fn paint_outset(o: &VectorObject) -> f64 {
    match o {
        VectorObject::Path(p) if p.style.stroke => {
            let scale = p
                .ctm
                .map_vector(Point::new(1.0, 0.0))
                .distance(Point::new(0.0, 0.0))
                .max(
                    p.ctm
                        .map_vector(Point::new(0.0, 1.0))
                        .distance(Point::new(0.0, 0.0)),
                );
            p.line_width * scale * 5.0
        }
        VectorObject::Text(t) => {
            let b = t.page_bbox;
            if b.is_empty() {
                0.0
            } else {
                (b.max.y - b.min.y).max(b.max.x - b.min.x).min(72.0) * 0.5
            }
        }
        _ => 0.0,
    }
}

/// `extent` mapped by `to_form`, as `[llx lly urx ury]`; `None` when it is
/// empty or not finite.
pub(crate) fn form_box(extent: Bounds, to_form: Matrix) -> Option<[f64; 4]> {
    if extent.is_empty() {
        return None;
    }
    let mapped = [
        extent.min,
        Point::new(extent.max.x, extent.min.y),
        extent.max,
        Point::new(extent.min.x, extent.max.y),
    ]
    .into_iter()
    .map(|p| to_form.map_point(p))
    .fold(Bounds::EMPTY, Bounds::union_point);
    let bbox = [mapped.min.x, mapped.min.y, mapped.max.x, mapped.max.y];
    bbox.iter().all(|v| v.is_finite()).then_some(bbox)
}

/// Every `(category, name)` resource `content` names, in first-use order.
///
/// # Errors
///
/// [`GroupError::Unportable`] when `content` does not parse or is not
/// balanced.
pub(crate) fn used_resources(content: &[u8]) -> Result<Vec<ResourceKey>, GroupError> {
    let stream = ContentStream::parse(content.to_vec()).map_err(|_| GroupError::Unportable {
        reason: "it does not parse",
    })?;
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    rewrite_with(&stream, &mut |u: ResourceUse<'_>| {
        if seen.insert((u.category, u.name.to_vec())) {
            out.push((u.category, u.name.to_vec()));
        }
        None
    })
    .map_err(|reason| GroupError::Unportable { reason })?;
    Ok(out)
}

/// A form's content written out as the `Do` that painted it: `q`, its
/// `matrix`, its `clip` box if one is wanted, the content with `rename`
/// applied ([`rewrite_with`]), `Q`.
///
/// # Errors
///
/// [`GroupError::Unportable`] with [`rewrite_with`]'s reason.
pub(crate) fn inlined(
    content: &ContentStream,
    matrix: [f64; 6],
    clip: Option<[f64; 4]>,
    rename: &mut dyn FnMut(ResourceUse<'_>) -> Option<Vec<u8>>,
) -> Result<Vec<u8>, GroupError> {
    let body = rewrite_with(content, rename).map_err(|reason| GroupError::Unportable { reason })?;
    let mut out = b"\nq ".to_vec();
    let numbers = |out: &mut Vec<u8>, values: &[f64]| {
        for v in values {
            emit_number(out, *v);
            out.push(b' ');
        }
    };
    if matrix != [1.0, 0.0, 0.0, 1.0, 0.0, 0.0] {
        numbers(&mut out, &matrix);
        out.extend_from_slice(b"cm ");
    }
    if let Some([llx, lly, urx, ury]) = clip {
        numbers(&mut out, &[llx, lly, urx - llx, ury - lly]);
        out.extend_from_slice(b"re W n ");
    }
    out.push(b'\n');
    out.extend_from_slice(&body);
    out.extend_from_slice(b"\nQ\n");
    Ok(out)
}

/// The `/PieceInfo` entry marking a form as a group pdfcer made.
pub(crate) fn marker() -> Dict {
    let mut private = Dict::new();
    private.insert(Name::from(b"Group"), Object::Boolean(true));
    let mut data = Dict::new();
    data.insert(
        Name::from(b"LastModified"),
        Object::String(MARKER_DATE.to_vec()),
    );
    data.insert(Name::from(b"Private"), Object::Dict(private));
    data
}

/// The date [`marker`] carries, which the form's own `/LastModified` must
/// match (§8.10.2 Table 95: required when `/PieceInfo` is present).
pub(crate) const fn marker_date() -> &'static [u8] {
    MARKER_DATE
}

/// Whether `piece_data` (a form's `/PieceInfo /pdfcer` dictionary,
/// resolved) is a group marker.
#[must_use]
pub(crate) fn is_marker(piece_data: &Dict) -> bool {
    piece_data
        .get(b"Private")
        .and_then(Object::as_dict)
        .is_some_and(|p| matches!(p.get(b"Group"), Some(Object::Boolean(true))))
}
