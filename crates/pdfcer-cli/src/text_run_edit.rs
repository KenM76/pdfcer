//! The text-run and text-object commands: delete, move, width, merge and
//! split one run or object of a page's text.

use super::*;

/// Grouped arguments for `text-run-delete` (`Pass 32.0`, `--leaf` added by
/// `G017`).
pub(crate) struct TextRunDeleteArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// The page paint-order index, when addressing a page object.
    pub(crate) object: Option<usize>,
    /// The index into this page's form leaves, when addressing a text object
    /// INSIDE a form XObject. Exactly one of this and `object` is set;
    /// `object_or_leaf` enforces it.
    pub(crate) leaf: Option<usize>,
    pub(crate) run: usize,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `text-run-delete` — remove one show operator from a text object
/// (`Pass 32.0`; reaches inside form XObjects since `G017`).
///
/// ## Contract
///
/// - One `text-run-delete …` line with the usual save-report fields, then
///   the exit code from [`finish_edit`].
/// - Refusals — an out-of-range run, and the §9.4.2 guard when the next run
///   inherits its position — go through [`report_edit_error`] before any
///   mutation. The guard's message names its own remedy.
/// - With `--leaf`, the form's reach is printed by [`report_form_reach`]
///   because the stream is shared and the delete lands on every page the form
///   is drawn on.
pub(crate) fn cmd_text_run_delete(args: &TextRunDeleteArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let target = match object_or_leaf(args.input, args.object, args.leaf) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let result = match target {
        GeometryTarget::Page(object) => session
            .delete_text_run(page_index, object, args.run)
            .map(|d| (d, None)),
        GeometryTarget::Leaf(leaf) => session
            .delete_text_run_in_form(page_index, leaf, args.run)
            .map(|o| (o.disclosures.clone(), Some(o))),
    };
    match result {
        Err(err) => return report_edit_error(args.input, &err),
        Ok((disclosures, form)) => {
            report_disclosures(&disclosures);
            report_form_reach(form.as_ref());
        }
    }
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
    let r = &outcome.report;
    println!(
        "text-run-delete {} page {} {} run={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page.max(1),
        target_token(args.object, args.leaf),
        args.run,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Grouped arguments for `text-object-split` (`Pass 306.0`).
pub(crate) struct TextObjectSplitArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// The page paint-order index, when addressing a page object.
    pub(crate) object: Option<usize>,
    /// The index into this page's form leaves, when addressing a text object
    /// INSIDE a form XObject; needs explicit `before` cuts.
    pub(crate) leaf: Option<usize>,
    pub(crate) granularity: SplitGranularityArg,
    /// Explicit cut points; overrides `granularity` when non-empty.
    pub(crate) before: &'a [usize],
    pub(crate) dry_run: bool,
    pub(crate) output: Option<&'a Path>,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `text-object-split` — cut one `BT`…`ET` into several (`Pass 306.0`).
///
/// ## Contract
///
/// - `--dry-run` prints one `text-object-split-plan …` line naming the cut
///   points and writes nothing. That is the honest shape for the `line`
///   granularity, which infers where the lines are: the operator can see the
///   cuts before committing to them. A plan the real run would refuse is
///   reported through [`report_edit_error`] with its exit code, so a dry run
///   exits as the real run would.
/// - Otherwise one `text-object-split …` line with the usual save-report
///   fields, then the exit code from [`finish_edit`].
/// - The `line` granularity's inference disclosure goes to **stderr** via
///   [`report_disclosures`], so stdout stays machine-parseable — rule 4's
///   "report separately", in the shell where the invocation IS the commit
///   (rule 11: there is no session to disclose into, so pdfcer prints on the
///   way past).
/// - Every §9.4.2/§14.6 refusal goes through [`report_edit_error`] before any
///   mutation, with the same sentences the GUI shows. One core, one answer.
pub(crate) fn cmd_text_object_split(args: &TextObjectSplitArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let object = match object_or_leaf(args.input, args.object, args.leaf) {
        Ok(GeometryTarget::Page(o)) => o,
        Ok(GeometryTarget::Leaf(leaf)) => return text_object_split_in_form(args, page_index, leaf),
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };

    // Explicit cuts win over a granularity, and skip the inference entirely —
    // so they also carry no disclosure, because nothing was guessed.
    let (points, disclosures): (Vec<usize>, Vec<String>) = if args.before.is_empty() {
        match session.text_object_split_plan(page_index, object, args.granularity.to_core()) {
            Ok(pair) => pair,
            Err(err) => return report_edit_error(args.input, &err),
        }
    } else {
        (args.before.to_vec(), Vec::new())
    };
    report_disclosures(&disclosures);

    if args.dry_run {
        return dry_run_text_object_split(&mut session, args, page_index, object, &points);
    }

    let Some(output) = args.output else {
        eprintln!("pdfcer: text-object-split needs --output unless --dry-run is given");
        return 2;
    };

    match session.split_text_object(page_index, object, &points) {
        Err(err) => return report_edit_error(args.input, &err),
        Ok(d) => report_disclosures(&d),
    }

    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    print_text_object_split(args, output, points.len(), &outcome);
    finish_edit(args.input, &outcome)
}

/// `text-object-split --leaf`: cut a text object inside a form XObject at the
/// explicit `--before` runs. The line inference and the dry-run preview read
/// page objects only, so both are refused here by name.
fn text_object_split_in_form(args: &TextObjectSplitArgs<'_>, page_index: usize, leaf: usize) -> u8 {
    if args.before.is_empty() || args.dry_run {
        eprintln!(
            "pdfcer: {}: text-object-split --leaf needs explicit --before cuts and takes no \
             --dry-run (the line inference and the plan preview read page objects only).",
            args.input.display()
        );
        return exit::RUNTIME_ERROR;
    }
    let Some(output) = args.output else {
        eprintln!("pdfcer: text-object-split needs --output unless --dry-run is given");
        return 2;
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    match session.split_text_object_in_form(page_index, leaf, args.before) {
        Err(err) => return report_edit_error(args.input, &err),
        Ok(o) => {
            report_disclosures(&o.disclosures);
            report_form_reach(Some(&o));
        }
    }
    let outcome = match save_edited(
        &mut session,
        &source,
        output,
        args.mode,
        ProducerArg::Preserve,
        args.verify_undo,
    ) {
        Ok(outcome) => outcome,
        Err(code) => return code,
    };
    print_text_object_split(args, output, args.before.len(), &outcome);
    finish_edit(args.input, &outcome)
}

/// The `text-object-split …` stdout line.
fn print_text_object_split(
    args: &TextObjectSplitArgs<'_>,
    output: &Path,
    cuts: usize,
    outcome: &EditOutcome,
) {
    let r = &outcome.report;
    println!(
        "text-object-split {} page {} {} granularity={} cuts={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page.max(1),
        target_token(args.object, args.leaf),
        split_granularity_name(args),
        cuts,
        args.mode.name(),
        output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
}

/// `text-object-split --dry-run`: print the plan, then exit as the real run
/// would — with its refusal when the split would be refused.
fn dry_run_text_object_split(
    session: &mut pdfcer_core::edit::EditSession,
    args: &TextObjectSplitArgs<'_>,
    page_index: usize,
    object: usize,
    points: &[usize],
) -> u8 {
    let refusal = match session.text_object_split_refusal(page_index, object, points) {
        Ok(r) => r,
        Err(err) => return report_edit_error(args.input, &err),
    };
    println!(
        "text-object-split-plan {} page {} object={} granularity={} cuts={} runs_before={:?}",
        args.input.display(),
        args.page.max(1),
        object,
        split_granularity_name(args),
        points.len(),
        points,
    );
    match refusal {
        Some(r) => report_edit_error(args.input, &pdfcer_core::edit::EditError::VectorEdit(r)),
        None => 0,
    }
}

/// The granularity a split report names: `explicit` when `--before` gave the cuts.
fn split_granularity_name(args: &TextObjectSplitArgs<'_>) -> &'static str {
    if args.before.is_empty() {
        args.granularity.name()
    } else {
        "explicit"
    }
}

/// Grouped arguments for `text-run-move`.
pub(crate) struct TextRunMoveArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// The page paint-order index, when addressing a page object.
    pub(crate) object: Option<usize>,
    /// The index into this page's form leaves, when addressing a text object
    /// INSIDE a form XObject.
    pub(crate) leaf: Option<usize>,
    /// One run uses `move_text_run`; several use the set verb.
    pub(crate) run: &'a [usize],
    pub(crate) dx: f64,
    pub(crate) dy: f64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `text-run-move` — translate one show operator, or a set of them as one
/// edit, inside a text object.
///
/// ## Contract
///
/// - One `text-run-move …` line with the usual save-report fields, then the
///   exit code from [`finish_edit`].
/// - The two §9.4.2 refusals — this run has no position of its own, or the
///   run after it has none — go through [`report_edit_error`] before any
///   mutation, with the same sentences the GUI shows. One core, one answer,
///   whichever shell the operator came through.
/// - Where a positioning operator had to be ADDED (a `TD`, whose second
///   operand is the leading, or a bare `T*`), the disclosure goes to stderr
///   via [`report_disclosures`] so stdout stays machine-parseable. Rule 4:
///   the page is unchanged and the bytes are not, and the operator does not
///   find that out from a diff.
pub(crate) fn cmd_text_run_move(args: &TextRunMoveArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let target = match object_or_leaf(args.input, args.object, args.leaf) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let result = match (target, args.run) {
        (GeometryTarget::Page(object), &[run]) => session
            .move_text_run(page_index, object, run, args.dx, args.dy)
            .map(|d| (d, None)),
        (GeometryTarget::Page(object), runs) => session
            .move_text_runs(page_index, object, runs, args.dx, args.dy)
            .map(|d| (d, None)),
        (GeometryTarget::Leaf(leaf), &[run]) => session
            .move_text_run_in_form(page_index, leaf, run, args.dx, args.dy)
            .map(|o| (o.disclosures.clone(), Some(o))),
        (GeometryTarget::Leaf(leaf), runs) => session
            .move_text_runs_in_form(page_index, leaf, runs, args.dx, args.dy)
            .map(|o| (o.disclosures.clone(), Some(o))),
    };
    match result {
        Err(err) => return report_edit_error(args.input, &err),
        Ok((disclosures, form)) => {
            report_disclosures(&disclosures);
            report_form_reach(form.as_ref());
        }
    }
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
    let r = &outcome.report;
    println!(
        "text-run-move {} page {} {} run={} dx={} dy={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page.max(1),
        target_token(args.object, args.leaf),
        args.run
            .iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(","),
        args.dx,
        args.dy,
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Grouped arguments for `text-run-merge`.
pub(crate) struct TextRunMergeArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// The page paint-order index, when addressing a page object.
    pub(crate) object: Option<usize>,
    /// The index into this page's form leaves, when addressing a text object
    /// INSIDE a form XObject.
    pub(crate) leaf: Option<usize>,
    pub(crate) runs: &'a [usize],
    pub(crate) separator: &'a str,
    pub(crate) fit: MergeFitArg,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// The reach of a form text edit: form object number, invocations, pages.
type Reach = Option<(u32, usize, usize)>;

fn reach_of<R>(o: &pdfcer_core::edit::FormTextOutcome<R>) -> Reach {
    Some((o.form.num, o.invocations, o.pages))
}

/// Print a text-format refusal and return its exit code.
fn report_format_error(
    command: &str,
    input: &Path,
    err: &pdfcer_core::text_edit::FormatError,
) -> u8 {
    use pdfcer_core::text_edit::FormatError;
    eprintln!("pdfcer: {command} refused on {}: {err}", input.display());
    match err {
        FormatError::Write(_) => exit::SAVE_REFUSED,
        FormatError::Content(_) | FormatError::PageTree(_) => exit::RUNTIME_ERROR,
        _ => exit::EDIT_REFUSED,
    }
}

/// Print the disclosures and the form reach of a format edit that succeeded.
fn report_format_outcome(disclosures: &[String], reach: Reach) {
    report_disclosures(disclosures);
    if let Some((form, invocations, pages)) = reach {
        report_reach(form, invocations, pages);
    }
}

/// `text-run-merge` — join consecutive show operators into one.
///
/// One `text-run-merge …` line with the merged text, the scale written and
/// the usual save-report fields, then the exit code from [`finish_edit`].
/// Disclosures go to stderr, and with `--leaf` the form's reach too. A
/// refusal exits `EDIT_REFUSED` before anything is written.
pub(crate) fn cmd_text_run_merge(args: &TextRunMergeArgs<'_>) -> u8 {
    use pdfcer_core::text_edit::{MergeFit, MergeOptions, MergeSeparator};
    let page_index = (args.page.max(1) - 1) as usize;
    let target = match object_or_leaf(args.input, args.object, args.leaf) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let separator = match args.separator {
        "none" => MergeSeparator::None,
        "space" => MergeSeparator::Space,
        other => MergeSeparator::Text(other.to_owned()),
    };
    let fit = match args.fit {
        MergeFitArg::Span => MergeFit::Span,
        MergeFitArg::Natural => MergeFit::Natural,
    };
    let opts = MergeOptions::default().separator(separator).fit(fit);
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let result = match target {
        GeometryTarget::Page(o) => session
            .merge_text_runs(page_index, o, args.runs, &opts)
            .map(|r| (r, None)),
        GeometryTarget::Leaf(l) => session
            .merge_text_runs_in_form(page_index, l, args.runs, &opts)
            .map(|o| (o.report.clone(), reach_of(&o))),
    };
    let report = match result {
        Ok((report, reach)) => {
            report_format_outcome(&report.disclosures, reach);
            report
        }
        Err(err) => return report_format_error("text-run-merge", args.input, &err),
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
    let r = &outcome.report;
    let runs: Vec<String> = args.runs.iter().map(ToString::to_string).collect();
    println!(
        "text-run-merge {} page {} {} runs={} merged={} text={:?} h_scale={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page.max(1),
        target_token(args.object, args.leaf),
        runs.join(","),
        report.runs_merged,
        report.text,
        report
            .h_scale_change
            .map_or_else(|| "unchanged".to_owned(), |(_, pct)| format!("{pct:.4}")),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}

/// Grouped arguments for `text-run-width`.
pub(crate) struct TextRunWidthArgs<'a> {
    pub(crate) input: &'a Path,
    pub(crate) page: u32,
    /// The page paint-order index, when addressing a page object.
    pub(crate) object: Option<usize>,
    /// The index into this page's form leaves, when addressing a text object
    /// INSIDE a form XObject.
    pub(crate) leaf: Option<usize>,
    pub(crate) run: usize,
    pub(crate) width: f64,
    pub(crate) output: &'a Path,
    pub(crate) mode: SaveMode,
    pub(crate) verify_undo: bool,
}

/// `text-run-width` — fit one show operator to a page width through `Tz`.
///
/// One `text-run-width …` line with the usual save-report fields and the
/// scale that was written, then the exit code from [`finish_edit`]. The
/// disclosure of the scale goes to stderr, and with `--leaf` the form's
/// reach too. A refusal exits `EDIT_REFUSED` before anything is written.
pub(crate) fn cmd_text_run_width(args: &TextRunWidthArgs<'_>) -> u8 {
    let page_index = (args.page.max(1) - 1) as usize;
    let target = match object_or_leaf(args.input, args.object, args.leaf) {
        Ok(t) => t,
        Err(code) => return code,
    };
    let (source, mut session) = match open_for_edit(args.input) {
        Ok(pair) => pair,
        Err(code) => return code,
    };
    let result = match target {
        GeometryTarget::Page(o) => session
            .set_text_run_width(page_index, o, args.run, args.width)
            .map(|r| (r, None)),
        GeometryTarget::Leaf(l) => session
            .set_text_run_width_in_form(page_index, l, args.run, args.width)
            .map(|o| (o.report.clone(), reach_of(&o))),
    };
    let report = match result {
        Ok((report, reach)) => {
            report_format_outcome(&report.disclosures, reach);
            report
        }
        Err(err) => return report_format_error("text-run-width", args.input, &err),
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
    let r = &outcome.report;
    println!(
        "text-run-width {} page {} {} run={} width={} h_scale={} mode={} -> {}; changed={} objects={} appended={} out_bytes={} undo_verified={} undo_identical={}",
        args.input.display(),
        args.page.max(1),
        target_token(args.object, args.leaf),
        args.run,
        args.width,
        report
            .h_scale_change
            .map_or_else(|| "unchanged".to_owned(), |(_, pct)| format!("{pct:.4}")),
        args.mode.name(),
        args.output.display(),
        outcome.changed,
        r.objects_written,
        r.bytes_appended,
        r.bytes_written,
        u32::from(outcome.undo_verified),
        u32::from(outcome.undo_identical),
    );
    finish_edit(args.input, &outcome)
}
