//! Insert a node, and convert a node's or a segment's kind, on the page and
//! inside a form XObject (pdfcer-gui request G154).

use super::{CommandKind, EditError, EditSession, FormSurgeryOutcome, vector_object_as_path};
use crate::content::ContentStream;
use crate::vector::{
    NodeKind, PageObjects, PathObject, PlannedEdit, SegmentKind, VectorEditError,
    plan_convert_node, plan_convert_segment, plan_insert_node,
};

fn page_path(model: &PageObjects, object_index: usize) -> Result<&PathObject, EditError> {
    let count = model.objects.len();
    let obj = model
        .objects
        .get(object_index)
        .ok_or(VectorEditError::ObjectOutOfRange {
            index: object_index,
            count,
        })?;
    Ok(vector_object_as_path(obj, object_index)?)
}

impl EditSession {
    fn node_shape_on_page(
        &mut self,
        kind: CommandKind,
        page_index: usize,
        object_index: usize,
        plan: impl Fn(&ContentStream, &PathObject) -> Result<PlannedEdit, VectorEditError>,
    ) -> Result<Vec<String>, EditError> {
        self.vector_surgery(kind, page_index, |stream, model| {
            Ok(plan(stream, page_path(model, object_index)?)?)
        })
    }

    fn node_shape_in_form(
        &mut self,
        kind: CommandKind,
        page_index: usize,
        leaf_index: usize,
        plan: impl Fn(&ContentStream, &PathObject) -> Result<PlannedEdit, VectorEditError>,
    ) -> Result<FormSurgeryOutcome, EditError> {
        self.form_surgery_inner(
            kind,
            page_index,
            leaf_index,
            |stream, model, object_index| {
                Ok(plan(stream, Self::leaf_as_path(model, object_index)?)?)
            },
        )
    }

    /// **Add a node** on the segment leaving node `node_index` of the path
    /// object at paint-order `object_index`, at Bézier parameter `t`
    /// (`0 < t < 1`), as one [`CommandKind::InsertNode`].
    ///
    /// The shape is unchanged (see [`crate::vector::plan_insert_node`]); the new
    /// node is `node_index + 1` and later node indices in the object shift up
    /// by one. Only the split operator is rewritten.
    ///
    /// # Errors
    ///
    /// [`EditError::VectorEdit`] wrapping `InvalidSegmentParameter`,
    /// `NodeOutOfRange`, `NoSegmentHere`, `ObjectOutOfRange`, `NotAPath`,
    /// `DegenerateCtm` or `MalformedOperand`; plus the page, contents,
    /// encryption and certification guards. Nothing changes on an error.
    ///
    /// # Returns
    ///
    /// The plan's disclosures (a rectangle rewritten as lines), which the
    /// caller must surface.
    pub fn insert_node(
        &mut self,
        page_index: usize,
        object_index: usize,
        node_index: usize,
        t: f64,
    ) -> Result<Vec<String>, EditError> {
        self.node_shape_on_page(CommandKind::InsertNode, page_index, object_index, |s, p| {
            plan_insert_node(s, p, node_index, t)
        })
    }

    /// **Make node `node_index` a corner, smooth or symmetric node** (see
    /// [`NodeKind`]), as one [`CommandKind::ConvertNode`]. The node does not
    /// move; its handles do.
    ///
    /// # Errors
    ///
    /// As [`Self::insert_node`], with `NodeHasOneSide` (smooth or symmetric on
    /// the end of an open path) in place of the segment refusals.
    ///
    /// # Returns
    ///
    /// Disclosures: straight sides turned into curves, a rectangle rewritten,
    /// a clipping path reshaped.
    pub fn convert_node(
        &mut self,
        page_index: usize,
        object_index: usize,
        node_index: usize,
        kind: NodeKind,
    ) -> Result<Vec<String>, EditError> {
        self.node_shape_on_page(
            CommandKind::ConvertNode,
            page_index,
            object_index,
            |s, p| plan_convert_node(s, p, node_index, kind),
        )
    }

    /// **Make the segment leaving node `node_index` a line or a curve** (see
    /// [`SegmentKind`]), as one [`CommandKind::ConvertSegment`].
    ///
    /// # Errors
    ///
    /// As [`Self::insert_node`], without `InvalidSegmentParameter`.
    ///
    /// # Returns
    ///
    /// Disclosures: a rectangle rewritten, a clipping path reshaped.
    pub fn convert_segment(
        &mut self,
        page_index: usize,
        object_index: usize,
        node_index: usize,
        kind: SegmentKind,
    ) -> Result<Vec<String>, EditError> {
        self.node_shape_on_page(
            CommandKind::ConvertSegment,
            page_index,
            object_index,
            |s, p| plan_convert_segment(s, p, node_index, kind),
        )
    }

    /// [`Self::insert_node`] for a path inside a form XObject, addressed by
    /// [`crate::vector::PageObjects::leaves`] index. The form is edited in
    /// place, so every placement of it changes (decision 076); the outcome
    /// says how many.
    ///
    /// # Errors
    ///
    /// As [`Self::insert_node`], with [`EditError::FormLeafOutOfRange`] for
    /// the leaf.
    pub fn insert_node_in_form(
        &mut self,
        page_index: usize,
        leaf_index: usize,
        node_index: usize,
        t: f64,
    ) -> Result<FormSurgeryOutcome, EditError> {
        self.node_shape_in_form(CommandKind::InsertNode, page_index, leaf_index, |s, p| {
            plan_insert_node(s, p, node_index, t)
        })
    }

    /// [`Self::convert_node`] for a path inside a form XObject; as
    /// [`Self::insert_node_in_form`].
    ///
    /// # Errors
    ///
    /// As [`Self::convert_node`], with [`EditError::FormLeafOutOfRange`].
    pub fn convert_node_in_form(
        &mut self,
        page_index: usize,
        leaf_index: usize,
        node_index: usize,
        kind: NodeKind,
    ) -> Result<FormSurgeryOutcome, EditError> {
        self.node_shape_in_form(CommandKind::ConvertNode, page_index, leaf_index, |s, p| {
            plan_convert_node(s, p, node_index, kind)
        })
    }

    /// [`Self::convert_segment`] for a path inside a form XObject; as
    /// [`Self::insert_node_in_form`].
    ///
    /// # Errors
    ///
    /// As [`Self::convert_segment`], with [`EditError::FormLeafOutOfRange`].
    pub fn convert_segment_in_form(
        &mut self,
        page_index: usize,
        leaf_index: usize,
        node_index: usize,
        kind: SegmentKind,
    ) -> Result<FormSurgeryOutcome, EditError> {
        self.node_shape_in_form(
            CommandKind::ConvertSegment,
            page_index,
            leaf_index,
            |s, p| plan_convert_segment(s, p, node_index, kind),
        )
    }
}
