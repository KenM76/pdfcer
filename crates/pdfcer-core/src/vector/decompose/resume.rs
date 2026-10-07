//! Resuming a decomposition at a `/Contents` stream boundary.
//!
//! The walk is a pure function of the tokens before a point, so after an edit
//! that left `/Contents[..k]` byte-identical, the objects painted by those
//! streams and the walk's state where stream `k` begins are what they were.
//! A [`WalkCheckpoint`] holds that state; resuming from it and walking the
//! rest yields the same [`PageObjects`] as walking the whole page.

use super::{
    DecomposeDiagnostics, Decomposer, FontResolver, FormLeaf, GState, PageObjects, VectorObject,
    XObjectResolver, collect_form_leaves_from,
};
use crate::content::ContentStream;
use crate::object::ObjId;
use crate::vector::Matrix;
use crate::view::DocumentView;

/// The walk's state immediately before token [`Self::token`].
///
/// Only taken where nothing is half-built: no operand run, path or text
/// object spans the point, so the walk from here depends on nothing but this.
#[derive(Debug, Clone)]
pub(crate) struct WalkCheckpoint {
    /// The token index the walk resumes at.
    pub(crate) token: usize,
    /// How many objects precede the point.
    pub(crate) objects: usize,
    stack: Vec<GState>,
    gs: GState,
    oc_stack: Vec<Option<ObjId>>,
    diag: DecomposeDiagnostics,
    total_nodes: usize,
    preview_budget: usize,
}

/// The form-leaf pass's progress at an object index: leaves emitted and the
/// two counters it adds to the diagnostics.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct LeafMark {
    /// Leaves collected from the objects before the mark.
    pub(crate) leaves: usize,
    form_cycles: usize,
    form_depth_overflows: usize,
}

impl Decomposer<'_> {
    /// The checkpoint for `boundary` when the walk is about to read token `i`
    /// with its current operand run starting at `run_start`.
    pub(super) fn checkpoint(
        &self,
        boundary: usize,
        i: usize,
        run_start: usize,
    ) -> Option<WalkCheckpoint> {
        let clean = boundary == i && run_start == i && self.path.is_none() && self.text.is_none();
        clean.then(|| WalkCheckpoint {
            token: i,
            objects: self.objects.len(),
            stack: self.stack.clone(),
            gs: self.gs.clone(),
            oc_stack: self.oc_stack.clone(),
            diag: self.diag.clone(),
            total_nodes: self.total_nodes,
            preview_budget: self.preview_budget,
        })
    }
}

/// Decompose `content` whole, with a checkpoint (or `None`) per entry of the
/// ascending token indices `boundaries`. Leaves are not collected.
pub(crate) fn decompose_checkpointed(
    content: &ContentStream,
    xobjects: &dyn XObjectResolver,
    fonts: &dyn FontResolver,
    boundaries: &[usize],
) -> (PageObjects, Vec<Option<WalkCheckpoint>>) {
    let mut d = Decomposer::new(content, Matrix::IDENTITY, xobjects, fonts);
    let mut marks = Vec::with_capacity(boundaries.len());
    d.run_from(0, boundaries, &mut marks);
    (finish(d), marks)
}

/// Continue a decomposition of `content` from `from`, whose `prefix` objects
/// (exactly `from.objects` of them) are carried over unchanged. Checkpoints
/// are recorded for `boundaries`, none of which may precede `from.token`.
pub(crate) fn decompose_resumed(
    content: &ContentStream,
    xobjects: &dyn XObjectResolver,
    fonts: &dyn FontResolver,
    prefix: Vec<VectorObject>,
    from: &WalkCheckpoint,
    boundaries: &[usize],
) -> (PageObjects, Vec<Option<WalkCheckpoint>>) {
    let mut d = Decomposer::new(content, Matrix::IDENTITY, xobjects, fonts);
    d.objects = prefix;
    d.stack.clone_from(&from.stack);
    d.gs = from.gs.clone();
    d.oc_stack.clone_from(&from.oc_stack);
    d.diag = from.diag.clone();
    d.total_nodes = from.total_nodes;
    d.preview_budget = from.preview_budget;
    let mut marks = Vec::with_capacity(boundaries.len());
    d.run_from(from.token, boundaries, &mut marks);
    (finish(d), marks)
}

fn finish(d: Decomposer<'_>) -> PageObjects {
    PageObjects {
        objects: d.objects,
        initial: Matrix::IDENTITY,
        diagnostics: d.diag,
        leaves: Vec::new(),
    }
}

/// Fill `model.leaves` from object `start` on, after `prior` (the leaves of
/// the objects before `start`, whose counters `prior_mark` holds), returning
/// a [`LeafMark`] at each ascending object index in `splits`.
pub(crate) fn collect_leaves_marked(
    view: &DocumentView<'_>,
    model: &mut PageObjects,
    start: usize,
    prior: Vec<FormLeaf>,
    prior_mark: Option<LeafMark>,
    splits: &[usize],
) -> Vec<LeafMark> {
    let mut leaves = prior;
    let mut diag = DecomposeDiagnostics::default();
    if let Some(m) = prior_mark {
        diag.form_cycles = m.form_cycles;
        diag.form_depth_overflows = m.form_depth_overflows;
    }
    let mut marks = Vec::with_capacity(splits.len());
    let mut at = start;
    let mut path = Vec::new();
    for &split in splits.iter().chain(std::iter::once(&usize::MAX)) {
        let end = split.min(model.objects.len()).max(at);
        let head = model.objects.get(..end).unwrap_or(&[]);
        collect_form_leaves_from(view, head, at, &mut path, &mut leaves, &mut diag, None);
        at = end;
        if split != usize::MAX {
            marks.push(LeafMark {
                leaves: leaves.len(),
                form_cycles: diag.form_cycles,
                form_depth_overflows: diag.form_depth_overflows,
            });
        }
    }
    model.diagnostics.form_cycles += diag.form_cycles;
    model.diagnostics.form_depth_overflows += diag.form_depth_overflows;
    model.leaves = leaves;
    marks
}
