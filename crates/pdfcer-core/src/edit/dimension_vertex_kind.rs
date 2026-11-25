//! The pure geometry step behind the ce-dimension vertex verbs: apply a
//! [`VertexEdit`] to a [`DimensionKind`] or say why it cannot be applied.

use super::{EditError, VertexEdit, check_centre_kind};
use crate::dimension::{DimensionId, DimensionKind};
use crate::vector::Point;

/// Apply a [`VertexEdit`] to a [`DimensionKind`], or say precisely why it
/// cannot be applied (`Pass 107.0`, centre lines `Pass 768.0`).
///
/// A free function rather than a method on the enum because its refusals name
/// a [`DimensionId`], which belongs to the record, not the geometry. Pure, so
/// [`super::EditSession::vertex_edit_preview`] and the three mutating verbs
/// share it exactly.
///
/// # The minimum vertex counts, and why they differ
///
/// An OPEN path needs **two** vertices, because one vertex has no segment and
/// therefore no length. A CLOSED one needs **three**, because two closed
/// vertices trace a line there and back: it draws as a single stroke and
/// prints twice the distance between two points.
///
/// # Errors
///
/// [`EditError::DimensionHasNoVertices`] for a circular, angular or
/// centre-mark ce dimension; [`EditError::DimensionVertexCountFixed`] for
/// insert/remove on a linear ce dimension or a centre line;
/// [`EditError::VertexIndexOutOfRange`]; [`EditError::VertexNotPlaceable`] for
/// a non-finite or absurd coordinate; [`EditError::PerimeterWouldBeDegenerate`]
/// for a removal below the minimum; [`EditError::CentreGeometryDegenerate`]
/// for a centre line whose ends would coincide.
pub(super) fn vertex_edited_kind(
    id: DimensionId,
    kind: &DimensionKind,
    edit: VertexEdit,
) -> Result<DimensionKind, EditError> {
    match kind {
        DimensionKind::Perimeter {
            points,
            closed,
            area,
            offset,
            text_along,
        } => Ok(DimensionKind::Perimeter {
            points: perimeter_edited(id, points, *closed, edit)?,
            closed: *closed,
            area: *area,
            offset: *offset,
            text_along: *text_along,
        }),
        DimensionKind::Linear {
            a,
            b,
            constraint,
            offset,
            text_along,
            extension_gap,
        } => {
            let (a, b) = two_ends_edited(id, *a, *b, edit)?;
            Ok(DimensionKind::Linear {
                a,
                b,
                // Untouched: the constraint is the operator's decision at
                // placement, and dragging one end is not a request to revoke
                // it. A Horizontal ce dimension re-measures its new span.
                constraint: *constraint,
                offset: *offset,
                text_along: *text_along,
                extension_gap: *extension_gap,
            })
        }
        DimensionKind::CentreLine { a, b } => {
            let (a, b) = two_ends_edited(id, *a, *b, edit)?;
            let edited = DimensionKind::CentreLine { a, b };
            check_centre_kind(&edited)?;
            Ok(edited)
        }
        DimensionKind::Circular { .. }
        | DimensionKind::Angular { .. }
        | DimensionKind::CentreMark { .. } => Err(EditError::DimensionHasNoVertices { id: id.0 }),
    }
}

/// The guard every written coordinate must pass. `usable_page_value` is the
/// sidecar reader's own test, so a coordinate the reader would reject is one
/// the writer refuses (R92).
fn placeable(id: DimensionId, p: Point) -> Result<Point, EditError> {
    if crate::dimension::sidecar::usable_page_value(p.x)
        && crate::dimension::sidecar::usable_page_value(p.y)
    {
        Ok(p)
    } else {
        Err(EditError::VertexNotPlaceable {
            id: id.0,
            x: p.x,
            y: p.y,
        })
    }
}

fn out_of_range(id: DimensionId, index: usize, count: usize) -> EditError {
    EditError::VertexIndexOutOfRange {
        id: id.0,
        index,
        count,
    }
}

fn perimeter_edited(
    id: DimensionId,
    points: &[Point],
    closed: bool,
    edit: VertexEdit,
) -> Result<Vec<Point>, EditError> {
    let mut points = points.to_vec();
    let count = points.len();
    match edit {
        VertexEdit::Move { index, dx, dy } => {
            let slot = points
                .get_mut(index)
                .ok_or_else(|| out_of_range(id, index, count))?;
            *slot = placeable(id, Point::new(slot.x + dx, slot.y + dy))?;
        }
        VertexEdit::Insert { after, at } => {
            if after >= count {
                return Err(out_of_range(id, after, count));
            }
            points.insert(after + 1, placeable(id, at)?);
        }
        VertexEdit::Remove { index } => {
            if index >= count {
                return Err(out_of_range(id, index, count));
            }
            let minimum = if closed { 3 } else { 2 };
            if count - 1 < minimum {
                return Err(EditError::PerimeterWouldBeDegenerate {
                    id: id.0,
                    shape: if closed { "closed" } else { "open" },
                    remaining: count - 1,
                    minimum,
                });
            }
            points.remove(index);
        }
    }
    Ok(points)
}

/// A two-point kind (linear ce dimension, centre line): index 0 is `a` and 1
/// is `b`, the pick order, so handles drawn in vertex order sit where the
/// operator clicked. The count is structural, so insert and remove refuse.
fn two_ends_edited(
    id: DimensionId,
    a: Point,
    b: Point,
    edit: VertexEdit,
) -> Result<(Point, Point), EditError> {
    match edit {
        VertexEdit::Move { index: 0, dx, dy } => {
            Ok((placeable(id, Point::new(a.x + dx, a.y + dy))?, b))
        }
        VertexEdit::Move { index: 1, dx, dy } => {
            Ok((a, placeable(id, Point::new(b.x + dx, b.y + dy))?))
        }
        VertexEdit::Move { index, .. } => Err(out_of_range(id, index, 2)),
        VertexEdit::Insert { .. } | VertexEdit::Remove { .. } => {
            Err(EditError::DimensionVertexCountFixed { id: id.0, count: 2 })
        }
    }
}
