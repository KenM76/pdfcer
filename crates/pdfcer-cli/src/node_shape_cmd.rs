//! `node-insert`, `node-convert` and `segment-convert`: add a point to a path,
//! or change what kind of point or segment it is.

use super::*;
use pdfcer_core::edit::{EditError, EditSession, FormSurgeryOutcome};
use pdfcer_core::vector::{NodeKind, SegmentKind};

/// The path node a node-shape command acts on, and how to save.
#[derive(Debug, Clone, clap::Args)]
pub(crate) struct NodeTargetArgs {
    /// Input PDF.
    pub(crate) input: PathBuf,
    /// 1-based page number.
    #[arg(long, default_value_t = 1)]
    pub(crate) page: u32,
    /// 0-based paint-order object index on the page, as `object-list` prints
    /// it on an `object index=` row. Pass exactly one of this and `--leaf`.
    #[arg(long)]
    pub(crate) object: Option<usize>,
    /// 0-based form leaf index, for a path drawn inside a form XObject, as
    /// `object-list` prints it on a `leaf index=` row. A form drawn many
    /// times is edited in every one of them; the count is printed on stderr.
    #[arg(long)]
    pub(crate) leaf: Option<usize>,
    /// 0-based node index in decomposition order, the numbering `node-move`
    /// takes.
    #[arg(long)]
    pub(crate) node: usize,
    /// Output path.
    #[arg(short, long)]
    pub(crate) output: PathBuf,
    /// Save mode.
    #[arg(long, value_enum, default_value_t = SaveMode::Incremental)]
    pub(crate) mode: SaveMode,
    /// Reload and verify the edit undoes byte-identically.
    #[arg(long)]
    pub(crate) verify_undo: bool,
}

/// `--kind` of `node-convert`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum NodeKindArg {
    /// Pull both handles into the point, so the sides meet at a sharp corner.
    Corner,
    /// Line the handles up through the point, each keeping its length.
    Smooth,
    /// Line the handles up and give them the same length.
    Symmetric,
}

/// `--to` of `segment-convert`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum SegmentKindArg {
    /// A straight line; a curve's handles are discarded.
    Line,
    /// A curve; a line becomes one that still looks straight, ready to bend.
    Curve,
}

impl NodeKindArg {
    fn core(self) -> NodeKind {
        match self {
            Self::Corner => NodeKind::Corner,
            Self::Smooth => NodeKind::Smooth,
            Self::Symmetric => NodeKind::Symmetric,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Corner => "corner",
            Self::Smooth => "smooth",
            Self::Symmetric => "symmetric",
        }
    }
}

impl SegmentKindArg {
    fn core(self) -> SegmentKind {
        match self {
            Self::Line => SegmentKind::Line,
            Self::Curve => SegmentKind::Curve,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Curve => "curve",
        }
    }
}

/// What a node-shape command does to the session.
pub(crate) enum NodeShapeOp {
    Insert(f64),
    Convert(NodeKindArg),
    Segment(SegmentKindArg),
}

impl NodeShapeOp {
    fn command(&self) -> &'static str {
        match self {
            Self::Insert(_) => "node-insert",
            Self::Convert(_) => "node-convert",
            Self::Segment(_) => "segment-convert",
        }
    }

    fn detail(&self) -> String {
        match self {
            Self::Insert(t) => format!("at={t}"),
            Self::Convert(k) => format!("kind={}", k.name()),
            Self::Segment(k) => format!("to={}", k.name()),
        }
    }

    fn apply(
        &self,
        session: &mut EditSession,
        page: usize,
        target: GeometryTarget,
        node: usize,
    ) -> Result<(Vec<String>, Option<FormSurgeryOutcome>), EditError> {
        let leaf = |o: FormSurgeryOutcome| (o.disclosures.clone(), Some(o));
        match (self, target) {
            (Self::Insert(t), GeometryTarget::Page(o)) => {
                session.insert_node(page, o, node, *t).map(|d| (d, None))
            }
            (Self::Insert(t), GeometryTarget::Leaf(l)) => {
                session.insert_node_in_form(page, l, node, *t).map(leaf)
            }
            (Self::Convert(k), GeometryTarget::Page(o)) => session
                .convert_node(page, o, node, k.core())
                .map(|d| (d, None)),
            (Self::Convert(k), GeometryTarget::Leaf(l)) => session
                .convert_node_in_form(page, l, node, k.core())
                .map(leaf),
            (Self::Segment(k), GeometryTarget::Page(o)) => session
                .convert_segment(page, o, node, k.core())
                .map(|d| (d, None)),
            (Self::Segment(k), GeometryTarget::Leaf(l)) => session
                .convert_segment_in_form(page, l, node, k.core())
                .map(leaf),
        }
    }
}

/// Run one node-shape command: edit, report disclosures and the form's
/// reach on stderr, save, and print one fixed-shape record on stdout.
pub(crate) fn cmd_node_shape(args: &NodeTargetArgs, op: &NodeShapeOp) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let target = match object_or_leaf(&args.input, args.object, args.leaf) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(&args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    match op.apply(&mut session, page_index, target, args.node) {
        Err(err) => return report_edit_error(&args.input, &err),
        Ok((disclosures, form)) => {
            report_disclosures(&disclosures);
            report_form_reach(form.as_ref());
        }
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        &args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    let r = &outcome.report;
    println!(
        "{} {} page {} {} node={} {} mode={} -> {}; changed={} objects={} appended={} \
         out_bytes={} undo_verified={} undo_identical={}",
        op.command(),
        args.input.display(),
        args.page,
        target_token(args.object, args.leaf),
        args.node,
        op.detail(),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(&args.input, &outcome)
}
