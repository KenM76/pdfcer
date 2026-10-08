//! `restack-objects` — move page objects to the front, to the back, or one
//! step forward or backward in paint order.

use super::*;
use clap::ValueEnum as _;
use pdfcer_core::vector::{RestackLimitReason, StackMove};

/// `restack-objects --to`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub(crate) enum StackMoveArg {
    /// Above every other object.
    Front,
    /// Below every other object.
    Back,
    /// Just above the nearest object above that it overlaps.
    Forward,
    /// Just below the nearest object below that it overlaps.
    Backward,
}

impl From<StackMoveArg> for StackMove {
    fn from(arg: StackMoveArg) -> Self {
        match arg {
            StackMoveArg::Front => Self::Front,
            StackMoveArg::Back => Self::Back,
            StackMoveArg::Forward => Self::Forward,
            StackMoveArg::Backward => Self::Backward,
        }
    }
}

/// `restack-objects` arguments, after clap.
pub(crate) struct RestackArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    pub(crate) objects: &'a [usize],
    pub(crate) to: StackMoveArg,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `restack-objects` — one undo-able move; limited objects get a stderr line.
pub(crate) fn cmd_restack_objects(args: &RestackArgs) -> u8 {
    let input = args.input;
    let (source, mut session) = match open_for_edit(input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let page_index = (args.page.max(1) - 1) as usize;
    let out = match session.restack_objects(page_index, args.objects, args.to.into()) {
        Ok(out) => out,
        Err(err) => return report_edit_error(input, &err),
    };
    let outcome = match save_edited(
        &mut session,
        &source,
        args.output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    for limit in &out.limited {
        let why = match limit.reason {
            RestackLimitReason::Scope => {
                "the position asked for is under a different clip, layer or tag, so it went as far as it could"
            }
            RestackLimitReason::Entangled => {
                "its own bytes set a clip, layer or tag that later objects rely on, so it stayed"
            }
            _ => "it could not go where asked",
        };
        eprintln!(
            "pdfcer: {}: object {} limited: {why}",
            input.display(),
            limit.object
        );
    }
    let r = &outcome.report;
    println!(
        "restack-objects {} page={} to={} mode={} -> {}; indices={} moved={} limited={} objects_written={} appended={} out_bytes={}",
        input.display(),
        args.page,
        args.to
            .to_possible_value()
            .map_or_else(String::new, |v| v.get_name().to_owned()),
        args.mode.name(),
        args.output.display(),
        join_indices(&out.indices),
        join_indices(&out.moved),
        out.limited.len(),
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
    );
    finish_edit(input, &outcome)
}
